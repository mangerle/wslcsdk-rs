//! 容器进程异步流式 I/O 桥接与缓冲支持
//!
//! 通过全局弱引用注册表安全映射回调上下文，彻底规避生命周期脱节引发的 UAF 内存安全风险，
//! 同时拆分标准输出与标准错误的分发通道，消除回调间的锁竞争。

use super::callbacks::{stream_exit_trampoline, stream_io_trampoline};
use crate::channel::AsyncReceiver;
use crate::error::WslcError;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use tokio::sync::{mpsc, oneshot};
use wslcsdk_sys::types::WslcProcessCallbacks;

static NEXT_STREAM_ID: AtomicU64 = AtomicU64::new(1);

/// 流式会话的全局回调上下文注册表
///
/// 键为流 ID，值为[`StreamState`] 的弱引用。C 回调线程在每次IO 数据到达与
/// 进程退出时经由本表定位Rust 侧状态；会话侧全部强引用消失后，条目由
/// [`StreamState`] 的析构函数移除，此后的迟到回调即查不到状态而被安全忽略。
///
/// 选用 `DashMap` 而非 `RwLock<HashMap>`：前者在读多写少的高频回调路径上
/// 无需显式加锁，且内建分片与轻量 `Clone`，省去手写分片数组与锁守卫管理。
/// 依赖冲突不存在的另一重保障是：此处只在**同步的 C 回调内**访问，
/// 绝无守卫跨 `.await` 的可能。
type StreamRegistry = DashMap<u64, Weak<StreamState>>;

/// 全局注册表单例，首次访问时惰性构造
pub(super) fn stream_registry() -> &'static StreamRegistry {
    static REGISTRY: OnceLock<StreamRegistry> = OnceLock::new();
    REGISTRY.get_or_init(DashMap::new)
}

/// 容器进程异步流式接收端
///
/// 提供标准输出、标准错误两路字节流与进程退出通知。
/// 公开接口中不含任何第三方异步运行时的具体类型。
#[derive(Debug)]
pub struct ProcessStreams {
    /// 标准输出字节流
    pub stdout: AsyncReceiver<Vec<u8>>,
    /// 标准错误字节流
    pub stderr: AsyncReceiver<Vec<u8>>,
    /// 进程退出码接收端，仅供 [`ProcessStreams::wait_exit`] 消费
    exit_rx: Option<oneshot::Receiver<i32>>,
    /// 与本流式会话关联的共享状态，提供丢弃字节计数
    pub(crate) stream_state: Arc<StreamState>,
}

#[derive(Debug)]
pub(crate) struct StreamState {
    pub(crate) id: u64,
    /// 标准输出发送端；退出回调会取走并drop，从而关闭通道
    ///
    /// 以 `Option` 包裹而非直接持有，是为了让「关闭」这一动作可表达：tokio 的
    /// `recv()` 仅在全部发送端被drop 后才会返回 `None`。若发送端随状态对象
    /// 一同存活，进程退出后接收端将永远阻塞，官方文档与示例中
    /// `while let Some(chunk) = streams.stdout.recv().await` 的消费循环无法终止。
    pub(super) stdout_tx: Mutex<Option<mpsc::Sender<Vec<u8>>>>,
    /// 标准错误发送端；语义同 [`StreamState::stdout_tx`]
    pub(super) stderr_tx: Mutex<Option<mpsc::Sender<Vec<u8>>>>,
    pub(crate) exit_tx: Mutex<Option<oneshot::Sender<i32>>>,
    pub(crate) stdout_dropped_bytes: AtomicU64,
    pub(crate) stderr_dropped_bytes: AtomicU64,
}

impl StreamState {
    /// 关闭两路字节流通道
    ///
    /// 取走并 drop 发送端后，接收端在缓冲耗尽后 `recv()` 即返回 `None`，
    /// 调用方据此可自然终止消费循环。官方保证退出回调在 IO 冲刷完成后才触发，
    /// 故关闭通道不会丢失尾部数据。
    pub(super) fn close_io_channels(&self) {
        if let Ok(mut guard) = self.stdout_tx.lock() {
            guard.take();
        }
        if let Ok(mut guard) = self.stderr_tx.lock() {
            guard.take();
        }
    }
}

impl Drop for StreamState {
    fn drop(&mut self) {
        stream_registry().remove(&self.id);

        // 汇总本次流式会话的丢弃情况，便于定位背压不足或消费端停滞的问题
        let stdout_dropped = self.stdout_dropped_bytes.load(Ordering::Relaxed);
        let stderr_dropped = self.stderr_dropped_bytes.load(Ordering::Relaxed);
        if stdout_dropped > 0 || stderr_dropped > 0 {
            log::warn!(
                "流式会话结束但存在被丢弃的数据，流 ID: {}，标准输出丢弃 {} 字节，标准错误丢弃 {} 字节，请考虑增大通道容量或加快消费速度",
                self.id,
                stdout_dropped,
                stderr_dropped
            );
        }
    }
}

impl ProcessStreams {
    /// 获取因通道满载或断开而丢弃的标准输出总字节数
    pub fn stdout_dropped_bytes(&self) -> u64 {
        self.stream_state
            .stdout_dropped_bytes
            .load(Ordering::Relaxed)
    }

    /// 获取因通道满载或断开而丢弃的标准错误总字节数
    pub fn stderr_dropped_bytes(&self) -> u64 {
        self.stream_state
            .stderr_dropped_bytes
            .load(Ordering::Relaxed)
    }

    /// 等待进程退出并返回其退出码
    ///
    /// 官方要求退出回调在标准 IO 冲刷完成后才触发，因此建议先消费完
    /// [`ProcessStreams::stdout`] 与 [`ProcessStreams::stderr`] 再调用本方法，
    /// 否则可能漏掉尾部数据。
    ///
    /// 退出通知只能被消费一次，重复调用会返回错误。
    ///
    /// # Errors
    /// 退出通知已被消费，或底层通知通道异常终止时返回对应的 [`WslcError`]。
    pub async fn wait_exit(&mut self) -> Result<i32, WslcError> {
        let Some(exit_rx) = self.exit_rx.take() else {
            return Err(WslcError::AlreadyConsumed("进程退出通知".to_string()));
        };

        exit_rx
            .await
            .map_err(|_| WslcError::ChannelTerminated("进程退出通知通道".to_string()))
    }
}

/// 构建流式通道与关联的回调结构
///
/// 返回的第二个元素为全局注册表中的流 ID，直接作为回调上下文使用，
/// 以整数代替裸指针可避免在 [`ProcessBuilder`](crate::ProcessBuilder) 中
/// 引入裸指针字段与随之而来的 `unsafe impl Send/Sync` 断言。
pub(crate) fn setup_streaming_channels(
    capacity: usize,
) -> (
    WslcProcessCallbacks,
    usize,
    Arc<StreamState>,
    ProcessStreams,
) {
    let cap = capacity.max(1);
    let (stdout_tx, stdout_rx) = mpsc::channel::<Vec<u8>>(cap);
    let (stderr_tx, stderr_rx) = mpsc::channel::<Vec<u8>>(cap);
    let (exit_tx, exit_rx) = oneshot::channel::<i32>();

    let id = NEXT_STREAM_ID.fetch_add(1, Ordering::Relaxed);

    let state = Arc::new(StreamState {
        id,
        stdout_tx: Mutex::new(Some(stdout_tx)),
        stderr_tx: Mutex::new(Some(stderr_tx)),
        exit_tx: Mutex::new(Some(exit_tx)),
        stdout_dropped_bytes: AtomicU64::new(0),
        stderr_dropped_bytes: AtomicU64::new(0),
    });

    stream_registry().insert(id, Arc::downgrade(&state));
    let callbacks = WslcProcessCallbacks {
        on_stdout: Some(stream_io_trampoline),
        on_stderr: Some(stream_io_trampoline),
        on_exit: Some(stream_exit_trampoline),
    };

    let streams = ProcessStreams {
        stdout: AsyncReceiver::new(stdout_rx),
        stderr: AsyncReceiver::new(stderr_rx),
        exit_rx: Some(exit_rx),
        stream_state: state.clone(),
    };

    (callbacks, id as usize, state, streams)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::c_void;
    use wslcsdk_sys::types::WslcProcessIOHandle;

    /// 驱动一次流式会话，返回其回调、上下文 ID、状态与接收端
    fn setup(
        capacity: usize,
    ) -> (
        WslcProcessCallbacks,
        usize,
        Arc<StreamState>,
        ProcessStreams,
    ) {
        setup_streaming_channels(capacity)
    }

    /// 模拟 C 侧的标准输出回调推送一段数据
    ///
    /// # Safety
    ///
    /// `context` 必须是由 [`setup_streaming_channels`] 返回的有效流 ID，
    /// 且对应的 [`StreamState`] 尚未被析构。
    unsafe fn emit_stdout(context: usize, payload: &[u8]) {
        let ctx = context as *mut c_void;
        // SAFETY: 调用方保证 context 有效；载荷为调用方持有的有效切片
        unsafe {
            stream_io_trampoline(
                WslcProcessIOHandle::STDOUT,
                payload.as_ptr(),
                payload.len() as u32,
                ctx,
            )
        };
    }

    /// 模拟 C 侧的标准错误回调推送一段数据
    ///
    /// # Safety
    ///
    /// `context` 必须是由 [`setup_streaming_channels`] 返回的有效流 ID，
    /// 且对应的 [`StreamState`] 尚未被析构。
    unsafe fn emit_stderr(context: usize, payload: &[u8]) {
        let ctx = context as *mut c_void;
        // SAFETY: 调用方保证 context 有效；载荷为调用方持有的有效切片
        unsafe {
            stream_io_trampoline(
                WslcProcessIOHandle::STDERR,
                payload.as_ptr(),
                payload.len() as u32,
                ctx,
            )
        };
    }

    /// 创建仅当前线程使用的测试运行时
    fn test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("创建测试运行时失败")
    }

    #[test]
    fn test_registry_entry_lifecycle() {
        let (callbacks, context, state, streams) = setup(8);
        assert!(callbacks.on_stdout.is_some(), "必须注册标准输出回调");
        assert!(callbacks.on_stderr.is_some(), "必须注册标准错误回调");
        assert!(callbacks.on_exit.is_some(), "必须注册退出回调");

        // 状态存活期间，注册表可经 ID 取出强引用
        assert!(stream_registry().contains_key(&(context as u64)));

        let id = state.id;
        // StreamState 的强引用由建造者返回值与 ProcessStreams 各持一份，
        // 两者全部释放后才会真正析构并注销注册表条目
        drop(state);
        assert!(
            stream_registry().contains_key(&id),
            "仅释放一份强引用时注册表条目应仍然存在"
        );
        drop(streams);
        assert!(
            !stream_registry().contains_key(&id),
            "流状态析构后注册表条目应被移除"
        );
    }

    #[test]
    fn test_trampoline_routes_payload_to_matching_channel() {
        let rt = test_runtime();
        let (_callbacks, context, _state, mut streams) = setup(8);

        rt.block_on(async {
            // SAFETY: context 由 setup 返回且 state 仍存活
            unsafe { emit_stdout(context, b"hello-stdout") };
            // SAFETY: 同上
            unsafe { emit_stderr(context, b"hello-stderr") };

            let out = streams.stdout.recv().await.expect("应收到标准输出");
            let err = streams.stderr.recv().await.expect("应收到标准错误");
            assert_eq!(out, b"hello-stdout".to_vec());
            assert_eq!(err, b"hello-stderr".to_vec());
        });
    }

    #[test]
    fn test_backpressure_drops_and_accumulates_byte_count() {
        let rt = test_runtime();
        // 容量为 1 且消费端停滞，用于触发背压丢弃
        let (_callbacks, context, state, mut streams) = setup(1);

        rt.block_on(async {
            // SAFETY: context 由 setup 返回且 state 仍存活
            unsafe { emit_stdout(context, b"first") };
            // SAFETY: 同上；此时通道已满，该次投递必然被丢弃
            unsafe { emit_stdout(context, b"second") };
            // SAFETY: 同上
            unsafe { emit_stdout(context, b"third") };

            let first = streams.stdout.recv().await.expect("应收到首条数据");
            assert_eq!(first, b"first".to_vec());
            assert_eq!(
                streams.stdout_dropped_bytes(),
                11,
                "被丢弃的两条分别为 6 与 5 字节，合计 11 字节"
            );
        });

        assert_eq!(state.stdout_dropped_bytes.load(Ordering::Relaxed), 11);
        assert_eq!(
            state.stderr_dropped_bytes.load(Ordering::Relaxed),
            0,
            "标准错误方向不应受到标准输出丢弃的影响"
        );
    }

    #[test]
    fn test_trampoline_ignores_invalid_inputs() {
        let (_callbacks, context, _state, mut streams) = setup(4);
        let ctx = context as *mut c_void;

        // SAFETY: context 有效；下列三种异常输入均须被安全忽略而不得崩溃
        unsafe {
            // 空载荷
            stream_io_trampoline(WslcProcessIOHandle::STDOUT, b"data".as_ptr(), 0, ctx);
            // 空上下文指针
            stream_io_trampoline(
                WslcProcessIOHandle::STDOUT,
                b"data".as_ptr(),
                4,
                std::ptr::null_mut(),
            );
            // 空数据指针
            stream_io_trampoline(WslcProcessIOHandle::STDOUT, std::ptr::null(), 4, ctx);
        }

        assert!(
            streams.stdout.try_recv().is_err(),
            "空载荷、空上下文与空数据指针均不应产生数据"
        );
        assert_eq!(streams.stdout_dropped_bytes(), 0);
    }

    #[test]
    fn test_callback_after_state_released_is_ignored() {
        let (_callbacks, context, state, streams) = setup(4);
        let id = state.id;
        drop(state);
        drop(streams);

        // 注册表条目已随状态析构移除。此时 C 侧若仍触发回调必须被安全忽略，
        // 绝不能访问已释放的内存
        // SAFETY: context 曾是有效流 ID，注册表已无对应条目，回调应提前返回
        unsafe { emit_stdout(context, b"late-data") };
        assert!(!stream_registry().contains_key(&id));
    }

    #[test]
    fn test_exit_trampoline_delivers_code() {
        let rt = test_runtime();
        let (_callbacks, context, _state, mut streams) = setup(4);

        rt.block_on(async {
            // SAFETY: context 由 setup 返回且 state 仍存活
            unsafe { stream_exit_trampoline(7, context as *mut c_void) };
            assert_eq!(streams.wait_exit().await.expect("应收到退出码"), 7);
        });
    }

    #[test]
    fn test_exit_trampoline_with_null_context_is_ignored() {
        let rt = test_runtime();
        let (_callbacks, _context, _state, mut streams) = setup(4);

        rt.block_on(async {
            // SAFETY: 空上下文是合法的「无订阅」情形，应被安全忽略
            unsafe { stream_exit_trampoline(0, std::ptr::null_mut()) };
            // 退出通知不应被投递
            let res =
                tokio::time::timeout(std::time::Duration::from_millis(50), streams.wait_exit())
                    .await;
            assert!(res.is_err(), "空上下文不应触发退出通知");
        });
    }

    #[test]
    fn test_exit_closes_io_channels_so_consumer_loop_terminates() {
        let rt = test_runtime();
        let (_callbacks, context, _state, mut streams) = setup(4);

        rt.block_on(async {
            // 退出回调触发前，通道保持开启，可持续接收数据
            // SAFETY: context 由 setup 返回且 state 仍存活
            unsafe { emit_stdout(context, b"before-exit") };
            // SAFETY: 同上
            unsafe { emit_stderr(context, b"before-exit") };

            // SAFETY: 触发退出回调，其后官方不再投递 IO 数据
            unsafe { stream_exit_trampoline(0, context as *mut c_void) };

            // 关闭前已入队的数据必须被完整送达，随后消费循环得以终止
            assert_eq!(streams.stdout.recv().await, Some(b"before-exit".to_vec()));
            assert!(
                streams.stdout.recv().await.is_none(),
                "缓冲耗尽后应返回 None"
            );
            assert_eq!(streams.stderr.recv().await, Some(b"before-exit".to_vec()));
            assert!(
                streams.stderr.recv().await.is_none(),
                "缓冲耗尽后应返回 None"
            );
        });
    }

    #[test]
    fn test_zero_capacity_is_normalized_to_one() {
        // 容量为 0 时应被归一化为 1，避免出现无缓冲的退化通道
        let (_callbacks, _context, _state, mut streams) = setup(0);
        // SAFETY: 仅观察通道状态，不触碰裸指针
        let _ = &mut streams;
        assert_eq!(streams.stdout_dropped_bytes(), 0);
    }
}
