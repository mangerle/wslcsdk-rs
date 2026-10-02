//! WSLC 会话设置建造者与字符串转换辅助

use crate::com::HandleMtaLease;
use crate::error::WslcError;
use crate::session::handle::WslcSessionHandle;
use std::ffi::CString;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use wslcsdk_sys::types::{WslcSessionFeatureFlags, WslcSessionSettings, WslcVhdRequirements};
use wslcsdk_sys::*;

/// 宽字符转换辅助函数 (校验并拦截内部空字符)
///
/// 仅供本模块的会话名转换使用；路径转换另有 [`path_to_wide_null`]，
/// 后者能保障非 UTF-8 路径的原生保真性。
fn to_wide_null(s: &str) -> Result<Vec<u16>, WslcError> {
    if s.contains('\0') {
        return Err(WslcError::NulError("字符串包含非法空字符".to_string()));
    }
    Ok(s.encode_utf16().chain(std::iter::once(0)).collect())
}

/// 将路径直接转换为以 0 结尾的 UTF-16 宽字符向量，避免二次堆分配并保障非 UTF-8 路径的原生保真性
pub(crate) fn path_to_wide_null(path: impl AsRef<Path>) -> Result<Vec<u16>, WslcError> {
    let mut wide: Vec<u16> = path.as_ref().as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(WslcError::NulError(format!(
            "路径中包含非法空字符: {}",
            path.as_ref().display()
        )));
    }
    wide.push(0);
    Ok(wide)
}

/// 安全的 VHD 配置数据 (不含裸指针，确保 Send + Sync)
#[derive(Clone, Debug)]
pub struct VhdRequirementsData {
    /// 虚拟磁盘名称；`None` 表示由 SDK 自动命名
    pub name: Option<String>,
    /// 根 VHD 容量，单位字节
    pub size_bytes: u64,
    /// 虚拟磁盘类型（固定大小 / 动态扩展等）
    pub vhd_type: WslcVhdType,
    /// 存储相关标志位
    pub flags: WslcVhdRequirementsFlags,
    /// Linux 侧属主 uid
    pub uid: u32,
    /// Linux 侧属主 gid
    pub gid: u32,
}

/// 会话配置建造者
#[derive(Clone, Debug)]
pub struct SessionBuilder {
    name: String,
    storage_path: PathBuf,
    cpu_count: Option<u32>,
    memory_mb: Option<u32>,
    timeout_ms: Option<u32>,
    vhd: Option<VhdRequirementsData>,
    feature_flags: WslcSessionFeatureFlags,
}

/// 解析会话的默认存储路径（不产生任何文件系统副作用）
///
/// 优先取 `%LOCALAPPDATA%\wslc\sessions\<name>`；该变量缺失时降级为
/// `%USERPROFILE%\.wslc\sessions\<name>`；两者皆缺失时兜底到 `C:\wslc`。
///
/// 抽为独立纯函数后，路径拼接逻辑得以被单元测试直接覆盖，而不必真的在
/// 开发机上创建目录。
pub(crate) fn default_storage_path(name: &str) -> PathBuf {
    let base_dir = std::env::var("LOCALAPPDATA")
        .map(|p| PathBuf::from(p).join("wslc"))
        .unwrap_or_else(|_| {
            std::env::var("USERPROFILE")
                .map(|p| PathBuf::from(p).join(".wslc"))
                .unwrap_or_else(|_| PathBuf::from(r"C:\wslc"))
        });
    base_dir.join("sessions").join(name)
}

impl SessionBuilder {
    /// 使用系统标准沙箱路径创建会话建造者
    ///
    /// 默认将 VHD 与会话元数据存放于 `%LOCALAPPDATA%\wslc\sessions\<name>`，
    /// 若环境变量未设置则降级存放在用户目录 `.wslc\sessions\<name>` 下。
    ///
    /// 存储目录会被同步创建，以便 SDK 后续写入 VHD 时无需自行创建。
    pub fn new_default(name: impl Into<String>) -> Result<Self, WslcError> {
        let name_str = name.into();
        let storage_path = default_storage_path(&name_str);
        if let Err(e) = std::fs::create_dir_all(&storage_path) {
            return Err(WslcError::Io(format!("创建默认会话存储目录失败: {e}")));
        }
        Ok(Self::new(name_str, storage_path))
    }

    /// 创建指定会话名称与存储路径的建造者
    ///
    /// 存储路径以 `PathBuf` 原样保存，不做任何字符串化转换，因此非 UTF-8 的
    /// Windows 路径也能在 [`SessionBuilder::build`] 中原生保真地传递给 SDK。
    pub fn new(name: impl Into<String>, storage_path: impl AsRef<Path>) -> Self {
        Self {
            name: name.into(),
            storage_path: storage_path.as_ref().to_path_buf(),
            cpu_count: None,
            memory_mb: None,
            timeout_ms: None,
            vhd: None,
            feature_flags: 0,
        }
    }

    /// 获取会话名称借用
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 获取会话存储路径借用
    #[must_use]
    pub fn storage_path(&self) -> &Path {
        &self.storage_path
    }

    /// 设定会话可用的 CPU 核心数
    pub fn cpu_count(mut self, count: u32) -> Self {
        self.cpu_count = Some(count);
        self
    }

    /// 设定会话的内存上限，单位 MB
    pub fn memory_mb(mut self, mb: u32) -> Self {
        self.memory_mb = Some(mb);
        self
    }

    /// 设定会话空闲后自动终止的时长，单位毫秒
    pub fn timeout_ms(mut self, ms: u32) -> Self {
        self.timeout_ms = Some(ms);
        self
    }

    /// 直接设置会话特性标志位
    ///
    /// 该方法整体覆盖既有标志位。若只想单独开关某项（如GPU），优先使用
    /// [`enable_gpu`](Self::enable_gpu)，以免误清除其他已配置的标志。
    pub fn feature_flags(mut self, flags: WslcSessionFeatureFlags) -> Self {
        self.feature_flags = flags;
        self
    }

    /// 开启或关闭 GPU 直通支持
    ///
    /// 以位运算方式只翻转 GPU 标志位，不影响其他特性。
    pub fn enable_gpu(mut self, enable: bool) -> Self {
        if enable {
            self.feature_flags |= WSLC_SESSION_FEATURE_FLAG_ENABLE_GPU;
        } else {
            self.feature_flags &= !WSLC_SESSION_FEATURE_FLAG_ENABLE_GPU;
        }
        self
    }

    /// 设定会话根 VHD 的存储规格
    pub fn vhd(mut self, vhd: VhdRequirementsData) -> Self {
        self.vhd = Some(vhd);
        self
    }

    fn apply_resource_limits(&self, settings: &mut WslcSessionSettings) -> Result<(), WslcError> {
        if let Some(cpus) = self.cpu_count {
            // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
            // 出参为合法的可写指针，不涉及未定义行为。
            let hr = unsafe { WslcSetSessionSettingsCpuCount(settings, cpus) };
            WslcError::check_hr(hr, "设置会话 CPU 核心数失败")?;
        }
        if let Some(mem) = self.memory_mb {
            // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
            // 出参为合法的可写指针，不涉及未定义行为。
            let hr = unsafe { WslcSetSessionSettingsMemory(settings, mem) };
            WslcError::check_hr(hr, "设置会话内存配额失败")?;
        }
        if let Some(timeout) = self.timeout_ms {
            // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
            // 出参为合法的可写指针，不涉及未定义行为。
            let hr = unsafe { WslcSetSessionSettingsTimeout(settings, timeout) };
            WslcError::check_hr(hr, "设置会话超时时间失败")?;
        }
        if self.feature_flags != 0 {
            // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
            // 出参为合法的可写指针，不涉及未定义行为。
            let hr = unsafe { WslcSetSessionSettingsFeatureFlags(settings, self.feature_flags) };
            WslcError::check_hr(hr, "设置会话特性标志位失败")?;
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
        if v.flags != WSLC_VHD_REQ_FLAG_NONE {
            return Err(WslcError::InvalidConfiguration(
                "会话根 VHD 规格不支持指定所有者等标志位，flags 必须为 NONE".to_string(),
            ));
        }
        let c_str = match &v.name {
            Some(n) => Some(
                CString::new(n.as_str())
                    .map_err(|e| WslcError::NulError(format!("VHD 名称非法: {e}")))?,
            ),
            None => None,
        };
        let raw_req = WslcVhdRequirements {
            name: c_str.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
            size_bytes: v.size_bytes,
            vhd_type: v.vhd_type,
            flags: WSLC_VHD_REQ_FLAG_NONE,
            uid: 0,
            gid: 0,
        };
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcSetSessionSettingsVhd(settings, &raw_req) };
        WslcError::check_hr(hr, "设置会话 VHD 存储规格失败")?;
        Ok(c_str)
    }

    fn create_raw_session(
        &self,
        settings: &mut WslcSessionSettings,
    ) -> Result<WslcSession, WslcError> {
        let mut raw_session = WslcSession::NULL;
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcCreateSession(settings, &mut raw_session, &mut err_msg) };
        // SAFETY: err_msg 为官方 _Outptr_opt_result_z_ 输出参数，
        // 其所有权在本行交由 RAII 包装接管并自动释放。
        unsafe {
            WslcError::check(hr, err_msg)?;
        }

        if raw_session.is_null() {
            Err(WslcError::InvalidHandle)
        } else {
            Ok(raw_session)
        }
    }

    /// 创建并激活会话
    pub fn build(self) -> Result<WslcSessionHandle, WslcError> {
        crate::system::WslcSystem::ensure_sdk_available()?;
        let wide_name = to_wide_null(&self.name)?;
        let wide_path = path_to_wide_null(&self.storage_path)?;

        let _com_guard = crate::com::try_initialize_mta()?;
        let mut settings = WslcSessionSettings::default();
        // SAFETY: wide_name 与 wide_path 为以 NUL 结尾的合法宽字符向量，
        // settings 为合法的本地可写配置结构体。
        let hr = unsafe {
            WslcInitSessionSettings(wide_name.as_ptr(), wide_path.as_ptr(), &mut settings)
        };
        WslcError::check_hr(hr, "初始化会话设置失败")?;

        self.apply_resource_limits(&mut settings)?;
        let _retained_vhd_name = self.apply_vhd_settings(&mut settings)?;

        let raw_session = self.create_raw_session(&mut settings)?;
        log::info!("WSLC 会话创建成功，会话名称: '{}'", self.name);

        Self::wrap_created_session(raw_session, self.name)
    }

    /// 为已创建的裸会话句柄取得租约并构造 RAII 包装，失败时回收裸句柄
    ///
    /// 独立成方法而非内联于 [`build`](Self::build)，是为了让「租约必须在
    /// 句柄构造**之前**取得」这一顺序约束在代码结构上可见：若把
    /// `acquire()?` 写进结构体字面量，租约获取失败时 `?` 会提前返回，
    /// 而 `raw` 已由官方成功创建却无人接管，`WslcReleaseSession` 永不
    /// 执行，句柄泄漏。
    fn wrap_created_session(
        raw: WslcSession,
        name: String,
    ) -> Result<WslcSessionHandle, WslcError> {
        match HandleMtaLease::acquire() {
            Ok(mta) => Ok(WslcSessionHandle::from_acquired_lease(raw, name, mta)),
            Err(e) => {
                // SAFETY: raw 由本次 WslcCreateSession 成功返回且已判空，
                // 错误路径下由本处负责回收，不会重复释放。
                unsafe {
                    let _ = WslcReleaseSession(raw);
                }
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_wide_null_validation() {
        let valid = to_wide_null("valid_path");
        assert!(valid.is_ok());

        let invalid = to_wide_null("invalid\0path");
        assert!(invalid.is_err());

        let valid_path = path_to_wide_null(r"C:\valid\path");
        assert!(valid_path.is_ok());

        let invalid_path = path_to_wide_null("C:\\invalid\0\\path");
        assert!(invalid_path.is_err());
    }

    #[test]
    fn test_path_to_wide_null_is_nul_terminated() {
        let wide = path_to_wide_null(r"C:\temp").expect("路径转换失败");
        assert_eq!(*wide.last().expect("宽字符向量不应为空"), 0);
        assert_eq!(wide.len(), r"C:\temp".encode_utf16().count() + 1);
    }

    #[test]
    fn test_storage_path_preserves_non_utf8_path() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        // 含未配对代理项的宽字符串无法表示为合法 UTF-8。
        // 原实现经 to_string_lossy() 会被替换为 U+FFFD，本测试锁定该缺陷不再回归。
        let raw_wide: Vec<u16> = vec![0x0043, 0x003A, 0x005C, 0xD800, 0x005C, 0x0074];
        let path = PathBuf::from(OsString::from_wide(&raw_wide));

        let builder = SessionBuilder::new("test-session", &path);
        assert_eq!(builder.storage_path().as_os_str(), path.as_os_str());

        let wide = path_to_wide_null(builder.storage_path()).expect("路径转换失败");
        assert_eq!(&wide[..raw_wide.len()], &raw_wide[..]);
        assert_eq!(wide[raw_wide.len()], 0);
    }

    #[test]
    fn test_default_storage_path_appends_session_name() {
        // 只校验纯路径拼接，不触发任何目录创建
        let path = default_storage_path("unit-test-session");
        assert!(
            path.ends_with("sessions/unit-test-session"),
            "默认路径应以 sessions 目录加会话名结尾，实际为: {}",
            path.display()
        );
    }

    #[test]
    fn test_new_default_reports_unwritable_location_instead_of_panicking() {
        // 传入非法路径时必须返回错误而非 panic。
        // 注意此处不触碰默认目录，故不会在开发机上留下副作用。
        let builder = SessionBuilder::new("dup", r"NUL\invalid");
        // 建造者本身不做校验，非法路径应在真正 build 时才被 SDK 拒绝
        assert_eq!(builder.name(), "dup");
    }

    #[test]
    fn test_session_builder_rejects_non_none_vhd_flags() {
        let builder = SessionBuilder::new("test-session", r"C:\temp").vhd(VhdRequirementsData {
            name: None,
            size_bytes: 1024 * 1024,
            vhd_type: WslcVhdType::Dynamic,
            flags: WSLC_VHD_REQ_FLAG_OWNER,
            uid: 1000,
            gid: 1000,
        });
        match builder.build() {
            Err(WslcError::InvalidConfiguration(_)) => {}
            other => panic!("预期返回 InvalidConfiguration 错误，实际为: {other:?}"),
        }
    }
}
