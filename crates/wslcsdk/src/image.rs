//! WSLC 镜像枚举、远程拉取、推送与归档载入

use crate::error::WslcError;
use crate::session::{WslcSessionHandle, to_wide_null};
use core::ffi::c_void;
use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString};
use std::os::windows::raw::HANDLE;
use std::path::Path;
use std::slice;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::core::HRESULT;
use wslcsdk_sys::types::{
    WslcImageInfo, WslcImageProgressMessage, WslcImportImageOptions, WslcLoadImageOptions,
    WslcPullImageOptions, WslcPushImageOptions, WslcTagImageOptions,
};
use wslcsdk_sys::*;

/// 容器镜像元数据
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageInfo {
    pub name: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub created_unix_time: u64,
}

/// 镜像拉取与推送进度结构
#[derive(Debug, Clone)]
pub struct ImageProgress<'a> {
    pub id: &'a str,
    pub status: wslcsdk_sys::types::WslcImageProgressStatus,
    pub current_bytes: u64,
    pub total_bytes: u64,
}

/// 镜像管理门面
pub struct WslcImageManager;

impl WslcImageManager {
    /// 获取当前会话内的镜像列表
    pub fn list_images(session: &WslcSessionHandle) -> Result<Vec<ImageInfo>, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut raw_images: *mut WslcImageInfo = std::ptr::null_mut();
        let mut count: u32 = 0;

        let hr = unsafe { WslcListSessionImages(session.as_raw(), &mut raw_images, &mut count) };
        if hr < 0 {
            return Err(WslcError::Win32(hr as u32, "获取镜像列表失败".to_string()));
        }

        if raw_images.is_null() {
            return Ok(Vec::new());
        }

        if count == 0 {
            unsafe {
                CoTaskMemFree(raw_images.cast());
            }
            return Ok(Vec::new());
        }

        let slice = unsafe { slice::from_raw_parts(raw_images, count as usize) };
        let mut list = Vec::with_capacity(count as usize);

        for item in slice {
            let name_end = item
                .name
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(item.name.len());
            let name = String::from_utf8_lossy(&item.name[..name_end]).to_string();

            let mut sha_hex = String::with_capacity(64);
            for byte in item.sha256 {
                use std::fmt::Write;
                let _ = write!(&mut sha_hex, "{:02x}", byte);
            }

            list.push(ImageInfo {
                name,
                sha256: sha_hex,
                size_bytes: item.size_bytes,
                created_unix_time: item.created_unix_time,
            });
        }

        unsafe {
            CoTaskMemFree(raw_images.cast());
        }

        Ok(list)
    }

    /// 拉取远程镜像
    pub fn pull_image<F>(
        session: &WslcSessionHandle,
        uri: &str,
        registry_auth: Option<&str>,
        mut on_progress: Option<F>,
    ) -> Result<(), WslcError>
    where
        F: FnMut(&ImageProgress<'_>) -> bool + Send + 'static,
    {
        let _com_guard = crate::com::try_initialize_mta()?;
        let resolved_uri = crate::registry::resolve_image_reference(uri)?;
        let c_uri = CString::new(resolved_uri.as_str())
            .map_err(|e| WslcError::Utf8Error(format!("URI 包含非法空字节: {e}")))?;
        let c_auth = match registry_auth {
            Some(a) => Some(
                CString::new(a)
                    .map_err(|e| WslcError::Utf8Error(format!("鉴权信息包含非法空字节: {e}")))?,
            ),
            None => None,
        };

        unsafe extern "system" fn progress_trampoline<F>(
            msg: *const WslcImageProgressMessage,
            ctx: *mut c_void,
        ) -> HRESULT
        where
            F: FnMut(&ImageProgress<'_>) -> bool,
        {
            if ctx.is_null() || msg.is_null() {
                return 0;
            }
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                let raw = &*msg;
                let id_str = if !raw.id.is_null() {
                    CStr::from_ptr(raw.id).to_str().unwrap_or_default()
                } else {
                    ""
                };
                let progress = ImageProgress {
                    id: id_str,
                    status: raw.status,
                    current_bytes: raw.detail.current_bytes,
                    total_bytes: raw.detail.total_bytes,
                };
                let callback = &mut *(ctx as *mut F);
                if callback(&progress) { 0 } else { -1 }
            }));
            res.unwrap_or(-1)
        }

        let (cb, ctx) = match on_progress.as_mut() {
            Some(f) => (
                Some(progress_trampoline::<F> as _),
                f as *mut F as *mut c_void,
            ),
            None => (None, std::ptr::null_mut()),
        };

        let options = WslcPullImageOptions {
            uri: c_uri.as_ptr(),
            progress_callback: cb,
            progress_callback_context: ctx,
            registry_auth: c_auth.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
        };

        let mut err_msg: *mut u16 = std::ptr::null_mut();
        let hr = unsafe { WslcPullSessionImage(session.as_raw(), &options, &mut err_msg) };
        unsafe { WslcError::check(hr, err_msg) }
    }

    /// 从文件导入 tar 镜像
    pub fn import_image_from_file(
        session: &WslcSessionHandle,
        image_name: &str,
        path: impl AsRef<Path>,
    ) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let c_name = CString::new(image_name)
            .map_err(|e| WslcError::Utf8Error(format!("镜像名称非法: {e}")))?;
        let wide_path = to_wide_null(&path.as_ref().to_string_lossy())?;

        let options = WslcImportImageOptions::default();
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        let hr = unsafe {
            WslcImportSessionImageFromFile(
                session.as_raw(),
                c_name.as_ptr(),
                wide_path.as_ptr(),
                &options,
                &mut err_msg,
            )
        };
        unsafe { WslcError::check(hr, err_msg) }
    }

    /// 从文件载入 docker save 镜像
    pub fn load_image_from_file(
        session: &WslcSessionHandle,
        path: impl AsRef<Path>,
    ) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let wide_path = to_wide_null(&path.as_ref().to_string_lossy())?;

        let options = WslcLoadImageOptions::default();
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        let hr = unsafe {
            WslcLoadSessionImageFromFile(
                session.as_raw(),
                wide_path.as_ptr(),
                &options,
                &mut err_msg,
            )
        };
        unsafe { WslcError::check(hr, err_msg) }
    }

    /// 从内存流载入镜像
    ///
    /// # Safety
    ///
    /// 调用方必须确保 `content_handle` 是具有可读权限的有效 Win32 文件或管道句柄，且其生命周期覆盖当前调用。
    pub unsafe fn load_image_from_handle(
        session: &WslcSessionHandle,
        content_handle: HANDLE,
        bytes: u64,
    ) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let options = WslcLoadImageOptions::default();
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        let hr = unsafe {
            WslcLoadSessionImage(
                session.as_raw(),
                content_handle,
                bytes,
                &options,
                &mut err_msg,
            )
        };
        unsafe { WslcError::check(hr, err_msg) }
    }

    /// 从内存流导入镜像
    ///
    /// # Safety
    ///
    /// 调用方必须确保 `content_handle` 是具有可读权限的有效 Win32 文件或管道句柄，且其生命周期覆盖当前调用。
    pub unsafe fn import_image_from_handle(
        session: &WslcSessionHandle,
        image_name: &str,
        content_handle: HANDLE,
        bytes: u64,
    ) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let c_name = CString::new(image_name)
            .map_err(|e| WslcError::Utf8Error(format!("镜像名称非法: {e}")))?;
        let options = WslcImportImageOptions::default();
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        let hr = unsafe {
            WslcImportSessionImage(
                session.as_raw(),
                c_name.as_ptr(),
                content_handle,
                bytes,
                &options,
                &mut err_msg,
            )
        };
        unsafe { WslcError::check(hr, err_msg) }
    }

    /// 为已有镜像打标签
    pub fn tag_image(
        session: &WslcSessionHandle,
        image: &str,
        repo: &str,
        tag: &str,
    ) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let c_img =
            CString::new(image).map_err(|e| WslcError::Utf8Error(format!("镜像名称非法: {e}")))?;
        let c_repo =
            CString::new(repo).map_err(|e| WslcError::Utf8Error(format!("仓库名称非法: {e}")))?;
        let c_tag =
            CString::new(tag).map_err(|e| WslcError::Utf8Error(format!("标签名称非法: {e}")))?;

        let options = WslcTagImageOptions {
            image: c_img.as_ptr(),
            repo: c_repo.as_ptr(),
            tag: c_tag.as_ptr(),
        };

        let mut err_msg: *mut u16 = std::ptr::null_mut();
        let hr = unsafe { WslcTagSessionImage(session.as_raw(), &options, &mut err_msg) };
        unsafe { WslcError::check(hr, err_msg) }
    }

    /// 推送镜像至远程仓库
    pub fn push_image(
        session: &WslcSessionHandle,
        image: &str,
        registry_auth: Option<&str>,
    ) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let c_img =
            CString::new(image).map_err(|e| WslcError::Utf8Error(format!("镜像名称非法: {e}")))?;
        let c_auth = match registry_auth {
            Some(a) => Some(
                CString::new(a).map_err(|e| WslcError::Utf8Error(format!("鉴权信息非法: {e}")))?,
            ),
            None => None,
        };

        let options = WslcPushImageOptions {
            image: c_img.as_ptr(),
            registry_auth: c_auth.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
            progress_callback: None,
            progress_callback_context: std::ptr::null_mut(),
        };

        let mut err_msg: *mut u16 = std::ptr::null_mut();
        let hr = unsafe { WslcPushSessionImage(session.as_raw(), &options, &mut err_msg) };
        unsafe { WslcError::check(hr, err_msg) }
    }

    /// 删除镜像
    pub fn delete_image(session: &WslcSessionHandle, name_or_id: &str) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let c_str = CString::new(name_or_id)
            .map_err(|e| WslcError::Utf8Error(format!("镜像名称或 ID 非法: {e}")))?;
        let mut err_msg: *mut u16 = std::ptr::null_mut();
        let hr = unsafe { WslcDeleteSessionImage(session.as_raw(), c_str.as_ptr(), &mut err_msg) };
        unsafe { WslcError::check(hr, err_msg) }
    }
}
