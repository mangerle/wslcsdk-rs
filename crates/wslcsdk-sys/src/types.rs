//! Microsoft.WSL.Containers 3.0.1 原始数据结构、枚举与回调函数定义

use core::ffi::c_void;
use windows_sys::Win32::Foundation::{BOOL, HANDLE};
use windows_sys::Win32::Networking::WinSock::SOCKADDR_STORAGE;
use windows_sys::core::HRESULT;

// ==================== 缓冲区尺寸常量 ====================

/// 会话配置不透明结构体字节大小
pub const WSLC_SESSION_OPTIONS_SIZE: usize = 72;
/// 会话配置对齐边界
pub const WSLC_SESSION_OPTIONS_ALIGNMENT: usize = 8;

/// 容器配置不透明结构体字节大小
pub const WSLC_CONTAINER_OPTIONS_SIZE: usize = 104;
/// 容器配置对齐边界
pub const WSLC_CONTAINER_OPTIONS_ALIGNMENT: usize = 8;

/// 进程配置不透明结构体字节大小
pub const WSLC_CONTAINER_PROCESS_OPTIONS_SIZE: usize = 72;
/// 进程配置对齐边界
pub const WSLC_CONTAINER_PROCESS_OPTIONS_ALIGNMENT: usize = 8;

/// 容器 ID 缓冲区长度 (64 字符十六进制 + 空终止符)
pub const WSLC_CONTAINER_ID_BUFFER_SIZE: usize = 65;

/// 镜像名称最大长度 (255 字符 + 空终止符)
pub const WSLC_IMAGE_NAME_LENGTH: usize = 256;

// ==================== 不透明句柄与配置结构体 ====================

/// WSLC 会话不透明句柄
///
/// 采用 `#[repr(transparent)]` Newtype 包装 `HANDLE`，
/// 确保与 C ABI 布局完全一致，同时在编译期阻止不同类型句柄的混淆传递。
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WslcSession(pub HANDLE);

impl WslcSession {
    /// 空句柄常量
    pub const NULL: Self = Self(core::ptr::null_mut());

    /// 检查句柄是否为空
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

impl Default for WslcSession {
    fn default() -> Self {
        Self::NULL
    }
}

/// WSLC 容器不透明句柄
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WslcContainer(pub HANDLE);

impl WslcContainer {
    /// 空句柄常量
    pub const NULL: Self = Self(core::ptr::null_mut());

    /// 检查句柄是否为空
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

impl Default for WslcContainer {
    fn default() -> Self {
        Self::NULL
    }
}

/// WSLC 进程不透明句柄
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WslcProcess(pub HANDLE);

impl WslcProcess {
    /// 空句柄常量
    pub const NULL: Self = Self(core::ptr::null_mut());

    /// 检查句柄是否为空
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

impl Default for WslcProcess {
    fn default() -> Self {
        Self::NULL
    }
}

/// 崩溃转储回调订阅不透明句柄
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WslcCrashDumpSubscription(pub HANDLE);

impl WslcCrashDumpSubscription {
    /// 空句柄常量
    pub const NULL: Self = Self(core::ptr::null_mut());

    /// 检查句柄是否为空
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

impl Default for WslcCrashDumpSubscription {
    fn default() -> Self {
        Self::NULL
    }
}

/// 会话初始化配置结构体 (72 字节，8 字节对齐)
#[repr(C, align(8))]
#[derive(Copy, Clone, Debug)]
pub struct WslcSessionSettings {
    pub(crate) _opaque: [u8; WSLC_SESSION_OPTIONS_SIZE],
}

impl Default for WslcSessionSettings {
    fn default() -> Self {
        Self {
            _opaque: [0u8; WSLC_SESSION_OPTIONS_SIZE],
        }
    }
}

/// 容器初始化配置结构体 (104 字节，8 字节对齐)
#[repr(C, align(8))]
#[derive(Copy, Clone, Debug)]
pub struct WslcContainerSettings {
    pub(crate) _opaque: [u8; WSLC_CONTAINER_OPTIONS_SIZE],
}

impl Default for WslcContainerSettings {
    fn default() -> Self {
        Self {
            _opaque: [0u8; WSLC_CONTAINER_OPTIONS_SIZE],
        }
    }
}

/// 进程初始化配置结构体 (72 字节，8 字节对齐)
#[repr(C, align(8))]
#[derive(Copy, Clone, Debug)]
pub struct WslcProcessSettings {
    pub(crate) _opaque: [u8; WSLC_CONTAINER_PROCESS_OPTIONS_SIZE],
}

impl Default for WslcProcessSettings {
    fn default() -> Self {
        Self {
            _opaque: [0u8; WSLC_CONTAINER_PROCESS_OPTIONS_SIZE],
        }
    }
}

// ==================== 枚举定义 ====================

/// 容器网络模式
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum WslcContainerNetworkingMode {
    #[default]
    None = 0,
    Bridged = 1,
}

/// VHD 虚拟磁盘分配类型
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum WslcVhdType {
    #[default]
    Dynamic = 0,
    Fixed = 1,
}

/// VHD 要求标志位
pub type WslcVhdRequirementsFlags = u32;
pub const WSLC_VHD_REQ_FLAG_NONE: WslcVhdRequirementsFlags = 0x00000000;
pub const WSLC_VHD_REQ_FLAG_OWNER: WslcVhdRequirementsFlags = 0x00000001;

/// 会话特性标志位
pub type WslcSessionFeatureFlags = u32;
pub const WSLC_SESSION_FEATURE_FLAG_NONE: WslcSessionFeatureFlags = 0x00000000;
pub const WSLC_SESSION_FEATURE_FLAG_ENABLE_GPU: WslcSessionFeatureFlags = 0x00000004;

/// 会话退出原因枚举
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum WslcSessionTerminationReason {
    #[default]
    Unknown = 0,
    Shutdown = 1,
    Crashed = 2,
}

/// 端口映射协议类型
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum WslcPortProtocol {
    #[default]
    Tcp = 0,
    Udp = 1,
}

/// 容器特性标志位
pub type WslcContainerFlags = u32;
pub const WSLC_CONTAINER_FLAG_NONE: WslcContainerFlags = 0x00000000;
pub const WSLC_CONTAINER_FLAG_AUTO_REMOVE: WslcContainerFlags = 0x00000001;
pub const WSLC_CONTAINER_FLAG_ENABLE_GPU: WslcContainerFlags = 0x00000002;
pub const WSLC_CONTAINER_FLAG_PRIVILEGED: WslcContainerFlags = 0x00000004;

/// 容器启动标志位
pub type WslcContainerStartFlags = u32;
pub const WSLC_CONTAINER_START_FLAG_NONE: WslcContainerStartFlags = 0x00000000;
pub const WSLC_CONTAINER_START_FLAG_ATTACH: WslcContainerStartFlags = 0x00000001;

/// 容器删除标志位
pub type WslcDeleteContainerFlags = u32;
pub const WSLC_DELETE_CONTAINER_FLAG_NONE: WslcDeleteContainerFlags = 0x00000000;
pub const WSLC_DELETE_CONTAINER_FLAG_FORCE: WslcDeleteContainerFlags = 0x00000001;

/// 容器运行状态
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum WslcContainerState {
    #[default]
    Invalid = 0,
    Created = 1,
    Running = 2,
    Exited = 3,
    Deleted = 4,
}

/// Linux 信号枚举
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum WslcSignal {
    #[default]
    None = 0,
    Sighup = 1,
    Sigint = 2,
    Sigquit = 3,
    Sigkill = 9,
    Sigterm = 15,
}

/// 进程选项标志位
pub type WslcProcessFlags = u32;
pub const WSLC_PROCESS_FLAG_NONE: WslcProcessFlags = 0x00000000;
pub const WSLC_PROCESS_FLAG_STDIN: WslcProcessFlags = 0x00000001;

/// 进程标准 IO 流类型
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum WslcProcessIOHandle {
    #[default]
    Stdin = 0,
    Stdout = 1,
    Stderr = 2,
}

/// 进程运行状态
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum WslcProcessState {
    #[default]
    Unknown = 0,
    Running = 1,
    Exited = 2,
    Signalled = 3,
}

/// 镜像拉取进度阶段状态
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum WslcImageProgressStatus {
    #[default]
    Unknown = 0,
    Pulling = 1,
    Waiting = 2,
    Downloading = 3,
    Verifying = 4,
    Extracting = 5,
    Complete = 6,
}

/// 身份认证令牌返回类型
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum WslcIdentityTokenType {
    #[default]
    Unknown = 0,
    Token = 1,
    Credentials = 2,
}

/// 系统缺失组件标志位
pub type WslcComponentFlags = u32;
pub const WSLC_COMPONENT_FLAG_NONE: WslcComponentFlags = 0;
pub const WSLC_COMPONENT_FLAG_VIRTUAL_MACHINE_PLATFORM: WslcComponentFlags = 1;
pub const WSLC_COMPONENT_FLAG_WSL_PACKAGE: WslcComponentFlags = 2;
pub const WSLC_COMPONENT_FLAG_SDK_NEEDS_UPDATE: WslcComponentFlags = 4;

/// 组件安装选项标志位
pub type WslcInstallOptions = u32;
pub const WSLC_INSTALL_OPTION_NONE: WslcInstallOptions = 0;
pub const WSLC_INSTALL_OPTION_REPAIR: WslcInstallOptions = 1;

// ==================== 复合结构体 ====================

/// VHD 规格要求
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcVhdRequirements {
    pub name: *const i8,
    pub size_bytes: u64,
    pub vhd_type: WslcVhdType,
    pub flags: WslcVhdRequirementsFlags,
    pub uid: u32,
    pub gid: u32,
}

/// 会话崩溃转储信息
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcSessionCrashDumpInfo {
    pub dump_path: *const u16,
    pub process_name: *const i8,
    pub pid: u32,
    pub signal: u32,
    pub timestamp: u64,
}

/// 容器端口映射配置
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcContainerPortMapping {
    pub windows_port: u16,
    pub container_port: u16,
    pub protocol: WslcPortProtocol,
    pub windows_address: *mut SOCKADDR_STORAGE,
}

/// 容器目录卷挂载
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcContainerVolume {
    pub windows_path: *const u16,
    pub container_path: *const i8,
    pub read_only: BOOL,
}

/// 容器具名 VHD 卷挂载
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcContainerNamedVolume {
    pub name: *const i8,
    pub container_path: *const i8,
    pub read_only: BOOL,
}

/// 镜像拉取字节进度明细
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WslcImageProgressDetail {
    pub current_bytes: u64,
    pub total_bytes: u64,
}

/// 镜像拉取进度单条消息
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcImageProgressMessage {
    pub id: *const i8,
    pub status: WslcImageProgressStatus,
    pub detail: WslcImageProgressDetail,
}

/// 镜像元数据信息
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcImageInfo {
    pub name: [u8; WSLC_IMAGE_NAME_LENGTH],
    pub sha256: [u8; 32],
    pub size_bytes: i64,
    pub created_unix_time: u64,
}

/// WSLC 系统版本号
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct WslcVersion {
    pub major: u32,
    pub minor: u32,
    pub revision: u32,
}

// ==================== 回调函数指针类型 ====================

/// 会话崩溃转储回调
pub type WslcSessionCrashDumpCallback =
    Option<unsafe extern "system" fn(info: *const WslcSessionCrashDumpInfo, context: *mut c_void)>;

/// 进程标准输出/错误读取回调
pub type WslcStdIOCallback = Option<
    unsafe extern "system" fn(
        io_handle: WslcProcessIOHandle,
        data: *const u8,
        data_bytes: u32,
        context: *mut c_void,
    ),
>;

/// 进程退出通知回调
pub type WslcProcessExitCallback =
    Option<unsafe extern "system" fn(exit_code: i32, context: *mut c_void)>;

/// 进程回调函数集合
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WslcProcessCallbacks {
    pub on_stdout: WslcStdIOCallback,
    pub on_stderr: WslcStdIOCallback,
    pub on_exit: WslcProcessExitCallback,
}

/// 镜像拉取/载入进度回调
pub type WslcContainerImageProgressCallback = Option<
    unsafe extern "system" fn(
        progress: *const WslcImageProgressMessage,
        context: *mut c_void,
    ) -> HRESULT,
>;

/// 镜像拉取配置选项
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcPullImageOptions {
    pub uri: *const i8,
    pub progress_callback: WslcContainerImageProgressCallback,
    pub progress_callback_context: *mut c_void,
    pub registry_auth: *const i8,
}

/// 镜像导入配置选项
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WslcImportImageOptions {
    pub progress_callback: WslcContainerImageProgressCallback,
    pub progress_callback_context: *mut c_void,
}

/// 镜像载入配置选项
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WslcLoadImageOptions {
    pub progress_callback: WslcContainerImageProgressCallback,
    pub progress_callback_context: *mut c_void,
}

/// 镜像打标签配置选项
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcTagImageOptions {
    pub image: *const i8,
    pub repo: *const i8,
    pub tag: *const i8,
}

/// 镜像推送配置选项
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcPushImageOptions {
    pub image: *const i8,
    pub registry_auth: *const i8,
    pub progress_callback: WslcContainerImageProgressCallback,
    pub progress_callback_context: *mut c_void,
}

/// 组件依赖安装进度回调
pub type WslcInstallCallback = Option<
    unsafe extern "system" fn(
        component: WslcComponentFlags,
        progress_steps: u32,
        total_steps: u32,
        context: *mut c_void,
    ),
>;
