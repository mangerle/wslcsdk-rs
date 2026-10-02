//! WSLC 会话句柄与崩溃转储订阅的RAII 生命周期管理

use crate::com::HandleMtaLease;
use crate::error::WslcError;
use core::ffi::c_void;
use std::os::windows::raw::HANDLE;
use std::sync::Arc;
use windows_sys::Win32::Foundation::{GetLastError, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::WaitForSingleObject;
use wslcsdk_sys::types::{
    WslcCrashDumpSubscription, WslcSession, WslcSessionCrashDumpInfo, WslcSessionTerminationReason,
};
use wslcsdk_sys::*;

#[derive(Debug)]
struct SessionInner {
    raw: WslcSession,
    name: String,
    /// 进程级 MTA 租约
    ///
    /// 会话句柄是 RPC 通道上的对象，其有效性绑定于建立时的 COM 套间。若在析构前
    /// 套间已被注销，`WslcReleaseSession` 将崩溃。此租约确保进程级 MTA 的存活期
    /// 完整覆盖句柄的生命周期。
    ///
    /// 字段以下划线开头：它不参与任何逻辑，仅通过 `Drop` 维持套间存活。
    _mta: HandleMtaLease,
}

impl Drop for SessionInner {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            log::debug!("释放 WSLC 会话句柄，会话名称: '{}'", self.name);
            // 无需在此重建 COM 套间：_mta 租约保证进程级 MTA 此刻仍然存活，
            // 而句柄的 RPC 绑定自建立起从未断开过
            // SAFETY: 会话句柄由本类型独占持有，此处已判空，
            // 且释放由 Arc 引用计数保证恰好执行一次。
            unsafe {
                let _ = WslcReleaseSession(self.raw);
            }
            self.raw = WslcSession::NULL;
        }
    }
}

// SAFETY: `raw` 为官方 `WslcCreateSession` 返回的不透明会话句柄，官方未标注线程
// 限制；WSLC 全链路基于 COM/RPC，句柄本身是可自由传递的 COM 引用。其余字段
// （`String` 与 `HandleMtaLease`）均为 `Send`。跨线程安全由 `_mta` 租约保证：
// 它维持进程级 MTA 存活，而句柄的 RPC 绑定在套间存活期间始终有效，
// 故在任意线程上释放都不会崩溃。
unsafe impl Send for SessionInner {}

// SAFETY: 会话句柄可跨线程共享使用，且本类型不提供任何会修改内部可变状态的
// `&self` 方法：`name()` 与 `as_raw()` 均为只读访问。句柄的释放仅发生在
// `Drop` 中，经由 `Arc` 引用计数保证恰好执行一次。
unsafe impl Sync for SessionInner {}

/// 安全的 WSLC 会话 RAII 包装对象 (内部通过 Arc 托管生命周期，支持轻量安全 Clone)
#[derive(Clone, Debug)]
pub struct WslcSessionHandle {
    inner: Arc<SessionInner>,
}

impl WslcSessionHandle {
    /// 接管一个由外部取得的原始会话句柄，构造具备 RAII 所有权语义的包装对象
    ///
    /// 本方法在无法取得进程级 MTA 租约时会降级：句柄仍被构造成功，但其存活期内
    /// 失去套间保障，**析构时可能崩溃**。该降级是为维持既有签名而作的让步，
    /// 绝大多数调用方应改用 [`try_from_raw`](Self::try_from_raw) 以显式处理失败。
    ///
    /// # Safety
    ///
    /// 调用方必须保证：
    /// - `raw` 是一个由 `WslcCreateSession` 成功返回、且**所有权已完整移交**给
    ///   本对象的有效 `WslcSession` 句柄；
    /// - 该句柄未在其他任何位置被包装为拥有所有权的类型，亦不会被再次交由
    ///   `WslcReleaseSession` 释放。
    ///
    /// 违反上述任一条件都会导致句柄被重复释放或释放无效指针，构成未定义行为。
    /// 常规场景请改用 [`SessionBuilder::build`](crate::SessionBuilder::build)
    /// 由SDK 自行创建会话。
    pub unsafe fn from_raw(raw: WslcSession, name: impl Into<String>) -> Self {
        // 租约获取失败时退化为无套间保障的句柄，以维持 from_raw 的既有签名；
        // 该状态下的析构风险已在文档中说明
        let _mta = HandleMtaLease::acquire().unwrap_or_else(|e| {
            log::error!("为外部会话句柄获取 MTA 租约失败，析构时可能崩溃: {e}");
            HandleMtaLease::degraded()
        });
        Self {
            inner: Arc::new(SessionInner {
                raw,
                name: name.into(),
                _mta,
            }),
        }
    }

    /// 与 [`from_raw`](Self::from_raw) 相同，但在无法取得进程级 MTA 租约时
    /// 返回错误而非降级
    ///
    /// 相比 `from_raw`，本方法不构造任何已知有风险的包装对象：要么成功返回一个
    /// 套间保障完整的句柄，要么明确告知失败原因。这符合「最小惊讶原则」——
    /// 调用方不会在毫不知情的情况下拿到一个析构即崩溃的对象。
    ///
    /// # Safety
    ///
    /// 约束与 [`from_raw`](Self::from_raw) 完全相同：`raw` 必须是所有权已完整移交
    /// 给本对象、且不会被再次释放的有效句柄。
    pub unsafe fn try_from_raw(
        raw: WslcSession,
        name: impl Into<String>,
    ) -> Result<Self, WslcError> {
        let _mta = HandleMtaLease::acquire().map_err(|e| {
            WslcError::InvalidConfiguration(format!(
                "为外部会话句柄获取 MTA 租约失败，该句柄将不受套间保障: {e}"
            ))
        })?;
        Ok(Self {
            inner: Arc::new(SessionInner {
                raw,
                name: name.into(),
                _mta,
            }),
        })
    }

    /// 由建造者创建出的会话句柄
    ///
    /// 与 [`from_raw`](Self::from_raw) 的区别是租约已由调用方取得：
    /// [`SessionBuilder::build`](crate::SessionBuilder::build) 必须在创建句柄
    /// **之前**获取租约，以便在获取失败时
    /// 及时回收裸句柄，故租约的获取时机无法收敛到本构造器内部。
    pub(crate) fn from_acquired_lease(raw: WslcSession, name: String, mta: HandleMtaLease) -> Self {
        Self {
            inner: Arc::new(SessionInner {
                raw,
                name,
                _mta: mta,
            }),
        }
    }

    /// 获取内部原始句柄
    pub fn as_raw(&self) -> WslcSession {
        self.inner.raw
    }

    /// 获取会话名称
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// 获取会话终止通知事件句柄
    ///
    /// # 所有权与生命周期
    /// 返回的 Win32 事件句柄（`HANDLE`）由底层 WSLC 会话管理生命周期，属于只读借用。
    /// 调用方可使用 `WaitForSingleObject` 等 Win32 同步原语监听该事件，
    /// 但**严禁**由调用方直接调用 `CloseHandle` 关闭该句柄，否则会导致 SDK 内部状态破坏。
    pub fn termination_event(&self) -> Result<HANDLE, WslcError> {
        let mut event: HANDLE = std::ptr::null_mut();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcGetSessionTerminationEvent(self.inner.raw, &mut event) };
        WslcError::check_hr(hr, "获取会话终止事件句柄失败")?;
        Ok(event)
    }

    /// 同步阻塞等待会话终止通知
    ///
    /// # 参数
    /// - `timeout_ms`: 等待超时毫秒数；若为 `u32::MAX`（即 `INFINITE`），则无限期等待。
    ///
    /// # 返回值
    /// - `Ok(true)`: 会话已终止。
    /// - `Ok(false)`: 等待超时，会话仍在运行。
    /// - `Err(...)`: 等待失败或底层 Windows API 异常。
    ///
    /// # Errors
    /// 当底层事件等待返回失败状态时返回对应的 `WslcError`。
    pub fn wait_termination(&self, timeout_ms: u32) -> Result<bool, WslcError> {
        let event = self.termination_event()?;
        // SAFETY: 事件句柄由官方 SDK 托管且在等待期间保持有效，
        // 该调用不要求调用方具备任何特殊权限。
        let wait_res = unsafe { WaitForSingleObject(event, timeout_ms) };
        match wait_res {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            WAIT_FAILED => {
                // SAFETY: 无参数无副作用，仅紧随失败的 Win32 调用读取错误码。
                let err = unsafe { GetLastError() };
                Err(WslcError::Hresult(err, "等待会话终止事件失败".to_string()))
            }
            other => Err(WslcError::Hresult(
                other,
                format!("等待会话终止返回非预期状态: {other}"),
            )),
        }
    }

    /// 异步等待会话终止 (基于 Win32 线程池事件驱动，0 轮询且具备 Tokio 取消安全性)
    ///
    /// # 参数
    /// - `timeout_ms`: 等待超时毫秒数；若为 `u32::MAX`（即 `INFINITE`），则无限期等待。
    ///
    /// # 返回值
    /// - `Ok(true)`: 会话已终止。
    /// - `Ok(false)`: 等待超时，会话仍在运行。
    /// - `Err(...)`: 等待失败或底层 Windows API 异常。
    pub async fn wait_termination_async(&self, timeout_ms: u32) -> Result<bool, WslcError> {
        if self.inner.raw.is_null() {
            return Err(WslcError::InvalidHandle);
        }
        let event = self.termination_event()?;
        crate::async_ops::wait_win32_event_async(event, timeout_ms).await
    }

    /// 获取会话终止的具体原因
    pub fn termination_reason(&self) -> Result<WslcSessionTerminationReason, WslcError> {
        let mut reason = WslcSessionTerminationReason::Unknown;
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcGetSessionTerminationReason(self.inner.raw, &mut reason) };
        WslcError::check_hr(hr, "获取会话终止原因失败")?;
        Ok(reason)
    }

    /// 强制终止当前会话
    pub fn terminate(&self) -> Result<(), WslcError> {
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcTerminateSession(self.inner.raw) };
        WslcError::check_hr(hr, "终止会话失败")?;
        log::info!("WSLC 会话已被主动终止，名称: '{}'", self.name());
        Ok(())
    }

    /// 注册 Linux 进程崩溃转储监控回调
    ///
    /// # 并发契约
    ///
    /// 回调上下文为 `Box<Mutex<F>>`：官方可能在多个线程间并发派发崩溃事件，
    /// 而本方法只要求 `F: Send`（而非 `Sync`），故闭包内部状态无法假定
    /// 只被单线程访问。以 `Mutex` 包裹后，即便官方并发回调，
    /// 对闭包的调用也严格串行化。
    ///
    /// 这与本库另两处回调上下文（镜像拉取进度、组件安装进度）采用
    /// `Box<Mutex<F>>` 的做法保持一致——三处回调的防护等级不应有别。
    pub fn register_crash_dump_callback<F>(
        &self,
        callback: F,
    ) -> Result<CrashDumpSubscription, WslcError>
    where
        F: Fn(&WslcSessionCrashDumpInfo) + Send + 'static,
    {
        let boxed_cb = Box::new(std::sync::Mutex::new(callback));
        let ctx = Box::into_raw(boxed_cb);

        let mut sub = WslcCrashDumpSubscription::NULL;
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe {
            WslcRegisterSessionCrashDumpCallback(
                self.inner.raw,
                Some(crash_trampoline::<F>),
                ctx as *mut c_void,
                &mut sub,
                &mut err_msg,
            )
        };

        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        if let Err(e) = unsafe { WslcError::check(hr, err_msg) } {
            // SAFETY: 指针由 Box::into_raw 移交所有权，本处为唯一回收路径，
            // 不会发生重复释放。
            let _ = unsafe { Box::from_raw(ctx) };
            return Err(e);
        }

        Ok(CrashDumpSubscription {
            _session: self.clone(),
            raw: sub,
            ctx: ctx as *mut c_void,
            drop_fn: drop_ctx::<F>,
        })
    }
}

/// 崩溃转储事件的 C 回调跳板
///
/// 官方可能在多个线程间并发派发崩溃事件，故上下文以 `Box<Mutex<F>>` 承接：
/// 本方法只要求 `F: Send`（而非 `Sync`），闭包内部状态无法假定单线程访问，
/// 必须由`Mutex` 串行化对闭包的调用。
unsafe extern "system" fn crash_trampoline<F>(
    info: *const WslcSessionCrashDumpInfo,
    context: *mut c_void,
) where
    F: Fn(&WslcSessionCrashDumpInfo),
{
    if !context.is_null() && !info.is_null() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // SAFETY: 官方保证回调期间 info 指向有效的结构体，
            // 借用不跨越回调边界。
            let info_ref = unsafe { &*info };
            log::warn!(
                "检测到会话 Linux 进程崩溃转储，PID: {}，信号: {}",
                info_ref.pid,
                info_ref.signal
            );
            // SAFETY: 上下文指针由本库在注册时从 Box<Mutex<F>> 拆出并原样回传，
            // 类型与注册时的 F 严格一致；官方契约保证回调期间该内存始终有效。
            let mutex = unsafe { &*(context as *const std::sync::Mutex<F>) };
            // 官方要求回调尽快返回，故此处只取锁执行而不做任何阻塞等待；
            // 若闭包自身 panic，已由外层 catch_unwind 兜住，不会 unwind 穿过 C栈帧
            if let Ok(callback) = mutex.lock() {
                callback(info_ref);
            }
        }));
    }
}

/// 类型擦除后的上下文释放函数
///
/// `F` 必须与注册时移交的 `Box<Mutex<F>>` 严格一致，否则按错误类型回收
/// 将导致内存损坏。
unsafe fn drop_ctx<F>(context: *mut c_void) {
    if !context.is_null() {
        // SAFETY: 指针由 Box::into_raw 移交所有权，本处为唯一回收路径，
        // 不会发生重复释放；类型 F 与注册时一致。
        let _ = unsafe { Box::from_raw(context as *mut std::sync::Mutex<F>) };
    }
}

/// 崩溃转储回调订阅的 RAII 守卫
///
/// 内部持有会话句柄克隆，确保所属会话与 MTA 租约存活期覆盖本订阅生命周期。
/// 析构时先向 SDK 注销订阅，确保 SDK 不会在注销后再次触发回调，随后才释放
/// 注册时移交的 Rust 闭包内存，彻底规避 UAF 竞态。
pub struct CrashDumpSubscription {
    _session: WslcSessionHandle,
    /// 官方不透明订阅句柄
    raw: WslcCrashDumpSubscription,
    /// 指向注册时移交的 `Box<Mutex<F>>`，仅在注销完成后释放
    ctx: *mut c_void,
    /// 类型擦除后的释放函数，用以按原类型回收闭包内存
    drop_fn: unsafe fn(*mut c_void),
}

impl std::fmt::Debug for CrashDumpSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrashDumpSubscription")
            .field("raw", &self.raw)
            .field("session", &self._session.name())
            // 上下文指针仅记录是否已注册，不打印其数值，避免泄漏堆地址
            .field("has_context", &!self.ctx.is_null())
            .finish()
    }
}

impl Drop for CrashDumpSubscription {
    fn drop(&mut self) {
        // 先调用 SDK 注销订阅，确保 SDK 不会在析构期间或之后再次触发回调
        if !self.raw.is_null() {
            // SAFETY: 订阅句柄由本类型独占持有，此处已判空；
            // 官方要求先注销订阅再释放回调上下文，此顺序已严格遵守。
            unsafe {
                let _ = WslcReleaseCrashDumpSubscription(self.raw);
            }
            self.raw = WslcCrashDumpSubscription::NULL;
        }
        // 注销完成后再安全释放 Rust 闭包内存，彻底规避 UAF 竞态
        if !self.ctx.is_null() {
            // SAFETY: drop_fn 是与 ctx 原始类型匹配的擦除释放函数，
            // 且此处已判空并在其后置空，保证恰好调用一次。
            unsafe {
                (self.drop_fn)(self.ctx);
            }
            self.ctx = std::ptr::null_mut();
        }
    }
}

// SAFETY: `raw` 为官方不透明订阅句柄，`ctx` 为指向 `Box<Mutex<F>>` 的裸指针。
// 底层回调要求 `F: Send`，`Mutex<F>` 在 `F: Send` 时亦为 `Send`，
// 且注册时已通过 `Box::into_raw` 移交所有权，故该指针
// 可安全地随本类型跨线程移动。
unsafe impl Send for CrashDumpSubscription {}

// SAFETY: 本类型的 `&self` 方法均不修改内部状态；`ctx` 指针本身不会被读取，
// 仅在 `Drop` 中经由 `drop_fn` 消费，而 `Drop` 需要独占 `&mut self`。
// 潜在的并发风险由官方契约保证：调用方必须先 `WslcReleaseCrashDumpSubscription`
// 注销订阅，SDK 才会停止触发回调，本库严格遵循该顺序。
unsafe impl Sync for CrashDumpSubscription {}
