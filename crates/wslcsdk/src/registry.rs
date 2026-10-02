//! WSLC 镜像仓库鉴权与 Identity Token 生成

use crate::com_memory::ComAnsiString;
use crate::error::WslcError;
use crate::session::WslcSessionHandle;
use std::ffi::CString;
use wslcsdk_sys::types::WslcIdentityTokenType;
use wslcsdk_sys::*;

/// 仓库鉴权认证结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthTokenResult {
    /// 身份令牌本身
    pub identity_token: String,
    /// 令牌的类型，决定其后续使用方式
    pub token_type: WslcIdentityTokenType,
}

/// 镜像仓库管理器
///
/// 全部方法均为关联函数，不持有状态；使用 `WslcSessionHandle` 显式指定操作会话。
#[derive(Debug, Clone, Copy, Default)]
pub struct WslcRegistryManager;

impl WslcRegistryManager {
    /// 登录镜像仓库并生成 Base64 编码的 identity token
    pub fn authenticate(
        session: &WslcSessionHandle,
        server_address: &str,
        username: &str,
        password: &str,
    ) -> Result<AuthTokenResult, WslcError> {
        let c_server = CString::new(server_address)
            .map_err(|e| WslcError::NulError(format!("服务器地址非法: {e}")))?;
        let c_user =
            CString::new(username).map_err(|e| WslcError::NulError(format!("用户名非法: {e}")))?;
        let c_pass =
            CString::new(password).map_err(|e| WslcError::NulError(format!("密码非法: {e}")))?;

        let mut token_ptr: *mut i8 = std::ptr::null_mut();
        let mut token_type = WslcIdentityTokenType::UNKNOWN;
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
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

        // 敏感数据卫生：使用 volatile 写入立即清空密码内存，防止堆残留
        let mut pass_bytes = c_pass.into_bytes_with_nul();
        for b in &mut pass_bytes {
            // SAFETY: 写入目标为本地刚刚拆解的拥有型字节切片，地址合法有效且独占借用。
            unsafe {
                std::ptr::write_volatile(b, 0);
            }
        }

        // SAFETY: err_msg 为官方 _Outptr_opt_result_z_ 输出参数，
        // 其所有权在本行交由 RAII 包装接管并自动释放。
        unsafe {
            WslcError::check(hr, err_msg)?;
        }

        // 接口已声明成功，却未按契约给出令牌指针，属于 SDK 侧的契约违背。
        // 此处不可使用 RegistryBlockedByPolicy：该变体表示镜像仓库访问被安全策略
        // 拦截，与本情形毫无关系，会严重误导排查方向。
        if token_ptr.is_null() {
            return Err(WslcError::UnexpectedSdkResult(
                "WslcSessionAuthenticate 返回成功状态却未给出令牌指针".to_string(),
            ));
        }

        // SAFETY: token_ptr 为官方 _Outptr_result_z_ 输出参数，所有权移交本侧
        let token = unsafe { ComAnsiString::from_raw(token_ptr) }
            .expect("前序空指针检查已保证令牌指针非空");

        Ok(AuthTokenResult {
            identity_token: token
                .as_str()
                .map_err(|e| {
                    WslcError::InvalidConfiguration(format!("仓库返回的认证令牌非合法 UTF-8: {e}"))
                })?
                .to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mirror_applies_to_default_registry() {
        let res = resolve_image_reference_with("ubuntu:latest", |k| {
            if k == DEFAULT_MIRROR_ENV {
                Some("mirror.example.com".to_string())
            } else {
                None
            }
        });
        assert_eq!(res.expect("解析失败"), "mirror.example.com/ubuntu:latest");
    }

    #[test]
    fn test_mirror_applies_to_named_registry() {
        let res = resolve_image_reference_with("ghcr.io/org/repo:1.0", |k| {
            if k == "WSLC_REGISTRY_MIRROR_GHCR_IO" {
                Some("ghcr-mirror.example.com".to_string())
            } else {
                None
            }
        });
        assert_eq!(
            res.expect("解析失败"),
            "ghcr-mirror.example.com/org/repo:1.0"
        );
    }

    #[test]
    fn test_default_mirror_env_is_not_applied_to_other_registries() {
        // 默认镜像加速器环境变量只能作用于 docker.io，不得污染其他仓库
        let res = resolve_image_reference_with("ghcr.io/org/repo:1.0", |k| {
            if k == DEFAULT_MIRROR_ENV {
                Some("mirror.example.com".to_string())
            } else {
                None
            }
        });
        assert_eq!(res.expect("解析失败"), "ghcr.io/org/repo:1.0");
    }

    #[test]
    fn test_mirror_with_scheme_and_trailing_slash_is_normalized() {
        let res = resolve_image_reference_with("ubuntu:latest", |k| {
            if k == DEFAULT_MIRROR_ENV {
                Some("https://mirror.example.com/".to_string())
            } else {
                None
            }
        });
        assert_eq!(res.expect("解析失败"), "mirror.example.com/ubuntu:latest");
    }

    #[test]
    fn test_blank_mirror_is_rejected() {
        let res = resolve_image_reference_with("ubuntu:latest", |k| {
            if k == DEFAULT_MIRROR_ENV {
                Some("   ".to_string())
            } else {
                None
            }
        });
        match res {
            Err(WslcError::InvalidConfiguration(_)) => {}
            other => panic!("预期返回 InvalidConfiguration，实际为: {other:?}"),
        }
    }

    #[test]
    fn test_parse_image_reference_registry_detection() {
        // 无仓库前缀：整体视为 docker.io 的仓库路径
        let plain = parse_image_reference("ubuntu:latest");
        assert_eq!(plain.registry, None);
        assert_eq!(plain.repository_with_tag, "ubuntu:latest");

        // 含点号的域名被视为仓库地址
        let domain = parse_image_reference("ghcr.io/org/repo:1.0");
        assert_eq!(domain.registry, Some("ghcr.io"));
        assert_eq!(domain.repository_with_tag, "org/repo:1.0");

        // 含端口的仓库地址
        let with_port = parse_image_reference("127.0.0.1:5000/team/app:v1");
        assert_eq!(with_port.registry, Some("127.0.0.1:5000"));
        assert_eq!(with_port.repository_with_tag, "team/app:v1");

        // localhost 无点号也必须识别为仓库地址
        let local = parse_image_reference("localhost/team/app");
        assert_eq!(local.registry, Some("localhost"));
        assert_eq!(local.repository_with_tag, "team/app");

        // 含冒号但无斜杠：属于 tag 而非仓库地址
        let tag_only = parse_image_reference("ubuntu:22.04");
        assert_eq!(tag_only.registry, None);
        assert_eq!(tag_only.repository_with_tag, "ubuntu:22.04");

        // 无斜杠且无冒号
        let bare = parse_image_reference("alpine");
        assert_eq!(bare.registry, None);
        assert_eq!(bare.repository_with_tag, "alpine");
    }

    #[test]
    fn test_no_mirror_configured_returns_input_verbatim() {
        let res = resolve_image_reference_with("ubuntu:latest", |_| None);
        assert_eq!(res.expect("解析失败"), "ubuntu:latest");
    }
}
