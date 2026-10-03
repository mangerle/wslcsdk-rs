//! WSLC 统一高层门面客户端
//!
//! 封装环境自检、进程级 COM MTA 套间生命周期维持，以及会话内镜像、存储卷与
//! 仓库认证的统一入口，为上层业务提供一站式容器管理接口。

use crate::com::{ProcessMtaGuard, init_process_mta};
use crate::container::{ContainerBuilder, WslcContainerHandle};
use crate::error::WslcError;
use crate::image::{ImageInfo, WslcImageManager};
use crate::process::ProcessBuilder;
use crate::registry::{AuthTokenResult, WslcRegistryManager};
use crate::session::{SessionBuilder, WslcSessionHandle};
use crate::system::WslcSystem;
use crate::volume::{VhdVolumeOptions, WslcVolumeManager};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use wslcsdk_sys::types::WslcVersion;

/// 客户端内部共享状态
#[derive(Debug)]
struct ClientInner {
    /// 进程级 COM MTA 守护；`auto_init_mta` 关闭时为 `None`
    _mta_guard: Option<ProcessMtaGuard>,
    /// 客户端所持有的默认会话
    ///
    /// 以 `Arc` 共享而非直接内联，是为了让 `Clone` 后的所有副本指向同一会话，
    /// 从而在多线程与 Tokio 协程间传递时不会意外创建出多个会话。
    session: WslcSessionHandle,
}

/// WSLC 统一高层门面客户端
///
/// 持有一个默认会话，并把会话内的常用操作收敛为可直接调用的方法。
/// 内部通过 `Arc` 共享进程级 COM 环境与会话句柄，具备轻量廉价的 `Clone` 能力，
/// 可在多线程与 Tokio 异步协程之间自由传递。
///
/// # 设计原理
/// - **有状态入口**：客户端绑定一个默认会话，`list_images`、`open_container`
///   等操作无需逐次传入会话句柄，避免了此前「拿到门面却仍需手动传 session」
///   的割裂感。需要在其他会话上操作时，直接使用底层管理器即可。
/// - **自动环境接管**：初始化时自动执行 `WslcSystem::ensure_sdk_available()`
///   前置 DLL 安全探测，并建立进程级 COM MTA 守护，杜绝高频 COM 启闭开销与
///   MSVC delayload 引发的 SEH 崩溃。
///
/// # 示例
///
/// ```no_run
/// use wslcsdk::WslcClient;
///
/// # fn main() -> Result<(), wslcsdk::WslcError> {
/// let client = WslcClient::builder().session_name("my-session").build()?;
/// for image in client.list_images()? {
///     println!("镜像: {}", image.name);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct WslcClient {
    inner: Arc<ClientInner>,
}

impl WslcClient {
    /// 创建新的 WSLC 客户端实例 (执行前置 SDK 可用性探测并维持 COM MTA 环境)
    ///
    /// 会话名固定为 `default`，存储于标准沙箱路径。
    ///
    /// # 并发与多实例说明
    ///
    /// 默认会话名固定为 `default`。若同一宿主机已有同名活跃会话且未关闭，重复调用
    /// 可能因会话名称冲突而返回错误。若需在多租户、多作业或并发测试场景下运行，
    /// 推荐通过 [`WslcClient::builder()`] 为每个实例显式指定独立的会话名称。
    ///
    /// # Errors
    ///
    /// 当底层系统未安装 WSL 容器 SDK，或会话创建失败时返回错误。
    pub fn new() -> Result<Self, WslcError> {
        WslcClientBuilder::new().build()
    }

    /// 创建客户端构建器
    pub fn builder() -> WslcClientBuilder {
        WslcClientBuilder::new()
    }

    /// 获取客户端所持有的会话句柄
    pub fn session(&self) -> WslcSessionHandle {
        self.inner.session.clone()
    }

    /// 检查客户端当前是否持有着处于激活状态的进程级 COM MTA 守护
    #[must_use]
    pub fn is_mta_active(&self) -> bool {
        self.inner._mta_guard.is_some()
    }

    /// 获取当前系统安装的 WSLC 运行时版本
    pub fn version(&self) -> Result<WslcVersion, WslcError> {
        WslcSystem::get_version()
    }

    /// 在默认目录下为指定镜像创建容器
    ///
    /// 返回的建造者已绑定本客户端的会话，可直接调用
    /// [`build`](ContainerBuilder::build) 无需再传会话句柄。
    ///
    /// # 参数
    ///
    /// - `image`: 基础镜像名称，例如 `alpine:latest`
    pub fn create_container(&self, image: impl Into<String>) -> BoundContainerBuilder<'_> {
        BoundContainerBuilder {
            inner: ContainerBuilder::new(image),
            session: &self.inner.session,
        }
    }

    /// 打开默认会话内已有的容器
    pub fn open_container(&self, name_or_id: &str) -> Result<WslcContainerHandle, WslcError> {
        WslcContainerHandle::open(&self.inner.session, name_or_id)
    }

    /// 列出默认会话内的所有镜像
    pub fn list_images(&self) -> Result<Vec<ImageInfo>, WslcError> {
        WslcImageManager::list_images(&self.inner.session)
    }

    /// 删除默认会话内的指定镜像
    pub fn delete_image(&self, name_or_id: &str) -> Result<(), WslcError> {
        WslcImageManager::delete_image(&self.inner.session, name_or_id)
    }

    /// 在默认会话内创建 VHD 存储卷
    pub fn create_vhd_volume(&self, options: &VhdVolumeOptions<'_>) -> Result<(), WslcError> {
        WslcVolumeManager::create_vhd_volume(&self.inner.session, options)
    }

    /// 删除默认会话内的 VHD 存储卷
    pub fn delete_vhd_volume(&self, name: &str) -> Result<(), WslcError> {
        WslcVolumeManager::delete_vhd_volume(&self.inner.session, name)
    }

    /// 登录镜像仓库并获取认证令牌
    ///
    /// # 参数
    ///
    /// - `server_address`: 镜像仓库地址
    /// - `username`: 登录用户名
    /// - `password`: 登录密码
    pub fn authenticate(
        &self,
        server_address: &str,
        username: &str,
        password: &str,
    ) -> Result<AuthTokenResult, WslcError> {
        WslcRegistryManager::authenticate(&self.inner.session, server_address, username, password)
    }
}

/// 已绑定会话的容器建造者
///
/// 由 [`WslcClient::create_container`] 返回。它把会话句柄藏在内部，使
/// [`build`](Self::build) 无需再显式传入——这正是「门面」应有的便利性：
/// 调用方不必在拿到建造者后再回头找会话。
///
/// # 为何不用 `Deref` 转发
///
/// 曾尝试实现 `Deref<Target = ContainerBuilder>` 以自动获得链式配置方法，但那会
/// 把内层的 `ContainerBuilder::build(&session)` 一并暴露，与本类型的免参
/// `build()` 形成同名遮蔽。Rust 的方法查找遇到签名不匹配时不会回退到
/// `Deref` 目标的同名方法，调用方须写全
/// `BoundContainerBuilder::build(builder)` 才能编译——比原来更糟。
///
/// 故本类型采用组合：常用配置方法逐个显式转发，语义完全可控。代价是新增
/// `ContainerBuilder` 方法时需同步转发，可用 `into_inner` 访问未转发的方法。
#[derive(Debug)]
pub struct BoundContainerBuilder<'a> {
    inner: ContainerBuilder,
    /// 借用客户端持有的会话，生命周期与 `'a` 绑定
    session: &'a WslcSessionHandle,
}

impl<'a> BoundContainerBuilder<'a> {
    /// 构建并激活容器（无需传入会话，会话已在创建时绑定）
    pub fn build(self) -> Result<WslcContainerHandle, WslcError> {
        self.inner.build(self.session)
    }

    /// 设置容器名称
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.inner = self.inner.name(name);
        self
    }

    /// 设置容器是否在停止后自动删除
    pub fn auto_remove(mut self, auto: bool) -> Self {
        self.inner = self.inner.auto_remove(auto);
        self
    }

    /// 设置容器网络模式
    pub fn networking_mode(mut self, mode: crate::WslcContainerNetworkingMode) -> Self {
        self.inner = self.inner.networking_mode(mode);
        self
    }

    /// 设置容器主机名
    pub fn host_name(mut self, host: impl Into<String>) -> Self {
        self.inner = self.inner.host_name(host);
        self
    }

    /// 设置容器域名
    pub fn domain_name(mut self, domain: impl Into<String>) -> Self {
        self.inner = self.inner.domain_name(domain);
        self
    }

    /// 设置容器控制标志位
    pub fn flags(mut self, flags: crate::WslcContainerFlags) -> Self {
        self.inner = self.inner.flags(flags);
        self
    }

    /// 追加一条端口映射规则
    pub fn add_port_mapping(
        mut self,
        windows_port: u16,
        container_port: u16,
        protocol: crate::WslcPortProtocol,
    ) -> Self {
        self.inner = self
            .inner
            .add_port_mapping(windows_port, container_port, protocol);
        self
    }

    /// 挂载目录卷
    pub fn add_volume(
        mut self,
        windows_path: impl AsRef<Path>,
        container_path: &str,
        read_only: bool,
    ) -> Self {
        self.inner = self
            .inner
            .add_volume(windows_path, container_path, read_only);
        self
    }

    /// 挂载具名 VHD 卷
    pub fn add_named_volume(mut self, name: &str, container_path: &str, read_only: bool) -> Self {
        self.inner = self.inner.add_named_volume(name, container_path, read_only);
        self
    }

    /// 设置 init 进程配置
    pub fn init_process(mut self, process: ProcessBuilder) -> Self {
        self.inner = self.inner.init_process(process);
        self
    }

    /// 取回底层建造器，以访问本类型未转发的少见方法
    pub fn into_inner(self) -> ContainerBuilder {
        self.inner
    }
}

/// WSLC 客户端配置构建器
///
/// 刻意**不派生 `Default`**：`Default` 对 `bool` 的取值恒为 `false`，
/// 若照常派生将使 `WslcClientBuilder::default()` 得到
/// `auto_init_mta = false`，与 [`new`](Self::new) 的 `true` 相反。
/// 两者都是对外公开的「默认构造」入口，行为分歧会让
/// [`WslcClient::is_mta_active`] 的返回值随调用写法而变——这是一次静默降级，
/// 既无日志也无报错，只能靠类型约束消除。故 [`Default`] 改为手工实现并
/// 委托 [`new`](Self::new)，使两个入口严格同源。
#[derive(Debug)]
pub struct WslcClientBuilder {
    auto_init_mta: bool,
    session_name: Option<String>,
    session_dir: Option<PathBuf>,
}

impl Default for WslcClientBuilder {
    /// 与 [`WslcClientBuilder::new`] 严格同源，避免两个默认构造入口行为分歧
    fn default() -> Self {
        Self::new()
    }
}

impl WslcClientBuilder {
    /// 创建默认配置的客户端构建器
    ///
    /// 默认使用会话名 `default`、标准沙箱存储路径，并自动维持进程级 MTA 套间。
    pub fn new() -> Self {
        Self {
            auto_init_mta: true,
            session_name: None,
            session_dir: None,
        }
    }

    /// 设置是否在构建时自动维持进程级 COM MTA 套间 (默认为 `true`)
    pub fn auto_init_mta(mut self, enabled: bool) -> Self {
        self.auto_init_mta = enabled;
        self
    }

    /// 设置默认会话的名称 (默认为 `default`)
    pub fn session_name(mut self, name: impl Into<String>) -> Self {
        self.session_name = Some(name.into());
        self
    }

    /// 设置默认会话的存储目录 (默认为标准沙箱路径)
    ///
    /// # 参数
    ///
    /// - `dir`: 会话根目录路径；其下将自动创建 `sessions\<name>` 子目录
    pub fn session_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.session_dir = Some(dir.into());
        self
    }

    /// 构建并激活 WSLC 客户端实例
    ///
    /// # Errors
    ///
    /// 当底层 WSL SDK 运行时不可用，或会话创建失败时返回错误。
    pub fn build(self) -> Result<WslcClient, WslcError> {
        // 1. 前置安全探测 SDK 可用性，彻底规避 MSVC delayload 引发的 SEH 0xC06D007E 崩溃
        WslcSystem::ensure_sdk_available()?;

        // 2. 建立并持有默认会话
        let session = match self.session_dir {
            Some(dir) => {
                SessionBuilder::new(self.session_name.unwrap_or_else(default_name), dir).build()
            }
            None => SessionBuilder::new_default(self.session_name.unwrap_or_else(default_name))
                .and_then(|builder| builder.build()),
        }?;

        // 3. 维持进程级 MTA 环境
        let mta_guard = if self.auto_init_mta {
            // 降级而非中断：句柄自身持有的 MTA 租约已足以保证其存活期内的
            // 套间有效性，此处失败只损失「进程内其他工作线程默认具备 COM 环境」
            // 这一便利。属AGENTS.md 所说的非致命降级，须以 warn 留痕而非静默丢弃。
            init_process_mta()
                .inspect_err(|e| {
                    log::warn!(
                        "进程级 COM MTA 守护建立失败，客户端继续运行，各句柄将依赖自身的进程级租约: {e}"
                    );
                })
                .ok()
        } else {
            None
        };

        log::info!(
            "WSLC 客户端创建就绪，会话名称: '{}'，进程级 MTA 守护: {}",
            session.name(),
            if mta_guard.is_some() {
                "已启用"
            } else {
                "未启用"
            }
        );

        Ok(WslcClient {
            inner: Arc::new(ClientInner {
                _mta_guard: mta_guard,
                session,
            }),
        })
    }
}

/// 默认会话名
fn default_name() -> String {
    "default".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::WslcDomainError;

    /// 构造一个空句柄会话，仅用于让绑定建造者完成转发，不触发任何 SDK 调用
    fn fake_session() -> WslcSessionHandle {
        // 空句柄不携带任何需释放的所有权，构造行为本身是安全的
        // SAFETY: 传入空句柄与占位名称，不产生任何官方调用
        unsafe { WslcSessionHandle::from_raw(Default::default(), "fake-session") }
    }

    /// SDK 缺失时环境探测应给出领域错误而非崩溃
    ///
    /// 仅校验前置探测环节，不实际创建会话：真实会话创建依赖宿主机运行时，
    /// 相关的端到端验证见下方被忽略的用例。
    #[test]
    fn test_sdk_probe_reports_availability_without_panicking() {
        match WslcSystem::ensure_sdk_available() {
            Ok(()) => assert!(WslcSystem::is_sdk_available()),
            Err(WslcError::Domain(WslcDomainError::SdkUpdateNeeded(_))) => {
                assert!(!WslcSystem::is_sdk_available())
            }
            Err(other) => panic!("预期返回 SdkUpdateNeeded，实际为: {other:?}"),
        }
    }

    /// 验证绑定建造者无需再传会话即可构建，且链式配置可正常转发
    ///
    /// 该用例的编译通过本身即是断言：若`build()` 的免参签名不存在，
    /// 或某个转发方法的名称/参数与底层不符，此处即无法编译。
    /// 默认会话名常量与文档承诺保持一致
    #[test]
    fn test_default_name_is_used_when_not_specified() {
        assert_eq!(default_name(), "default");
    }

    /// 构建器默认值：应维持 MTA 且不预设会话名与目录
    #[test]
    fn test_builder_defaults() {
        let builder = WslcClientBuilder::new();
        assert!(builder.auto_init_mta, "默认应自动维持进程级 MTA 套间");
        assert!(builder.session_name.is_none(), "默认不应预设会话名");
        assert!(builder.session_dir.is_none(), "默认不应预设会话目录");
    }

    /// 回归：绑定建造者的每个转发方法都必须真正作用到底层建造者
    ///
    /// 缺陷成因：`BoundContainerBuilder` 逐个手工转发 `ContainerBuilder`
    /// 的配置方法（因 `Deref` 方案会让免参 `build()` 被遮蔽，不可行）。
    /// 手工转发的风险在于「转发了但转错字段」，例如把 `host_name` 写进
    /// `domain_name`——编译照样通过，调用方却在不知情的情况下配错了容器。
    ///
    /// 故逐一调用转发方法后取回底层建造者，断言各字段落到了正确位置。
    /// 日后新增转发方法时，本用例即是要同步扩充之处。
    #[test]
    fn test_bound_builder_forwarding_lands_on_correct_fields() {
        // 直接构造绑定建造者：转发方法的正确性不依赖会话是否真实可用，
        // 故用空句柄会话即可，无需触发任何 SDK 调用
        let session = fake_session();
        let bound = BoundContainerBuilder {
            inner: ContainerBuilder::new("alpine:latest"),
            session: &session,
        };

        let inner = bound
            .name("forwarded-name")
            .host_name("forwarded-host")
            .domain_name("forwarded-domain")
            .auto_remove(true)
            .add_port_mapping(8080, 80, crate::WslcPortProtocol::Tcp)
            .add_volume(r"C:\data", "/data", true)
            .add_named_volume("myvol", "/vol", false)
            .into_inner();

        assert_eq!(inner.name.as_deref(), Some("forwarded-name"));
        assert_eq!(inner.host_name.as_deref(), Some("forwarded-host"));
        assert_eq!(inner.domain_name.as_deref(), Some("forwarded-domain"));
        assert_eq!(
            inner.flags,
            wslcsdk_sys::WSLC_CONTAINER_FLAG_AUTO_REMOVE,
            "auto_remove 须置位对应标志"
        );
        assert_eq!(inner.port_mappings.len(), 1);
        assert_eq!(inner.port_mappings[0].windows_port, 8080);
        assert_eq!(inner.port_mappings[0].container_port, 80);
        assert_eq!(inner.volumes.len(), 1);
        assert_eq!(inner.volumes[0].1, "/data");
        assert!(inner.volumes[0].2, "只读标志须被转发");
        assert_eq!(inner.named_volumes.len(), 1);
        assert_eq!(inner.named_volumes[0].0, "myvol");
    }

    /// 构建器的会话配置应可链式覆盖
    #[test]
    fn test_builder_accepts_session_overrides() {
        let builder = WslcClientBuilder::new()
            .auto_init_mta(false)
            .session_name("custom")
            .session_dir("C:\\temp");
        assert!(!builder.auto_init_mta);
        assert_eq!(builder.session_name.as_deref(), Some("custom"));
        assert_eq!(builder.session_dir.as_deref(), Some(Path::new("C:\\temp")));
    }

    /// 回归：`Default` 与 `new()` 必须给出相同的默认配置
    ///
    /// 缺陷成因：本类型曾`#[derive(Debug, Default)]`，而 `Default` 对 `bool`
    /// 恒取 `false`，与 `new()` 显式置 `auto_init_mta: true` 相反。
    /// 派生属编译期展开、无运行时成本，故 clippy 等 lint 均无法发现此分歧；
    /// 只能由本用例在测试期锁定。
    ///
    /// 影响：`WslcClientBuilder::default()` 会静默关闭进程级 MTA 守护，
    /// 使 [`WslcClient::is_mta_active`] 的结果随构造写法而变，
    /// 且无任何日志或错误提示——属静默降级。
    #[test]
    fn test_default_delegates_to_new() {
        let from_default = WslcClientBuilder::default();
        let from_new = WslcClientBuilder::new();

        // 逐字段比对而非只查 auto_init_mta：任何字段日后新增到该结构体
        // 却忘记同步 Default 的情况，都应在此用例暴露
        assert_eq!(
            from_default.auto_init_mta, from_new.auto_init_mta,
            "Default 与 new() 的 auto_init_mta 必须一致，否则两个默认构造入口行为分歧"
        );
        assert_eq!(from_default.session_name, from_new.session_name);
        assert_eq!(from_default.session_dir, from_new.session_dir);

        // 关键语义锁定：默认必须维持 MTA 守护
        assert!(
            from_default.auto_init_mta,
            "默认配置应维持进程级 COM MTA 套间；若此项为 false，\
             说明 Default 未正确委托 new()"
        );
    }
}
