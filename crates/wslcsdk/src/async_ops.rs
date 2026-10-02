//! 面向 Tokio 运行时的异步非阻塞扩展
//!
//! 包含基于 Windows 原生线程池 `RegisterWaitForSingleObject` 的零轮询事件驱动等待机制，
//! 以及将密集型文件/镜像与容器管理安全卸载至阻塞线程池的异步方法扩展。
//!
//! # 取消语义
//!
//! 本模块的阻塞类异步方法均基于 [`tokio::task::spawn_blocking`]。该任务一经派发便
//! **无法被取消**：即便调用方在 `select!` 中超时放弃等待，底层的 WSLC 调用
//! （如镜像拉取、容器启动）仍会在阻塞线程池中执行到底，并持续持有其参数中的
//! 会话或容器句柄克隆。
//!
//! 因此超时返回**不代表操作已被撤销**。若业务需要真正的中止语义，请改用本库提供的
//! 同步接口自行配合超时与取消令牌，或在 SDK 层面确认对应操作是否可中断。

use crate::channel::AsyncReceiver;
use crate::container::{ContainerBuilder, WslcContainerHandle};
use crate::error::WslcError;
use crate::image::{ImageInfo, OwnedImageProgress, WslcImageManager};
use crate::session::{SessionBuilder, WslcSessionHandle};
use core::ffi::c_void;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use windows_sys::Win32::Foundation::{BOOLEAN, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Threading::{
    RegisterWaitForSingleObject, UnregisterWaitEx, WT_EXECUTEONLYONCE,
};
use wslcsdk_sys::types::WslcSignal;

// ==================== 方案 B: Win32 线程池事件驱动等待 ====================

struct EventWaitShared {
    sender: Mutex<Option<oneshot::Sender<bool>>>,
    is_done: AtomicBool,
}

unsafe extern "system" fn win32_wait_callback(context: *mut c_void, timer_or_wait_fired: BOOLEAN) {
    if context.is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: 裸指针的所有权配对关系（本模块最需论证的一处）：
        //
        // 该指针由 [`wait_win32_event_async`] 中的 `Arc::into_raw(shared.clone())`
        // 拆出，使引用计数 +1 并移交本回调。回收端**有且只有两条路径**，
        // 二者互斥，故恰好重建一次、不存在重复释放或泄漏：
        //
        // 1. 本回调执行 `Arc::from_raw` 重建（此处），随后 `shared` 随闭包
        //    析构使计数回落；
        // 2. [`WaitGuard::drop`] 在 `UnregisterWaitEx(handle, NULL)` 返回非零
        //    （成功取消、承诺回调不再执行）时回收。
        //
        // 关键在于二者不会同时生效：若本回调已经开始执行，`is_done` 必为
        // true（下方先 store 后send），`WaitGuard::drop` 会走
        // `UnregisterWaitEx(handle, INVALID_HANDLE_VALUE)` 分支而**不**回收；
        // 反之若回调尚未开始，`is_done` 为 false，drop 走 NULL 分支并回收，
        // 本回调则永不会被调用。三方（回调、drop、注册失败分支）合起来
        // 覆盖了全部时序。
        let shared = unsafe { Arc::from_raw(context as *const EventWaitShared) };
        let is_signaled = timer_or_wait_fired == 0;
        // 先置 is_done 再发送通知：drop 侧以 Acquire 读该标志决定回收路径，
        // 故此处的 Release 写入必须先于 send，二者不可调换
        shared.is_done.store(true, Ordering::Release);
        if let Ok(mut lock) = shared.sender.lock()
            && let Some(tx) = lock.take()
        {
            let _ = tx.send(is_signaled);
        }
    }));
}

/// 等待句柄与回调上下文的 RAII 守卫
///
/// # 为何两个指针字段都以 `usize` 而非原生指针类型承载
///
/// 本守卫在 `rx.await` 期间**跨挂起点存活**，故其字段类型直接决定整个
/// future 是否为 `Send`。而 `HANDLE` 与 `*mut EventWaitShared` 都是裸指针，
/// 二者会让 `wait_win32_event_async` 的 future 失去 `Send`——调用方因此
/// 无法 `tokio::spawn` 它，也无法在多线程运行时上跨 `select!` 组合。
///
/// 改为整数承载后，字段本身是 `Send`，future 随之可用。这与本库
/// `process/stream.rs` 中「回调上下文以整数而非裸指针承载」的既有做法
/// 一致。回收时按原类型 cast 回即可，`debug_assert` 守护同源关系。
struct WaitGuard {
    shared: Arc<EventWaitShared>,
    /// `RegisterWaitForSingleObject` 返回的等待句柄，以整数承载以保持 `Send`
    wait_handle: usize,
    /// `Arc::into_raw` 拆出的上下文指针地址，回收时 cast 回 `*const EventWaitShared`
    raw_ctx: usize,
}

impl Drop for WaitGuard {
    fn drop(&mut self) {
        if let Ok(mut lock) = self.shared.sender.lock() {
            let _ = lock.take();
        }
        if self.wait_handle != 0 {
            // SAFETY: 两个整数字段均源自本模块内的同一次构造，
            // wait_handle 来自成功的 RegisterWaitForSingleObject，
            // raw_ctx 为 Arc::as_ptr 的取值，cast 回原类型合法。
            let wait_handle = self.wait_handle as HANDLE;
            if self.shared.is_done.load(Ordering::Acquire) {
                // 回调已完成（其内的Arc::from_raw 已消费掉本份所有权），
                // 此处只注销句柄、不再回收裸指针。
                // 用 INVALID_HANDLE_VALUE 会同步等待正在进行的回调收尾，
                // 但既然 is_done 已为 true，回调必已越过发送阶段，
                // 实际等待时间可忽略，故不影响「drop 不阻塞」的前提。
                //
                // SAFETY: 句柄来自同一次成功的 RegisterWaitForSingleObject，
                // 且本类型未实现 Clone、Drop 需独占 &mut self，
                // 保证句柄至多注销一次。
                unsafe {
                    let _ = UnregisterWaitEx(wait_handle, INVALID_HANDLE_VALUE);
                }
            } else {
                // 回调尚未开始，传入 NULL 立即返回，杜绝阻塞当前 Tokio 工作线程
                let success =
                    // SAFETY: 句柄来源同上，Drop 独占保证至多注销一次。
                    unsafe { UnregisterWaitEx(wait_handle, std::ptr::null_mut()) };
                if success != 0 {
                    // 成功取消等待，官方承诺回调不会再执行，故由本处
                    // 消费 `Arc::into_raw` 拆出的那一份引用计数。
                    //
                    // SAFETY: 该裸指针由 `Arc::into_raw(shared.clone())` 拆出，
                    // 其回收方在本回调与此处之间二选一（论证见
                    // `win32_wait_callback` 的 SAFETY 注释）。
                    // 返回非零即等价于「官方承诺不再执行回调」，
                    // 故此处回收不会与回调内的 `Arc::from_raw` 重复。
                    let _ = unsafe { Arc::from_raw(self.raw_ctx as *const EventWaitShared) };
                }
                // 若返回 0，回调已被并发调度，其内的 Arc::from_raw 负责回收
            }
            self.wait_handle = 0;
        }
    }
}

/// 注册 Win32 线程池等待事件，失败时安全回收裸指针所有权并返回 HRESULT 错误
fn register_win32_wait(
    event: HANDLE,
    raw_ctx: *mut EventWaitShared,
    timeout_ms: u32,
) -> Result<HANDLE, WslcError> {
    let mut wait_handle: HANDLE = std::ptr::null_mut();
    // SAFETY: 事件句柄由官方 SDK 托管且生命周期覆盖本次等待；上下文指针为有效
    // Arc 拆出的裸指针。注册失败（返回 0）时下方分支立即回收其所有权。
    let success = unsafe {
        RegisterWaitForSingleObject(
            &mut wait_handle,
            event,
            Some(win32_wait_callback),
            raw_ctx as *mut c_void,
            timeout_ms,
            WT_EXECUTEONLYONCE,
        )
    };

    if success == 0 {
        // SAFETY: 该指针由 Arc::into_raw 从同一 Arc 拆出，引用计数所有权
        // 随之转移；此处按对应路径恰好重建一次，未发生重复释放。
        let _ = unsafe { Arc::from_raw(raw_ctx) };
        // SAFETY: 无参数无副作用，仅紧随失败的 Win32 调用读取错误码。
        let err = unsafe { GetLastError() };
        return Err(WslcError::Hresult(
            err,
            "注册 Win32 线程池等待事件失败".to_string(),
        ));
    }

    Ok(wait_handle)
}

/// 基于 Windows 线程池的非轮询异步事件等待 (真正 0 CPU、0 协程切片搅动，具备 Tokio 取消安全性)
///
/// # 为何写为「返回 impl Future」而非 `async fn`
///
/// `HANDLE` 是裸指针。若本函数写作 `async fn`，该参数会被生成器捕获并
/// 跨 await 存活，使返回的 future 失去 `Send`——调用方因此无法
/// `tokio::spawn` 它，也无法在多线程运行时上跨 `select!` 组合。
///
/// 改为在进入 async 块之前就把裸指针消费干净（注册完毕、守卫以整数承载
/// 两个地址），async 块内便不再有任何裸指针，future 随之可用。
pub(crate) fn wait_win32_event_async(
    event: HANDLE,
    timeout_ms: u32,
) -> impl Future<Output = Result<bool, WslcError>> {
    let (tx, rx) = oneshot::channel();
    let shared = Arc::new(EventWaitShared {
        sender: Mutex::new(Some(tx)),
        is_done: AtomicBool::new(false),
    });

    // ---- 同步段：裸指针仅在此段出现，不进入下方 async 块 ----
    let prepared = if event.is_null() {
        Err(WslcError::InvalidHandle)
    } else {
        // 拆出一份所有权交给 Win32 回调；其地址同时以整数记入守卫
        let raw_ctx = Arc::into_raw(shared.clone()) as *mut EventWaitShared;
        register_win32_wait(event, raw_ctx, timeout_ms).map(|handle| {
            let guard = WaitGuard {
                shared: shared.clone(),
                wait_handle: handle as usize,
                raw_ctx: raw_ctx as usize,
            };
            debug_assert_eq!(
                guard.raw_ctx,
                Arc::as_ptr(&guard.shared) as usize,
                "守卫记录的上下文地址必须与本 Arc 指向同一对象"
            );
            guard
        })
    };

    async move {
        // 守卫移入 async 块：即便 future 被丢弃未 poll，其 Drop 仍会注销等待
        let guard = prepared?;

        let result = rx
            .await
            .map_err(|_| WslcError::ChannelTerminated("Win32 事件等待通知通道".to_string()))?;

        drop(guard);
        Ok(result)
    }
}

// ==================== 阻塞任务调度辅助 ====================

/// 将同步的 WSLC 调用卸载至阻塞线程池执行
///
/// 统一承担三件事：
/// 1. 把可能长时间阻塞的 SDK 调用移出 Tokio 工作线程，避免拖累整个调度器；
/// 2. 收敛 `JoinError` 到 [`WslcError::TaskJoin`] 的转换，并附带具体操作名称；
/// 3. 抹平各调用点重复的样板代码。
///
/// 被调用的同步接口自身已负责建立线程级 COM MTA 上下文（见
/// [`try_initialize_mta`](crate::try_initialize_mta)），故此处不再重复包裹 `with_mta`：
/// 那会在同一次调用中初始化两次 MTA 并吞掉 STA 降级信息，徒增调用链深度而无收益。
///
/// # 取消语义
///
/// [`tokio::task::spawn_blocking`] 派发的任务**无法被取消**。调用方超时或放弃等待时，
/// 底层调用仍会执行到底，本方法不提供中止能力。
async fn run_blocking<F, R>(operation: &str, task: F) -> Result<R, WslcError>
where
    F: FnOnce() -> Result<R, WslcError> + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|e| WslcError::TaskJoin(format!("{operation}失败: {e}")))?
}

// ==================== 会话异步扩展 ====================

impl SessionBuilder {
    /// 异步创建并激活会话 (防止阻塞 Tokio 工作线程)
    pub async fn build_async(self) -> Result<WslcSessionHandle, WslcError> {
        run_blocking("异步创建会话", move || self.build()).await
    }
}

impl ContainerBuilder {
    /// 异步创建容器
    pub async fn build_async(
        self,
        session: &WslcSessionHandle,
    ) -> Result<WslcContainerHandle, WslcError> {
        let session = session.clone();
        run_blocking("异步创建容器", move || self.build(&session)).await
    }
}

impl WslcSessionHandle {
    /// 异步获取会话内的镜像列表
    pub async fn list_images_async(&self) -> Result<Vec<ImageInfo>, WslcError> {
        let session = self.clone();
        run_blocking("异步获取镜像列表", move || {
            WslcImageManager::list_images(&session)
        })
        .await
    }

    /// 异步拉取远程镜像并支持订阅进度
    ///
    /// 返回的进度接收端与拉取任务共享同一个有界通道：若消费端跟不上，
    /// 通道会按背压策略丢弃进度消息（丢弃量由 `AsyncReceiver` 侧统计），
    /// 拉取本身则继续进行。
    pub fn pull_image_with_progress_async(
        &self,
        uri: String,
        registry_auth: Option<String>,
        progress_capacity: usize,
    ) -> (
        impl Future<Output = Result<(), WslcError>> + Send + 'static,
        AsyncReceiver<OwnedImageProgress>,
    ) {
        let (tx, rx) = mpsc::channel(progress_capacity.max(1));
        let dropped_counter = Arc::new(AtomicU64::new(0));
        let drop_counter_clone = Arc::clone(&dropped_counter);
        let session = self.clone();
        let fut = async move {
            run_blocking("异步拉取镜像", move || {
                WslcImageManager::pull_image(
                    &session,
                    &uri,
                    registry_auth.as_deref(),
                    Some(move |p: &crate::image::ImageProgress<'_>| {
                        let owned = p.to_owned();
                        if tx.try_send(owned).is_err() {
                            drop_counter_clone.fetch_add(1, Ordering::Relaxed);
                        }
                        true
                    }),
                )
            })
            .await
        };
        (
            fut,
            AsyncReceiver::new_with_drop_counter(rx, dropped_counter),
        )
    }

    /// 异步拉取远程镜像
    ///
    /// # 取消语义
    ///
    /// 底层拉取一经开始便无法中断：本方法超时返回时，SDK 侧的拉取动作
    /// 仍会在阻塞线程池中继续执行，并持续持有会话句柄克隆。
    pub async fn pull_image_async(
        &self,
        uri: String,
        registry_auth: Option<String>,
    ) -> Result<(), WslcError> {
        // 刻意不复用 pull_image_with_progress_async：后者会建立进度通道，
        // 而本方法丢弃接收端后，每条进度都要白做一次 OwnedImageProgress 的
        // 字符串分配、一次必然失败的 try_send 与一次原子自增。大镜像拉取
        // 有数千条进度事件，这些开销全部落空。
        let session = self.clone();
        run_blocking("异步拉取镜像", move || {
            WslcImageManager::pull_image(
                &session,
                &uri,
                registry_auth.as_deref(),
                None::<fn(&crate::image::ImageProgress<'_>) -> bool>,
            )
        })
        .await
    }

    /// 异步推送镜像至远程仓库
    pub async fn push_image_async(
        &self,
        image: String,
        registry_auth: Option<String>,
    ) -> Result<(), WslcError> {
        let session = self.clone();
        run_blocking("异步推送镜像", move || {
            WslcImageManager::push_image(&session, &image, registry_auth.as_deref())
        })
        .await
    }

    /// 异步从文件导入 tar 镜像
    pub async fn import_image_from_file_async(
        &self,
        image_name: String,
        path: PathBuf,
    ) -> Result<(), WslcError> {
        let session = self.clone();
        run_blocking("异步导入镜像文件", move || {
            WslcImageManager::import_image_from_file(&session, &image_name, path)
        })
        .await
    }

    /// 异步从文件载入 docker save 镜像
    pub async fn load_image_from_file_async(&self, path: PathBuf) -> Result<(), WslcError> {
        let session = self.clone();
        run_blocking("异步载入镜像文件", move || {
            WslcImageManager::load_image_from_file(&session, path)
        })
        .await
    }

    /// 异步为已有镜像打标签
    pub async fn tag_image_async(
        &self,
        image: String,
        repo: String,
        tag: String,
    ) -> Result<(), WslcError> {
        let session = self.clone();
        run_blocking("异步镜像打标签", move || {
            WslcImageManager::tag_image(&session, &image, &repo, &tag)
        })
        .await
    }

    /// 异步删除镜像
    pub async fn delete_image_async(&self, name_or_id: String) -> Result<(), WslcError> {
        let session = self.clone();
        run_blocking("异步删除镜像", move || {
            WslcImageManager::delete_image(&session, &name_or_id)
        })
        .await
    }

    /// 异步终止会话
    pub async fn terminate_async(&self) -> Result<(), WslcError> {
        let session = self.clone();
        run_blocking("异步终止会话", move || session.terminate()).await
    }
}

// ==================== 容器异步扩展 ====================

impl WslcContainerHandle {
    /// 异步启动容器
    pub async fn start_async(&self, attach: bool) -> Result<(), WslcError> {
        let container = self.clone();
        run_blocking("异步启动容器", move || container.start(attach)).await
    }

    /// 异步停止容器
    pub async fn stop_async(
        &self,
        signal: WslcSignal,
        timeout_seconds: u32,
    ) -> Result<(), WslcError> {
        let container = self.clone();
        run_blocking("异步停止容器", move || {
            container.stop(signal, timeout_seconds)
        })
        .await
    }

    /// 异步删除容器
    pub async fn delete_async(&self, force: bool) -> Result<(), WslcError> {
        let container = self.clone();
        run_blocking("异步删除容器", move || container.delete(force)).await
    }

    /// 异步获取容器 JSON 检查快照
    pub async fn inspect_async(&self) -> Result<serde_json::Value, WslcError> {
        let container = self.clone();
        run_blocking("异步检查容器", move || container.inspect()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent, SetEvent};

    fn current_thread_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("创建测试用 Tokio 运行时失败")
    }

    #[test]
    fn test_event_wait_timeout_signal_and_cancellation() {
        let rt = current_thread_runtime();

        rt.block_on(async {
            // 创建一个手动重置、初始未激活的事件
            // SAFETY: Win32 API 调用，传入的句柄与缓冲区均为栈上有效内存，
            // 长度参数与实际可读长度一致。
            let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
            assert!(!event.is_null());

            // 1. 超时场景：事件未激活，预期返回 Ok(false)
            let timeout_res = wait_win32_event_async(event, 30).await;
            assert!(!timeout_res.expect("等待超时场景不应失败"));

            // 2. 信号唤醒场景：派生任务在 20ms 后触发事件
            let ev_clone = event as usize;
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                // SAFETY: 句柄由本测试经 CreateEventW 自行创建并在同作用域释放，
                // 不属于 SDK 托管资源。
                unsafe {
                    SetEvent(ev_clone as _);
                }
            });
            let signal_res = wait_win32_event_async(event, 1000).await;
            assert!(signal_res.expect("等待信号场景不应失败"));

            // 3. 取消安全性：Future 被 select! 中断后不得执行到完成分支
            // SAFETY: 句柄由本测试经 CreateEventW 自行创建并在同作用域释放，
            // 不属于 SDK 托管资源。
            unsafe {
                ResetEvent(event);
            }
            tokio::select! {
                _ = wait_win32_event_async(event, 5000) => {
                    panic!("等待不应在超时前完成");
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(20)) => {}
            }

            // SAFETY: 句柄由本测试经 CreateEventW 自行创建并在同作用域释放，
            // 不属于 SDK 托管资源。
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(event);
            }
        });
    }

    /// 等待类 future 必须可用于 `tokio::spawn`
    ///
    /// 缺陷成因：`WaitGuard` 曾持有 `HANDLE` 与 `*mut EventWaitShared` 两个
    /// 裸指针字段，而该守卫跨 `rx.await` 存活，使整个 future 失去 `Send`。
    /// 这属遗漏而非设计取舍——同文件的 `pull_image_with_progress_async`
    /// 明确标注了 `+ Send + 'static`，作者有意识地保证了 Send。
    ///
    /// 本用例的**编译通过**即是断言：一旦有人把裸指针字段加回来，此处即报
    /// E0277，无需任何运行时行为即可挡住回归。
    #[test]
    fn test_wait_future_is_send() {
        fn assert_send<T: Send>(_: &T) {}
        let fut = wait_win32_event_async(std::ptr::null_mut(), 1);
        assert_send(&fut);
        // 只验证类型约束，不实际 poll（空句柄会在同步段即返回错误）
        drop(fut);
    }

    #[test]
    fn test_null_event_is_rejected() {
        let rt = current_thread_runtime();
        rt.block_on(async {
            let res = wait_win32_event_async(std::ptr::null_mut(), 10).await;
            assert_eq!(res.unwrap_err(), WslcError::InvalidHandle);
        });
    }
}
