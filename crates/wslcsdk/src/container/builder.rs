//! WSLC 容器构建与配置建造者

use super::handle::WslcContainerHandle;
use super::settings;
use super::types::{ContainerPortMappingData, RetainedContainerMetadata};
use crate::error::WslcError;
use crate::process::ProcessBuilder;
use crate::session::WslcSessionHandle;
use std::ffi::CString;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use wslcsdk_sys::types::{
    WslcContainer, WslcContainerFlags, WslcContainerNetworkingMode, WslcContainerSettings,
    WslcPortProtocol,
};
use wslcsdk_sys::*;

/// 容器构建与配置建造者
///
/// 本类型不实现 `Clone`：其内嵌的 [`ProcessBuilder`] 可能已注册流式 I/O 回调，
/// 浅克隆会使多个容器共用同一套回调上下文与通道。
#[derive(Debug)]
pub struct ContainerBuilder {
    pub(super) image_name: String,
    pub(super) name: Option<String>,
    pub(super) init_process: Option<ProcessBuilder>,
    pub(super) networking_mode: Option<WslcContainerNetworkingMode>,
    pub(super) host_name: Option<String>,
    pub(super) domain_name: Option<String>,
    pub(super) flags: WslcContainerFlags,
    pub(super) port_mappings: Vec<ContainerPortMappingData>,
    pub(super) volumes: Vec<(PathBuf, String, bool)>,
    pub(super) named_volumes: Vec<(String, String, bool)>,
}

impl ContainerBuilder {
    /// 基于镜像名称创建容器建造者
    pub fn new(image_name: impl Into<String>) -> Self {
        Self {
            image_name: image_name.into(),
            name: None,
            init_process: None,
            networking_mode: None,
            host_name: None,
            domain_name: None,
            flags: 0,
            port_mappings: Vec::new(),
            volumes: Vec::new(),
            named_volumes: Vec::new(),
        }
    }

    /// 设置容器名称
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// 设置容器启动时执行的初始化主进程配置
    pub fn init_process(mut self, process: ProcessBuilder) -> Self {
        self.init_process = Some(process);
        self
    }

    /// 设置容器网络模式
    pub fn networking_mode(mut self, mode: WslcContainerNetworkingMode) -> Self {
        self.networking_mode = Some(mode);
        self
    }

    /// 设置容器主机名
    pub fn host_name(mut self, host_name: impl Into<String>) -> Self {
        self.host_name = Some(host_name.into());
        self
    }

    /// 设置容器域名
    pub fn domain_name(mut self, domain_name: impl Into<String>) -> Self {
        self.domain_name = Some(domain_name.into());
        self
    }

    /// 设置容器运行标志位
    pub fn flags(mut self, flags: WslcContainerFlags) -> Self {
        self.flags = flags;
        self
    }

    /// 获取当前设置的容器运行标志位
    pub fn get_flags(&self) -> WslcContainerFlags {
        self.flags
    }

    /// 设置是否在容器退出后自动删除
    pub fn auto_remove(mut self, enable: bool) -> Self {
        if enable {
            self.flags |= WSLC_CONTAINER_FLAG_AUTO_REMOVE;
        } else {
            self.flags &= !WSLC_CONTAINER_FLAG_AUTO_REMOVE;
        }
        self
    }

    /// 设置是否开启特权容器模式
    pub fn privileged(mut self, enable: bool) -> Self {
        if enable {
            self.flags |= WSLC_CONTAINER_FLAG_PRIVILEGED;
        } else {
            self.flags &= !WSLC_CONTAINER_FLAG_PRIVILEGED;
        }
        self
    }

    /// 设置是否为容器启用 GPU 虚拟化直通
    pub fn enable_gpu(mut self, enable: bool) -> Self {
        if enable {
            self.flags |= WSLC_CONTAINER_FLAG_ENABLE_GPU;
        } else {
            self.flags &= !WSLC_CONTAINER_FLAG_ENABLE_GPU;
        }
        self
    }

    /// 添加端口映射规则 (默认绑定所有宿主机接口 0.0.0.0)
    pub fn add_port_mapping(
        mut self,
        windows_port: u16,
        container_port: u16,
        protocol: WslcPortProtocol,
    ) -> Self {
        self.port_mappings.push(ContainerPortMappingData {
            windows_port,
            container_port,
            protocol,
            bind_ip: None,
        });
        self
    }

    /// 添加指定宿主机绑定 IP 的端口映射规则
    pub fn add_port_mapping_with_ip(
        mut self,
        windows_port: u16,
        container_port: u16,
        protocol: WslcPortProtocol,
        bind_ip: IpAddr,
    ) -> Self {
        self.port_mappings.push(ContainerPortMappingData {
            windows_port,
            container_port,
            protocol,
            bind_ip: Some(bind_ip),
        });
        self
    }

    /// 通过端口映射描述字符串添加规则 (例如 `"8080:80"`、`"127.0.0.1:8080:80/tcp"`、`"[::1]:9090:90/udp"`)
    pub fn add_port_mapping_str(mut self, mapping_str: &str) -> Result<Self, WslcError> {
        let mapping: ContainerPortMappingData = mapping_str.parse()?;
        self.port_mappings.push(mapping);
        Ok(self)
    }

    /// 添加宿主机目录卷挂载
    pub fn add_volume(
        mut self,
        windows_path: impl AsRef<Path>,
        container_path: &str,
        read_only: bool,
    ) -> Self {
        self.volumes.push((
            windows_path.as_ref().to_path_buf(),
            container_path.to_string(),
            read_only,
        ));
        self
    }

    /// 添加会话具名 VHD 存储卷挂载
    pub fn add_named_volume(mut self, name: &str, container_path: &str, read_only: bool) -> Self {
        self.named_volumes
            .push((name.to_string(), container_path.to_string(), read_only));
        self
    }

    /// 在指定会话中创建容器并返回 RAII 托管句柄
    pub fn build(self, session: &WslcSessionHandle) -> Result<WslcContainerHandle, WslcError> {
        // 句柄创建路径**必须**建立线程级 COM 套间：官方 SDK 在
        // WslcCreateSession / WslcOpenContainer / WslcCreateContainer /
        // WslcCreateContainerProcess 内部会回调至本进程，未初始化套间时
        // 返回 CO_E_NOTINITIALIZED。此处不可依赖句柄自带的进程级租约——
        // 那只保证已建立句柄的 RPC 绑定有效，不提供调用入口所需的套间上下文。
        let _com_guard = crate::com::try_initialize_mta()?;

        let c_image = CString::new(self.image_name.as_str())
            .map_err(|e| WslcError::NulError(format!("镜像名称含非法空字节: {e}")))?;

        let mut settings = WslcContainerSettings::default();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcInitContainerSettings(c_image.as_ptr(), &mut settings) };
        WslcError::check_hr(hr, "初始化容器配置失败")?;

        let basic = settings::apply_metadata(&self, &mut settings)?;
        let (ports, sockaddrs) = settings::apply_port_mappings(&self, &mut settings)?;
        let volumes = settings::apply_volumes(&self, &mut settings)?;
        let named_volumes = settings::apply_named_volumes(&self, &mut settings)?;

        let (init_retained, init_stream_state) = if let Some(init_builder) = self.init_process {
            let stream_state = init_builder.stream_state();
            let mut retained = init_builder.build_raw_settings()?;
            // SAFETY: err_msg 为官方 _Outptr_opt_result_z_ 输出参数，
            // 其所有权在本行交由 RAII 包装接管并自动释放。
            let hr = unsafe {
                WslcSetContainerSettingsInitProcess(&mut settings, &mut retained.settings)
            };
            WslcError::check_hr(hr, "设置容器主进程配置失败")?;
            (Some(retained), stream_state)
        } else {
            (None, None)
        };

        let _retained_data = RetainedContainerMetadata {
            _image: c_image,
            _basic: basic,
            _ports: ports,
            _sockaddrs: sockaddrs,
            _volumes: volumes,
            _named_volumes: named_volumes,
            _init_process: init_retained,
        };

        let mut raw_container = WslcContainer::NULL;
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe {
            WslcCreateContainer(
                session.as_raw(),
                &settings,
                &mut raw_container,
                &mut err_msg,
            )
        };

        // SAFETY: err_msg 为官方 _Outptr_opt_result_z_ 输出参数，
        // 其所有权在本行交由 RAII 包装接管并自动释放。
        unsafe {
            WslcError::check(hr, err_msg)?;
        }

        if raw_container.is_null() {
            return Err(WslcError::InvalidHandle);
        }

        log::info!(
            "WSLC 容器创建成功，基础镜像: '{}'，所属会话: '{}'",
            self.image_name,
            session.name()
        );

        WslcContainerHandle::from_raw_inner(raw_container, session.clone(), init_stream_state)
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

    /// 回归：容器创建路径必须自行建立线程级 COM 套间
    ///
    /// `WslcCreateContainer` 与 `WslcCreateSession` / `WslcOpenContainer` /
    /// `WslcCreateContainerProcess` 同属官方「创建路径」，内部均会回调本进程。
    /// 本库已在其余三条创建路径上建立线程级套间，唯独此处一度遗漏，
    /// 使 `build_async` 在未初始化套间的工作线程上会以
    /// `CO_E_NOTINITIALIZED`(0x800401F0) 失败。
    ///
    /// 与 `session.rs` 中同类回归测试的判据一致：SDK 缺失时跳过，
    /// 否则断言错误不属于套间缺失。

    #[test]
    fn test_flag_setters_toggle_independent_bits() {
        let builder = ContainerBuilder::new("ubuntu:latest")
            .name("test-container")
            .auto_remove(true);
        assert_eq!(builder.get_flags() & WSLC_CONTAINER_FLAG_AUTO_REMOVE, 1);

        let privileged = ContainerBuilder::new("ubuntu:latest").privileged(true);
        assert_eq!(
            privileged.get_flags() & WSLC_CONTAINER_FLAG_PRIVILEGED,
            WSLC_CONTAINER_FLAG_PRIVILEGED
        );
        assert_eq!(privileged.get_flags() & WSLC_CONTAINER_FLAG_AUTO_REMOVE, 0);

        // 关闭标志位应清除对应比特，且不影响其他比特
        let cleared = ContainerBuilder::new("ubuntu:latest")
            .auto_remove(true)
            .enable_gpu(true)
            .auto_remove(false);
        assert_eq!(cleared.get_flags() & WSLC_CONTAINER_FLAG_AUTO_REMOVE, 0);
        assert_eq!(
            cleared.get_flags() & WSLC_CONTAINER_FLAG_ENABLE_GPU,
            WSLC_CONTAINER_FLAG_ENABLE_GPU
        );
    }

    #[test]
    fn test_port_mapping_with_explicit_bind_ip() {
        let builder = ContainerBuilder::new("ubuntu:latest")
            .add_port_mapping_with_ip(
                8080,
                80,
                WslcPortProtocol::Tcp,
                "127.0.0.1".parse().unwrap(),
            )
            .add_port_mapping_with_ip(9090, 90, WslcPortProtocol::Udp, "::1".parse().unwrap());
        // IPv4 与 IPv6 绑定地址均需被接受，且不污染标志位
        assert_eq!(builder.get_flags(), 0);
    }

    #[test]
    fn test_port_mapping_str_chaining() {
        let builder = ContainerBuilder::new("ubuntu:latest")
            .add_port_mapping_str("8080:80")
            .expect("IPv4 端口映射解析失败")
            .add_port_mapping_str("127.0.0.1:9000:9000/udp")
            .expect("带绑定地址的端口映射解析失败");
        assert_eq!(builder.get_flags(), 0);

        // 非法格式必须在构建阶段被拒绝
        assert!(
            ContainerBuilder::new("ubuntu:latest")
                .add_port_mapping_str("not-a-mapping")
                .is_err()
        );
    }

    #[test]
    fn test_volume_validation_rejects_invalid_mounts() {
        let session = fake_session();

        // 宿主机挂载路径不能为空
        let empty_host = ContainerBuilder::new("ubuntu:latest").add_volume("", "/data", false);
        assert!(empty_host.build(&session).is_err());

        // 容器挂载路径必须是绝对路径
        let relative_container =
            ContainerBuilder::new("ubuntu:latest").add_volume("C:\\data", "relative/path", false);
        assert!(relative_container.build(&session).is_err());

        // 具名卷名称不能为空
        let empty_name =
            ContainerBuilder::new("ubuntu:latest").add_named_volume("   ", "/data", false);
        assert!(empty_name.build(&session).is_err());

        // 具名卷的容器挂载路径同样必须是绝对路径
        let relative_named =
            ContainerBuilder::new("ubuntu:latest").add_named_volume("myvol", "data", false);
        assert!(relative_named.build(&session).is_err());
    }

    #[test]
    fn test_container_with_plain_init_process() {
        let process_builder = ProcessBuilder::new()
            .command(&["/bin/bash", "-c", "echo hello"])
            .working_directory("/workspace")
            .env("ENV_KEY", "ENV_VAL");

        let builder = ContainerBuilder::new("ubuntu:latest")
            .name("test-container")
            .init_process(process_builder);
        assert_eq!(builder.get_flags(), 0);
    }

    #[test]
    fn test_container_with_streaming_init_process_survives_early_stream_drop() {
        let (proc_builder, streams) = ProcessBuilder::new()
            .command(&["/bin/sh"])
            .with_streaming_io();
        // 外部即使过早 drop 掉接收端，init 进程的 stream_state 仍被安全托管
        drop(streams);
        let builder = ContainerBuilder::new("alpine:latest").init_process(proc_builder);
        assert_eq!(builder.get_flags(), 0);
    }
}
