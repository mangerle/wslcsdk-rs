//! Microsoft.WSL.Containers (WSLC) 官方 SDK 安全 Rust 抽象门面
//!
//! 基于官方 3.0.1 GA SDK 构建，提供具备 RAII 自动资源释放、强类型错误模型与并发安全保障的统一客户端接口。

pub mod async_ops;
pub mod com;
pub mod container;
pub mod error;
pub mod image;
pub mod process;
pub mod registry;
pub mod session;
pub mod system;
pub mod volume;

pub use com::{ComGuard, try_initialize_mta};

pub use container::{ContainerBuilder, WslcContainerHandle};
pub use error::WslcError;
pub use image::{ImageInfo, ImageProgress, WslcImageManager};
pub use process::{ProcessBuilder, ProcessStreams, WslcProcessHandle};
pub use registry::{AuthTokenResult, WslcRegistryManager};
pub use session::{CrashDumpSubscription, SessionBuilder, WslcSessionHandle};
pub use system::WslcSystem;
pub use volume::{VhdVolumeOptions, WslcVolumeManager};

// 重新导出底层有用的枚举与常量
pub use wslcsdk_sys::errors::*;
pub use wslcsdk_sys::types::{
    WslcComponentFlags, WslcContainerFlags, WslcContainerNetworkingMode, WslcContainerPortMapping,
    WslcContainerStartFlags, WslcContainerState, WslcDeleteContainerFlags, WslcIdentityTokenType,
    WslcImageProgressStatus, WslcInstallOptions, WslcPortProtocol, WslcProcessFlags,
    WslcProcessIOHandle, WslcProcessState, WslcSessionFeatureFlags, WslcSessionTerminationReason,
    WslcSignal, WslcVersion, WslcVhdRequirementsFlags, WslcVhdType,
};

// ==================== 便民方法绑定扩展 ====================

impl session::WslcSessionHandle {
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
        volume::WslcVolumeManager::create_vhd_volume(self, options)
    }

    /// 删除当前会话内的 VHD 存储卷
    pub fn delete_vhd_volume(&self, name: &str) -> Result<(), WslcError> {
        volume::WslcVolumeManager::delete_vhd_volume(self, name)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_version_or_availability() {
        // 在安装了 WSL 3.0.1+ 的系统上，WslcSystem::get_version 应当成功返回版本号
        match WslcSystem::get_version() {
            Ok(v) => {
                assert!(v.major > 0 || v.minor > 0 || v.revision > 0);
            }
            Err(e) => {
                // 如果运行在非支持的环境，应当返回明确的 Win32 错误而不是崩溃
                match e {
                    WslcError::Win32(code, _) => assert!(code != 0),
                    _ => panic!("预期返回 Win32 错误，实际为: {e:?}"),
                }
            }
        }
    }

    #[test]
    fn test_error_code_mapping() {
        unsafe {
            let err = WslcError::from_hresult_and_raw_msg(
                WSLC_E_CONTAINER_NOT_FOUND,
                std::ptr::null_mut(),
            );
            assert_eq!(err, WslcError::ContainerNotFound(String::new()));

            let err2 =
                WslcError::from_hresult_and_raw_msg(WSLC_E_IMAGE_NOT_FOUND, std::ptr::null_mut());
            assert_eq!(err2, WslcError::ImageNotFound(String::new()));

            let err3 =
                WslcError::from_hresult_and_raw_msg(WSLC_E_VM_NOT_RUNNING, std::ptr::null_mut());
            assert_eq!(err3, WslcError::VmNotRunning(String::new()));
        }
    }

    #[test]
    fn test_container_builder_parameters() {
        let builder = ContainerBuilder::new("ubuntu:latest")
            .name("test-container")
            .auto_remove(true)
            .add_port_mapping(8080, 80, WslcPortProtocol::Tcp);

        assert_eq!(
            builder.get_flags() & wslcsdk_sys::types::WSLC_CONTAINER_FLAG_AUTO_REMOVE,
            1
        );
    }

    #[test]
    fn test_com_guard_mta() {
        let guard_result = try_initialize_mta();
        assert!(guard_result.is_ok());
    }

    #[test]
    fn test_registry_mirror_resolution() {
        let res = registry::resolve_image_reference_with("ubuntu:latest", |k| {
            if k == "WSLC_REGISTRY_MIRROR" {
                Some("mirror.example.com".to_string())
            } else {
                None
            }
        });
        assert_eq!(res.unwrap(), "mirror.example.com/ubuntu:latest");

        let res_ghcr = registry::resolve_image_reference_with("ghcr.io/org/repo:1.0", |k| {
            if k == "WSLC_REGISTRY_MIRROR_GHCR_IO" {
                Some("ghcr-mirror.example.com".to_string())
            } else {
                None
            }
        });
        assert_eq!(res_ghcr.unwrap(), "ghcr-mirror.example.com/org/repo:1.0");
    }

    #[test]
    fn test_default_session_path() {
        let builder_res = SessionBuilder::new_default("test-vessel-session");
        assert!(builder_res.is_ok());
    }

    #[test]
    fn test_process_streaming_io_setup() {
        let builder = ProcessBuilder::new().command(&["/bin/sh", "-c", "echo hello"]);
        let mut streams = builder.with_streaming_io().1;
        // 验证通道已建立且未提前收到退出信号
        assert!(streams.exit_rx.try_recv().is_err());
    }

    #[test]
    fn test_system_is_sdk_available() {
        // 在安装了 WSL 的系统上应当能安全探测
        let _available = WslcSystem::is_sdk_available();
    }

    #[test]
    fn test_to_wide_null_validation() {
        let valid = session::to_wide_null("valid_path");
        assert!(valid.is_ok());

        let invalid = session::to_wide_null("invalid\0path");
        assert!(invalid.is_err());
    }

    #[test]
    fn test_empty_mirror_returns_invalid_configuration() {
        let res = registry::resolve_image_reference_with("ubuntu:latest", |k| {
            if k == "WSLC_REGISTRY_MIRROR" {
                Some("   ".to_string())
            } else {
                None
            }
        });
        match res {
            Err(WslcError::InvalidConfiguration(_)) => {}
            other => panic!("预期返回 InvalidConfiguration，实际为: {other:?}"),
        }
    }

    #[test]
    fn test_vhd_volume_options_builder() {
        let options = VhdVolumeOptions {
            name: "test-volume",
            size_bytes: 1024 * 1024,
            vhd_type: WslcVhdType::Dynamic,
            owner: Some((1000, 1000)),
        };
        assert_eq!(options.name, "test-volume");
        assert_eq!(options.size_bytes, 1024 * 1024);
    }

    #[test]
    fn test_handle_newtypes_safety_and_null_checks() {
        assert!(wslcsdk_sys::WslcSession::NULL.is_null());
        assert!(wslcsdk_sys::WslcContainer::default().is_null());
        assert!(wslcsdk_sys::WslcProcess::NULL.is_null());
        assert!(wslcsdk_sys::WslcCrashDumpSubscription::default().is_null());
    }

    #[test]
    fn test_process_streaming_io_early_drop_safety() {
        let builder = ProcessBuilder::new().command(&["/bin/sh", "-c", "echo hello"]);
        let (builder, streams) = builder.with_streaming_io();
        // 验证提前 drop 流式接收端不破坏 builder 内部状态
        drop(streams);
        let res = builder.build_raw_settings();
        assert!(res.is_ok());
    }
}
