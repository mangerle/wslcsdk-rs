//! Microsoft.WSL.Containers 3.0.1 原始数据结构、枚举与回调函数定义
//!
//! 本模块仅收纳由 Rust 侧单向传入 C 库的参数类型。凡取值可能由 C 库
//! 回填（`out` 参数或回调回传）的枚举，一律经由 `c_backfilled` 子模块
//! 以 Newtype 形式承载，避免官方新增枚举成员时触发未定义行为。
//!
//! # 子模块划分
//!
//! - `handles`：不透明句柄、配置结构体与单向下行枚举；
//! - `flags`：各领域标志位与安装选项常量；
//! - `composite`：端口映射、卷挂载与镜像元数据等复合结构体；
//! - `callbacks`：回调函数指针类型与镜像操作选项结构体；
//! - `c_backfilled`：取值可能由 C 库回填的枚举（Newtype 承载）；
//! - `abi`：编译期 ABI 布局断言，仅测试期参与编译。

mod callbacks;
mod composite;
mod flags;
mod handles;

/// 编译期 ABI 布局断言：仅在测试期参与编译，使违约在编译时即暴露
#[cfg(test)]
mod abi;

mod c_backfilled;

pub use c_backfilled::*;
pub use callbacks::*;
pub use composite::*;
pub use flags::*;
pub use handles::*;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opaque_settings_default_is_zeroed() {
        assert!(
            WslcSessionSettings::default()
                ._opaque
                .iter()
                .all(|b| *b == 0)
        );
        assert!(
            WslcContainerSettings::default()
                ._opaque
                .iter()
                .all(|b| *b == 0)
        );
        assert!(
            WslcProcessSettings::default()
                ._opaque
                .iter()
                .all(|b| *b == 0)
        );
    }

    #[test]
    fn test_handle_newtype_null_semantics() {
        assert!(WslcSession::NULL.is_null());
        assert!(WslcSession::default().is_null());
        assert!(WslcContainer::NULL.is_null());
        assert!(WslcContainer::default().is_null());
        assert!(WslcProcess::NULL.is_null());
        assert!(WslcProcess::default().is_null());
        assert!(WslcCrashDumpSubscription::NULL.is_null());
        assert!(WslcCrashDumpSubscription::default().is_null());
    }

    #[test]
    fn test_c_backfilled_enum_known_name_and_debug() {
        assert_eq!(WslcContainerState::Running.name(), Some("Running"));
        assert_eq!(WslcProcessState::Signalled.name(), Some("Signalled"));
        assert_eq!(format!("{:?}", WslcContainerState::Exited), "Exited");
        assert_eq!(
            format!("{:?}", WslcImageProgressStatus::Downloading),
            "Downloading"
        );
    }

    #[test]
    fn test_c_backfilled_enum_unknown_value_is_safe() {
        // 模拟官方新增枚举成员：任意 u32 都必须是合法的 Rust 值，且可被识别为未知
        let future_state = WslcContainerState(9_999);
        assert_eq!(future_state.name(), None);
        assert_eq!(format!("{future_state:?}"), "Unknown(9999)");
        assert_ne!(future_state, WslcContainerState::RUNNING);
    }

    #[test]
    fn test_c_backfilled_enum_conversions_are_lossless() {
        let raw: u32 = WslcProcessIOHandle::STDERR.into();
        assert_eq!(raw, 2);
        assert_eq!(WslcProcessIOHandle::from(raw), WslcProcessIOHandle::STDERR);

        // 官方新增取值也必须能无损往返
        let unknown = WslcIdentityTokenType::from(4242_u32);
        assert_eq!(u32::from(unknown), 4242);
    }

    #[test]
    fn test_default_values_match_official_zero_variant() {
        assert_eq!(WslcContainerState::default(), WslcContainerState::INVALID);
        assert_eq!(WslcProcessState::default(), WslcProcessState::UNKNOWN);
        assert_eq!(
            WslcSessionTerminationReason::default(),
            WslcSessionTerminationReason::UNKNOWN
        );
        assert_eq!(
            WslcImageProgressStatus::default(),
            WslcImageProgressStatus::UNKNOWN
        );
        assert_eq!(
            WslcIdentityTokenType::default(),
            WslcIdentityTokenType::UNKNOWN
        );
        assert_eq!(WslcProcessIOHandle::default(), WslcProcessIOHandle::STDIN);
    }
}
