//! WSLC 容器内进程创建、信号分发与标准 I/O 交互

use crate::container::WslcContainerHandle;
use crate::error::WslcError;
use core::ffi::c_void;
use std::ffi::CString;
use std::os::windows::raw::HANDLE;
use wslcsdk_sys::types::{
    WslcProcess, WslcProcessCallbacks, WslcProcessFlags, WslcProcessIOHandle, WslcProcessSettings,
    WslcProcessState, WslcSignal,
};
use wslcsdk_sys::*;

/// 容器内进程构建建造者
pub struct ProcessBuilder {
    working_directory: Option<String>,
    cmd_line: Vec<String>,
    env_variables: Vec<String>,
    flags: WslcProcessFlags,
    callbacks: Option<WslcProcessCallbacks>,
    callback_context: *mut c_void,
    stream_state: Option<std::sync::Arc<std::sync::Mutex<StreamState>>>,
}

impl Default for ProcessBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessBuilder {
    pub fn new() -> Self {
        Self {
            working_directory: None,
            cmd_line: Vec::new(),
            env_variables: Vec::new(),
            flags: 0,
            callbacks: None,
            callback_context: std::ptr::null_mut(),
            stream_state: None,
        }
    }

    pub fn working_directory(mut self, dir: impl Into<String>) -> Self {
        self.working_directory = Some(dir.into());
        self
    }

    pub fn command(mut self, args: &[impl AsRef<str>]) -> Self {
        self.cmd_line = args.iter().map(|s| s.as_ref().to_string()).collect();
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env_variables.push(format!("{key}={value}"));
        self
    }

    pub fn enable_stdin(mut self, enable: bool) -> Self {
        if enable {
            self.flags |= wslcsdk_sys::types::WSLC_PROCESS_FLAG_STDIN;
        } else {
            self.flags &= !wslcsdk_sys::types::WSLC_PROCESS_FLAG_STDIN;
        }
        self
    }

    pub fn callbacks(mut self, callbacks: WslcProcessCallbacks, context: *mut c_void) -> Self {
        self.callbacks = Some(callbacks);
        self.callback_context = context;
        self
    }

    fn apply_cmd_line(
        &self,
        settings: &mut WslcProcessSettings,
        retained_strings: &mut Vec<CString>,
    ) -> Result<(), WslcError> {
        if self.cmd_line.is_empty() {
            return Ok(());
        }
        let mut c_args = Vec::with_capacity(self.cmd_line.len());
        for arg in &self.cmd_line {
            let c_str = CString::new(arg.as_str())
                .map_err(|e| WslcError::Utf8Error(format!("命令参数非法: {e}")))?;
            c_args.push(c_str);
        }
        let ptrs: Vec<*const i8> = c_args.iter().map(|c| c.as_ptr()).collect();
        let hr = unsafe { WslcSetProcessSettingsCmdLine(settings, ptrs.as_ptr(), ptrs.len()) };
        if hr < 0 {
            return Err(WslcError::Win32(
                hr as u32,
                "设置进程命令行参数失败".to_string(),
            ));
        }
        retained_strings.extend(c_args);
        Ok(())
    }

    fn apply_env_variables(
        &self,
        settings: &mut WslcProcessSettings,
        retained_strings: &mut Vec<CString>,
    ) -> Result<(), WslcError> {
        if self.env_variables.is_empty() {
            return Ok(());
        }
        let mut c_envs = Vec::with_capacity(self.env_variables.len());
        for env in &self.env_variables {
            let c_str = CString::new(env.as_str())
                .map_err(|e| WslcError::Utf8Error(format!("环境变量非法: {e}")))?;
            c_envs.push(c_str);
        }
        let ptrs: Vec<*const i8> = c_envs.iter().map(|c| c.as_ptr()).collect();
        let hr = unsafe { WslcSetProcessSettingsEnvVariables(settings, ptrs.as_ptr(), ptrs.len()) };
        if hr < 0 {
            return Err(WslcError::Win32(
                hr as u32,
                "设置进程环境变量失败".to_string(),
            ));
        }
        retained_strings.extend(c_envs);
        Ok(())
    }

    /// 转换为底层 C 进程设置
    pub(crate) fn build_raw_settings(
        self,
    ) -> Result<(WslcProcessSettings, Vec<CString>), WslcError> {
        let mut settings = WslcProcessSettings::default();
        let hr = unsafe { WslcInitProcessSettings(&mut settings) };
        if hr < 0 {
            return Err(WslcError::Win32(
                hr as u32,
                "初始化进程配置失败".to_string(),
            ));
        }

        let mut retained_strings = Vec::new();

        if let Some(ref dir) = self.working_directory {
            let c_dir = CString::new(dir.as_str())
                .map_err(|e| WslcError::Utf8Error(format!("工作目录非法: {e}")))?;
            let hr =
                unsafe { WslcSetProcessSettingsWorkingDirectory(&mut settings, c_dir.as_ptr()) };
            if hr < 0 {
                return Err(WslcError::Win32(
                    hr as u32,
                    "设置进程工作目录失败".to_string(),
                ));
            }
            retained_strings.push(c_dir);
        }

        self.apply_cmd_line(&mut settings, &mut retained_strings)?;
        self.apply_env_variables(&mut settings, &mut retained_strings)?;

        if self.flags != 0 {
            let hr = unsafe { WslcSetProcessSettingsFlags(&mut settings, self.flags) };
            if hr < 0 {
                return Err(WslcError::Win32(
                    hr as u32,
                    "设置进程标志位失败".to_string(),
                ));
            }
        }

        if let Some(ref cb) = self.callbacks {
            let hr = unsafe {
                WslcSetProcessSettingsCallbacks(&mut settings, cb, self.callback_context)
            };
            if hr < 0 {
                return Err(WslcError::Win32(
                    hr as u32,
                    "设置进程回调函数失败".to_string(),
                ));
            }
        }

        Ok((settings, retained_strings))
    }

    /// 在运行中的容器中派生新进程
    pub fn spawn(self, container: &WslcContainerHandle) -> Result<WslcProcessHandle, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let stream_state = self.stream_state.clone();
        let (mut raw_settings, _keep_alive) = self.build_raw_settings()?;
        let mut raw_process = WslcProcess::NULL;
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        let hr = unsafe {
            WslcCreateContainerProcess(
                container.as_raw(),
                &mut raw_settings,
                &mut raw_process,
                &mut err_msg,
            )
        };

        unsafe {
            WslcError::check(hr, err_msg)?;
        }

        if raw_process.is_null() {
            return Err(WslcError::InvalidHandle);
        }

        Ok(WslcProcessHandle {
            inner: std::sync::Arc::new(ProcessInner {
                raw: raw_process,
                _container: Some(container.clone()),
                _stream_state: stream_state,
            }),
        })
    }
}

struct ProcessInner {
    raw: WslcProcess,
    _container: Option<WslcContainerHandle>,
    _stream_state: Option<std::sync::Arc<std::sync::Mutex<StreamState>>>,
}

impl Drop for ProcessInner {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                let _ = WslcReleaseProcess(self.raw);
            }
            self.raw = WslcProcess::NULL;
        }
    }
}

unsafe impl Send for ProcessInner {}
unsafe impl Sync for ProcessInner {}

/// 安全的 WSLC 进程 RAII 句柄包装 (内部通过 Arc 托管生命周期，支持轻量安全 Clone)
#[derive(Clone)]
pub struct WslcProcessHandle {
    inner: std::sync::Arc<ProcessInner>,
}

impl WslcProcessHandle {
    pub(crate) fn from_raw(raw: WslcProcess, container: Option<WslcContainerHandle>) -> Self {
        Self {
            inner: std::sync::Arc::new(ProcessInner {
                raw,
                _container: container,
                _stream_state: None,
            }),
        }
    }

    /// 获取所属容器句柄克隆
    pub fn container(&self) -> Option<WslcContainerHandle> {
        self.inner._container.clone()
    }

    /// 获取内部原始句柄
    pub fn as_raw(&self) -> WslcProcess {
        self.inner.raw
    }

    /// 获取进程 PID
    pub fn pid(&self) -> Result<u32, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut pid = 0u32;
        let hr = unsafe { WslcGetProcessPid(self.inner.raw, &mut pid) };
        if hr >= 0 {
            Ok(pid)
        } else {
            Err(WslcError::Win32(hr as u32, "获取进程 PID 失败".to_string()))
        }
    }

    /// 获取进程退出通知事件句柄
    ///
    /// # 所有权与生命周期
    /// 返回的 Win32 事件句柄（`HANDLE`）由底层 WSLC SDK 管理其生命周期，属于只读借用。
    /// 调用方可使用 `WaitForSingleObject` 等 Win32 同步原语监听进程退出事件，
    /// 但**严禁**由调用方直接调用 `CloseHandle`，否则将破坏 SDK 内部状态并导致双重释放。
    pub fn exit_event(&self) -> Result<HANDLE, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut event: HANDLE = std::ptr::null_mut();
        let hr = unsafe { WslcGetProcessExitEvent(self.inner.raw, &mut event) };
        if hr >= 0 {
            Ok(event)
        } else {
            Err(WslcError::Win32(
                hr as u32,
                "获取进程退出事件句柄失败".to_string(),
            ))
        }
    }

    /// 获取当前进程运行状态
    pub fn state(&self) -> Result<WslcProcessState, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut state = WslcProcessState::Unknown;
        let hr = unsafe { WslcGetProcessState(self.inner.raw, &mut state) };
        if hr >= 0 {
            Ok(state)
        } else {
            Err(WslcError::Win32(hr as u32, "获取进程状态失败".to_string()))
        }
    }

    /// 获取进程退出码
    pub fn exit_code(&self) -> Result<i32, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut code = 0i32;
        let hr = unsafe { WslcGetProcessExitCode(self.inner.raw, &mut code) };
        if hr >= 0 {
            Ok(code)
        } else {
            Err(WslcError::Win32(
                hr as u32,
                "获取进程退出码失败".to_string(),
            ))
        }
    }

    /// 向容器内进程发送 POSIX 信号
    pub fn signal(&self, sig: WslcSignal) -> Result<(), WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let hr = unsafe { WslcSignalProcess(self.inner.raw, sig) };
        if hr >= 0 {
            Ok(())
        } else {
            Err(WslcError::Win32(
                hr as u32,
                "向进程发送信号失败".to_string(),
            ))
        }
    }

    /// 获取进程指定流的 Win32 文件句柄 (stdin / stdout / stderr)
    ///
    /// # 所有权与生命周期
    /// 返回的 Win32 文件流句柄（`HANDLE`）由底层容器进程管理生命周期，属于借用。
    /// 调用方可用于直接读取或写入进程标准流，但**严禁**随意调用 `CloseHandle`，
    /// 句柄会在进程终止或 `WslcProcessHandle` 析构时由 SDK 自动清理。
    pub fn io_handle(&self, io: WslcProcessIOHandle) -> Result<HANDLE, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut handle: HANDLE = std::ptr::null_mut();
        let hr = unsafe { WslcGetProcessIOHandle(self.inner.raw, io, &mut handle) };
        if hr >= 0 && !handle.is_null() {
            Ok(handle)
        } else {
            Err(WslcError::Win32(
                hr as u32,
                "获取进程标准流句柄失败".to_string(),
            ))
        }
    }
}

// ==================== 异步流式 I/O 桥接支持 ====================

/// 容器进程异步流式接收端
pub struct ProcessStreams {
    pub stdout_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    pub stderr_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    pub exit_rx: tokio::sync::oneshot::Receiver<i32>,
    _stream_state: std::sync::Arc<std::sync::Mutex<StreamState>>,
}

struct StreamState {
    stdout_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    stderr_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    exit_tx: Option<tokio::sync::oneshot::Sender<i32>>,
}

unsafe extern "system" fn stream_io_trampoline(
    io_handle: WslcProcessIOHandle,
    data: *const u8,
    data_bytes: u32,
    context: *mut c_void,
) {
    if context.is_null() || data.is_null() || data_bytes == 0 {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let bytes = unsafe { std::slice::from_raw_parts(data, data_bytes as usize) }.to_vec();
        let state_mutex = unsafe { &*(context as *const std::sync::Mutex<StreamState>) };
        if let Ok(guard) = state_mutex.lock() {
            match io_handle {
                WslcProcessIOHandle::Stdout => {
                    if let Err(e) = guard.stdout_tx.try_send(bytes) {
                        log::warn!("容器标准输出缓冲区满载或已断开，数据可能丢失: {e}");
                    }
                }
                WslcProcessIOHandle::Stderr => {
                    if let Err(e) = guard.stderr_tx.try_send(bytes) {
                        log::warn!("容器标准错误缓冲区满载或已断开，数据可能丢失: {e}");
                    }
                }
                _ => {}
            }
        }
    }));
}

unsafe extern "system" fn stream_exit_trampoline(exit_code: i32, context: *mut c_void) {
    if context.is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let state_mutex = unsafe { &*(context as *const std::sync::Mutex<StreamState>) };
        if let Ok(mut guard) = state_mutex.lock()
            && let Some(tx) = guard.exit_tx.take()
        {
            let _ = tx.send(exit_code);
        }
    }));
}

impl ProcessBuilder {
    /// 启用基于 Tokio 有界通道的异步流式 I/O 捕获
    ///
    /// 通过 C 回调拦截标准输出与标准错误并推入异步通道，
    /// 彻底规避 Windows 同步匿名管道无法接入重叠 I/O 的缺陷。
    pub fn with_streaming_io(mut self) -> (Self, ProcessStreams) {
        let (stdout_tx, stdout_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
        let (stderr_tx, stderr_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
        let (exit_tx, exit_rx) = tokio::sync::oneshot::channel::<i32>();

        let state = std::sync::Arc::new(std::sync::Mutex::new(StreamState {
            stdout_tx,
            stderr_tx,
            exit_tx: Some(exit_tx),
        }));

        let raw_state_ptr = std::sync::Arc::as_ptr(&state) as *mut c_void;

        let callbacks = WslcProcessCallbacks {
            on_stdout: Some(stream_io_trampoline),
            on_stderr: Some(stream_io_trampoline),
            on_exit: Some(stream_exit_trampoline),
        };

        self.callbacks = Some(callbacks);
        self.callback_context = raw_state_ptr;
        self.stream_state = Some(state.clone());

        let streams = ProcessStreams {
            stdout_rx,
            stderr_rx,
            exit_rx,
            _stream_state: state,
        };

        (self, streams)
    }
}
