//! WSLC 回调函数指针类型与镜像操作选项结构体

use super::*;
use core::ffi::c_void;
use windows_sys::core::HRESULT;

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
