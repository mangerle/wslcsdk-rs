//! 容器配置的官方结构体编排
//!
//! [`ContainerBuilder`] 的链式 API 只收集意图，本模块负责把这些意图翻译成
//! 官方 `WslcSetContainerSettings*` 系列调用。为使官方结构体中的裸指针
//! 在调用期间有效，各函数同时返回一组「生命周期保全结构体」，
//! 调用方须将其持有至 `WslcCreateContainer` 返回为止。
//!
//! 参数一律以 `builder: &ContainerBuilder` 传入而非作为 `self`，
//! 使本模块可独立于建造者的实现演进。

use super::builder::ContainerBuilder;
use super::sockaddr::build_sockaddrs;
use super::types::{RetainedBasicMetadata, RetainedNamedVolumes, RetainedVolumes};
use crate::error::WslcError;
use std::ffi::CString;
use windows_sys::Win32::Networking::WinSock::SOCKADDR_STORAGE;
use wslcsdk_sys::types::{
    WslcContainerNamedVolume, WslcContainerPortMapping, WslcContainerSettings, WslcContainerVolume,
};
use wslcsdk_sys::*;

pub(super) fn apply_metadata(
    builder: &ContainerBuilder,
    settings: &mut WslcContainerSettings,
) -> Result<RetainedBasicMetadata, WslcError> {
    let name = if let Some(ref n) = builder.name {
        let c = CString::new(n.as_str())
            .map_err(|e| WslcError::NulError(format!("容器名称含非法空字节: {e}")))?;
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcSetContainerSettingsName(settings, c.as_ptr()) };
        WslcError::check_hr(hr, "设置容器名称失败")?;
        Some(c)
    } else {
        None
    };

    if let Some(mode) = builder.networking_mode {
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcSetContainerSettingsNetworkingMode(settings, mode) };
        WslcError::check_hr(hr, "设置容器网络模式失败")?;
    }

    let host = if let Some(ref h) = builder.host_name {
        let c = CString::new(h.as_str())
            .map_err(|e| WslcError::NulError(format!("主机名含非法空字节: {e}")))?;
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcSetContainerSettingsHostName(settings, c.as_ptr()) };
        WslcError::check_hr(hr, "设置容器主机名失败")?;
        Some(c)
    } else {
        None
    };

    let domain = if let Some(ref d) = builder.domain_name {
        let c = CString::new(d.as_str())
            .map_err(|e| WslcError::NulError(format!("域名含非法空字节: {e}")))?;
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcSetContainerSettingsDomainName(settings, c.as_ptr()) };
        WslcError::check_hr(hr, "设置容器域名失败")?;
        Some(c)
    } else {
        None
    };

    if builder.flags != 0 {
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcSetContainerSettingsFlags(settings, builder.flags) };
        WslcError::check_hr(hr, "设置容器标志位失败")?;
    }

    Ok(RetainedBasicMetadata {
        _name: name,
        _host: host,
        _domain: domain,
    })
}

pub(super) fn apply_port_mappings(
    builder: &ContainerBuilder,
    settings: &mut WslcContainerSettings,
) -> Result<(Vec<WslcContainerPortMapping>, Vec<SOCKADDR_STORAGE>), WslcError> {
    if builder.port_mappings.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    // SOCKADDR 的位操作已抽至 sockaddr 模块，此处仅负责编排
    let mut sockaddrs = build_sockaddrs(&builder.port_mappings);

    let mut raw_ports: Vec<WslcContainerPortMapping> =
        Vec::with_capacity(builder.port_mappings.len());
    let mut storage_idx = 0;

    for p in &builder.port_mappings {
        let addr_ptr = if p.bind_ip.is_some() {
            let ptr = &mut sockaddrs[storage_idx] as *mut SOCKADDR_STORAGE;
            storage_idx += 1;
            ptr
        } else {
            std::ptr::null_mut()
        };

        raw_ports.push(WslcContainerPortMapping {
            windows_port: p.windows_port,
            container_port: p.container_port,
            protocol: p.protocol,
            windows_address: addr_ptr,
        });
    }

    // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
    // 出参为合法的可写指针，不涉及未定义行为。
    let hr = unsafe {
        WslcSetContainerSettingsPortMappings(settings, raw_ports.as_ptr(), raw_ports.len() as u32)
    };
    WslcError::check_hr(hr, "设置容器端口映射失败")?;
    Ok((raw_ports, sockaddrs))
}

pub(super) fn apply_volumes(
    builder: &ContainerBuilder,
    settings: &mut WslcContainerSettings,
) -> Result<RetainedVolumes, WslcError> {
    if builder.volumes.is_empty() {
        return Ok(RetainedVolumes {
            _strings: Vec::new(),
            _wide_paths: Vec::new(),
            _volumes: Vec::new(),
        });
    }
    let mut volumes = Vec::with_capacity(builder.volumes.len());
    let mut strings = Vec::with_capacity(builder.volumes.len());
    let mut wide_paths = Vec::with_capacity(builder.volumes.len());
    for (w, c, _ro) in &builder.volumes {
        if w.as_os_str().is_empty() {
            return Err(WslcError::InvalidConfiguration(
                "宿主机挂载路径不能为空".to_string(),
            ));
        }
        if !c.starts_with('/') {
            return Err(WslcError::InvalidConfiguration(format!(
                "容器挂载路径必须是绝对路径（以 '/' 开头），实际为: {c}"
            )));
        }
        let wide = crate::session::path_to_wide_null(w)?;
        let c_str = CString::new(c.as_str())
            .map_err(|e| WslcError::NulError(format!("挂载路径非法: {e}")))?;
        wide_paths.push(wide);
        strings.push(c_str);
    }
    for i in 0..builder.volumes.len() {
        volumes.push(WslcContainerVolume {
            windows_path: wide_paths[i].as_ptr(),
            container_path: strings[i].as_ptr(),
            read_only: if builder.volumes[i].2 { 1 } else { 0 },
        });
    }
    // SAFETY: err_msg 为官方 _Outptr_opt_result_z_ 输出参数，
    // 其所有权在本行交由 RAII 包装接管并自动释放。
    let hr = unsafe {
        WslcSetContainerSettingsVolumes(settings, volumes.as_ptr(), volumes.len() as u32)
    };
    WslcError::check_hr(hr, "设置容器目录卷挂载失败")?;
    Ok(RetainedVolumes {
        _strings: strings,
        _wide_paths: wide_paths,
        _volumes: volumes,
    })
}

pub(super) fn apply_named_volumes(
    builder: &ContainerBuilder,
    settings: &mut WslcContainerSettings,
) -> Result<RetainedNamedVolumes, WslcError> {
    if builder.named_volumes.is_empty() {
        return Ok(RetainedNamedVolumes {
            _strings: Vec::new(),
            _volumes: Vec::new(),
        });
    }
    let mut volumes = Vec::with_capacity(builder.named_volumes.len());
    let mut strings = Vec::with_capacity(builder.named_volumes.len());
    for (n, c, ro) in &builder.named_volumes {
        if n.trim().is_empty() {
            return Err(WslcError::InvalidConfiguration(
                "具名卷名称不能为空".to_string(),
            ));
        }
        if !c.starts_with('/') {
            return Err(WslcError::InvalidConfiguration(format!(
                "容器挂载路径必须是绝对路径（以 '/' 开头），实际为: {c}"
            )));
        }
        let c_name = CString::new(n.as_str())
            .map_err(|e| WslcError::NulError(format!("卷名称非法: {e}")))?;
        let c_cont = CString::new(c.as_str())
            .map_err(|e| WslcError::NulError(format!("容器路径非法: {e}")))?;
        volumes.push(WslcContainerNamedVolume {
            name: c_name.as_ptr(),
            container_path: c_cont.as_ptr(),
            read_only: if *ro { 1 } else { 0 },
        });
        strings.push((c_name, c_cont));
    }
    // SAFETY: err_msg 为官方 _Outptr_opt_result_z_ 输出参数，
    // 其所有权在本行交由 RAII 包装接管并自动释放。
    let hr = unsafe {
        WslcSetContainerSettingsNamedVolumes(settings, volumes.as_ptr(), volumes.len() as u32)
    };
    WslcError::check_hr(hr, "设置容器具名 VHD 卷挂载失败")?;
    Ok(RetainedNamedVolumes {
        _strings: strings,
        _volumes: volumes,
    })
}
