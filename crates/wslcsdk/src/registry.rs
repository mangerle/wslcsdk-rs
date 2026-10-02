//! WSLC 镜像仓库鉴权与 Identity Token 生成

use crate::error::WslcError;
use crate::session::WslcSessionHandle;
use std::ffi::{CStr, CString};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use wslcsdk_sys::types::WslcIdentityTokenType;
use wslcsdk_sys::*;

/// 仓库鉴权认证结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthTokenResult {
    pub identity_token: String,
    pub token_type: WslcIdentityTokenType,
}

/// 镜像仓库管理器
pub struct WslcRegistryManager;

impl WslcRegistryManager {
    /// 登录镜像仓库并生成 Base64 编码的 identity token
    pub fn authenticate(
        session: &WslcSessionHandle,
        server_address: &str,
        username: &str,
        password: &str,
    ) -> Result<AuthTokenResult, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let c_server = CString::new(server_address)
            .map_err(|e| WslcError::Utf8Error(format!("服务器地址非法: {e}")))?;
        let c_user =
            CString::new(username).map_err(|e| WslcError::Utf8Error(format!("用户名非法: {e}")))?;
        let c_pass =
            CString::new(password).map_err(|e| WslcError::Utf8Error(format!("密码非法: {e}")))?;

        let mut token_ptr: *mut i8 = std::ptr::null_mut();
        let mut token_type = WslcIdentityTokenType::Unknown;
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        let hr = unsafe {
            WslcSessionAuthenticate(
                session.as_raw(),
                c_server.as_ptr(),
                c_user.as_ptr(),
                c_pass.as_ptr(),
                &mut token_ptr,
                &mut token_type,
                &mut err_msg,
            )
        };

        unsafe {
            WslcError::check(hr, err_msg)?;
        }

        if token_ptr.is_null() {
            return Err(WslcError::RegistryBlockedByPolicy(
                "认证返回了空令牌".to_string(),
            ));
        }

        let token_str = unsafe {
            let s = CStr::from_ptr(token_ptr).to_string_lossy().to_string();
            CoTaskMemFree(token_ptr.cast());
            s
        };

        Ok(AuthTokenResult {
            identity_token: token_str,
            token_type,
        })
    }
}

// ==================== 镜像地址与加速器解析 ====================

const DEFAULT_REGISTRY: &str = "docker.io";
const DEFAULT_MIRROR_ENV: &str = "WSLC_REGISTRY_MIRROR";
const MIRROR_ENV_PREFIX: &str = "WSLC_REGISTRY_MIRROR_";

/// 解析镜像地址并应用配置的环境变量镜像加速器
pub fn resolve_image_reference(image: &str) -> Result<String, WslcError> {
    resolve_image_reference_with(image, |key| std::env::var(key).ok())
}

/// 支持自定义环境变量查找器的镜像解析
pub(crate) fn resolve_image_reference_with<F>(image: &str, mut env: F) -> Result<String, WslcError>
where
    F: FnMut(&str) -> Option<String>,
{
    let parsed = parse_image_reference(image);
    let registry = parsed.registry.unwrap_or(DEFAULT_REGISTRY);

    let Some(mirror) = mirror_for_registry(registry, &mut env)? else {
        return Ok(image.to_string());
    };

    Ok(format!(
        "{}/{}",
        mirror.trim_end_matches('/'),
        parsed.repository_with_tag
    ))
}

fn mirror_for_registry<F>(registry: &str, env: &mut F) -> Result<Option<String>, WslcError>
where
    F: FnMut(&str) -> Option<String>,
{
    let registry_key: String = registry
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    let exact_key = format!("{MIRROR_ENV_PREFIX}{registry_key}");

    let mirror = env(&exact_key).or_else(|| {
        if registry == DEFAULT_REGISTRY {
            env(DEFAULT_MIRROR_ENV)
        } else {
            None
        }
    });

    match mirror.map(|v| v.trim().to_string()) {
        Some(v) if v.is_empty() => Err(WslcError::InvalidConfiguration(
            "镜像加速器环境变量未配置具体地址".to_string(),
        )),
        Some(v) if v.contains("://") => {
            let stripped = v.split("://").nth(1).unwrap_or(&v).to_string();
            Ok(Some(stripped))
        }
        Some(v) => Ok(Some(v)),
        None => Ok(None),
    }
}

struct ParsedImage<'a> {
    registry: Option<&'a str>,
    repository_with_tag: &'a str,
}

fn parse_image_reference(image: &str) -> ParsedImage<'_> {
    let (first, rest) = image.split_once('/').unwrap_or((image, ""));
    let has_registry = first == "localhost" || first.contains('.') || first.contains(':');
    if has_registry && !rest.is_empty() {
        ParsedImage {
            registry: Some(first),
            repository_with_tag: rest,
        }
    } else {
        ParsedImage {
            registry: None,
            repository_with_tag: image,
        }
    }
}
