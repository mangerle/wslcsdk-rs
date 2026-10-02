//! WSLC 容器设置建造者与容器生命周期控制

use crate::error::WslcError;
use crate::session::WslcSessionHandle;
use core::ffi::c_void;
use std::ffi::{CStr, CString};
use std::sync::Arc;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use wslcsdk_sys::types::{
    WSLC_CONTAINER_ID_BUFFER_SIZE, WslcContainer, WslcContainerFlags, WslcContainerNamedVolume,
    WslcContainerNetworkingMode, WslcContainerPortMapping, WslcContainerSettings,
    WslcContainerStartFlags, WslcContainerState, WslcContainerVolume, WslcDeleteContainerFlags,
    WslcPortProtocol, WslcProcess, WslcProcessCallbacks, WslcProcessSettings, WslcSignal,
};
use wslcsdk_sys::*;

/// 安全的端口映射数据
#[derive(Clone, Debug)]
pub struct ContainerPortMappingData {
    pub windows_port: u16,
    pub container_port: u16,
    pub protocol: WslcPortProtocol,
}

struct RetainedContainerMetadata {
    _name: Option<CString>,
    _host: Option<CString>,
    _domain: Option<CString>,
}

/// 容器构建与配置建造者
#[derive(Clone, Debug)]
pub struct ContainerBuilder {
    image_name: String,
    name: Option<String>,
    init_process: Option<WslcProcessSettings>,
    networking_mode: Option<WslcContainerNetworkingMode>,
    host_name: Option<String>,
    domain_name: Option<String>,
    flags: WslcContainerFlags,
    port_mappings: Vec<ContainerPortMappingData>,
    volumes: Vec<(Vec<u16>, String, bool)>,
    named_volumes: Vec<(String, String, bool)>,
}

impl ContainerBuilder {
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

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn networking_mode(mut self, mode: WslcContainerNetworkingMode) -> Self {
        self.networking_mode = Some(mode);
        self
    }

    pub fn host_name(mut self, host_name: impl Into<String>) -> Self {
        self.host_name = Some(host_name.into());
        self
    }

    pub fn domain_name(mut self, domain_name: impl Into<String>) -> Self {
        self.domain_name = Some(domain_name.into());
        self
    }

    pub fn flags(mut self, flags: WslcContainerFlags) -> Self {
        self.flags = flags;
        self
    }

    pub fn get_flags(&self) -> WslcContainerFlags {
        self.flags
    }

    pub fn auto_remove(mut self, enable: bool) -> Self {
        if enable {
            self.flags |= wslcsdk_sys::types::WSLC_CONTAINER_FLAG_AUTO_REMOVE;
        } else {
            self.flags &= !wslcsdk_sys::types::WSLC_CONTAINER_FLAG_AUTO_REMOVE;
        }
        self
    }

    pub fn privileged(mut self, enable: bool) -> Self {
        if enable {
            self.flags |= wslcsdk_sys::types::WSLC_CONTAINER_FLAG_PRIVILEGED;
        } else {
            self.flags &= !wslcsdk_sys::types::WSLC_CONTAINER_FLAG_PRIVILEGED;
        }
        self
    }

    pub fn enable_gpu(mut self, enable: bool) -> Self {
        if enable {
            self.flags |= wslcsdk_sys::types::WSLC_CONTAINER_FLAG_ENABLE_GPU;
        } else {
            self.flags &= !wslcsdk_sys::types::WSLC_CONTAINER_FLAG_ENABLE_GPU;
        }
        self
    }

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
        });
        self
    }

    pub fn add_volume(mut self, windows_path: &str, container_path: &str, read_only: bool) -> Self {
        let wide_win: Vec<u16> = windows_path
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        self.volumes
            .push((wide_win, container_path.to_string(), read_only));
        self
    }

    pub fn add_named_volume(mut self, name: &str, container_path: &str, read_only: bool) -> Self {
        self.named_volumes
            .push((name.to_string(), container_path.to_string(), read_only));
        self
    }

    fn apply_metadata(
        &self,
        settings: &mut WslcContainerSettings,
    ) -> Result<RetainedContainerMetadata, WslcError> {
        let _name = if let Some(ref name) = self.name {
            let c = CString::new(name.as_str())
                .map_err(|e| WslcError::Utf8Error(format!("容器名称含非法空字节: {e}")))?;
            let hr = unsafe { WslcSetContainerSettingsName(settings, c.as_ptr()) };
            if hr < 0 {
                return Err(WslcError::Win32(hr as u32, "设置容器名称失败".to_string()));
            }
            Some(c)
        } else {
            None
        };

        if let Some(mode) = self.networking_mode {
            let hr = unsafe { WslcSetContainerSettingsNetworkingMode(settings, mode) };
            if hr < 0 {
                return Err(WslcError::Win32(
                    hr as u32,
                    "设置容器网络模式失败".to_string(),
                ));
            }
        }

        let _host = if let Some(ref host) = self.host_name {
            let c = CString::new(host.as_str())
                .map_err(|e| WslcError::Utf8Error(format!("主机名含非法空字节: {e}")))?;
            let hr = unsafe { WslcSetContainerSettingsHostName(settings, c.as_ptr()) };
            if hr < 0 {
                return Err(WslcError::Win32(
                    hr as u32,
                    "设置容器主机名失败".to_string(),
                ));
            }
            Some(c)
        } else {
            None
        };

        let _domain = if let Some(ref domain) = self.domain_name {
            let c = CString::new(domain.as_str())
                .map_err(|e| WslcError::Utf8Error(format!("域名含非法空字节: {e}")))?;
            let hr = unsafe { WslcSetContainerSettingsDomainName(settings, c.as_ptr()) };
            if hr < 0 {
                return Err(WslcError::Win32(hr as u32, "设置容器域名失败".to_string()));
            }
            Some(c)
        } else {
            None
        };

        if self.flags != 0 {
            let hr = unsafe { WslcSetContainerSettingsFlags(settings, self.flags) };
            if hr < 0 {
                return Err(WslcError::Win32(
                    hr as u32,
                    "设置容器标志位失败".to_string(),
                ));
            }
        }

        Ok(RetainedContainerMetadata {
            _name,
            _host,
            _domain,
        })
    }

    fn apply_port_mappings(&self, settings: &mut WslcContainerSettings) -> Result<(), WslcError> {
        if !self.port_mappings.is_empty() {
            let raw_ports: Vec<WslcContainerPortMapping> = self
                .port_mappings
                .iter()
                .map(|p| WslcContainerPortMapping {
                    windows_port: p.windows_port,
                    container_port: p.container_port,
                    protocol: p.protocol,
                    windows_address: std::ptr::null_mut(),
                })
                .collect();
            let hr = unsafe {
                WslcSetContainerSettingsPortMappings(
                    settings,
                    raw_ports.as_ptr(),
                    raw_ports.len() as u32,
                )
            };
            if hr < 0 {
                return Err(WslcError::Win32(
                    hr as u32,
                    "设置容器端口映射失败".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn apply_volumes(
        &self,
        settings: &mut WslcContainerSettings,
    ) -> Result<Vec<CString>, WslcError> {
        if self.volumes.is_empty() {
            return Ok(Vec::new());
        }
        let mut c_vols = Vec::with_capacity(self.volumes.len());
        let mut retained = Vec::with_capacity(self.volumes.len());
        for (w, c, ro) in &self.volumes {
            if w[..w.len().saturating_sub(1)].contains(&0) {
                return Err(WslcError::Utf8Error(
                    "挂载宿主机路径含非法空字符".to_string(),
                ));
            }
            let c_str = CString::new(c.as_str())
                .map_err(|e| WslcError::Utf8Error(format!("挂载路径非法: {e}")))?;
            c_vols.push(WslcContainerVolume {
                windows_path: w.as_ptr(),
                container_path: c_str.as_ptr(),
                read_only: if *ro { 1 } else { 0 },
            });
            retained.push(c_str);
        }
        let hr = unsafe {
            WslcSetContainerSettingsVolumes(settings, c_vols.as_ptr(), c_vols.len() as u32)
        };
        if hr < 0 {
            return Err(WslcError::Win32(
                hr as u32,
                "设置容器目录卷挂载失败".to_string(),
            ));
        }
        Ok(retained)
    }

    fn apply_named_volumes(
        &self,
        settings: &mut WslcContainerSettings,
    ) -> Result<Vec<(CString, CString)>, WslcError> {
        if self.named_volumes.is_empty() {
            return Ok(Vec::new());
        }
        let mut c_nvols = Vec::with_capacity(self.named_volumes.len());
        let mut retained = Vec::with_capacity(self.named_volumes.len());
        for (n, c, ro) in &self.named_volumes {
            let c_name = CString::new(n.as_str())
                .map_err(|e| WslcError::Utf8Error(format!("卷名称非法: {e}")))?;
            let c_cont = CString::new(c.as_str())
                .map_err(|e| WslcError::Utf8Error(format!("容器路径非法: {e}")))?;
            c_nvols.push(WslcContainerNamedVolume {
                name: c_name.as_ptr(),
                container_path: c_cont.as_ptr(),
                read_only: if *ro { 1 } else { 0 },
            });
            retained.push((c_name, c_cont));
        }
        let hr = unsafe {
            WslcSetContainerSettingsNamedVolumes(settings, c_nvols.as_ptr(), c_nvols.len() as u32)
        };
        if hr < 0 {
            return Err(WslcError::Win32(
                hr as u32,
                "设置容器具名 VHD 卷挂载失败".to_string(),
            ));
        }
        Ok(retained)
    }

    /// 在指定会话中创建容器
    pub fn build(self, session: &WslcSessionHandle) -> Result<WslcContainerHandle, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let c_image = CString::new(self.image_name.as_str())
            .map_err(|e| WslcError::Utf8Error(format!("镜像名称含非法空字节: {e}")))?;

        let mut settings = WslcContainerSettings::default();
        let hr = unsafe { WslcInitContainerSettings(c_image.as_ptr(), &mut settings) };
        if hr < 0 {
            return Err(WslcError::Win32(
                hr as u32,
                "初始化容器配置失败".to_string(),
            ));
        }

        let _retained_meta = self.apply_metadata(&mut settings)?;
        self.apply_port_mappings(&mut settings)?;
        let _retained_vols = self.apply_volumes(&mut settings)?;
        let _retained_named_vols = self.apply_named_volumes(&mut settings)?;

        if let Some(mut init) = self.init_process {
            let hr = unsafe { WslcSetContainerSettingsInitProcess(&mut settings, &mut init) };
            if hr < 0 {
                return Err(WslcError::Win32(
                    hr as u32,
                    "设置容器主进程配置失败".to_string(),
                ));
            }
        }

        let mut raw_container = WslcContainer::NULL;
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        let hr = unsafe {
            WslcCreateContainer(
                session.as_raw(),
                &settings,
                &mut raw_container,
                &mut err_msg,
            )
        };

        unsafe {
            WslcError::check(hr, err_msg)?;
        }

        if raw_container.is_null() {
            return Err(WslcError::InvalidHandle);
        }

        Ok(WslcContainerHandle {
            inner: Arc::new(ContainerInner {
                raw: raw_container,
                _session: session.clone(),
            }),
        })
    }
}

struct ContainerInner {
    raw: WslcContainer,
    _session: WslcSessionHandle,
}

impl Drop for ContainerInner {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                let _ = WslcReleaseContainer(self.raw);
            }
            self.raw = WslcContainer::NULL;
        }
    }
}

unsafe impl Send for ContainerInner {}
unsafe impl Sync for ContainerInner {}

/// 安全的 WSLC 容器 RAII 包装对象 (内部通过 Arc 托管生命周期，支持轻量安全 Clone)
#[derive(Clone)]
pub struct WslcContainerHandle {
    inner: Arc<ContainerInner>,
}

impl WslcContainerHandle {
    /// 通过名称或容器 ID 打开已存在的容器
    pub fn open(session: &WslcSessionHandle, name_or_id: &str) -> Result<Self, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let c_str = CString::new(name_or_id)
            .map_err(|e| WslcError::Utf8Error(format!("名称或 ID 包含非法空字节: {e}")))?;

        let mut raw = WslcContainer::NULL;
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        let hr =
            unsafe { WslcOpenContainer(session.as_raw(), c_str.as_ptr(), &mut raw, &mut err_msg) };

        unsafe {
            WslcError::check(hr, err_msg)?;
        }

        if raw.is_null() {
            return Err(WslcError::InvalidHandle);
        }

        Ok(Self {
            inner: Arc::new(ContainerInner {
                raw,
                _session: session.clone(),
            }),
        })
    }

    /// 获取该容器所属的会话句柄克隆
    pub fn session(&self) -> WslcSessionHandle {
        self.inner._session.clone()
    }

    /// 获取内部原始句柄
    pub fn as_raw(&self) -> WslcContainer {
        self.inner.raw
    }

    /// 获取容器 64 位十六进制唯一哈希 ID
    pub fn id(&self) -> Result<String, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut buffer = [0i8; WSLC_CONTAINER_ID_BUFFER_SIZE];
        let hr = unsafe { WslcGetContainerID(self.inner.raw, buffer.as_mut_ptr()) };
        if hr < 0 {
            return Err(WslcError::Win32(hr as u32, "获取容器 ID 失败".to_string()));
        }

        let c_str = unsafe { CStr::from_ptr(buffer.as_ptr()) };
        c_str
            .to_str()
            .map(|s| s.to_string())
            .map_err(|e| WslcError::Utf8Error(e.to_string()))
    }

    /// 获取容器当前运行状态
    pub fn state(&self) -> Result<WslcContainerState, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut state = WslcContainerState::Invalid;
        let hr = unsafe { WslcGetContainerState(self.inner.raw, &mut state) };
        if hr >= 0 {
            Ok(state)
        } else {
            Err(WslcError::Win32(hr as u32, "获取容器状态失败".to_string()))
        }
    }

    /// 启动容器
    pub fn start(&self, attach: bool) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let flags: WslcContainerStartFlags = if attach {
            wslcsdk_sys::types::WSLC_CONTAINER_START_FLAG_ATTACH
        } else {
            wslcsdk_sys::types::WSLC_CONTAINER_START_FLAG_NONE
        };

        let mut err_msg: *mut u16 = std::ptr::null_mut();
        let hr = unsafe { WslcStartContainer(self.inner.raw, flags, &mut err_msg) };
        unsafe { WslcError::check(hr, err_msg) }
    }

    /// 停止容器
    pub fn stop(&self, signal: WslcSignal, timeout_seconds: u32) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut err_msg: *mut u16 = std::ptr::null_mut();
        let hr =
            unsafe { WslcStopContainer(self.inner.raw, signal, timeout_seconds, &mut err_msg) };
        unsafe { WslcError::check(hr, err_msg) }
    }

    /// 删除容器
    pub fn delete(&self, force: bool) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let flags: WslcDeleteContainerFlags = if force {
            wslcsdk_sys::types::WSLC_DELETE_CONTAINER_FLAG_FORCE
        } else {
            wslcsdk_sys::types::WSLC_DELETE_CONTAINER_FLAG_NONE
        };

        let mut err_msg: *mut u16 = std::ptr::null_mut();
        let hr = unsafe { WslcDeleteContainer(self.inner.raw, flags, &mut err_msg) };
        unsafe { WslcError::check(hr, err_msg) }
    }

    /// 获取容器检查数据 (JSON 快照)
    pub fn inspect(&self) -> Result<serde_json::Value, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut inspect_ptr: *mut i8 = std::ptr::null_mut();
        let hr = unsafe { WslcInspectContainer(self.inner.raw, &mut inspect_ptr) };
        if hr < 0 || inspect_ptr.is_null() {
            return Err(WslcError::Win32(
                hr as u32,
                "检查容器元数据失败".to_string(),
            ));
        }

        let json_str = unsafe {
            let s = CStr::from_ptr(inspect_ptr).to_string_lossy().to_string();
            CoTaskMemFree(inspect_ptr.cast());
            s
        };

        serde_json::from_str(&json_str).map_err(|e| WslcError::JsonError(e.to_string()))
    }

    /// 获取容器的 init 主进程句柄
    pub fn get_init_process(&self) -> Result<crate::process::WslcProcessHandle, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut raw_process = WslcProcess::NULL;
        let hr = unsafe { WslcGetContainerInitProcess(self.inner.raw, &mut raw_process) };
        if hr < 0 || raw_process.is_null() {
            return Err(WslcError::Win32(
                hr as u32,
                "获取容器主进程句柄失败".to_string(),
            ));
        }

        Ok(crate::process::WslcProcessHandle::from_raw(
            raw_process,
            Some(self.clone()),
        ))
    }

    /// 设置容器 init 主进程的标准 IO 回调
    ///
    /// # Safety
    ///
    /// 调用方必须保证 `callbacks` 与 `context` 指针在回调运行期间保持合法与存活。
    pub unsafe fn set_init_process_io_callbacks(
        &self,
        callbacks: &WslcProcessCallbacks,
        context: *mut c_void,
    ) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let hr =
            unsafe { WslcSetContainerInitProcessIOCallbacks(self.inner.raw, callbacks, context) };
        if hr >= 0 {
            Ok(())
        } else {
            Err(WslcError::Win32(
                hr as u32,
                "设置主进程 IO 回调失败".to_string(),
            ))
        }
    }
}
