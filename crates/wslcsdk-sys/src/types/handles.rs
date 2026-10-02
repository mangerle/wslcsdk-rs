//! WSLC 不透明句柄、配置结构体与单向下行枚举

use super::*;
use windows_sys::Win32::Foundation::HANDLE;

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

// ==================== 单向下行的枚举定义 ====================

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

/// 端口映射协议类型
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum WslcPortProtocol {
    #[default]
    Tcp = 0,
    Udp = 1,
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
