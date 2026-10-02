//! 端口映射的 WinSock 地址构造
//!
//! 官方 `WslcContainerPortMapping::windows_address` 要求一个
//! `SOCKADDR_STORAGE` 指针。该结构体是 WinSock 中尺寸最大的地址联合体
//! （28 字节），无法在栈上按具体协议族直接构造，须先按族构造对应的
//! `SOCKADDR_IN` / `SOCKADDR_IN6`，再拷贝进联合体。
//!
//! 本模块把这段纯位操作从 [`ContainerBuilder`](super::ContainerBuilder) 中
//! 独立出来，便于单独测试，也避免在建造者中堆叠 unsafe 细节。

use crate::container::types::ContainerPortMappingData;
use core::mem::{size_of, zeroed};
use core::ptr::copy_nonoverlapping;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, IN_ADDR, IN_ADDR_0, IN6_ADDR, IN6_ADDR_0, SOCKADDR_IN, SOCKADDR_IN6,
    SOCKADDR_STORAGE,
};

/// 构造绑定到指定 IPv4 地址的 `SOCKADDR_STORAGE`
///
/// 端口以网络字节序（大端）写入，与 WinSock 约定一致。
fn sockaddr_in_v4(addr: Ipv4Addr, port: u16) -> SOCKADDR_STORAGE {
    // SAFETY: SOCKADDR_IN 为 POD 类型，全零位模式是其合法初始状态
    let mut sin: SOCKADDR_IN = unsafe { zeroed() };
    sin.sin_family = AF_INET;
    sin.sin_port = port.to_be();
    sin.sin_addr = IN_ADDR {
        S_un: IN_ADDR_0 {
            S_addr: u32::from_ne_bytes(addr.octets()),
        },
    };

    // SAFETY: 同上，SOCKADDR_STORAGE 全零位模式合法
    let mut storage: SOCKADDR_STORAGE = unsafe { zeroed() };
    // SAFETY: 源与目标为不重叠的缓冲区；SOCKADDR_STORAGE（28 字节）不小于
    // SOCKADDR_IN（16 字节），且已预先清零，故尾部余量保持为零，符合
    // copy_nonoverlapping 的前置条件。
    unsafe {
        copy_nonoverlapping(
            &sin as *const SOCKADDR_IN as *const u8,
            &mut storage as *mut SOCKADDR_STORAGE as *mut u8,
            size_of::<SOCKADDR_IN>(),
        );
    }
    storage
}

/// 构造绑定到指定 IPv6 地址的 `SOCKADDR_STORAGE`
fn sockaddr_in_v6(addr: Ipv6Addr, port: u16) -> SOCKADDR_STORAGE {
    // SAFETY: SOCKADDR_IN6 为 POD 类型，全零位模式是其合法初始状态
    let mut sin6: SOCKADDR_IN6 = unsafe { zeroed() };
    sin6.sin6_family = AF_INET6;
    sin6.sin6_port = port.to_be();
    sin6.sin6_addr = IN6_ADDR {
        u: IN6_ADDR_0 {
            Byte: addr.octets(),
        },
    };

    // SAFETY: 同上，SOCKADDR_STORAGE 全零位模式合法
    let mut storage: SOCKADDR_STORAGE = unsafe { zeroed() };
    // SAFETY: 同 sockaddr_in_v4，SOCKADDR_STORAGE 容量不小于 SOCKADDR_IN6（28 字节），
    // 二者不重叠且目标已清零。
    unsafe {
        copy_nonoverlapping(
            &sin6 as *const SOCKADDR_IN6 as *const u8,
            &mut storage as *mut SOCKADDR_STORAGE as *mut u8,
            size_of::<SOCKADDR_IN6>(),
        );
    }
    storage
}

/// 为每条带绑定地址的映射构造 `SOCKADDR_STORAGE`
///
/// 返回值顺序与 `mappings` 中 `bind_ip` 为 `Some` 的条目一一对应，
/// 调用方据此按序取用。
pub(crate) fn build_sockaddrs(mappings: &[ContainerPortMappingData]) -> Vec<SOCKADDR_STORAGE> {
    // 预统计需要分配的条目数，避免中途扩容
    let bound_count = mappings.iter().filter(|m| m.bind_ip.is_some()).count();
    let mut sockaddrs = Vec::with_capacity(bound_count);

    for mapping in mappings {
        match mapping.bind_ip {
            Some(IpAddr::V4(ipv4)) => sockaddrs.push(sockaddr_in_v4(ipv4, mapping.windows_port)),
            Some(IpAddr::V6(ipv6)) => sockaddrs.push(sockaddr_in_v6(ipv6, mapping.windows_port)),
            // 未指定绑定地址时传入空指针，由 SDK 采用默认绑定
            None => {}
        }
    }

    sockaddrs
}

#[cfg(test)]
mod tests {
    use super::*;
    use wslcsdk_sys::types::WslcPortProtocol;

    /// 构造一条带绑定地址的映射
    fn mapping(bind_ip: Option<IpAddr>, windows_port: u16) -> ContainerPortMappingData {
        ContainerPortMappingData {
            windows_port,
            container_port: 80,
            protocol: WslcPortProtocol::Tcp,
            bind_ip,
        }
    }

    /// 以字节视图读取联合体内容，便于断言布局
    fn as_bytes(storage: &SOCKADDR_STORAGE) -> &[u8] {
        // SAFETY: SOCKADDR_STORAGE 为 POD 类型，任何有效位模式皆可安全读取为字节
        unsafe {
            core::slice::from_raw_parts(
                storage as *const SOCKADDR_STORAGE as *const u8,
                size_of::<SOCKADDR_STORAGE>(),
            )
        }
    }

    #[test]
    fn test_ipv4_socketaddr_layout() {
        let ip: Ipv4Addr = "127.0.0.1".parse().expect("解析失败");
        let storage = sockaddr_in_v4(ip, 8080);
        let bytes = as_bytes(&storage);

        // 族标识位于首部，AF_INET 在 WinSock 中取 2
        assert_eq!(u16::from_ne_bytes([bytes[0], bytes[1]]), AF_INET);
        // 端口以网络字节序存放：8080 = 0x1F90
        assert_eq!(&bytes[2..4], &8080u16.to_be_bytes());
        // IPv4 地址紧随其后：127.0.0.1
        assert_eq!(&bytes[4..8], &[127, 0, 0, 1]);
        // 未使用的尾部必须保持为零
        assert!(bytes[8..].iter().all(|b| *b == 0), "尾部余量应为零");
    }

    #[test]
    fn test_ipv6_socketaddr_layout() {
        let ip: Ipv6Addr = "::1".parse().expect("解析失败");
        let storage = sockaddr_in_v6(ip, 9090);
        let bytes = as_bytes(&storage);

        assert_eq!(u16::from_ne_bytes([bytes[0], bytes[1]]), AF_INET6);
        assert_eq!(&bytes[2..4], &9090u16.to_be_bytes());
        // IPv6 地址为 16 字节，位于偏移 8 之后
        let mut expected = [0u8; 16];
        expected[15] = 1;
        assert_eq!(&bytes[8..24], &expected[..]);
    }

    #[test]
    fn test_port_is_stored_in_network_byte_order() {
        // 0x1234 在小端机器上若被误写为本机序，字节序将与预期相反
        let storage = sockaddr_in_v4("0.0.0.0".parse().expect("解析失败"), 0x1234);
        assert_eq!(&as_bytes(&storage)[2..4], &[0x12, 0x34]);
    }

    #[test]
    fn test_build_sockaddrs_skips_unbound_entries() {
        let mappings = vec![
            mapping(None, 1000),
            mapping(Some("127.0.0.1".parse().expect("解析失败")), 1001),
            mapping(None, 1002),
            mapping(Some("::1".parse().expect("解析失败")), 1003),
        ];
        let sockaddrs = build_sockaddrs(&mappings);

        // 仅两条带绑定地址，故只产出两个 SOCKADDR_STORAGE
        assert_eq!(sockaddrs.len(), 2);
        // 顺序须与映射出现顺序一致：先 IPv4 后 IPv6
        assert_eq!(family_of(&sockaddrs[0]), AF_INET);
        assert_eq!(family_of(&sockaddrs[1]), AF_INET6);
    }

    /// 读取地址联合体首部的协议族标识
    fn family_of(storage: &SOCKADDR_STORAGE) -> u16 {
        let bytes = as_bytes(storage);
        u16::from_ne_bytes([bytes[0], bytes[1]])
    }

    #[test]
    fn test_build_sockaddrs_empty_input_yields_empty_output() {
        assert!(build_sockaddrs(&[]).is_empty());
        assert!(build_sockaddrs(&[mapping(None, 8080)]).is_empty());
    }

    #[test]
    fn test_storage_size_is_sufficient_for_both_families() {
        // SOCKADDR_STORAGE 必须不小于两种具体结构，否则拷贝将越界
        assert!(size_of::<SOCKADDR_STORAGE>() >= size_of::<SOCKADDR_IN>());
        assert!(size_of::<SOCKADDR_STORAGE>() >= size_of::<SOCKADDR_IN6>());
    }
}
