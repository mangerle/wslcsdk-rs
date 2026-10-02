//! WSLC 容器句柄与生命周期控制
//!
//! 提供面向对象的容器生命周期管理，基于 RAII 实现资源的确定性释放。

use crate::com::HandleMtaLease;
use crate::com_memory::ComAnsiString;
use crate::error::WslcError;
use crate::process::{ProcessStreams, StreamState, WslcProcessHandle, setup_streaming_channels};
use crate::session::WslcSessionHandle;
use std::ffi::{CStr, CString};
use std::sync::{Arc, Mutex};
use wslcsdk_sys::types::{
    WSLC_CONTAINER_ID_BUFFER_SIZE, WslcContainer, WslcContainerStartFlags, WslcContainerState,
    WslcDeleteContainerFlags, WslcProcess, WslcSignal,
};
use wslcsdk_sys::{
    WslcDeleteContainer, WslcGetContainerID, WslcGetContainerInitProcess, WslcGetContainerState,
    WslcInspectContainer, WslcOpenContainer, WslcReleaseContainer, WslcReleaseProcess,
    WslcSetContainerInitProcessIOCallbacks, WslcStartContainer, WslcStopContainer,
};

/// init 进程句柄的获取状态
///
/// 官方标准 API 规范确证：`WslcGetContainerInitProcess` 返回的句柄归调用方拥有，需调用
/// `WslcReleaseProcess` 释放。本库采取由容器集中持有并在容器析构时释放至多一次的
/// 状态机缓存借用设计，对上层派生的包装对象仅暴露借用视图而不重复释放，既杜绝句柄泄漏，
/// 又避免重复索取产生多份未纳管句柄。
///
/// 以单一枚举按值表达，使「尚未索取 / 已借用」两种状态在类型层面清晰可辨，
/// 杜绝布尔标志位带来的非法状态组合。
#[derive(Debug)]
enum InitProcessSource {
    /// 尚未向 SDK 索取过 init 进程句柄
    Unfetched,
    /// 来自 `WslcGetContainerInitProcess` 的查询型句柄
    ///
    /// 包装对象仅借用，不承担释放责任；重复调用 [`WslcContainerHandle::get_init_process`]
    /// 会复用同一底层句柄，故任意次调用都不会触发重复释放。
    Borrowed(WslcProcess),
}

/// 容器内部资源托管结构体
///
/// 封装底层 C SDK 的 `WslcContainer` 句柄，并在析构时调用 `WslcReleaseContainer`。
#[derive(Debug)]
struct ContainerInner {
    raw: WslcContainer,
    _session: WslcSessionHandle,
    /// 进程级 MTA 租约
    ///
    /// 容器句柄同样是 RPC 通道上的对象，其有效性绑定于建立时的 COM 套间。
    /// 虽已有 `_session` 间接持有会话的租约，但那份租约的类型为
    /// `WslcSessionHandle` 的内部实现，语义上依附于会话而非容器；显式持有
    /// 自己的一份可使租约与句柄的对应关系在代码中自明。
    _mta: HandleMtaLease,
    /// init 进程的流式 I/O 状态
    ///
    /// 既可在构造时由 `ProcessBuilder::with_streaming_io` 预置，也可事后经
    /// `with_init_process_io_callbacks` 注册。以 `Mutex` 承载是为了让「已注册回调」
    /// 这一事实成为容器自身的状态，从而在 `get_init_process` 派生的句柄上正确传递，
    /// 使 IO 句柄与回调的互斥校验得以生效。
    init_stream_state: Mutex<Option<Arc<StreamState>>>,
    /// 已向 SDK 索取的 init 进程句柄
    ///
    /// 容器仅缓存原始句柄并保证其存活期，`WslcProcessHandle` 只借用不释放，
    /// 以此避免「容器缓存进程包装对象、包装对象又反向强引用容器」构成的 `Arc`
    /// 循环——该循环会使容器与进程句柄双双永久泄漏，两者的释放函数永不执行。
    init_process_raw: Mutex<InitProcessSource>,
}

impl Drop for ContainerInner {
    fn drop(&mut self) {
        // 无需在此重建 COM 套间：_mta 租约保证进程级 MTA 此刻仍然存活
        // 释放已索取的 init 进程句柄至多一次，随后清零槽位；再释放容器句柄，保证层级析构顺序
        let mut slot = self
            .init_process_raw
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let InitProcessSource::Borrowed(proc_raw) = *slot
            && !proc_raw.is_null()
        {
            // SAFETY: 该句柄来自 WslcGetContainerInitProcess 且由容器统一托管，
            // 在容器析构时恰好释放至多一次，杜绝句柄泄漏。
            unsafe {
                let _ = WslcReleaseProcess(proc_raw);
            }
        }
        *slot = InitProcessSource::Unfetched;
        drop(slot);

        if !self.raw.is_null() {
            // SAFETY: 容器句柄由本类型独占持有，此处已判空，
            // 且释放由 Arc 引用计数保证恰好执行一次。
            unsafe {
                let _ = WslcReleaseContainer(self.raw);
            }
            self.raw = WslcContainer::NULL;
        }
    }
}

// SAFETY: `raw` 为官方不透明容器句柄（`WslcOpenContainer` / `WslcCreateContainer`
// 返回），可跨线程传递；`_session` 为 `WslcSessionHandle`，已单独论证其 `Send`。
// `init_stream_state` 为 `Arc<StreamState>`，内部仅含通道发送端与原子计数器，
// 均为 `Send + Sync`；`init_process_raw` 内的句柄仅在 `Drop` 中于独占访问下读取。
unsafe impl Send for ContainerInner {}

// SAFETY: 全部字段均为 `Sync`，且不提供任何 `&self` 可变操作。`init_process_raw`
// 以 `Mutex` 保护，其「至多索取一次」的原子性依赖该锁而非外部同步，故并发
// 调用 `get_init_process` 是安全的。
unsafe impl Sync for ContainerInner {}

/// 安全的 WSLC 容器 RAII 包装句柄
///
/// 内部通过 `Arc` 托管底层资源，支持轻量安全克隆并在所有引用释放时安全回收容器句柄。
#[derive(Clone, Debug)]
pub struct WslcContainerHandle {
    inner: Arc<ContainerInner>,
}

impl WslcContainerHandle {
    /// 从原始底层容器句柄与会话包装构建容器句柄对象
    pub(crate) fn from_raw_inner(
        raw: WslcContainer,
        session: WslcSessionHandle,
        init_stream_state: Option<Arc<StreamState>>,
    ) -> Result<Self, WslcError> {
        // 租约须在句柄结构体构造**之前**取得：若把 `acquire()?` 写在结构体
        // 字面量内部，租约获取失败时 `?` 会提前返回，而 `raw` 已由官方成功
        // 创建却无人接管，`WslcReleaseContainer` 永不执行，句柄泄漏。
        let mta = match HandleMtaLease::acquire() {
            Ok(lease) => lease,
            Err(e) => {
                // 失败路径上句柄所有权尚未移交本对象，由本处负责回收
                // SAFETY: `raw` 由本次 `WslcOpenContainer` / `WslcCreateContainer`
                // 成功返回，且已判空，错误路径下无其他持有者，释放恰好执行一次。
                unsafe {
                    let _ = WslcReleaseContainer(raw);
                }
                return Err(e);
            }
        };

        Ok(Self {
            inner: Arc::new(ContainerInner {
                raw,
                _session: session,
                _mta: mta,
                init_stream_state: Mutex::new(init_stream_state),
                init_process_raw: Mutex::new(InitProcessSource::Unfetched),
            }),
        })
    }

    /// 通过名称或容器 ID 打开已存在的容器
    pub fn open(session: &WslcSessionHandle, name_or_id: &str) -> Result<Self, WslcError> {
        // 句柄创建路径**必须**建立线程级 COM 套间：官方 SDK 在
        // WslcCreateSession / WslcOpenContainer / WslcCreateContainerProcess
        // 内部会回调至本进程，未初始化套间时返回 CO_E_NOTINITIALIZED。
        // 此处不可依赖句柄自带的进程级租约——那只保证已建立句柄的 RPC 绑定
        // 有效，不提供调用入口所需的套间上下文。
        let _com_guard = crate::com::try_initialize_mta()?;
        let c_str = CString::new(name_or_id)
            .map_err(|e| WslcError::NulError(format!("名称或 ID 包含非法空字节: {e}")))?;

        let mut raw = WslcContainer::NULL;
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        let hr =
            // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
            // 出参为合法的可写指针，不涉及未定义行为。
            unsafe { WslcOpenContainer(session.as_raw(), c_str.as_ptr(), &mut raw, &mut err_msg) };

        // SAFETY: err_msg 为官方 _Outptr_opt_result_z_ 输出参数，
        // 其所有权在本行交由 RAII 包装接管并自动释放。
        unsafe {
            WslcError::check(hr, err_msg)?;
        }

        if raw.is_null() {
            return Err(WslcError::InvalidHandle);
        }

        Self::from_raw_inner(raw, session.clone(), None)
    }

    /// 获取该容器所属的会话句柄克隆
    pub fn session(&self) -> WslcSessionHandle {
        self.inner._session.clone()
    }

    /// 获取内部原始句柄
    pub fn as_raw(&self) -> WslcContainer {
        self.inner.raw
    }

    /// 获取容器 64 位十六进制唯一哈希 ID
    pub fn id(&self) -> Result<String, WslcError> {
        let mut buffer = [0i8; WSLC_CONTAINER_ID_BUFFER_SIZE];
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcGetContainerID(self.inner.raw, buffer.as_mut_ptr()) };
        WslcError::check_hr(hr, "获取容器 ID 失败")?;

        // SAFETY: 指针非空且指向以 NUL 结尾的字符串，
        // 该不变量由官方 _z_ 参数标注保证。
        let c_str = unsafe { CStr::from_ptr(buffer.as_ptr()) };
        c_str
            .to_str()
            .map(|s| s.to_string())
            .map_err(|e| WslcError::Utf8Error(e.to_string()))
    }

    /// 获取容器当前运行状态
    pub fn state(&self) -> Result<WslcContainerState, WslcError> {
        let mut state = WslcContainerState::INVALID;
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcGetContainerState(self.inner.raw, &mut state) };
        WslcError::check_hr(hr, "获取容器状态失败")?;
        Ok(state)
    }

    /// 启动容器
    pub fn start(&self, attach: bool) -> Result<(), WslcError> {
        let flags: WslcContainerStartFlags = if attach {
            wslcsdk_sys::types::WSLC_CONTAINER_START_FLAG_ATTACH
        } else {
            wslcsdk_sys::types::WSLC_CONTAINER_START_FLAG_NONE
        };

        let mut err_msg: *mut u16 = std::ptr::null_mut();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcStartContainer(self.inner.raw, flags, &mut err_msg) };
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        unsafe { WslcError::check(hr, err_msg) }?;
        log::info!("WSLC 容器启动成功，attach 模式: {attach}");
        Ok(())
    }

    /// 停止容器
    pub fn stop(&self, signal: WslcSignal, timeout_seconds: u32) -> Result<(), WslcError> {
        let mut err_msg: *mut u16 = std::ptr::null_mut();
        let hr =
            // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
            // 出参为合法的可写指针，不涉及未定义行为。
            unsafe { WslcStopContainer(self.inner.raw, signal, timeout_seconds, &mut err_msg) };
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        unsafe { WslcError::check(hr, err_msg) }?;
        log::info!("WSLC 容器停止成功，信号: {signal:?}，超时: {timeout_seconds} 秒");
        Ok(())
    }

    /// 删除容器
    pub fn delete(&self, force: bool) -> Result<(), WslcError> {
        let flags: WslcDeleteContainerFlags = if force {
            wslcsdk_sys::types::WSLC_DELETE_CONTAINER_FLAG_FORCE
        } else {
            wslcsdk_sys::types::WSLC_DELETE_CONTAINER_FLAG_NONE
        };

        let mut err_msg: *mut u16 = std::ptr::null_mut();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcDeleteContainer(self.inner.raw, flags, &mut err_msg) };
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        unsafe { WslcError::check(hr, err_msg) }?;
        log::info!("WSLC 容器删除成功，强制标志: {force}");
        Ok(())
    }

    /// 获取容器检查数据 (JSON 快照)
    pub fn inspect(&self) -> Result<serde_json::Value, WslcError> {
        let mut inspect_ptr: *mut i8 = std::ptr::null_mut();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcInspectContainer(self.inner.raw, &mut inspect_ptr) };
        if hr < 0 || inspect_ptr.is_null() {
            return Err(WslcError::from_hresult(hr, "检查容器元数据失败"));
        }

        // SAFETY: inspect_ptr 为官方 _Outptr_result_z_ 输出参数，所有权移交本侧
        let json = unsafe { ComAnsiString::from_raw(inspect_ptr) }
            .expect("前序判定已确保 hr 成功时指针非空");

        // 显式处理非 UTF-8：静默降级为空串会让 JSON 解析报出「语法错误」，
        // 把问题指向错误的方位，掩盖真正的根因
        let text = json.as_str().map_err(|e| {
            WslcError::JsonError(format!(
                "容器检查数据非合法 UTF-8（官方返回的字节序列含非法编码）: {e}"
            ))
        })?;

        serde_json::from_str(text).map_err(|e| WslcError::JsonError(e.to_string()))
    }

    /// 获取容器的 init 主进程句柄
    ///
    /// 本方法幂等：对同一容器至多向 SDK 索取一次 `WslcProcess`，重复调用复用同一
    /// 底层句柄。返回的包装对象仅借用该句柄，其所有权由容器统一持有并在容器析构
    /// 时释放至多一次，因此调用多少次都不会触发重复释放。
    pub fn get_init_process(&self) -> Result<WslcProcessHandle, WslcError> {
        // 临界区仅覆盖句柄的读取与首次索取，避免持锁执行后续逻辑
        let raw_process = {
            let mut slot = self
                .inner
                .init_process_raw
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            match &*slot {
                InitProcessSource::Borrowed(raw) => *raw,
                InitProcessSource::Unfetched => {
                    let mut raw = WslcProcess::NULL;
                    // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
                    // 出参为合法的可写指针，不涉及未定义行为。
                    let hr = unsafe { WslcGetContainerInitProcess(self.inner.raw, &mut raw) };
                    if hr < 0 || raw.is_null() {
                        return Err(WslcError::from_hresult(hr, "获取容器主进程句柄失败"));
                    }
                    *slot = InitProcessSource::Borrowed(raw);
                    raw
                }
            }
        };

        let stream_state = self
            .inner
            .init_stream_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();

        Ok(WslcProcessHandle::from_borrowed(
            raw_process,
            self.clone(),
            stream_state,
        ))
    }

    /// 为容器 init 主进程注册流式标准 IO 回调 (标准输出与标准错误)
    ///
    /// 适用于未显式指定 init 进程、直接沿用镜像默认 CMD 的容器。回调上下文由库内部
    /// 的全局弱引用注册表托管，调用方无需手工管理任何裸指针生命周期。
    ///
    /// # 调用时机
    ///
    /// 官方要求本接口必须在 [`WslcContainerHandle::start`] 之前、且配合
    /// `attach` 模式调用；对已运行的容器调用不会产生任何效果。
    ///
    /// # 与 IO 句柄的互斥性
    ///
    /// 官方明确规定：一旦注册任何 IO 回调，相应的 IO 句柄即被消耗，无法再通过
    /// [`WslcProcessHandle::io_handle`] 获取。本库据此在运行时拒绝该组合，
    /// 避免向调用方返回一个已被消耗的无效句柄。
    ///
    /// # 参数
    ///
    /// - `capacity`: 标准输出与标准错误各自的有界通道容量；为 0 时按 1 处理。
    ///
    /// # 生命周期
    ///
    /// 返回的 [`ProcessStreams`] 持有回调上下文的强引用。只要它与容器句柄均未释放，
    /// 回调即可安全地把字节流传入通道；二者全部释放后注册表条目自动移除，
    /// 此后即便 C 侧仍触发回调也会被安全忽略。
    pub fn with_init_process_io_callbacks(
        &self,
        capacity: usize,
    ) -> Result<ProcessStreams, WslcError> {
        if self.inner.raw.is_null() {
            return Err(WslcError::InvalidHandle);
        }

        let (callbacks, stream_id, state, streams) = setup_streaming_channels(capacity);
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe {
            WslcSetContainerInitProcessIOCallbacks(
                self.inner.raw,
                &callbacks,
                stream_id as *mut core::ffi::c_void,
            )
        };
        WslcError::check_hr(hr, "设置主进程 IO 回调失败")?;

        // 记录到容器自身，使后续派生的进程句柄能够感知「IO 句柄已被消耗」
        *self
            .inner
            .init_stream_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(state);

        Ok(streams)
    }
}
