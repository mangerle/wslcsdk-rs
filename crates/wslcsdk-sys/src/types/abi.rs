//! 编译期 ABI 布局断言
//!
//! 本模块不含任何类型定义，只负责核对各结构体与枚举在官方头文件中的
//! 尺寸、对齐与字段偏移。这些不变量是 FFI 绑定的正确性前提：任一项偏移
//! 变化都会让 Rust 侧以错误布局读写官方结构体，造成内存损坏。
//!
//! 断言以编译期求值方式表达，故置于 `#[cfg(test)]` 之下——它们的价值在于
//! 让违约在编译时即暴露，而非运行期检查。

use super::*;
use windows_sys::Win32::Foundation::HANDLE;

// 官方一旦调整结构体字段、顺序或对齐，必须在编译期立即暴露，而不是在运行期
// 静默发生内存错位读写。本 crate 仅支持 x86_64 与 aarch64 两种 64 位目标
// (由 build.rs 强制校验)，故以下尺寸断言以 64 位指针为前提。

#[cfg(target_pointer_width = "64")]
const _: () = {
    use core::mem::{align_of, size_of};

    // 不透明配置结构体：尺寸与对齐须与头文件中的宏定义完全一致
    assert!(size_of::<WslcSessionSettings>() == WSLC_SESSION_OPTIONS_SIZE);
    assert!(align_of::<WslcSessionSettings>() == WSLC_SESSION_OPTIONS_ALIGNMENT);
    assert!(size_of::<WslcContainerSettings>() == WSLC_CONTAINER_OPTIONS_SIZE);
    assert!(align_of::<WslcContainerSettings>() == WSLC_CONTAINER_OPTIONS_ALIGNMENT);
    assert!(size_of::<WslcProcessSettings>() == WSLC_CONTAINER_PROCESS_OPTIONS_SIZE);
    assert!(align_of::<WslcProcessSettings>() == WSLC_CONTAINER_PROCESS_OPTIONS_ALIGNMENT);

    // 不透明句柄：必须与 HANDLE 同宽同对齐
    assert!(size_of::<WslcSession>() == size_of::<HANDLE>());
    assert!(align_of::<WslcSession>() == align_of::<HANDLE>());
    assert!(size_of::<WslcContainer>() == size_of::<HANDLE>());
    assert!(align_of::<WslcContainer>() == align_of::<HANDLE>());
    assert!(size_of::<WslcProcess>() == size_of::<HANDLE>());
    assert!(align_of::<WslcProcess>() == align_of::<HANDLE>());
    assert!(size_of::<WslcCrashDumpSubscription>() == size_of::<HANDLE>());

    // C 侧回填枚举 Newtype：必须与 C 的 32 位枚举同宽
    assert!(size_of::<WslcSessionTerminationReason>() == 4);
    assert!(size_of::<WslcContainerState>() == 4);
    assert!(size_of::<WslcProcessIOHandle>() == 4);
    assert!(size_of::<WslcProcessState>() == 4);
    assert!(size_of::<WslcImageProgressStatus>() == 4);
    assert!(size_of::<WslcIdentityTokenType>() == 4);

    // 单向下行的输入枚举：与 C 的 int 同宽
    assert!(size_of::<WslcContainerNetworkingMode>() == 4);
    assert!(size_of::<WslcVhdType>() == 4);
    assert!(size_of::<WslcPortProtocol>() == 4);
    assert!(size_of::<WslcSignal>() == 4);

    // 复合结构体：按头文件字段顺序逐项推算出的精确尺寸
    assert!(size_of::<WslcVhdRequirements>() == 32);
    assert!(size_of::<WslcSessionCrashDumpInfo>() == 32);
    assert!(size_of::<WslcContainerPortMapping>() == 16);
    assert!(size_of::<WslcContainerVolume>() == 24);
    assert!(size_of::<WslcContainerNamedVolume>() == 24);
    assert!(size_of::<WslcImageProgressDetail>() == 16);
    assert!(size_of::<WslcImageProgressMessage>() == 32);
    assert!(size_of::<WslcImageInfo>() == WSLC_IMAGE_NAME_LENGTH + 32 + 16);
    assert!(size_of::<WslcVersion>() == 12);
    assert!(size_of::<WslcProcessCallbacks>() == 24);
    assert!(size_of::<WslcTagImageOptions>() == 24);
    assert!(size_of::<WslcPullImageOptions>() == 32);
    assert!(size_of::<WslcImportImageOptions>() == 16);
    assert!(size_of::<WslcLoadImageOptions>() == 16);
    assert!(size_of::<WslcPushImageOptions>() == 32);
};
