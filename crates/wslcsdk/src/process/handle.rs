//! WSLC 容器进程句柄与生命周期控制
//!
//! 提供面向对象的子进程管理接口，封装进程状态查询、POSIX 信号发送与标准 I/O 句柄获取。

use super::stream::StreamState;
use crate::com::HandleMtaLease;
use crate::container::WslcContainerHandle;
use crate::error::WslcError;
use std::os::windows::raw::HANDLE;
use std::sync::Arc;
use windows_sys::Win32::Foundation::{WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::WaitForSingleObject;
use wslcsdk_sys::types::{WslcProcess, WslcProcessIOHandle, WslcProcessState, WslcSignal};
use wslcsdk_sys::{
    WslcGetProcessExitCode, WslcGetProcessExitEvent, WslcGetProcessIOHandle, WslcGetProcessPid,
    WslcGetProcessState, WslcReleaseProcess, WslcSignalProcess,
};

/// `raw` 句柄的来源与所有权归属
///
/// 以单一枚举按值表达，使「借用他人句柄却声称拥有所有权」这类非法状态在编译期即被
/// 排除：借用态必须携带所属容器，析构时一律不释放；自有态则必须携带归属容器，
/// 由本包装对象负责释放。
#[derive(Debug)]
pub(crate) enum ProcessOwnership {
    /// 由 `WslcCreateContainerProcess` 派生，所有权已移交本包装对象
    Owned {
        /// 派生进程的容器，保证裸句柄的存活期不短于本包装对象
        container: WslcContainerHandle,
    },
    /// 来自 `WslcGetContainerInitProcess` 的句柄，由所属容器统一托管并释放
    ///
    /// 官方标准 API 规范确证该句柄归调用方拥有，本库设计由所属容器统一缓存持有
    /// 并在容器析构时释放至多一次。本包装对象在此作为借用视图，析构时不重复释放。
    /// 容器句柄同时充当存活期担保，确保 `raw` 不会先于本包装对象失效。
    Borrowed {
        /// 提供该句柄的容器，兼作存活期担保
        container: WslcContainerHandle,
    },
}

#[derive(Debug)]
pub(crate) struct ProcessInner {
    pub(crate) raw: WslcProcess,
    pub(crate) ownership: ProcessOwnership,
    pub(crate) _stream_state: Option<Arc<StreamState>>,
    /// 进程级 MTA 租约
    ///
    /// 进程句柄同样是 RPC 通道上的对象，其有效性绑定于建立时的 COM 套间。
    /// `ProcessOwnership` 中的容器字段虽已间接持有会话的租约，但那份租约
    /// 在语义上依附于容器；此处显式持有自己的一份，使「句柄—套间」的对应
    /// 关系自明，且不依赖所有权变体是否携带容器。
    _mta: HandleMtaLease,
}

impl Drop for ProcessInner {
    fn drop(&mut self) {
        // 借用态的句柄由所属容器统一释放，此处必须跳过，否则构成重复释放
        if matches!(self.ownership, ProcessOwnership::Owned { .. }) && !self.raw.is_null() {
            log::debug!("释放 WSLC 进程句柄");
            // 无需在此重建 COM 套间：_mta 租约保证进程级 MTA 此刻仍然存活
            // SAFETY: 句柄由本类型独占持有（自有态），此处已判空，
            // 且释放由 Arc 引用计数保证恰好执行一次。
            unsafe {
                let _ = WslcReleaseProcess(self.raw);
            }
            self.raw = WslcProcess::NULL;
        }
    }
}

// SAFETY: `raw` 为官方不透明进程句柄，可跨线程传递；`ownership` 枚举的两个变体
// 均携带 `WslcContainerHandle`（本身已论证 `Send`），确保裸句柄的存活期不短于
// 本类型，从而杜绝「句柄已释放而包装对象仍在使用」的悬垂；`_stream_state` 为
// `Arc<StreamState>`，内部状态均为 `Send`。
unsafe impl Send for ProcessInner {}

// SAFETY: 全部字段均为 `Sync`，且本类型不提供任何 `&self` 可变操作。
// 句柄的实际释放仅发生于 `Drop`，由 `Arc` 引用计数保证恰好执行一次；
// 并发访问底层 SDK 句柄的安全性由官方 COM 契约保证。
unsafe impl Sync for ProcessInner {}

/// 安全的 WSLC 进程句柄包装
///
/// 内部通过 `Arc` 托管生命周期，支持轻量安全 `Clone`。
///
/// 依据来源不同，析构语义分为两类：
/// - 由 [`ProcessBuilder::spawn`](crate::ProcessBuilder::spawn) 派生的进程，
///   本对象拥有底层句柄，析构时调用 `WslcReleaseProcess`；
/// - 由 [`WslcContainerHandle::get_init_process`](WslcContainerHandle::get_init_process)
///   取得的 init 进程句柄，官方规范确证归调用方拥有，本库由所属容器统一托管并在容器析构时释放至多一次，
///   本对象作为借用视图，析构时不重复释放。
#[derive(Clone, Debug)]
pub struct WslcProcessHandle {
    pub(crate) inner: Arc<ProcessInner>,
}

impl WslcProcessHandle {
    /// 接管一个由外部取得的原始进程句柄，构造具备 RAII 所有权语义的包装对象
    ///
    /// # Safety
    ///
    /// 调用方必须保证：
    /// - `raw` 是一个由 `WslcCreateContainerProcess` 成功返回、且**所有权已完整
    ///   移交**给本对象的有效 `WslcProcess` 句柄；
    /// - 该句柄未在其他任何位置被包装为拥有所有权的类型，亦不会被再次交由
    ///   `WslcReleaseProcess` 释放。
    ///
    /// 违反上述任一条件都会导致句柄被重复释放或释放无效指针，构成未定义行为。
    /// 常规场景请改用 [`ProcessBuilder::spawn`](crate::ProcessBuilder::spawn)。
    pub unsafe fn from_raw(raw: WslcProcess, container: WslcContainerHandle) -> Self {
        Self::from_raw_with_stream(raw, container, None)
    }

    /// 构造拥有 `raw` 所有权、且关联所属容器的进程包装对象
    pub(crate) fn from_raw_with_stream(
        raw: WslcProcess,
        container: WslcContainerHandle,
        stream_state: Option<Arc<StreamState>>,
    ) -> Self {
        Self::new_inner(raw, ProcessOwnership::Owned { container }, stream_state)
    }

    /// 构造仅借用 `raw`、由所属容器统一托管释放的进程包装对象
    ///
    /// 用于 `WslcGetContainerInitProcess` 取得的 init 进程句柄：官方规范确证其归调用方拥有，
    /// 本库由所属容器统一托管并在容器析构时释放至多一次，本包装对象在析构时
    /// 不会重复调用 `WslcReleaseProcess`，杜绝重复释放与句柄泄漏。
    pub(crate) fn from_borrowed(
        raw: WslcProcess,
        container: WslcContainerHandle,
        stream_state: Option<Arc<StreamState>>,
    ) -> Self {
        Self::new_inner(raw, ProcessOwnership::Borrowed { container }, stream_state)
    }

    fn new_inner(
        raw: WslcProcess,
        ownership: ProcessOwnership,
        stream_state: Option<Arc<StreamState>>,
    ) -> Self {
        // 租约保证句柄存活期内进程级 MTA 不被注销。此处刻意不做错误返回：
        // 三个调用方（spawn / from_raw / from_borrowed）中后者为 unsafe 构造器，
        // 让它们传播 Result 会改变既有签名。取不到租约时降级并记录错误，
        // 风险仅限于该进程句柄析构时可能崩溃。
        let _mta = HandleMtaLease::acquire().unwrap_or_else(|e| {
            log::error!("为进程句柄获取 MTA 租约失败，析构时可能崩溃: {e}");
            HandleMtaLease::degraded()
        });
        Self {
            inner: Arc::new(ProcessInner {
                raw,
                ownership,
                _stream_state: stream_state,
                _mta,
            }),
        }
    }

    /// 获取所属容器句柄克隆
    pub fn container(&self) -> WslcContainerHandle {
        match &self.inner.ownership {
            ProcessOwnership::Owned { container } | ProcessOwnership::Borrowed { container } => {
                container.clone()
            }
        }
    }

    /// 该进程句柄是否为借用态 (底层句柄由所属容器负责释放)
    pub fn is_borrowed(&self) -> bool {
        matches!(self.inner.ownership, ProcessOwnership::Borrowed { .. })
    }

    /// 获取内部原始句柄
    pub fn as_raw(&self) -> WslcProcess {
        self.inner.raw
    }

    /// 获取进程 PID
    pub fn pid(&self) -> Result<u32, WslcError> {
        let mut pid = 0u32;
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcGetProcessPid(self.inner.raw, &mut pid) };
        WslcError::check_hr(hr, "获取进程 PID 失败")?;
        Ok(pid)
    }

    /// 获取进程退出通知事件句柄
    ///
    /// # 所有权与生命周期
    /// 返回的 Win32 事件句柄（`HANDLE`）由底层 WSLC SDK 管理其生命周期，属于只读借用。
    /// 调用方可使用 `WaitForSingleObject` 等 Win32 同步原语监听进程退出事件，
    /// 但**严禁**由调用方直接调用 `CloseHandle`，否则将破坏 SDK 内部状态并导致双重释放。
    pub fn exit_event(&self) -> Result<HANDLE, WslcError> {
        let mut event: HANDLE = std::ptr::null_mut();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcGetProcessExitEvent(self.inner.raw, &mut event) };
        WslcError::check_hr(hr, "获取进程退出事件句柄失败")?;
        Ok(event)
    }

    /// 获取当前进程运行状态
    pub fn state(&self) -> Result<WslcProcessState, WslcError> {
        let mut state = WslcProcessState::Unknown;
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcGetProcessState(self.inner.raw, &mut state) };
        WslcError::check_hr(hr, "获取进程状态失败")?;
        Ok(state)
    }

    /// 获取进程退出码
    pub fn exit_code(&self) -> Result<i32, WslcError> {
        let mut code = 0i32;
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcGetProcessExitCode(self.inner.raw, &mut code) };
        WslcError::check_hr(hr, "获取进程退出码失败")?;
        Ok(code)
    }

    /// 向容器内进程发送 POSIX 信号
    pub fn signal(&self, sig: WslcSignal) -> Result<(), WslcError> {
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcSignalProcess(self.inner.raw, sig) };
        WslcError::check_hr(hr, "向进程发送信号失败")?;
        log::debug!("成功向容器内进程发送 POSIX 信号: {:?}", sig);
        Ok(())
    }

    /// 获取进程指定流的 Win32 文件句柄 (stdin / stdout / stderr)
    ///
    /// # 与流式回调的互斥性
    ///
    /// 官方明确规定：使用任何 IO 回调都会消耗对应的 IO 句柄，使其无法再通过本方法获取。
    /// 本库据此在运行时主动拒绝该组合并返回明确错误，避免把一个已被消耗的无效句柄
    /// 交给调用方。凡是经 [`ProcessBuilder::with_streaming_io`](crate::ProcessBuilder::with_streaming_io)
    /// 或 [`WslcContainerHandle::with_init_process_io_callbacks`](WslcContainerHandle::with_init_process_io_callbacks)
    /// 构造的进程，均视为已消耗 IO 句柄。
    ///
    /// # 所有权与生命周期
    /// 返回的 Win32 文件流句柄（`HANDLE`）由底层容器进程管理生命周期，属于借用。
    /// 调用方可用于直接读取或写入进程标准流，但**严禁**调用 `CloseHandle`，
    /// 句柄会在进程终止或 `WslcProcessHandle` 析构时由 SDK 自动清理。
    pub fn io_handle(&self, io: WslcProcessIOHandle) -> Result<HANDLE, WslcError> {
        if self.inner._stream_state.is_some() {
            return Err(WslcError::InvalidConfiguration(format!(
                "进程 {io:?} 已启用流式 IO 回调，官方规定此时无法再获取对应 IO 句柄；\
                 请改用流式通道消费输出，或在未注册回调的进程上调用本方法"
            )));
        }
        let mut handle: HANDLE = std::ptr::null_mut();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcGetProcessIOHandle(self.inner.raw, io, &mut handle) };
        if hr < 0 || handle.is_null() {
            Err(WslcError::from_hresult(hr, "获取进程标准流句柄失败"))
        } else {
            Ok(handle)
        }
    }

    /// 同步阻塞等待进程退出并获取退出码
    ///
    /// # 参数
    /// - `timeout_ms`: 等待超时毫秒数；若为 `u32::MAX`（即 `INFINITE`），则无限期等待直至退出。
    ///
    /// # 返回值
    /// - `Ok(Some(exit_code))`: 进程在超时前正常或异常退出，返回其退出码。
    /// - `Ok(None)`: 等待超时，进程仍在运行。
    /// - `Err(...)`: 等待失败或底层 Windows API 异常。
    ///
    /// # Errors
    /// 当底层事件等待返回失败状态或获取退出码失败时返回对应的 `WslcError`。
    pub fn wait(&self, timeout_ms: u32) -> Result<Option<i32>, WslcError> {
        let event = self.exit_event()?;
        // SAFETY: 事件句柄由官方 SDK 托管且在等待期间保持有效，
        // 该调用不要求调用方具备任何特殊权限。
        let wait_res = unsafe { WaitForSingleObject(event, timeout_ms) };
        match wait_res {
            WAIT_OBJECT_0 => {
                let code = self.exit_code()?;
                Ok(Some(code))
            }
            WAIT_TIMEOUT => Ok(None),
            WAIT_FAILED => Err(
                // SAFETY: 紧随失败的 WaitForSingleObject 调用，
                // 读到的错误码即属于本次失败。
                unsafe { WslcError::last_win32_error("等待进程退出事件失败") },
            ),
            other => Err(WslcError::Win32(
                other,
                format!("等待进程退出返回非预期状态: {other}"),
            )),
        }
    }

    /// 异步等待进程退出并获取退出码 (基于 Win32 线程池事件驱动，0 轮询且具备 Tokio 取消安全性)
    ///
    /// # 参数
    /// - `timeout_ms`: 等待超时毫秒数；若为 `u32::MAX`（即 `INFINITE`），则无限期等待直至退出。
    ///
    /// # 返回值
    /// - `Ok(Some(exit_code))`: 进程在超时前正常或异常退出，返回其退出码。
    /// - `Ok(None)`: 等待超时，进程仍在运行。
    /// - `Err(...)`: 等待失败或底层 Windows API 异常。
    pub async fn wait_async(&self, timeout_ms: u32) -> Result<Option<i32>, WslcError> {
        if self.inner.raw.is_null() {
            return Err(WslcError::InvalidHandle);
        }
        let event = self.exit_event()?;
        let is_signaled = crate::async_ops::wait_win32_event_async(event, timeout_ms).await?;
        if is_signaled {
            let code = self.exit_code()?;
            Ok(Some(code))
        } else {
            Ok(None)
        }
    }
}
