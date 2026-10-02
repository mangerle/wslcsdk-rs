//! WSLC 安全错误类型与 Win32 HRESULT 解析器

use std::slice;
use thiserror::Error;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::core::HRESULT;
use wslcsdk_sys::errors::*;

/// WSLC 业务领域强类型错误枚举
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum WslcError {
    #[error("镜像未找到: {0}")]
    ImageNotFound(String),

    #[error("容器 ID 前缀存在歧义，匹配到多个容器: {0}")]
    ContainerPrefixAmbiguous(String),

    #[error("容器未找到: {0}")]
    ContainerNotFound(String),

    #[error("存储卷未找到: {0}")]
    VolumeNotFound(String),

    #[error("容器未处于运行状态: {0}")]
    ContainerNotRunning(String),

    #[error("容器已处于运行状态: {0}")]
    ContainerAlreadyRunning(String),

    #[error("会话名称属于系统保留名称: {0}")]
    SessionReserved(String),

    #[error("会话名称非法: {0}")]
    InvalidSessionName(String),

    #[error("网络未找到: {0}")]
    NetworkNotFound(String),

    #[error("Windows Update 组件搜索失败: {0}")]
    WindowsUpdateSearchFailed(String),

    #[error("WSLC SDK 版本落后，需要更新系统组件: {0}")]
    SdkUpdateNeeded(String),

    #[error("容器功能被系统或安全策略禁用: {0}")]
    ContainerDisabled(String),

    #[error("镜像仓库访问被安全策略拦截: {0}")]
    RegistryBlockedByPolicy(String),

    #[error("存储卷当前不可用或被独占锁定: {0}")]
    VolumeNotAvailable(String),

    #[error("会话未找到: {0}")]
    SessionNotFound(String),

    #[error("WSL 虚拟机未处于运行状态: {0}")]
    VmNotRunning(String),

    #[error("事件队列溢出，部分事件已丢失: {0}")]
    EventsLost(String),

    #[error("事件流已终止: {0}")]
    EventStreamFinished(String),

    #[error("容器已被删除: {0}")]
    ContainerDeleted(String),

    #[error("Win32 系统调用失败，HRESULT: 0x{0:08X}，详情: {1}")]
    Win32(u32, String),

    #[error("空指针或无效句柄")]
    InvalidHandle,

    #[error("文本编码转换失败: {0}")]
    Utf8Error(String),

    #[error("JSON 序列化或反序列化失败: {0}")]
    JsonError(String),

    #[error("异步任务执行失败: {0}")]
    TaskJoin(String),

    #[error("I/O 操作失败: {0}")]
    Io(String),

    #[error("配置或环境变量非法: {0}")]
    InvalidConfiguration(String),
}

impl WslcError {
    /// 解析 HRESULT 及官方返回的动态宽字符串错误信息，并确保释放 COM 堆内存
    pub(crate) unsafe fn from_hresult_and_raw_msg(hr: HRESULT, msg_ptr: *mut u16) -> Self {
        let message = if !msg_ptr.is_null() {
            let mut len = 0;
            unsafe {
                while *msg_ptr.add(len) != 0 {
                    len += 1;
                }
                let slice = slice::from_raw_parts(msg_ptr, len);
                let s = String::from_utf16_lossy(slice);
                CoTaskMemFree(msg_ptr.cast());
                s
            }
        } else {
            String::new()
        };

        match hr {
            WSLC_E_IMAGE_NOT_FOUND => Self::ImageNotFound(message),
            WSLC_E_CONTAINER_PREFIX_AMBIGUOUS => Self::ContainerPrefixAmbiguous(message),
            WSLC_E_CONTAINER_NOT_FOUND => Self::ContainerNotFound(message),
            WSLC_E_VOLUME_NOT_FOUND => Self::VolumeNotFound(message),
            WSLC_E_CONTAINER_NOT_RUNNING => Self::ContainerNotRunning(message),
            WSLC_E_CONTAINER_IS_RUNNING => Self::ContainerAlreadyRunning(message),
            WSLC_E_SESSION_RESERVED => Self::SessionReserved(message),
            WSLC_E_INVALID_SESSION_NAME => Self::InvalidSessionName(message),
            WSLC_E_NETWORK_NOT_FOUND => Self::NetworkNotFound(message),
            WSLC_E_WU_SEARCH_FAILED => Self::WindowsUpdateSearchFailed(message),
            WSLC_E_SDK_UPDATE_NEEDED => Self::SdkUpdateNeeded(message),
            WSLC_E_CONTAINER_DISABLED => Self::ContainerDisabled(message),
            WSLC_E_REGISTRY_BLOCKED_BY_POLICY => Self::RegistryBlockedByPolicy(message),
            WSLC_E_VOLUME_NOT_AVAILABLE => Self::VolumeNotAvailable(message),
            WSLC_E_SESSION_NOT_FOUND => Self::SessionNotFound(message),
            WSLC_E_VM_NOT_RUNNING => Self::VmNotRunning(message),
            WSLC_E_EVENTS_LOST => Self::EventsLost(message),
            WSLC_E_EVENT_STREAM_FINISHED => Self::EventStreamFinished(message),
            WSLC_E_CONTAINER_DELETED => Self::ContainerDeleted(message),
            other => Self::Win32(other as u32, message),
        }
    }

    /// 检查 HRESULT，若失败则解析错误，成功则返回 Ok(())
    pub(crate) unsafe fn check(hr: HRESULT, msg_ptr: *mut u16) -> Result<(), Self> {
        if hr >= 0 {
            if !msg_ptr.is_null() {
                unsafe {
                    CoTaskMemFree(msg_ptr.cast());
                }
            }
            Ok(())
        } else {
            Err(unsafe { Self::from_hresult_and_raw_msg(hr, msg_ptr) })
        }
    }
}
