//! WSLC 会话生命周期与设置建造者

use crate::error::WslcError;
use core::ffi::c_void;
use std::ffi::CString;
use std::os::windows::raw::HANDLE;
use std::path::Path;
use std::sync::Arc;
use wslcsdk_sys::types::{
    WslcCrashDumpSubscription, WslcSession, WslcSessionCrashDumpInfo, WslcSessionFeatureFlags,
    WslcSessionSettings, WslcSessionTerminationReason, WslcVhdRequirements,
};
use wslcsdk_sys::*;

/// 宽字符转换辅助函数 (校验并拦截内部空字符)
pub(crate) fn to_wide_null(s: &str) -> Result<Vec<u16>, WslcError> {
    if s.contains('\0') {
        return Err(WslcError::Utf8Error("字符串包含非法空字符".to_string()));
    }
    Ok(s.encode_utf16().chain(std::iter::once(0)).collect())
}

/// 安全的 VHD 配置数据 (不含裸指针，确保 Send + Sync)
#[derive(Clone, Debug)]
pub struct VhdRequirementsData {
    pub name: Option<String>,
    pub size_bytes: u64,
    pub vhd_type: wslcsdk_sys::types::WslcVhdType,
    pub flags: wslcsdk_sys::types::WslcVhdRequirementsFlags,
    pub uid: u32,
    pub gid: u32,
}

/// 会话配置建造者
#[derive(Clone, Debug)]
pub struct SessionBuilder {
    name: String,
    storage_path: String,
    cpu_count: Option<u32>,
    memory_mb: Option<u32>,
    timeout_ms: Option<u32>,
    vhd: Option<VhdRequirementsData>,
    feature_flags: WslcSessionFeatureFlags,
}

impl SessionBuilder {
    /// 使用系统标准沙箱路径创建会话建造者
    ///
    /// 默认将 VHD 与会话元数据存放于 `%LOCALAPPDATA%\wslc\sessions\<name>`，
    /// 若环境变量未设置则降级存放在用户目录 `.wslc\sessions\<name>` 下。
    pub fn new_default(name: impl Into<String>) -> Result<Self, WslcError> {
        let name_str = name.into();
        let base_dir = std::env::var("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("USERPROFILE")
                    .map(|p| std::path::PathBuf::from(p).join(".wslc"))
                    .unwrap_or_else(|_| std::path::PathBuf::from(r"C:\wslc"))
            });
        let storage_path = base_dir.join("wslc").join("sessions").join(&name_str);
        if let Err(e) = std::fs::create_dir_all(&storage_path) {
            return Err(WslcError::Io(format!("创建默认会话存储目录失败: {e}")));
        }
        Ok(Self::new(name_str, storage_path))
    }

    pub fn new(name: impl Into<String>, storage_path: impl AsRef<Path>) -> Self {
        Self {
            name: name.into(),
            storage_path: storage_path.as_ref().to_string_lossy().to_string(),
            cpu_count: None,
            memory_mb: None,
            timeout_ms: None,
            vhd: None,
            feature_flags: 0,
        }
    }

    pub fn cpu_count(mut self, count: u32) -> Self {
        self.cpu_count = Some(count);
        self
    }

    pub fn memory_mb(mut self, mb: u32) -> Self {
        self.memory_mb = Some(mb);
        self
    }

    pub fn timeout_ms(mut self, ms: u32) -> Self {
        self.timeout_ms = Some(ms);
        self
    }

    pub fn feature_flags(mut self, flags: WslcSessionFeatureFlags) -> Self {
        self.feature_flags = flags;
        self
    }

    pub fn enable_gpu(mut self, enable: bool) -> Self {
        if enable {
            self.feature_flags |= wslcsdk_sys::types::WSLC_SESSION_FEATURE_FLAG_ENABLE_GPU;
        } else {
            self.feature_flags &= !wslcsdk_sys::types::WSLC_SESSION_FEATURE_FLAG_ENABLE_GPU;
        }
        self
    }

    pub fn vhd(mut self, vhd: VhdRequirementsData) -> Self {
        self.vhd = Some(vhd);
        self
    }

    fn apply_resource_limits(&self, settings: &mut WslcSessionSettings) -> Result<(), WslcError> {
        if let Some(cpus) = self.cpu_count {
            let hr = unsafe { WslcSetSessionSettingsCpuCount(settings, cpus) };
            if hr < 0 {
                return Err(WslcError::Win32(
                    hr as u32,
                    "设置会话 CPU 核心数失败".to_string(),
                ));
            }
        }
        if let Some(mem) = self.memory_mb {
            let hr = unsafe { WslcSetSessionSettingsMemory(settings, mem) };
            if hr < 0 {
                return Err(WslcError::Win32(
                    hr as u32,
                    "设置会话内存配额失败".to_string(),
                ));
            }
        }
        if let Some(timeout) = self.timeout_ms {
            let hr = unsafe { WslcSetSessionSettingsTimeout(settings, timeout) };
            if hr < 0 {
                return Err(WslcError::Win32(
                    hr as u32,
                    "设置会话超时时间失败".to_string(),
                ));
            }
        }
        if self.feature_flags != 0 {
            let hr = unsafe { WslcSetSessionSettingsFeatureFlags(settings, self.feature_flags) };
            if hr < 0 {
                return Err(WslcError::Win32(
                    hr as u32,
                    "设置会话特性标志位失败".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn apply_vhd_settings(
        &self,
        settings: &mut WslcSessionSettings,
    ) -> Result<Option<CString>, WslcError> {
        let Some(ref v) = self.vhd else {
            return Ok(None);
        };
        let c_str = match &v.name {
            Some(n) => Some(
                CString::new(n.as_str())
                    .map_err(|e| WslcError::Utf8Error(format!("VHD 名称非法: {e}")))?,
            ),
            None => None,
        };
        let raw_req = WslcVhdRequirements {
            name: c_str.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
            size_bytes: v.size_bytes,
            vhd_type: v.vhd_type,
            flags: v.flags,
            uid: v.uid,
            gid: v.gid,
        };
        let hr = unsafe { WslcSetSessionSettingsVhd(settings, &raw_req) };
        if hr < 0 {
            return Err(WslcError::Win32(
                hr as u32,
                "设置会话 VHD 存储规格失败".to_string(),
            ));
        }
        Ok(c_str)
    }

    /// 创建并激活会话
    pub fn build(self) -> Result<WslcSessionHandle, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let wide_name = to_wide_null(&self.name)?;
        let wide_path = to_wide_null(&self.storage_path)?;

        let mut settings = WslcSessionSettings::default();
        let hr = unsafe {
            WslcInitSessionSettings(wide_name.as_ptr(), wide_path.as_ptr(), &mut settings)
        };
        if hr < 0 {
            return Err(WslcError::Win32(
                hr as u32,
                "初始化会话设置失败".to_string(),
            ));
        }

        self.apply_resource_limits(&mut settings)?;
        let _retained_vhd_name = self.apply_vhd_settings(&mut settings)?;

        let mut raw_session = WslcSession::NULL;
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        let hr = unsafe { WslcCreateSession(&mut settings, &mut raw_session, &mut err_msg) };
        unsafe {
            WslcError::check(hr, err_msg)?;
        }

        if raw_session.is_null() {
            return Err(WslcError::InvalidHandle);
        }

        Ok(WslcSessionHandle {
            inner: Arc::new(SessionInner {
                raw: raw_session,
                name: self.name,
            }),
        })
    }
}

struct SessionInner {
    raw: WslcSession,
    name: String,
}

impl Drop for SessionInner {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                let _ = WslcReleaseSession(self.raw);
            }
            self.raw = WslcSession::NULL;
        }
    }
}

unsafe impl Send for SessionInner {}
unsafe impl Sync for SessionInner {}

/// 安全的 WSLC 会话 RAII 包装对象 (内部通过 Arc 托管生命周期，支持轻量安全 Clone)
#[derive(Clone)]
pub struct WslcSessionHandle {
    inner: Arc<SessionInner>,
}

impl WslcSessionHandle {
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
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut event: HANDLE = std::ptr::null_mut();
        let hr = unsafe { WslcGetSessionTerminationEvent(self.inner.raw, &mut event) };
        if hr >= 0 {
            Ok(event)
        } else {
            Err(WslcError::Win32(
                hr as u32,
                "获取会话终止事件句柄失败".to_string(),
            ))
        }
    }

    /// 获取会话终止的具体原因
    pub fn termination_reason(&self) -> Result<WslcSessionTerminationReason, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut reason = WslcSessionTerminationReason::Unknown;
        let hr = unsafe { WslcGetSessionTerminationReason(self.inner.raw, &mut reason) };
        if hr >= 0 {
            Ok(reason)
        } else {
            Err(WslcError::Win32(
                hr as u32,
                "获取会话终止原因失败".to_string(),
            ))
        }
    }

    /// 强制终止当前会话
    pub fn terminate(&self) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let hr = unsafe { WslcTerminateSession(self.inner.raw) };
        if hr >= 0 {
            Ok(())
        } else {
            Err(WslcError::Win32(hr as u32, "终止会话失败".to_string()))
        }
    }

    /// 注册 Linux 进程崩溃转储监控回调
    pub fn register_crash_dump_callback<F>(
        &self,
        callback: F,
    ) -> Result<CrashDumpSubscription, WslcError>
    where
        F: Fn(&WslcSessionCrashDumpInfo) + Send + 'static,
    {
        let _com_guard = crate::com::try_initialize_mta()?;
        unsafe extern "system" fn crash_trampoline<F>(
            info: *const WslcSessionCrashDumpInfo,
            context: *mut c_void,
        ) where
            F: Fn(&WslcSessionCrashDumpInfo),
        {
            if !context.is_null() && !info.is_null() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let cb = unsafe { &*(context as *const F) };
                    cb(unsafe { &*info });
                }));
            }
        }

        let boxed_cb = Box::new(callback);
        let ctx = Box::into_raw(boxed_cb);

        let mut sub = WslcCrashDumpSubscription::NULL;
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        let hr = unsafe {
            WslcRegisterSessionCrashDumpCallback(
                self.inner.raw,
                Some(crash_trampoline::<F>),
                ctx as *mut c_void,
                &mut sub,
                &mut err_msg,
            )
        };

        if let Err(e) = unsafe { WslcError::check(hr, err_msg) } {
            let _ = unsafe { Box::from_raw(ctx) };
            return Err(e);
        }

        unsafe fn drop_ctx<F>(context: *mut c_void) {
            if !context.is_null() {
                let _ = unsafe { Box::from_raw(context as *mut F) };
            }
        }

        Ok(CrashDumpSubscription {
            raw: sub,
            ctx: ctx as *mut c_void,
            drop_fn: drop_ctx::<F>,
        })
    }
}

/// 崩溃转储回调订阅的 RAII 守卫
pub struct CrashDumpSubscription {
    raw: WslcCrashDumpSubscription,
    ctx: *mut c_void,
    drop_fn: unsafe fn(*mut c_void),
}

impl Drop for CrashDumpSubscription {
    fn drop(&mut self) {
        // 先调用 SDK 注销订阅，确保 SDK 不会在析构期间或之后再次触发回调
        if !self.raw.is_null() {
            unsafe {
                let _ = WslcReleaseCrashDumpSubscription(self.raw);
            }
            self.raw = WslcCrashDumpSubscription::NULL;
        }
        // 注销完成后再安全释放 Rust 闭包内存，彻底规避 UAF 竞态
        if !self.ctx.is_null() {
            unsafe {
                (self.drop_fn)(self.ctx);
            }
            self.ctx = std::ptr::null_mut();
        }
    }
}

unsafe impl Send for CrashDumpSubscription {}
unsafe impl Sync for CrashDumpSubscription {}
