//! WSLC 复合结构体：端口映射、卷挂载与镜像元数据

use super::*;
use windows_sys::Win32::Foundation::BOOL;
use windows_sys::Win32::Networking::WinSock::SOCKADDR_STORAGE;

// ==================== 复合结构体 ====================

/// VHD 规格要求
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcVhdRequirements {
    pub name: *const i8,
    pub size_bytes: u64,
    pub vhd_type: WslcVhdType,
    pub flags: WslcVhdRequirementsFlags,
    pub uid: u32,
    pub gid: u32,
}

/// 会话崩溃转储信息
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcSessionCrashDumpInfo {
    pub dump_path: *const u16,
    pub process_name: *const i8,
    pub pid: u32,
    pub signal: u32,
    pub timestamp: u64,
}

/// 容器端口映射配置
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcContainerPortMapping {
    pub windows_port: u16,
    pub container_port: u16,
    pub protocol: WslcPortProtocol,
    pub windows_address: *mut SOCKADDR_STORAGE,
}

/// 容器目录卷挂载
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcContainerVolume {
    pub windows_path: *const u16,
    pub container_path: *const i8,
    pub read_only: BOOL,
}

/// 容器具名 VHD 卷挂载
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcContainerNamedVolume {
    pub name: *const i8,
    pub container_path: *const i8,
    pub read_only: BOOL,
}

/// 镜像拉取字节进度明细
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WslcImageProgressDetail {
    pub current_bytes: u64,
    pub total_bytes: u64,
}

/// 镜像拉取进度单条消息
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcImageProgressMessage {
    pub id: *const i8,
    pub status: WslcImageProgressStatus,
    pub detail: WslcImageProgressDetail,
}

/// 镜像元数据信息
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WslcImageInfo {
    pub name: [u8; WSLC_IMAGE_NAME_LENGTH],
    pub sha256: [u8; 32],
    pub size_bytes: i64,
    pub created_unix_time: u64,
}

/// WSLC 系统版本号
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct WslcVersion {
    pub major: u32,
    pub minor: u32,
    pub revision: u32,
}
