//! WSLC 镜像仓库鉴权与 Identity Token 生成
//!
//! # 镜像加速器环境变量
//!
//! [`resolve_image_reference`] 会按下列环境变量改写拉取地址（仅影响拉取，
//! 不影响推送——推送目标应由调用方显式给出）：
//!
//! - `WSLC_REGISTRY_MIRROR`：作用于默认仓库 `docker.io`；
//! - `WSLC_REGISTRY_MIRROR_<REGISTRY>`：作用于指定仓库，
//!   其中 `<REGISTRY>` 为仓库地址中非字母数字字符统一替换为 `_` 并转为
//!   大写后的形式，例如 `ghcr.io` 对应 `WSLC_REGISTRY_MIRROR_GHCR_IO`。
//!
//! 取值可带 `http://` / `https://` 前缀，该前缀会被剥离。
//!
//! > **安全提示**：这些变量构成一处外部可控的镜像地址注入点——任何能写入
//! > 进程环境的主体都能借此把拉取静默重定向到任意主机。因此取值会经字符
//! > 白名单校验（非白名单字符一律拒绝并报错），但**能够设置这些变量的主体
//! > 本身即已具备影响本进程行为的权限**。多租户或共享构建环境下，请用
//! > 容器/服务级环境变量而非全局环境变量承载镜像加速器配置。

use crate::com_memory::ComAnsiString;
use crate::error::WslcError;
use crate::session::WslcSessionHandle;
use std::ffi::CString;
use wslcsdk_sys::types::WslcIdentityTokenType;
use wslcsdk_sys::*;

/// 仓库鉴权认证结果
///
/// 刻意**不派生 `Debug`**，改为手工实现并对 `identity_token` 脱敏。
/// 该令牌是可直接用于拉取私有镜像的 Bearer 凭证，敏感级别等同于密码；
/// 若沿用派生实现，任何 `{:?}` 格式化都会把它明文写出——包括
/// `unwrap()` 的 panic 消息、`Result` 的错误链，以及调用方随手写的
/// 调试日志。派生属编译期展开、无运行时代码，clippy 等 lint 无法检出。
///
/// 对照：本库 `CrashDumpSubscription` 亦手工实现了 `Debug` 并刻意
/// 不打印上下文指针数值（避免泄漏堆地址），可见「`Debug` 须防泄漏」
/// 的防护意识已确立，此处只是漏掉。
#[derive(Clone, PartialEq, Eq)]
pub struct AuthTokenResult {
    /// 身份令牌本身
    ///
    /// 属敏感凭证：不要写入日志，持久化前请评估其有效期与泄漏后果。
    pub identity_token: String,
    /// 令牌的类型，决定其后续使用方式
    pub token_type: WslcIdentityTokenType,
}

impl std::fmt::Debug for AuthTokenResult {
    /// 令牌字段固定输出 `[已脱敏]`，令牌类型如常打印
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthTokenResult")
            .field("identity_token", &"[已脱敏]")
            .field("token_type", &self.token_type)
            .finish()
    }
}

impl Drop for AuthTokenResult {
    fn drop(&mut self) {
        // 敏感数据卫生：令牌是可直接用于拉取私有镜像的 Bearer 凭证，
        // 敏感级别等同密码。若放任 String 直接释放，明文会留在堆上等待
        // 被复用，可被堆转储或内存扫描取得。
        //
        // 与密码处一致，用 write_volatile 而非普通写入：后者可能被编译器
        // 当作死存储消除，使清零形同虚设。
        //
        // SAFETY: 写入目标为本类型独占持有的 String 缓冲区，长度自 as_mut_vec
        // 取得故不会越界；全零字节是合法 UTF-8，清零后 String 仍处于有效状态，
        // 其析构可正常进行。
        unsafe {
            for byte in self.identity_token.as_mut_vec() {
                std::ptr::write_volatile(byte, 0);
            }
        }
    }
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
        let mut token_type = WslcIdentityTokenType::Unknown;
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
        //
        // 刻意**只清密码**，不清理用户名、服务器地址与返回的令牌：
        // 1. 三者的敏感级别远低于密码——用户名与服务器地址本就是
        //    调用方自己持有的入参，清零本副本对该入参无影响；
        // 2. `identity_token` 需作为返回值交付调用方（`String` 所有权转移），
        //    其擦除责任在调用方；若在此处连同 `String` 一并清零，
        //    等于交还一个已被破坏的凭证，破坏 API 契约。
        //
        // 之所以用 `write_volatile` 而非普通写入：普通写入可能被编译器
        // 优化为死存储消除，使清零形同虚设；volatile 写入保证真实发生。
        //
        // 之所以清零**确实擦到了**交给官方的那块内存：`into_bytes_with_nul`
        // 走 `Box<[u8]>::into_vec`，复用 CString 原有分配而不另建副本，
        // 故下方写入的就是 SDK 实际读取过的缓冲区。
        //
        // 已知的残余（本库无法消除，记录备查）：`CString::new` 内部先
        // `to_vec()` 再 `push(0)`，而 `to_vec()` 的容量恰等于长度，故 push
        // 会触发一次扩容——扩容前的那份缓冲区含明文密码且被直接释放，
        // 未及清零。要消除它只能改用自定义的安全分配器，代价远超收益。
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
        let token = unsafe { ComAnsiString::from_raw(token_ptr) }.ok_or_else(|| {
            WslcError::UnexpectedSdkResult(
                "WslcSessionAuthenticate 返回成功状态却未给出令牌字符串".to_string(),
            )
        })?;

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
        Some(v) => {
            let normalized = strip_mirror_scheme(&v)?;
            validate_mirror_address(&normalized)?;
            Ok(Some(normalized))
        }
        None => Ok(None),
    }
}

/// 剥离镜像加速器地址的 URL 方案前缀
///
/// 加速器常被配置为 `https://mirror.example.com` 形式，故需剥离 `scheme://`
/// 前缀，只保留官方镜像地址所能接受的 `host[:port][/prefix]` 部分。
///
/// 刻意按**首个** `://` 一次性切分并校验其后不含 `://`：`split("://").nth(1)`
/// 虽能取出第二段，却会把 `a://b://c` 这类畸形值悄悄截断为 `b`，把注入痕迹
/// 藏进了一个看似合法的主机名里。宁可显式报错，也不静默改写调用方的配置。
fn strip_mirror_scheme(value: &str) -> Result<String, WslcError> {
    let Some((scheme, rest)) = value.split_once("://") else {
        return Ok(value.to_string());
    };

    // 方案部分只允许 http/https：官方镜像地址不接受自定义 scheme，
    // 而 `file://` 之类若放行会带来本地文件读取的意外语义
    if !matches!(scheme, "http" | "https") {
        return Err(WslcError::InvalidConfiguration(format!(
            "镜像加速器仅支持 http 或 https 方案，实际为: {scheme}://"
        )));
    }
    if rest.contains("://") {
        return Err(WslcError::InvalidConfiguration(format!(
            "镜像加速器地址含多个 scheme 分隔符，疑似畸形配置: {value}"
        )));
    }

    Ok(rest.to_string())
}

/// 校验镜像加速器地址的字符构成
///
/// 该值来自环境变量，并会被拼进官方 `WslcPullSessionImageOptions::uri`，
/// 等价于一处**外部可控的镜像地址注入点**：任何能写入进程环境的主体
/// （CI 配置、共享构建机的全局环境、容器 `-e`、父进程的环境块）都能影响
/// 此处取值。若放任任意字符，`docker.io/library/alpine:latest` 会被静默
/// 重写到非预期主机，且官方调用照样成功返回——失败模式完全不可见，
/// 构成供应链投毒路径。
///
/// 故按白名单收紧：只放行构成合法 registry 地址所必需的字符
/// （字母数字、`.`、`-` 构成主机名与域名标签，`:` 构成端口，
/// `/` 构成部分镜像服务所需的路径前缀），其余一律拒绝。
fn validate_mirror_address(value: &str) -> Result<(), WslcError> {
    const ALLOWED_EXTRA: &[char] = &['.', '-', ':', '/'];

    if value.is_empty() {
        return Err(WslcError::InvalidConfiguration(
            "镜像加速器地址剥离方案前缀后为空".to_string(),
        ));
    }

    if let Some(bad) = value
        .chars()
        .find(|c| !c.is_ascii_alphanumeric() && !ALLOWED_EXTRA.contains(c))
    {
        return Err(WslcError::InvalidConfiguration(format!(
            "镜像加速器地址含非法字符 {bad:?}：{value}；\
             仅允许字母数字与 `.` `-` `:` `/`（分别用于主机名、端口与路径前缀）"
        )));
    }

    Ok(())
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

    /// 构造一个恒返回指定加速器值的查找器
    fn mirror_env(value: &str) -> impl FnMut(&str) -> Option<String> + '_ {
        move |_| Some(value.to_string())
    }

    /// 回归：加速器地址含非白名单字符时必须报错，不得静默放行
    ///
    /// 缺陷成因：原实现仅做 `trim` 与剥scheme，随后直接
    /// `format!("{mirror}/{repo}")` 拼进官方拉取 URI，等价于一处
    /// 外部可控的镜像地址注入点。任何能写入进程环境的主体（CI 配置、
    /// 共享构建机全局环境、容器 `-e`、父进程环境块）都可借此把
    /// `docker.io/library/alpine` 静默重定向到任意主机，
    /// 且官方调用照样成功返回——失败模式完全不可见。
    #[test]
    fn test_mirror_with_injection_characters_is_rejected() {
        // 用户信息分隔符：形如 user@host 的注入
        assert!(
            resolve_image_reference_with("alpine:latest", mirror_env("user@evil.com")).is_err(),
            "含 `@` 的加速器地址必须被拒绝，否则可注入用户信息段"
        );
        // 查询串与片段：可改变官方对地址的解释
        assert!(
            resolve_image_reference_with("alpine:latest", mirror_env("evil.com?a=b")).is_err(),
            "含 `?` 的加速器地址必须被拒绝"
        );
        assert!(
            resolve_image_reference_with("alpine:latest", mirror_env("evil.com#f")).is_err(),
            "含 `#` 的加速器地址必须被拒绝"
        );
        // 空白与控制字符：可绕过高亮显示或日志审计
        assert!(
            resolve_image_reference_with("alpine:latest", mirror_env("evil .com")).is_err(),
            "含空格的加速器地址必须被拒绝"
        );
        assert!(
            resolve_image_reference_with("alpine:latest", mirror_env("evil\n.com")).is_err(),
            "含换行的加速器地址必须被拒绝，否则可污染日志输出"
        );
        assert!(
            resolve_image_reference_with("alpine:latest", mirror_env("evil\t.com")).is_err(),
            "含制表符的加速器地址必须被拒绝"
        );
        // 首尾空白属无害格式，已由前置的 trim 归一化，不应误判为非法
        assert_eq!(
            resolve_image_reference_with("alpine:latest", mirror_env("  evil.example.com\n"))
                .expect("首尾空白应被 trim 归一化后接受"),
            "evil.example.com/alpine:latest"
        );
        // 反斜杠：Windows 路径语义，不应出现在 registry 地址中
        assert!(
            resolve_image_reference_with("alpine:latest", mirror_env(r"evil.com\path")).is_err(),
            "含反斜杠的加速器地址必须被拒绝"
        );
    }

    /// 合法地址必须被正常接受，避免白名单过严损害可用性
    #[test]
    fn test_mirror_with_legal_address_forms_is_accepted() {
        // 主机名
        assert_eq!(
            resolve_image_reference_with("alpine:latest", mirror_env("mirror.example.com"))
                .expect("合法主机名应被接受"),
            "mirror.example.com/alpine:latest"
        );
        // 主机名 + 端口（私有镜像仓库常见形态）
        assert_eq!(
            resolve_image_reference_with("alpine:latest", mirror_env("127.0.0.1:5000"))
                .expect("合法 host:port 应被接受"),
            "127.0.0.1:5000/alpine:latest"
        );
        // 主机名 + 路径前缀（如 registry.example.com/docker.io）
        assert_eq!(
            resolve_image_reference_with(
                "alpine:latest",
                mirror_env("registry.example.com/docker")
            )
            .expect("合法路径前缀应被接受"),
            "registry.example.com/docker/alpine:latest"
        );
        // 连字符域名标签
        assert_eq!(
            resolve_image_reference_with("alpine:latest", mirror_env("my-mirror-01.example.com"))
                .expect("含连字符的合法主机名应被接受"),
            "my-mirror-01.example.com/alpine:latest"
        );
    }

    /// 非 http/https 方案必须被拒绝
    ///
    /// `file://` 之类若放行，会给镜像拉取带来本地文件读取的意外语义。
    #[test]
    fn test_mirror_rejects_non_http_schemes() {
        for bad in ["file://evil/path", "ftp://evil.com", "gopher://evil.com"] {
            let res = resolve_image_reference_with("alpine:latest", mirror_env(bad));
            assert!(
                matches!(res, Err(WslcError::InvalidConfiguration(_))),
                "方案 {bad:?} 应被拒绝，实际为: {res:?}"
            );
        }
        // http 与 https 属放行范围
        assert!(
            resolve_image_reference_with("alpine:latest", mirror_env("http://m.example.com"))
                .is_ok()
        );
        assert!(
            resolve_image_reference_with("alpine:latest", mirror_env("https://m.example.com"))
                .is_ok()
        );
    }

    /// 回归：多重 scheme 分隔符不得被静默截断
    ///
    /// 原实现为 `v.split("://").nth(1)`：对 `a://b://c` 会取出 `b` 并丢弃
    /// 其余部分，把注入痕迹藏进一个看似合法的主机名里。现改为按首个
    /// `://` 切分后显式校验其后不含分隔符，畸形配置一律报错。
    #[test]
    fn test_mirror_with_multiple_scheme_separators_is_rejected() {
        let res = resolve_image_reference_with(
            "alpine:latest",
            mirror_env("https://a.example.com://evil.com"),
        );
        assert!(
            matches!(res, Err(WslcError::InvalidConfiguration(_))),
            "含多个 scheme 分隔符的值必须报错而非被静默截断，实际为: {res:?}"
        );
    }

    /// 剥离 scheme 后为空的值必须报错
    #[test]
    fn test_mirror_empty_after_scheme_strip_is_rejected() {
        let res = resolve_image_reference_with("alpine:latest", mirror_env("https://"));
        assert!(
            matches!(res, Err(WslcError::InvalidConfiguration(_))),
            "剥离 scheme 后为空的地址必须报错，实际为: {res:?}"
        );
    }

    /// 校验函数本身的边界行为
    #[test]
    fn test_validate_mirror_address_boundaries() {
        // 空串在调用链中已被「未配置具体地址」分支拦截，此处锁定函数自身行为
        assert!(validate_mirror_address("").is_err());
        assert!(validate_mirror_address("a").is_ok());
        assert!(validate_mirror_address("a.b-c.d:1/x").is_ok());
        // 非 ASCII 亦不在白名单内（如中文、中文标点、全角字符）
        assert!(validate_mirror_address("evil.com/镜像").is_err());
        assert!(validate_mirror_address("evil.com/日本").is_err());
    }

    /// 回归：`Debug` 输出不得包含令牌明文
    ///
    /// 缺陷成因：本类型曾`#[derive(Debug, ..)]`，而 `identity_token`
    /// 是可直接用于拉取私有镜像的 Bearer 凭证，敏感级别等同密码。
    /// 派生实现会被任何 `{:?}` 格式化调用——包括 `unwrap()` 的 panic
    /// 消息、`Result` 错误链与随手写的调试日志——从而明文外泄，
    /// 违反 `AGENTS.md` §5「严禁在任何日志输出中打印明文Token」。
    ///
    /// 该缺陷无法由 clippy检出：`derive` 是编译期展开，不产生可分析的
    /// 运行时代码，只能由本用例锁定。
    #[test]
    fn test_debug_output_redacts_identity_token() {
        let result = AuthTokenResult {
            identity_token: "dG9rZW4tc2VjcmV0LXZhbHVl".to_string(),
            token_type: WslcIdentityTokenType::Unknown,
        };
        let debug = format!("{result:?}");

        assert!(
            !debug.contains("dG9rZW4tc2VjcmV0LXZhbHVl"),
            "Debug 输出绝不得包含令牌明文，实际为: {debug}"
        );
        assert!(
            debug.contains("[已脱敏]"),
            "令牌位置应以脱敏占位符呈现，实际为: {debug}"
        );
        // 令牌类型不敏感，应照常打印以便排查问题
        assert!(
            debug.contains("token_type"),
            "令牌类型应保留在 Debug 输出中，实际为: {debug}"
        );
    }
}
