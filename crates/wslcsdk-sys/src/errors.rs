//! Microsoft.WSL.Containers 3.0.1 错误码定义
//!
//! 官方定义基址为 WSLC_E_BASE = 0x0600，采用 Windows ITF 设施的 HRESULT 格式。

use windows_sys::core::HRESULT;

/// WSLC 基础错误偏移码 (0x0600)
pub const WSLC_E_BASE: u32 = 0x0600;

/// 构造 FACILITY_ITF 类型的 HRESULT
const fn make_hresult(code: u32) -> HRESULT {
    // SEVERITY_ERROR (1) | FACILITY_ITF (4) << 16 | code
    (0x8004_0000 | code) as HRESULT
}

/// 镜像未找到 (0x80040601)
pub const WSLC_E_IMAGE_NOT_FOUND: HRESULT = make_hresult(WSLC_E_BASE + 1);

/// 容器 ID 前缀存在歧义，匹配到多个容器 (0x80040602)
pub const WSLC_E_CONTAINER_PREFIX_AMBIGUOUS: HRESULT = make_hresult(WSLC_E_BASE + 2);

/// 容器未找到 (0x80040603)
pub const WSLC_E_CONTAINER_NOT_FOUND: HRESULT = make_hresult(WSLC_E_BASE + 3);

/// 卷未找到 (0x80040604)
pub const WSLC_E_VOLUME_NOT_FOUND: HRESULT = make_hresult(WSLC_E_BASE + 4);

/// 容器当前未处于运行状态 (0x80040605)
pub const WSLC_E_CONTAINER_NOT_RUNNING: HRESULT = make_hresult(WSLC_E_BASE + 5);

/// 容器已处于运行状态 (0x80040606)
pub const WSLC_E_CONTAINER_IS_RUNNING: HRESULT = make_hresult(WSLC_E_BASE + 6);

/// 会话名称属于系统保留名称，不可使用 (0x80040607)
pub const WSLC_E_SESSION_RESERVED: HRESULT = make_hresult(WSLC_E_BASE + 7);

/// 会话名称格式不合法 (0x80040608)
pub const WSLC_E_INVALID_SESSION_NAME: HRESULT = make_hresult(WSLC_E_BASE + 8);

/// 网络未找到 (0x80040609)
pub const WSLC_E_NETWORK_NOT_FOUND: HRESULT = make_hresult(WSLC_E_BASE + 9);

/// Windows Update 搜索依赖组件失败 (0x8004060A)
pub const WSLC_E_WU_SEARCH_FAILED: HRESULT = make_hresult(WSLC_E_BASE + 10);

/// SDK 版本落后，需要更新 (0x8004060B)
pub const WSLC_E_SDK_UPDATE_NEEDED: HRESULT = make_hresult(WSLC_E_BASE + 11);

/// 容器功能被系统或安全策略禁用 (0x8004060C)
pub const WSLC_E_CONTAINER_DISABLED: HRESULT = make_hresult(WSLC_E_BASE + 12);

/// 镜像仓库访问被安全策略拦截 (0x8004060D)
pub const WSLC_E_REGISTRY_BLOCKED_BY_POLICY: HRESULT = make_hresult(WSLC_E_BASE + 13);

/// 存储卷当前不可用或被独占占用 (0x8004060E)
pub const WSLC_E_VOLUME_NOT_AVAILABLE: HRESULT = make_hresult(WSLC_E_BASE + 14);

/// 会话未找到 (0x8004060F)
pub const WSLC_E_SESSION_NOT_FOUND: HRESULT = make_hresult(WSLC_E_BASE + 15);

/// WSL 虚拟机未在运行 (0x80040610)
pub const WSLC_E_VM_NOT_RUNNING: HRESULT = make_hresult(WSLC_E_BASE + 16);

/// 事件队列溢出，部分事件已丢失 (0x80040611)
pub const WSLC_E_EVENTS_LOST: HRESULT = make_hresult(WSLC_E_BASE + 17);

/// 事件流已传输完毕 (0x80040612)
pub const WSLC_E_EVENT_STREAM_FINISHED: HRESULT = make_hresult(WSLC_E_BASE + 18);

/// 容器已被删除 (0x80040613)
pub const WSLC_E_CONTAINER_DELETED: HRESULT = make_hresult(WSLC_E_BASE + 19);

#[cfg(test)]
mod tests {
    use super::*;

    /// 逐个核对官方头文件中 19 个具名错误码的十六进制取值
    ///
    /// 这些值直接决定上层能否把 HRESULT 正确映射为业务领域错误，一旦与官方
    /// 失配，错误分类会整体错位且难以察觉，故在此显式锁定。
    #[test]
    fn test_official_error_code_values() {
        let cases: [(HRESULT, u32); 19] = [
            (WSLC_E_IMAGE_NOT_FOUND, 0x8004_0601),
            (WSLC_E_CONTAINER_PREFIX_AMBIGUOUS, 0x8004_0602),
            (WSLC_E_CONTAINER_NOT_FOUND, 0x8004_0603),
            (WSLC_E_VOLUME_NOT_FOUND, 0x8004_0604),
            (WSLC_E_CONTAINER_NOT_RUNNING, 0x8004_0605),
            (WSLC_E_CONTAINER_IS_RUNNING, 0x8004_0606),
            (WSLC_E_SESSION_RESERVED, 0x8004_0607),
            (WSLC_E_INVALID_SESSION_NAME, 0x8004_0608),
            (WSLC_E_NETWORK_NOT_FOUND, 0x8004_0609),
            (WSLC_E_WU_SEARCH_FAILED, 0x8004_060A),
            (WSLC_E_SDK_UPDATE_NEEDED, 0x8004_060B),
            (WSLC_E_CONTAINER_DISABLED, 0x8004_060C),
            (WSLC_E_REGISTRY_BLOCKED_BY_POLICY, 0x8004_060D),
            (WSLC_E_VOLUME_NOT_AVAILABLE, 0x8004_060E),
            (WSLC_E_SESSION_NOT_FOUND, 0x8004_060F),
            (WSLC_E_VM_NOT_RUNNING, 0x8004_0610),
            (WSLC_E_EVENTS_LOST, 0x8004_0611),
            (WSLC_E_EVENT_STREAM_FINISHED, 0x8004_0612),
            (WSLC_E_CONTAINER_DELETED, 0x8004_0613),
        ];

        for (actual, expected) in cases {
            assert_eq!(actual as u32, expected, "错误码取值与官方头文件不符");
        }
    }

    #[test]
    fn test_all_error_codes_are_failures() {
        // 官方 19 个具名错误码全部为 SEVERITY_ERROR，必须小于 0，
        // 否则上层基于 hr >= 0 的成功判定会把失败误判为成功
        let codes: [HRESULT; 19] = [
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

        for code in codes {
            assert!(
                code < 0,
                "错误码 0x{:08X} 未置位 SEVERITY_ERROR",
                code as u32
            );
        }
    }

    #[test]
    fn test_error_codes_are_unique() {
        let mut codes: Vec<u32> = vec![
            WSLC_E_IMAGE_NOT_FOUND as u32,
            WSLC_E_CONTAINER_PREFIX_AMBIGUOUS as u32,
            WSLC_E_CONTAINER_NOT_FOUND as u32,
            WSLC_E_VOLUME_NOT_FOUND as u32,
            WSLC_E_CONTAINER_NOT_RUNNING as u32,
            WSLC_E_CONTAINER_IS_RUNNING as u32,
            WSLC_E_SESSION_RESERVED as u32,
            WSLC_E_INVALID_SESSION_NAME as u32,
            WSLC_E_NETWORK_NOT_FOUND as u32,
            WSLC_E_WU_SEARCH_FAILED as u32,
            WSLC_E_SDK_UPDATE_NEEDED as u32,
            WSLC_E_CONTAINER_DISABLED as u32,
            WSLC_E_REGISTRY_BLOCKED_BY_POLICY as u32,
            WSLC_E_VOLUME_NOT_AVAILABLE as u32,
            WSLC_E_SESSION_NOT_FOUND as u32,
            WSLC_E_VM_NOT_RUNNING as u32,
            WSLC_E_EVENTS_LOST as u32,
            WSLC_E_EVENT_STREAM_FINISHED as u32,
            WSLC_E_CONTAINER_DELETED as u32,
        ];
        let total = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), total, "存在重复的官方错误码取值");
    }
}
