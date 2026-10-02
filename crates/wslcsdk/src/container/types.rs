//! WSLC 容器数据模型与生命周期保全结构体

use crate::process::RetainedProcessSettings;
use std::ffi::CString;
use wslcsdk_sys::types::{
    WslcContainerNamedVolume, WslcContainerPortMapping, WslcContainerVolume, WslcPortProtocol,
};

use crate::error::WslcError;
use std::net::IpAddr;
use std::str::FromStr;
use windows_sys::Win32::Networking::WinSock::SOCKADDR_STORAGE;

/// 安全的端口映射数据
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerPortMappingData {
    /// 宿主机侧监听的端口
    pub windows_port: u16,
    /// 容器内部目标端口
    pub container_port: u16,
    /// 端口承载的传输协议
    pub protocol: WslcPortProtocol,
    /// 宿主机侧的绑定地址；`None` 表示交由 SDK 采用默认绑定
    pub bind_ip: Option<IpAddr>,
}

/// 解析端口映射中的单个端口号，失败时按所在方位给出可定位的描述
fn parse_port(text: &str, side: &str) -> Result<u16, WslcError> {
    text.parse()
        .map_err(|e| WslcError::InvalidConfiguration(format!("{side}端口非法: {e}")))
}

impl FromStr for ContainerPortMappingData {
    type Err = WslcError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (main_part, protocol) = if let Some((front, proto_str)) = s.split_once('/') {
            let proto = match proto_str.to_ascii_lowercase().as_str() {
                "tcp" => WslcPortProtocol::Tcp,
                "udp" => WslcPortProtocol::Udp,
                other => {
                    return Err(WslcError::InvalidConfiguration(format!(
                        "未知的端口协议: {other}，仅支持 tcp 或 udp"
                    )));
                }
            };
            (front, proto)
        } else {
            (s, WslcPortProtocol::Tcp)
        };

        if let Some(stripped) = main_part.strip_prefix('[') {
            let Some((ip_str, rest)) = stripped.split_once("]:") else {
                return Err(WslcError::InvalidConfiguration(format!(
                    "IPv6 端口映射格式非法: {s}"
                )));
            };
            let ip: IpAddr = ip_str
                .parse()
                .map_err(|e| WslcError::InvalidConfiguration(format!("IPv6 地址解析失败: {e}")))?;
            let Some((win_port_str, cont_port_str)) = rest.split_once(':') else {
                return Err(WslcError::InvalidConfiguration(format!(
                    "端口映射缺少容器端口: {s}"
                )));
            };
            let win_port: u16 = win_port_str
                .parse()
                .map_err(|e| WslcError::InvalidConfiguration(format!("宿主机端口非法: {e}")))?;
            let cont_port: u16 = cont_port_str
                .parse()
                .map_err(|e| WslcError::InvalidConfiguration(format!("容器端口非法: {e}")))?;
            return Ok(Self {
                windows_port: win_port,
                container_port: cont_port,
                protocol,
                bind_ip: Some(ip),
            });
        }

        // 以迭代器按序取样并保留「是否还有第四段」的信息，
        // 避免为求长度与索引而 collect 出一个临时 Vec
        let mut parts = main_part.split(':');
        let (first, second, third, fourth) =
            (parts.next(), parts.next(), parts.next(), parts.next());

        match (first, second, third, fourth) {
            (Some(w), Some(c), None, _) => {
                let windows_port = parse_port(w, "宿主机")?;
                let container_port = parse_port(c, "容器")?;
                Ok(Self {
                    windows_port,
                    container_port,
                    protocol,
                    bind_ip: None,
                })
            }
            (Some(ip_str), Some(w), Some(c), None) => {
                let ip: IpAddr = ip_str.parse().map_err(|e| {
                    WslcError::InvalidConfiguration(format!("IP 地址解析失败: {e}"))
                })?;
                let windows_port = parse_port(w, "宿主机")?;
                let container_port = parse_port(c, "容器")?;
                Ok(Self {
                    windows_port,
                    container_port,
                    protocol,
                    bind_ip: Some(ip),
                })
            }
            _ => Err(WslcError::InvalidConfiguration(format!(
                "端口映射格式不匹配，支持 [ip:]host_port:container_port[/protocol]，实际为: {s}"
            ))),
        }
    }
}

pub(crate) struct RetainedBasicMetadata {
    pub(crate) _name: Option<CString>,
    pub(crate) _host: Option<CString>,
    pub(crate) _domain: Option<CString>,
}

pub(crate) struct RetainedVolumes {
    pub(crate) _strings: Vec<CString>,
    pub(crate) _wide_paths: Vec<Vec<u16>>,
    pub(crate) _volumes: Vec<WslcContainerVolume>,
}

pub(crate) struct RetainedNamedVolumes {
    pub(crate) _strings: Vec<(CString, CString)>,
    pub(crate) _volumes: Vec<WslcContainerNamedVolume>,
}

pub(crate) struct RetainedContainerMetadata {
    pub(crate) _image: CString,
    pub(crate) _basic: RetainedBasicMetadata,
    pub(crate) _ports: Vec<WslcContainerPortMapping>,
    pub(crate) _sockaddrs: Vec<SOCKADDR_STORAGE>,
    pub(crate) _volumes: RetainedVolumes,
    pub(crate) _named_volumes: RetainedNamedVolumes,
    pub(crate) _init_process: Option<RetainedProcessSettings>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> ContainerPortMappingData {
        input.parse().expect("端口映射解析失败")
    }

    #[test]
    fn test_parse_host_and_container_port_with_default_protocol() {
        let mapping = parse("8080:80");
        assert_eq!(mapping.windows_port, 8080);
        assert_eq!(mapping.container_port, 80);
        assert_eq!(mapping.protocol, WslcPortProtocol::Tcp);
        assert_eq!(mapping.bind_ip, None);
    }

    #[test]
    fn test_parse_ipv4_bind_address() {
        let mapping = parse("127.0.0.1:8080:80/tcp");
        assert_eq!(mapping.windows_port, 8080);
        assert_eq!(mapping.container_port, 80);
        assert_eq!(mapping.protocol, WslcPortProtocol::Tcp);
        assert_eq!(mapping.bind_ip, Some(IpAddr::from([127, 0, 0, 1])));
    }

    #[test]
    fn test_parse_ipv6_bind_address_with_brackets() {
        let mapping = parse("[::1]:9090:90/udp");
        assert_eq!(mapping.windows_port, 9090);
        assert_eq!(mapping.container_port, 90);
        assert_eq!(mapping.protocol, WslcPortProtocol::Udp);
        assert_eq!(
            mapping.bind_ip,
            Some(IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1]))
        );
    }

    #[test]
    fn test_protocol_matching_is_case_insensitive() {
        assert_eq!(parse("8080:80/TCP").protocol, WslcPortProtocol::Tcp);
        assert_eq!(parse("8080:80/UDP").protocol, WslcPortProtocol::Udp);
    }

    #[test]
    fn test_invalid_inputs_are_rejected() {
        assert!("invalid_port".parse::<ContainerPortMappingData>().is_err());
        assert!(
            "8080:80/invalid"
                .parse::<ContainerPortMappingData>()
                .is_err()
        );
        assert!("8080".parse::<ContainerPortMappingData>().is_err());
        assert!("".parse::<ContainerPortMappingData>().is_err());
        // 端口越界
        assert!("99999:80".parse::<ContainerPortMappingData>().is_err());
        // IPv6 缺少闭括号
        assert!("[::1:9090:90".parse::<ContainerPortMappingData>().is_err());
        // IPv6 缺少容器端口
        assert!("[::1]:9090".parse::<ContainerPortMappingData>().is_err());
        // 三段式 IPv4 地址非法
        assert!(
            "not-an-ip:8080:80"
                .parse::<ContainerPortMappingData>()
                .is_err()
        );
    }

    #[test]
    fn test_boundary_ports_are_accepted() {
        let min = parse("1:1");
        assert_eq!((min.windows_port, min.container_port), (1, 1));
        let max = parse("65535:65535");
        assert_eq!((max.windows_port, max.container_port), (65535, 65535));
    }
}
