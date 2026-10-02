//! WSLC 安全错误类型与 Win32 HRESULT 解析器
//!
//! 错误模型分为两层：官方头文件中 19 个具名 HRESULT 收敛为 [`WslcDomainError`]，
//! 由 [`WslcError::Domain`] 变体包裹；文本编码、IO、JSON、任务调度等基础设施类
//! 错误保留在 [`WslcError`] 外层。调用方据此可快速区分「业务终态」与
//! 「可重试的系统抖动」，无需解析错误文本。

use crate::com_memory::ComWideString;
use thiserror::Error;
use windows_sys::Win32::Foundation::{
    CO_E_NOTINITIALIZED, E_ABORT, E_ACCESSDENIED, E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_NOTIMPL,
    E_OUTOFMEMORY, E_POINTER, E_UNEXPECTED, RPC_E_CHANGED_MODE, RPC_E_DISCONNECTED,
    RPC_E_WRONG_THREAD,
};
use windows_sys::core::HRESULT;
use wslcsdk_sys::errors::*;

/// WSLC 官方业务领域错误
///
/// 与官方头文件中的具名错误码一一对应。变体载荷为调用点上下文与官方返回的
/// 动态错误描述；需要进行程序化判断时请匹配变体本身，切勿解析该文本。
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WslcDomainError {
    /// 按名称、ID 或摘要引用的镜像在当前会话中不存在
    #[error("镜像未找到: {0}")]
    ImageNotFound(String),

    /// 容器 ID 前缀匹配到多个候选，无法确定目标容器
    #[error("容器 ID 前缀存在歧义，匹配到多个容器: {0}")]
    ContainerPrefixAmbiguous(String),

    /// 按名称或 ID 指定的容器在当前会话中不存在
    #[error("容器未找到: {0}")]
    ContainerNotFound(String),

    /// 按名称指定的存储卷在当前会话中不存在
    #[error("存储卷未找到: {0}")]
    VolumeNotFound(String),

    /// 要求容器处于运行状态的操作（如向其派生进程）失败
    #[error("容器未处于运行状态: {0}")]
    ContainerNotRunning(String),

    /// 要求容器处于停止状态的操作（如启动）因其已在运行而失败
    #[error("容器已处于运行状态: {0}")]
    ContainerAlreadyRunning(String),

    /// 会话名与系统内置名称冲突，改用其他名称即可
    #[error("会话名称属于系统保留名称: {0}")]
    SessionReserved(String),

    /// 会话名含非法字符，或不符合命名规则
    #[error("会话名称非法: {0}")]
    InvalidSessionName(String),

    /// 指定的网络不存在，可能已被删除或名称拼写有误
    #[error("网络未找到: {0}")]
    NetworkNotFound(String),

    /// 通过 Windows Update 补齐缺失组件时，组件搜索未能完成
    #[error("Windows Update 组件搜索失败: {0}")]
    WindowsUpdateSearchFailed(String),

    /// 已安装的 WSLC 版本低于本库要求，需先升级系统组件
    #[error("WSLC SDK 版本落后，需要更新系统组件: {0}")]
    SdkUpdateNeeded(String),

    /// 容器相关功能被系统或安全策略（如企业策略）禁用
    #[error("容器功能被系统或安全策略禁用: {0}")]
    ContainerDisabled(String),

    /// 镜像仓库访问被本机安全策略拦截，非凭据问题
    #[error("镜像仓库访问被安全策略拦截: {0}")]
    RegistryBlockedByPolicy(String),

    /// 存储卷存在但当前不可用，常见于被其他进程独占挂载
    #[error("存储卷当前不可用或被独占锁定: {0}")]
    VolumeNotAvailable(String),

    /// 指定的会话不存在或已被终止
    #[error("会话未找到: {0}")]
    SessionNotFound(String),

    /// WSL 虚拟机未启动，多数操作需先触发其启动
    #[error("WSL 虚拟机未处于运行状态: {0}")]
    VmNotRunning(String),

    /// 事件队列溢出，部分事件被丢弃；可增大缓冲区后重试
    #[error("事件队列溢出，部分事件已丢失: {0}")]
    EventsLost(String),

    /// 事件流已正常结束，通常意味着相关会话或容器已终止
    #[error("事件流已终止: {0}")]
    EventStreamFinished(String),

    /// 操作目标容器已被删除，无法再对其执行任何操作
    #[error("容器已被删除: {0}")]
    ContainerDeleted(String),
}

impl WslcDomainError {
    /// 依据官方 HRESULT 解析业务领域错误；非官方业务码返回 `None`
    pub fn from_hresult(hr: HRESULT, context: impl Into<String>) -> Option<Self> {
        let message = context.into();
        Some(match hr {
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
            _ => return None,
        })
    }
}

/// WSLC SDK 统一错误类型
///
/// 分两层组织：官方业务领域错误由 [`WslcError::Domain`] 包裹，其余均为基础设施类
/// 错误。本枚举标记为 `#[non_exhaustive]`，后续新增变体不构成破坏性变更。
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WslcError {
    /// WSLC 官方业务领域错误
    #[error(transparent)]
    Domain(#[from] WslcDomainError),

    /// 未能识别的 Windows HRESULT
    ///
    /// 载荷为原始错误码，以及「标准错误名 + 调用点上下文」的描述。常见标准码会被
    /// 还原为可读名称，避免笼统的「系统调用失败」误导排查方向。
    #[error("Windows 调用失败，HRESULT: 0x{0:08X}，详情: {1}")]
    Hresult(u32, String),

    /// Win32 系统调用失败（错误码来自 `GetLastError`）
    ///
    /// 与 [`Self::Hresult`] 分列：Win32 错误码与 HRESULT 是两套编码体系，
    /// 前者为小整数（`6` = ERROR_INVALID_HANDLE），后者最高位表示严重性。
    /// 若把 Win32 码塞进 `Hresult`，会按 `0x00000006` 呈现——最高位为 0，
    /// 与本库「hr >= 0 即成功」的判定方向相反，极易被误读为一次成功的调用。
    #[error("Win32 调用失败，错误码: {0}，详情: {1}")]
    Win32(u32, String),

    /// SDK 返回了违背其公开契约的非预期结果
    ///
    /// 例如接口声明返回成功状态，却未按要求给出输出指针。此类情形无法归入任何
    /// 官方错误码，也不属于调用方的入参问题，故单列一类以便上层识别。
    #[error("SDK 返回非预期结果: {0}")]
    UnexpectedSdkResult(String),

    /// 空指针或无效句柄
    #[error("空指针或无效句柄")]
    InvalidHandle,

    /// 文本编码转换失败
    #[error("文本编码转换失败: {0}")]
    Utf8Error(String),

    /// 字符串中包含非法空字符 (Nul Byte)
    #[error("字符串中包含非法空字符 (Nul Byte): {0}")]
    NulError(String),

    /// JSON 序列化或反序列化失败
    #[error("JSON 序列化或反序列化失败: {0}")]
    JsonError(String),

    /// 异步任务调度或执行失败
    #[error("异步任务执行失败: {0}")]
    TaskJoin(String),

    /// 一次性资源已被消费，无可重复领取的剩余
    ///
    /// 单列一类而非复用 [`Self::TaskJoin`]：该变体描述的是**调用方重复调用**
    /// 所触发的业务终态（同一退出码只通知一次），而非异步任务本身失败。
    /// 两者若混同，调用方按「任务可重试」处理就会陷入无效重试。
    #[error("{0}已被消费，不可重复获取")]
    AlreadyConsumed(String),

    /// 内部异步通知通道异常终止
    ///
    /// 与 [`Self::TaskJoin`] 区分：此处并非任务执行出错，而是承载通知的
    /// 通道两端失联，调用方重试无意义。
    #[error("内部通知通道异常终止: {0}")]
    ChannelTerminated(String),

    /// I/O 操作失败
    #[error("I/O 操作失败: {0}")]
    Io(String),

    /// 客户端侧配置、环境变量或入参校验失败
    #[error("配置或环境变量非法: {0}")]
    InvalidConfiguration(String),
}

impl WslcError {
    /// 根据 HRESULT 与上下文描述解析为强类型错误
    ///
    /// 优先匹配官方具名业务错误码；未识别时归入 [`WslcError::Hresult`]，并在描述中
    /// 尽可能还原标准错误名称。
    pub fn from_hresult(hr: HRESULT, context_desc: impl Into<String>) -> Self {
        let message = context_desc.into();

        if let Some(domain) = WslcDomainError::from_hresult(hr, message.clone()) {
            return Self::Domain(domain);
        }

        let code = hr as u32;
        match hresult_name(code) {
            Some(name) => Self::Hresult(code, format!("{name}，{message}")),
            None => Self::Hresult(code, message),
        }
    }

    /// 检查 HRESULT，若小于 0 则解析为带上下文描述的领域错误
    pub(crate) fn check_hr(hr: HRESULT, context_desc: impl Into<String>) -> Result<(), Self> {
        if hr >= 0 {
            Ok(())
        } else {
            Err(Self::from_hresult(hr, context_desc))
        }
    }

    /// 解析 HRESULT 及官方返回的动态宽字符串错误信息
    ///
    /// `msg_ptr` 交由 [`ComWideString`] 托管：无论本方法走哪个分支，COM 堆内存
    /// 都会被自动释放，调用方无需也不应再手工调用 `CoTaskMemFree`。
    pub(crate) unsafe fn from_hresult_and_raw_msg(hr: HRESULT, msg_ptr: *mut u16) -> Self {
        // SAFETY: 调用方均为官方 API 的 _Outptr_opt_result_z_ 输出参数，
        // 所有权在交接给本方法的那一刻即转由 Rust 侧接管
        let message = unsafe { ComWideString::from_raw(msg_ptr) }
            .map(|msg| msg.to_string_lossy())
            .unwrap_or_default();

        Self::from_hresult(hr, message)
    }

    /// 读取 `GetLastError` 并包装为带上下文描述的 [`WslcError::Win32`]
    ///
    /// # Safety
    ///
    /// 必须**紧随**失败的 Win32 调用之后执行：任何其他 Win32 调用都可能
    /// 覆盖线程内的最后一个错误码，使读到的值不属于本次失败。
    pub(crate) unsafe fn last_win32_error(context_desc: impl Into<String>) -> Self {
        // SAFETY: 由调用方保证本方法紧随失败的 Win32 调用，
        // 且该 API 无参数、无副作用。
        let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        Self::Win32(code, context_desc.into())
    }

    /// 检查 HRESULT，若失败则解析错误，成功则返回 `Ok(())`
    ///
    /// 无论成功与否，`msg_ptr` 都会被 [`ComWideString`] 自动释放。
    pub(crate) unsafe fn check(hr: HRESULT, msg_ptr: *mut u16) -> Result<(), Self> {
        if hr >= 0 {
            // SAFETY: msg_ptr 为官方输出的宽字符串指针，
            // 本函数负责接管其所有权并在读取后释放。
            drop(unsafe { ComWideString::from_raw(msg_ptr) });
            Ok(())
        } else {
            // SAFETY: msg_ptr 为官方输出的宽字符串指针，
            // 本函数负责接管其所有权并在读取后释放。
            Err(unsafe { Self::from_hresult_and_raw_msg(hr, msg_ptr) })
        }
    }
}

/// 将常见标准 HRESULT 还原为可读名称，未知错误码返回 `None`
///
/// 覆盖 WSLC 调用链路上最可能遇到的标准错误码，用于替代原先笼统的
/// 「Win32 系统调用失败」描述。
fn hresult_name(code: u32) -> Option<&'static str> {
    // 还原为 HRESULT 后即可直接使用 windows-sys 导出的标准常量做模式匹配
    Some(match code as HRESULT {
        E_ACCESSDENIED => "E_ACCESSDENIED (拒绝访问)",
        E_INVALIDARG => "E_INVALIDARG (参数或标志位不合法)",
        E_OUTOFMEMORY => "E_OUTOFMEMORY (内存不足)",
        E_FAIL => "E_FAIL (未指定的失败)",
        E_NOTIMPL => "E_NOTIMPL (未实现该功能)",
        E_NOINTERFACE => "E_NOINTERFACE (不支持所请求的接口)",
        E_POINTER => "E_POINTER (无效指针)",
        E_UNEXPECTED => "E_UNEXPECTED (非预期状态)",
        E_ABORT => "E_ABORT (操作已中止)",
        CO_E_NOTINITIALIZED => "CO_E_NOTINITIALIZED (COM 尚未初始化)",
        RPC_E_CHANGED_MODE => "RPC_E_CHANGED_MODE (COM 套间模型冲突，当前线程已被初始化为 STA)",
        RPC_E_WRONG_THREAD => "RPC_E_WRONG_THREAD (在错误的线程上调用)",
        RPC_E_DISCONNECTED => "RPC_E_DISCONNECTED (对象已与调用方断开连接)",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方 19 个具名错误码，用于批量验证映射行为
    const ALL_DOMAIN_CODES: [HRESULT; 19] = [
        WSLC_E_IMAGE_NOT_FOUND,
        WSLC_E_CONTAINER_PREFIX_AMBIGUOUS,
        WSLC_E_CONTAINER_NOT_FOUND,
        WSLC_E_VOLUME_NOT_FOUND,
        WSLC_E_CONTAINER_NOT_RUNNING,
        WSLC_E_CONTAINER_IS_RUNNING,
        WSLC_E_SESSION_RESERVED,
        WSLC_E_INVALID_SESSION_NAME,
        WSLC_E_NETWORK_NOT_FOUND,
        WSLC_E_WU_SEARCH_FAILED,
        WSLC_E_SDK_UPDATE_NEEDED,
        WSLC_E_CONTAINER_DISABLED,
        WSLC_E_REGISTRY_BLOCKED_BY_POLICY,
        WSLC_E_VOLUME_NOT_AVAILABLE,
        WSLC_E_SESSION_NOT_FOUND,
        WSLC_E_VM_NOT_RUNNING,
        WSLC_E_EVENTS_LOST,
        WSLC_E_EVENT_STREAM_FINISHED,
        WSLC_E_CONTAINER_DELETED,
    ];

    #[test]
    fn test_domain_error_mapping_with_raw_message() {
        // SAFETY: msg_ptr 为官方输出的宽字符串指针，
        // 本函数负责接管其所有权并在读取后释放。
        unsafe {
            let err = WslcError::from_hresult_and_raw_msg(
                WSLC_E_CONTAINER_NOT_FOUND,
                std::ptr::null_mut(),
            );
            assert_eq!(
                err,
                WslcError::Domain(WslcDomainError::ContainerNotFound(String::new()))
            );

            let err2 =
                WslcError::from_hresult_and_raw_msg(WSLC_E_IMAGE_NOT_FOUND, std::ptr::null_mut());
            assert_eq!(
                err2,
                WslcError::Domain(WslcDomainError::ImageNotFound(String::new()))
            );

            let err3 =
                WslcError::from_hresult_and_raw_msg(WSLC_E_VM_NOT_RUNNING, std::ptr::null_mut());
            assert_eq!(
                err3,
                WslcError::Domain(WslcDomainError::VmNotRunning(String::new()))
            );
        }

        let err_direct = WslcError::from_hresult(WSLC_E_CONTAINER_NOT_FOUND, "测试上下文");
        assert_eq!(
            err_direct,
            WslcError::Domain(WslcDomainError::ContainerNotFound("测试上下文".to_string()))
        );
    }

    #[test]
    fn test_every_official_code_lands_in_domain_layer() {
        // 全部官方具名错误码都必须归入 Domain 层，不得退化为通用 Hresult，
        // 否则调用方无法区分业务终态与基础设施故障
        for code in ALL_DOMAIN_CODES {
            let err = WslcError::from_hresult(code, "上下文");
            assert!(
                matches!(err, WslcError::Domain(_)),
                "错误码 0x{:08X} 未映射为领域错误: {err:?}",
                code as u32
            );
        }

        // 反向校验：非官方码不得被误判为领域错误
        assert!(WslcDomainError::from_hresult(0x8004_9999_u32 as HRESULT, "x").is_none());
    }

    #[test]
    fn test_standard_hresult_reported_by_name() {
        let err = WslcError::from_hresult(E_INVALIDARG, "设置容器标志位失败");
        match err {
            WslcError::Hresult(code, detail) => {
                assert_eq!(code, E_INVALIDARG as u32);
                assert!(detail.contains("E_INVALIDARG"), "实际描述: {detail}");
                assert!(detail.contains("设置容器标志位失败"), "实际描述: {detail}");
            }
            other => panic!("预期返回 Hresult 变体，实际为: {other:?}"),
        }
    }

    #[test]
    fn test_unknown_hresult_keeps_raw_code_and_context() {
        let err = WslcError::from_hresult(0x8004_9999_u32 as HRESULT, "未知调用失败");
        match err {
            WslcError::Hresult(code, detail) => {
                assert_eq!(code, 0x8004_9999);
                assert_eq!(detail, "未知调用失败");
            }
            other => panic!("预期返回 Hresult 变体，实际为: {other:?}"),
        }
    }

    #[test]
    fn test_check_hr_boundary() {
        assert!(WslcError::check_hr(0, "S_OK").is_ok());
        assert!(WslcError::check_hr(1, "S_FALSE 亦视为成功").is_ok());
        assert!(WslcError::check_hr(WSLC_E_CONTAINER_NOT_FOUND, "失败").is_err());
    }

    #[test]
    fn test_hresult_name_coverage() {
        for code in [
            E_ACCESSDENIED,
            E_FAIL,
            CO_E_NOTINITIALIZED,
            RPC_E_CHANGED_MODE,
        ] {
            assert!(
                hresult_name(code as u32).is_some(),
                "0x{:08X} 应有可读名称",
                code as u32
            );
        }
        assert!(hresult_name(0x8004_9999).is_none());
    }

    /// 业务终态与基础设施抖动必须可编程区分
    ///
    /// 这三个变体都曾在实现中被`TaskJoin` 混同。若调用方按「任务失败」
    /// 处理「通知已被消费」，会陷入无效重试——退出码通知本就只投递一次，
    /// 重试永远不会成功。
    #[test]
    fn test_business_terminal_states_are_distinguishable_from_task_failure() {
        // 重复消费：业务终态，调用方重试无意义
        let consumed = WslcError::AlreadyConsumed("进程退出通知".to_string());
        assert!(matches!(consumed, WslcError::AlreadyConsumed(_)));
        assert!(!matches!(consumed, WslcError::TaskJoin(_)));
        assert_eq!(
            consumed.to_string(),
            "进程退出通知已被消费，不可重复获取",
            "错误描述须指出被消费的资源"
        );

        // 通道失联：与任务执行失败区分开
        let channel = WslcError::ChannelTerminated("进程退出通知通道".to_string());
        assert!(matches!(channel, WslcError::ChannelTerminated(_)));
        assert!(!matches!(channel, WslcError::TaskJoin(_)));

        // 三者互不相等，模式匹配可无歧义地区分
        assert_ne!(consumed, channel);
        assert_ne!(consumed, WslcError::TaskJoin("x".to_string()));
    }
}
