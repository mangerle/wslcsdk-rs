//! WSLC 镜像枚举、远程拉取、推送与归档载入

use crate::com_memory::ComArray;
use crate::error::WslcError;
use crate::session::{WslcSessionHandle, path_to_wide_null};
use core::ffi::c_void;
use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString};
use std::fmt::Write as _;
use std::os::windows::raw::HANDLE;
use std::path::Path;
use windows_sys::core::HRESULT;
use wslcsdk_sys::types::{
    WslcImageInfo, WslcImageProgressMessage, WslcImportImageOptions, WslcLoadImageOptions,
    WslcPullImageOptions, WslcPushImageOptions, WslcTagImageOptions,
};
use wslcsdk_sys::*;

/// 容器镜像元数据
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageInfo {
    /// 镜像的完整引用名（如 `docker.io/library/alpine:latest`）
    pub name: String,
    /// 内容寻址摘要，可用于精确比对两镜像是否一致
    pub sha256: String,
    /// 展开后的镜像大小，单位字节
    pub size_bytes: i64,
    /// 创建时间，Unix纪元秒
    pub created_unix_time: u64,
}

/// 镜像拉取与推送进度结构 (借用切片)
#[derive(Debug, Clone)]
pub struct ImageProgress<'a> {
    /// 本次进度的阶段标识（如某一 layer 的摘要）
    pub id: &'a str,
    /// 当前所处的传输阶段
    pub status: WslcImageProgressStatus,
    /// 已传输字节数
    pub current_bytes: u64,
    /// 总字节数；未知大小时为 0
    pub total_bytes: u64,
}

impl ImageProgress<'_> {
    /// 转换为拥有独立所有权的进度对象，便于跨线程或异步通道传递
    pub fn to_owned(&self) -> OwnedImageProgress {
        OwnedImageProgress {
            id: self.id.to_string(),
            status: self.status,
            current_bytes: self.current_bytes,
            total_bytes: self.total_bytes,
        }
    }
}

/// 拥有独立所有权的镜像进度结构体
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedImageProgress {
    /// 本次进度的阶段标识（如某一 layer 的摘要）
    pub id: String,
    /// 当前所处的传输阶段
    pub status: WslcImageProgressStatus,
    /// 已传输字节数
    pub current_bytes: u64,
    /// 总字节数；未知大小时为 0
    pub total_bytes: u64,
}

/// 将二进制摘要编码为小写十六进制字符串
///
/// 输出长度恒为输入的两倍，字符集限定为 `0-9a-f`，因此结果必为合法 UTF-8。
/// 该不变量由`write!` 格式化保证，无需调用方再做编码校验。
fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        // 写入 String 永不失败，故忽略返回值是安全的
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// 镜像管理门面
///
/// 镜像进度回调的 FFI跳板
///
/// 将官方以裸指针传入的进度消息转回 [`ImageProgress`] 后再交给调用方的闭包。
/// 供 [`WslcImageManager::pull_image`] 与
/// [`WslcImageManager::push_image_with_progress`] 共用。
///
/// 跨 FFI 边界必须捕获 panic：unwind 穿过 C 栈帧属未定义行为。
///
/// # Safety
///
/// `ctx` 必须是由调用方以 `Box::into_raw` 移交的 `Mutex<F>` 指针，且在官方停止回调前
/// 保持有效；`msg` 由官方保证在回调期间指向有效结构体。
unsafe extern "system" fn progress_trampoline<F>(
    msg: *const WslcImageProgressMessage,
    ctx: *mut c_void,
) -> HRESULT
where
    F: FnMut(&ImageProgress<'_>) -> bool,
{
    if ctx.is_null() || msg.is_null() {
        return windows_sys::Win32::Foundation::S_OK;
    }
    // SAFETY: 官方保证回调期间 msg 指向有效的进度结构体，
    // 其内部指针的有效性由官方契约保证。
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
        let mutex = &*(ctx as *const std::sync::Mutex<F>);
        if let Ok(mut callback) = mutex.lock() {
            if callback(&progress) {
                windows_sys::Win32::Foundation::S_OK
            } else {
                windows_sys::Win32::Foundation::E_ABORT
            }
        } else {
            windows_sys::Win32::Foundation::E_ABORT
        }
    }));
    res.unwrap_or(windows_sys::Win32::Foundation::E_ABORT)
}

/// 全部方法均为关联函数，不持有状态；使用 `WslcSessionHandle` 显式指定操作会话。
#[derive(Debug, Clone, Copy, Default)]
pub struct WslcImageManager;

impl WslcImageManager {
    /// 获取当前会话内的镜像列表
    pub fn list_images(session: &WslcSessionHandle) -> Result<Vec<ImageInfo>, WslcError> {
        let mut raw_images: *mut WslcImageInfo = std::ptr::null_mut();
        let mut count: u32 = 0;

        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcListSessionImages(session.as_raw(), &mut raw_images, &mut count) };
        if hr < 0 {
            return Err(WslcError::from_hresult(hr, "获取镜像列表失败"));
        }

        // 数组所有权移交 ComArray：空指针与 count 为 0 两种情形均被自动收敛，
        // 无论后续如何提前返回，COM 堆内存都会被恰好释放一次
        // SAFETY: raw_images 为官方 _Outptr_result_buffer_ 输出数组，所有权移交本侧
        let images = unsafe { ComArray::from_raw(raw_images, count as usize) }
            .expect("非空长度下的官方输出数组不应构造失败");

        let slice = images.as_slice();
        if slice.is_empty() {
            return Ok(Vec::new());
        }

        let mut list = Vec::with_capacity(slice.len());

        for item in slice {
            let name_end = item
                .name
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(item.name.len());
            let name = String::from_utf8_lossy(&item.name[..name_end]).to_string();

            // 摘要以标准小写十六进制呈现。长度由摘要数组自身推导而非硬编码，
            // 官方若变更摘要长度不会越界panic；格式化宏保证输出必为合法 UTF-8，
            // 无需 unsafe 转换。
            let sha_hex = to_hex(&item.sha256);

            list.push(ImageInfo {
                name,
                sha256: sha_hex,
                size_bytes: item.size_bytes,
                created_unix_time: item.created_unix_time,
            });
        }

        Ok(list)
    }

    /// 拉取远程镜像
    pub fn pull_image<F>(
        session: &WslcSessionHandle,
        uri: &str,
        registry_auth: Option<&str>,
        on_progress: Option<F>,
    ) -> Result<(), WslcError>
    where
        F: FnMut(&ImageProgress<'_>) -> bool + Send,
    {
        let resolved_uri = crate::registry::resolve_image_reference(uri)?;
        let c_uri = CString::new(resolved_uri.as_str())
            .map_err(|e| WslcError::NulError(format!("URI 包含非法空字节: {e}")))?;
        let c_auth = match registry_auth {
            Some(a) => Some(
                CString::new(a)
                    .map_err(|e| WslcError::NulError(format!("鉴权信息包含非法空字节: {e}")))?,
            ),
            None => None,
        };

        let (cb, ctx) = match on_progress {
            Some(f) => {
                let boxed = Box::into_raw(Box::new(std::sync::Mutex::new(f)));
                (Some(progress_trampoline::<F> as _), boxed as *mut c_void)
            }
            None => (None, std::ptr::null_mut()),
        };

        let options = WslcPullImageOptions {
            uri: c_uri.as_ptr(),
            progress_callback: cb,
            progress_callback_context: ctx,
            registry_auth: c_auth.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
        };

        let mut err_msg: *mut u16 = std::ptr::null_mut();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcPullSessionImage(session.as_raw(), &options, &mut err_msg) };
        if !ctx.is_null() {
            // SAFETY: ctx 在本函数开头由 Box::into_raw 分配，调用结束立即安全回收
            let _ = unsafe { Box::from_raw(ctx as *mut std::sync::Mutex<F>) };
        }
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        unsafe { WslcError::check(hr, err_msg) }?;
        log::info!(
            "镜像拉取完成，URI: '{}'，所属会话: '{}'",
            uri,
            session.name()
        );
        Ok(())
    }

    /// 从文件导入 tar 镜像
    pub fn import_image_from_file(
        session: &WslcSessionHandle,
        image_name: &str,
        path: impl AsRef<Path>,
    ) -> Result<(), WslcError> {
        let c_name = CString::new(image_name)
            .map_err(|e| WslcError::NulError(format!("镜像名称非法: {e}")))?;
        let wide_path = path_to_wide_null(path)?;

        let options = WslcImportImageOptions::default();
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe {
            WslcImportSessionImageFromFile(
                session.as_raw(),
                c_name.as_ptr(),
                wide_path.as_ptr(),
                &options,
                &mut err_msg,
            )
        };
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        unsafe { WslcError::check(hr, err_msg) }?;
        log::info!(
            "镜像导入完成，名称: '{}'，所属会话: '{}'",
            image_name,
            session.name()
        );
        Ok(())
    }

    /// 从文件载入 docker save 镜像
    pub fn load_image_from_file(
        session: &WslcSessionHandle,
        path: impl AsRef<Path>,
    ) -> Result<(), WslcError> {
        let wide_path = path_to_wide_null(path)?;

        let options = WslcLoadImageOptions::default();
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe {
            WslcLoadSessionImageFromFile(
                session.as_raw(),
                wide_path.as_ptr(),
                &options,
                &mut err_msg,
            )
        };
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        unsafe { WslcError::check(hr, err_msg) }?;
        log::info!("镜像载入完成，所属会话: '{}'", session.name());
        Ok(())
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
        let options = WslcLoadImageOptions::default();
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe {
            WslcLoadSessionImage(
                session.as_raw(),
                content_handle,
                bytes,
                &options,
                &mut err_msg,
            )
        };
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
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
        let c_name = CString::new(image_name)
            .map_err(|e| WslcError::NulError(format!("镜像名称非法: {e}")))?;
        let options = WslcImportImageOptions::default();
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
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
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        unsafe { WslcError::check(hr, err_msg) }
    }

    /// 为已有镜像打标签
    pub fn tag_image(
        session: &WslcSessionHandle,
        image: &str,
        repo: &str,
        tag: &str,
    ) -> Result<(), WslcError> {
        let c_img =
            CString::new(image).map_err(|e| WslcError::NulError(format!("镜像名称非法: {e}")))?;
        let c_repo =
            CString::new(repo).map_err(|e| WslcError::NulError(format!("仓库名称非法: {e}")))?;
        let c_tag =
            CString::new(tag).map_err(|e| WslcError::NulError(format!("标签名称非法: {e}")))?;

        let options = WslcTagImageOptions {
            image: c_img.as_ptr(),
            repo: c_repo.as_ptr(),
            tag: c_tag.as_ptr(),
        };

        let mut err_msg: *mut u16 = std::ptr::null_mut();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcTagSessionImage(session.as_raw(), &options, &mut err_msg) };
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        unsafe { WslcError::check(hr, err_msg) }
    }

    /// 推送镜像至远程仓库 (支持进度监控)
    pub fn push_image_with_progress<F>(
        session: &WslcSessionHandle,
        image: &str,
        registry_auth: Option<&str>,
        on_progress: Option<F>,
    ) -> Result<(), WslcError>
    where
        F: FnMut(&ImageProgress<'_>) -> bool + Send,
    {
        let c_img =
            CString::new(image).map_err(|e| WslcError::NulError(format!("镜像名称非法: {e}")))?;
        let c_auth = match registry_auth {
            Some(a) => Some(
                CString::new(a).map_err(|e| WslcError::NulError(format!("鉴权信息非法: {e}")))?,
            ),
            None => None,
        };

        let (cb, ctx) = match on_progress {
            Some(f) => {
                let boxed = Box::into_raw(Box::new(std::sync::Mutex::new(f)));
                (Some(progress_trampoline::<F> as _), boxed as *mut c_void)
            }
            None => (None, std::ptr::null_mut()),
        };

        let options = WslcPushImageOptions {
            image: c_img.as_ptr(),
            registry_auth: c_auth.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
            progress_callback: cb,
            progress_callback_context: ctx,
        };

        let mut err_msg: *mut u16 = std::ptr::null_mut();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcPushSessionImage(session.as_raw(), &options, &mut err_msg) };
        if !ctx.is_null() {
            // SAFETY: ctx 在本函数开头由 Box::into_raw 分配，调用结束立即安全回收
            let _ = unsafe { Box::from_raw(ctx as *mut std::sync::Mutex<F>) };
        }
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        unsafe { WslcError::check(hr, err_msg) }?;
        log::info!(
            "镜像推送完成，镜像: '{}'，所属会话: '{}'",
            image,
            session.name()
        );
        Ok(())
    }

    /// 推送镜像至远程仓库
    pub fn push_image(
        session: &WslcSessionHandle,
        image: &str,
        registry_auth: Option<&str>,
    ) -> Result<(), WslcError> {
        Self::push_image_with_progress(
            session,
            image,
            registry_auth,
            None::<fn(&ImageProgress<'_>) -> bool>,
        )
    }

    /// 删除镜像
    pub fn delete_image(session: &WslcSessionHandle, name_or_id: &str) -> Result<(), WslcError> {
        let c_str = CString::new(name_or_id)
            .map_err(|e| WslcError::NulError(format!("镜像名称或 ID 非法: {e}")))?;
        let mut err_msg: *mut u16 = std::ptr::null_mut();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcDeleteSessionImage(session.as_raw(), c_str.as_ptr(), &mut err_msg) };
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        unsafe { WslcError::check(hr, err_msg) }?;
        log::info!(
            "镜像删除成功，目标: '{}'，所属会话: '{}'",
            name_or_id,
            session.name()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_progress_to_owned_roundtrip() {
        let borrowed = ImageProgress {
            id: "sha256:1234",
            status: WslcImageProgressStatus::DOWNLOADING,
            current_bytes: 1024,
            total_bytes: 2048,
        };
        let owned = borrowed.to_owned();
        assert_eq!(owned.id, "sha256:1234");
        assert_eq!(owned.status, WslcImageProgressStatus::DOWNLOADING);
        assert_eq!(owned.current_bytes, 1024);
        assert_eq!(owned.total_bytes, 2048);
    }

    #[test]
    fn test_image_progress_to_owned_is_independent() {
        let mut owned = {
            let borrowed = ImageProgress {
                id: "layer-a",
                status: WslcImageProgressStatus::PULLING,
                current_bytes: 0,
                total_bytes: 0,
            };
            borrowed.to_owned()
        };
        // 借用对象已随作用域释放，拥有所有权的副本仍可独立使用
        owned.id.push_str("-mutated");
        assert_eq!(owned.id, "layer-a-mutated");
    }

    #[test]
    fn test_image_info_serde_roundtrip() {
        let info = ImageInfo {
            name: "alpine:latest".to_string(),
            sha256: "a".repeat(64),
            size_bytes: 3_500_000,
            created_unix_time: 1_700_000_000,
        };
        let json = serde_json::to_string(&info).expect("序列化失败");
        let back: ImageInfo = serde_json::from_str(&json).expect("反序列化失败");
        assert_eq!(info, back);
    }

    /// 十六进制编码须为定长两倍输出，且字符集限定为小写十六进制
    #[test]
    fn test_to_hex_produces_lowercase_fixed_width() {
        assert_eq!(to_hex(&[]), "");
        assert_eq!(to_hex(&[0x00]), "00");
        assert_eq!(to_hex(&[0xff]), "ff");
        // 高低半字节均需补零，锁定宽度语义
        assert_eq!(to_hex(&[0x0f, 0xf0]), "0ff0");
        // 摘要全字节全覆盖，锁定不会漏位或错位
        let all: Vec<u8> = (0..=255u8).collect();
        let hex = to_hex(&all);
        assert_eq!(hex.len(), 512);
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
        );
        assert!(hex.starts_with("000102"));
        assert!(hex.ends_with("fdfeff"));
    }
}
