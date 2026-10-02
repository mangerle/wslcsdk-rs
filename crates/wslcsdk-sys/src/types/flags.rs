//! WSLC 各领域标志位与安装选项常量

// ==================== 标志位常量 ====================

/// VHD 要求标志位
pub type WslcVhdRequirementsFlags = u32;
pub const WSLC_VHD_REQ_FLAG_NONE: WslcVhdRequirementsFlags = 0x00000000;
pub const WSLC_VHD_REQ_FLAG_OWNER: WslcVhdRequirementsFlags = 0x00000001;

/// 会话特性标志位
pub type WslcSessionFeatureFlags = u32;
pub const WSLC_SESSION_FEATURE_FLAG_NONE: WslcSessionFeatureFlags = 0x00000000;
pub const WSLC_SESSION_FEATURE_FLAG_ENABLE_GPU: WslcSessionFeatureFlags = 0x00000004;

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

/// 进程选项标志位
pub type WslcProcessFlags = u32;
pub const WSLC_PROCESS_FLAG_NONE: WslcProcessFlags = 0x00000000;
pub const WSLC_PROCESS_FLAG_STDIN: WslcProcessFlags = 0x00000001;

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
