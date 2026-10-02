//! Microsoft.WSL.Containers (WSLC) 官方 SDK 安全 Rust 抽象门面
//!
//! 基于官方 3.0.1 GA SDK 构建，提供具备 RAII 自动资源释放、强类型错误模型与并发安全保障的统一客户端接口。
//!
//! # 平台要求
//!
//! 本 crate 深度依赖 Win32 COM/RPC 与系统组件 `wslcsdk.dll`，**仅支持 Windows 平台**。
//! 在非 Windows 目标上编译会立即得到一条明确的 `compile_error!` 提示，
//! 而非在 `std::os::windows` 等处报出难以定位的错误。

#![cfg_attr(docsrs, feature(doc_auto_cfg))]
// 本 crate 大量使用 FFI 与裸指针，逐项论证成本高，故强制显式书写 unsafe 块：
// 开启该 lint 后，unsafe fn 体内的裸操作必须被 unsafe 块包裹，编译器据此
// 校验「函数签名承诺不安全」与「实现确实逐处声明」两者一致。
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(rust_2018_idioms)]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(clippy::missing_safety_doc)]
// 文档完备性已达成：结构体字段、枚举变体与公开方法均已补齐注释，
// 下列两项自此作为门禁生效，新增公开项若缺注释将在 CI 阶段即告警。
#![warn(missing_docs, missing_debug_implementations)]

#[cfg(not(windows))]
compile_error!("wslcsdk 仅支持 Windows 平台：WSLC 依赖 Win32 COM/RPC 与系统组件 wslcsdk.dll");

// 平台相关模块统一门控：非 Windows 目标下整体不参与编译（见上方 compile_error）
#[cfg(windows)]
pub mod async_ops;
// C 回调上下文的统一访问入口：仅 crate 内部使用，不对外暴露
#[cfg(windows)]
pub(crate) mod callback;
#[cfg(windows)]
pub mod channel;
#[cfg(windows)]
pub mod client;
#[cfg(windows)]
pub mod com;
#[cfg(windows)]
pub mod com_memory;
#[cfg(windows)]
pub mod container;
#[cfg(windows)]
pub mod error;
#[cfg(windows)]
pub mod image;
#[cfg(windows)]
pub mod process;
#[cfg(windows)]
pub mod registry;
#[cfg(windows)]
pub mod session;
/// WSLC 系统服务门面：版本查询、组件缺失检测与依赖安装
#[cfg(windows)]
pub mod system;
#[cfg(windows)]
pub mod volume;

#[cfg(windows)]
pub use channel::{AsyncReceiver, AsyncRecvError};
#[cfg(windows)]
pub use client::{WslcClient, WslcClientBuilder};
#[cfg(windows)]
pub use com::{ComGuard, ProcessMtaGuard, init_process_mta, try_initialize_mta, with_mta};
#[cfg(windows)]
pub use com_memory::{ComAnsiString, ComArray, ComWideString};

#[cfg(windows)]
pub use container::{ContainerBuilder, ContainerPortMappingData, WslcContainerHandle};
#[cfg(windows)]
pub use error::{WslcDomainError, WslcError};
#[cfg(windows)]
pub use image::{ImageInfo, ImageProgress, OwnedImageProgress, WslcImageManager};
#[cfg(windows)]
pub use process::{ProcessBuilder, ProcessStreams, WslcProcessHandle};
#[cfg(windows)]
pub use registry::{AuthTokenResult, WslcRegistryManager};
#[cfg(windows)]
pub use session::{CrashDumpSubscription, SessionBuilder, WslcSessionHandle, default_session_root};
#[cfg(windows)]
pub use system::WslcSystem;
#[cfg(windows)]
pub use volume::{VhdVolumeOptions, WslcVolumeManager};

// 重新导出底层有用的枚举与常量
#[cfg(windows)]
pub use wslcsdk_sys::errors::*;
#[cfg(windows)]
pub use wslcsdk_sys::types::{
    WslcComponentFlags, WslcContainerFlags, WslcContainerNetworkingMode, WslcContainerPortMapping,
    WslcContainerStartFlags, WslcContainerState, WslcDeleteContainerFlags, WslcIdentityTokenType,
    WslcImageProgressStatus, WslcInstallOptions, WslcPortProtocol, WslcProcessFlags,
    WslcProcessIOHandle, WslcProcessState, WslcSessionFeatureFlags, WslcSessionTerminationReason,
    WslcSignal, WslcVersion, WslcVhdRequirementsFlags, WslcVhdType,
};

// ==================== 便民方法绑定扩展 ====================

#[cfg(windows)]
impl WslcSessionHandle {
    /// 快速打开该会话内已有的容器
    pub fn open_container(&self, name_or_id: &str) -> Result<WslcContainerHandle, WslcError> {
        WslcContainerHandle::open(self, name_or_id)
    }

    /// 列出当前会话内的所有镜像
    pub fn list_images(&self) -> Result<Vec<ImageInfo>, WslcError> {
        WslcImageManager::list_images(self)
    }

    /// 删除当前会话内的指定镜像
    pub fn delete_image(&self, name_or_id: &str) -> Result<(), WslcError> {
        WslcImageManager::delete_image(self, name_or_id)
    }

    /// 在当前会话内创建 VHD 存储卷
    pub fn create_vhd_volume(&self, options: &VhdVolumeOptions<'_>) -> Result<(), WslcError> {
        WslcVolumeManager::create_vhd_volume(self, options)
    }

    /// 删除当前会话内的 VHD 存储卷
    pub fn delete_vhd_volume(&self, name: &str) -> Result<(), WslcError> {
        WslcVolumeManager::delete_vhd_volume(self, name)
    }

    /// 登录镜像仓库并获取认证令牌
    pub fn authenticate(
        &self,
        server_address: &str,
        username: &str,
        password: &str,
    ) -> Result<AuthTokenResult, WslcError> {
        WslcRegistryManager::authenticate(self, server_address, username, password)
    }
}
