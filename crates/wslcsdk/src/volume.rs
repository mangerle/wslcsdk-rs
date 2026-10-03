//! WSLC 会话持久化 VHD 存储卷管理

use crate::error::WslcError;
use crate::session::WslcSessionHandle;
use std::ffi::CString;
use wslcsdk_sys::types::{WslcVhdRequirements, WslcVhdType};
use wslcsdk_sys::*;

/// VHD 存储卷创建选项配置
#[derive(Debug, Clone)]
pub struct VhdVolumeOptions<'a> {
    /// 存储卷名称，在所属会话内唯一
    pub name: &'a str,
    /// 卷容量，单位字节
    pub size_bytes: u64,
    /// 虚拟磁盘类型（固定大小 / 动态扩展等）
    pub vhd_type: WslcVhdType,
    /// Linux 侧属主 `(uid, gid)`；`None` 表示沿用默认属主
    pub owner: Option<(u32, u32)>,
}

/// 存储卷管理门面
///
/// 全部方法均为关联函数，不持有状态；使用 `WslcSessionHandle` 显式指定操作会话。
#[derive(Debug, Clone, Copy, Default)]
pub struct WslcVolumeManager;

impl WslcVolumeManager {
    /// 在指定会话中创建 VHD 卷
    pub fn create_vhd_volume(
        session: &WslcSessionHandle,
        options: &VhdVolumeOptions<'_>,
    ) -> Result<(), WslcError> {
        if options.name.trim().is_empty() {
            return Err(WslcError::InvalidConfiguration(
                "卷名称不能为空".to_string(),
            ));
        }
        if options.size_bytes == 0 {
            return Err(WslcError::InvalidConfiguration(
                "卷大小必须大于 0 字节".to_string(),
            ));
        }
        let c_name = CString::new(options.name)
            .map_err(|e| WslcError::NulError(format!("卷名称含非法空字符: {e}")))?;

        let (flags, uid, gid) = match options.owner {
            Some((u, g)) => (WSLC_VHD_REQ_FLAG_OWNER, u, g),
            None => (WSLC_VHD_REQ_FLAG_NONE, 0, 0),
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
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcCreateSessionVhdVolume(session.as_raw(), &req, &mut err_msg) };
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        unsafe {
            WslcError::check(
                hr,
                err_msg,
                format!("创建 VHD 卷失败，名称: '{}'", options.name),
            )?;
        }
        log::info!(
            "VHD 卷创建成功，名称: '{}'，大小: {} 字节",
            options.name,
            options.size_bytes
        );
        Ok(())
    }

    /// 删除指定会话中的 VHD 卷
    pub fn delete_vhd_volume(session: &WslcSessionHandle, name: &str) -> Result<(), WslcError> {
        if name.trim().is_empty() {
            return Err(WslcError::InvalidConfiguration(
                "卷名称不能为空".to_string(),
            ));
        }
        let c_name = CString::new(name)
            .map_err(|e| WslcError::NulError(format!("卷名称含非法空字符: {e}")))?;
        let mut err_msg: *mut u16 = std::ptr::null_mut();
        let hr =
            // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
            // 出参为合法的可写指针，不涉及未定义行为。
            unsafe { WslcDeleteSessionVhdVolume(session.as_raw(), c_name.as_ptr(), &mut err_msg) };
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        unsafe {
            WslcError::check(hr, err_msg, format!("删除 VHD 卷失败，名称: '{name}'"))?;
        }
        log::info!("VHD 卷删除成功，名称: '{}'", name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个空句柄会话，仅用于触发参数前置校验分支
    fn fake_session() -> WslcSessionHandle {
        // 空句柄不携带任何需释放的所有权，构造行为本身是安全的
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        unsafe { WslcSessionHandle::from_raw(WslcSession::NULL, "fake-session") }
    }

    #[test]
    fn test_vhd_volume_options_field_access() {
        let options = VhdVolumeOptions {
            name: "test-volume",
            size_bytes: 1024 * 1024,
            vhd_type: WslcVhdType::Dynamic,
            owner: Some((1000, 1000)),
        };
        assert_eq!(options.name, "test-volume");
        assert_eq!(options.size_bytes, 1024 * 1024);
        assert_eq!(options.owner, Some((1000, 1000)));
    }

    #[test]
    fn test_create_rejects_blank_name() {
        let session = fake_session();
        let options = VhdVolumeOptions {
            name: "  ",
            size_bytes: 1024,
            vhd_type: WslcVhdType::Dynamic,
            owner: None,
        };
        assert!(WslcVolumeManager::create_vhd_volume(&session, &options).is_err());
    }

    #[test]
    fn test_create_rejects_zero_size() {
        let session = fake_session();
        let options = VhdVolumeOptions {
            name: "valid-name",
            size_bytes: 0,
            vhd_type: WslcVhdType::Dynamic,
            owner: None,
        };
        assert!(WslcVolumeManager::create_vhd_volume(&session, &options).is_err());
    }

    #[test]
    fn test_delete_rejects_blank_name() {
        let session = fake_session();
        assert!(WslcVolumeManager::delete_vhd_volume(&session, "").is_err());
        assert!(WslcVolumeManager::delete_vhd_volume(&session, "   ").is_err());
    }
}
