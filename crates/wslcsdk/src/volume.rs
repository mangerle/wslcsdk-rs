//! WSLC 会话持久化 VHD 存储卷管理

use crate::error::WslcError;
use crate::session::WslcSessionHandle;
use std::ffi::CString;
use wslcsdk_sys::types::{WslcVhdRequirements, WslcVhdType};
use wslcsdk_sys::*;

/// VHD 存储卷创建选项配置
#[derive(Debug, Clone)]
pub struct VhdVolumeOptions<'a> {
    pub name: &'a str,
    pub size_bytes: u64,
    pub vhd_type: WslcVhdType,
    pub owner: Option<(u32, u32)>,
}

/// 存储卷管理门面
pub struct WslcVolumeManager;

impl WslcVolumeManager {
    /// 在指定会话中创建 VHD 卷
    pub fn create_vhd_volume(
        session: &WslcSessionHandle,
        options: &VhdVolumeOptions<'_>,
    ) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let c_name = CString::new(options.name)
            .map_err(|e| WslcError::Utf8Error(format!("卷名称含非法空字符: {e}")))?;

        let (flags, uid, gid) = match options.owner {
            Some((u, g)) => (wslcsdk_sys::types::WSLC_VHD_REQ_FLAG_OWNER, u, g),
            None => (wslcsdk_sys::types::WSLC_VHD_REQ_FLAG_NONE, 0, 0),
        };

        let req = WslcVhdRequirements {
            name: c_name.as_ptr(),
            size_bytes: options.size_bytes,
            vhd_type: options.vhd_type,
            flags,
            uid,
            gid,
        };

        let mut err_msg: *mut u16 = std::ptr::null_mut();
        let hr = unsafe { WslcCreateSessionVhdVolume(session.as_raw(), &req, &mut err_msg) };
        unsafe { WslcError::check(hr, err_msg) }
    }

    /// 删除指定会话中的 VHD 卷
    pub fn delete_vhd_volume(session: &WslcSessionHandle, name: &str) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let c_name = CString::new(name)
            .map_err(|e| WslcError::Utf8Error(format!("卷名称含非法空字符: {e}")))?;
        let mut err_msg: *mut u16 = std::ptr::null_mut();
        let hr =
            unsafe { WslcDeleteSessionVhdVolume(session.as_raw(), c_name.as_ptr(), &mut err_msg) };
        unsafe { WslcError::check(hr, err_msg) }
    }
}
