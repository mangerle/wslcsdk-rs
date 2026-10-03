//! WSLC 容器内进程构建建造者
//!
//! 提供链式参数配置，包含命令行参数、环境变量、工作目录、回调函数与流式 I/O 桥接。

use super::handle::WslcProcessHandle;
use super::stream::{ProcessStreams, StreamState, setup_streaming_channels};
use crate::container::WslcContainerHandle;
use crate::error::WslcError;
use core::ffi::c_void;
use std::ffi::CString;
use std::sync::Arc;
use wslcsdk_sys::types::{
    WslcProcess, WslcProcessCallbacks, WslcProcessFlags, WslcProcessSettings,
};
use wslcsdk_sys::*;

/// 保留进程配置期间所引用的所有 C 字符串与指针数组，确保指针生命周期覆盖进程创建调用
pub(crate) struct RetainedProcessSettings {
    pub(crate) settings: WslcProcessSettings,
    _working_dir: Option<CString>,
    _cmd_args: Vec<CString>,
    _cmd_ptrs: Vec<*const i8>,
    _env_vars: Vec<CString>,
    _env_ptrs: Vec<*const i8>,
}

/// 进程回调注册信息
///
/// 上下文刻意以 `usize` 承载而非裸指针，使 [`ProcessBuilder`] 自身天然满足
/// `Send + Sync`，无需依赖 `unsafe impl` 这类无条件断言；由此带来的线程安全性
/// 责任改由 [`ProcessBuilder::callbacks`] 的 `unsafe` 契约显式约束。
#[derive(Debug)]
enum ProcessCallbackMode {
    /// 未注册任何回调
    None,
    /// 已注册回调 (流式 I/O 桥接或调用方自定义)
    Registered {
        callbacks: WslcProcessCallbacks,
        context: usize,
    },
}

/// 容器内进程构建建造者
///
/// 本类型刻意不实现 `Clone`：其可能持有已注册的回调 (尤其流式 I/O 的通道发送端
/// 与全局注册表条目)，浅克隆会使多个进程共用同一回调上下文与同一套通道，
/// 造成输出互相串流、退出信号互相抢先。
#[derive(Debug)]
pub struct ProcessBuilder {
    working_directory: Option<String>,
    cmd_line: Vec<String>,
    env_variables: Vec<String>,
    flags: WslcProcessFlags,
    callback_mode: ProcessCallbackMode,
    stream_state: Option<Arc<StreamState>>,
}

impl Default for ProcessBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessBuilder {
    /// 创建空的进程建造者
    ///
    /// 至少需通过 [`command`](Self::command) 设置命令行，否则 SDK 会拒绝创建。
    pub fn new() -> Self {
        Self {
            working_directory: None,
            cmd_line: Vec::new(),
            env_variables: Vec::new(),
            flags: 0,
            callback_mode: ProcessCallbackMode::None,
            stream_state: None,
        }
    }

    /// 设定进程在容器内的工作目录
    pub fn working_directory(mut self, dir: impl Into<String>) -> Self {
        self.working_directory = Some(dir.into());
        self
    }

    /// 设定要执行的命令行
    ///
    /// 会覆盖此前设置的命令行。`args[0]` 为可执行文件名，其余为参数。
    pub fn command(mut self, args: &[impl AsRef<str>]) -> Self {
        self.cmd_line = args.iter().map(|s| s.as_ref().to_string()).collect();
        self
    }

    /// 追加一条环境变量
    ///
    /// 同名变量可重复追加，行为与官方一致。
    ///
    /// # 参数约束
    ///
    /// `key` 不得含等号或空字符：环境变量以 `key=value` 单串形式交付官方，
    /// 键中含 `=` 会让官方按第一个等号切分，把本属键名的部分误并入值；
    /// 而键名若来自外部输入（配置文件、HTTP 请求、CI 变量），这就构成一处
    /// 环境变量注入面——注入者可借此覆盖容器内的 `PATH`、`LD_PRELOAD` 等。
    /// 故在库内直接拒绝，不再把校验责任推给调用方。`value` 则可含等号。
    ///
    /// # Errors
    ///
    /// `key` 含等号或空字符时返回 [`WslcError::InvalidConfiguration`]。
    pub fn env(mut self, key: &str, value: &str) -> Result<Self, WslcError> {
        if key.contains('=') {
            return Err(WslcError::InvalidConfiguration(format!(
                "环境变量名不得含等号，否则官方会将其误解析为键值分隔: {key}"
            )));
        }
        if key.is_empty() {
            return Err(WslcError::InvalidConfiguration(
                "环境变量名不得为空".to_string(),
            ));
        }
        self.env_variables.push(format!("{key}={value}"));
        Ok(self)
    }

    /// 开启或关闭进程的标准输入
    pub fn enable_stdin(mut self, enable: bool) -> Self {
        if enable {
            self.flags |= WSLC_PROCESS_FLAG_STDIN;
        } else {
            self.flags &= !WSLC_PROCESS_FLAG_STDIN;
        }
        self
    }

    /// 注册调用方自定义的进程回调及其上下文指针
    ///
    /// # Safety
    ///
    /// 调用方必须保证：
    /// - `context` 指向的内存在进程存活期间始终有效，且其使用方式与回调签名相符；
    /// - 若该 [`ProcessBuilder`] 会被发送到其他线程执行，`context` 所指数据必须
    ///   满足 `Send` 要求。
    ///
    /// 由于上下文以 `usize` 承载，编译器无法对上述线程安全性做任何校验，
    /// 责任完全由调用方承担。若仅需捕获标准输出与标准错误，请优先使用
    /// [`ProcessBuilder::with_streaming_io`]，其上下文生命周期由库内部托管，
    /// 不涉及任何不安全约定。
    pub unsafe fn callbacks(
        mut self,
        callbacks: WslcProcessCallbacks,
        context: *mut c_void,
    ) -> Self {
        self.callback_mode = ProcessCallbackMode::Registered {
            callbacks,
            context: context as usize,
        };
        self
    }

    /// 启用基于 Tokio 有界通道的异步流式 I/O 捕获 (默认通道缓冲区容量 64)
    pub fn with_streaming_io(self) -> (Self, ProcessStreams) {
        self.with_streaming_io_capacity(64)
    }

    /// 启用基于指定有界通道容量的异步流式 I/O 捕获
    pub fn with_streaming_io_capacity(mut self, capacity: usize) -> (Self, ProcessStreams) {
        let setup = setup_streaming_channels(capacity);
        self.callback_mode = ProcessCallbackMode::Registered {
            callbacks: setup.callbacks,
            context: setup.context,
        };
        self.stream_state = Some(setup.state);
        (self, setup.streams)
    }

    /// 获取关联的异步流式状态句柄克隆
    pub(crate) fn stream_state(&self) -> Option<Arc<StreamState>> {
        self.stream_state.clone()
    }

    fn apply_cmd_line(
        &self,
        settings: &mut WslcProcessSettings,
    ) -> Result<(Vec<CString>, Vec<*const i8>), WslcError> {
        if self.cmd_line.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        let mut c_args = Vec::with_capacity(self.cmd_line.len());
        for arg in &self.cmd_line {
            let c_str = CString::new(arg.as_str())
                .map_err(|e| WslcError::NulError(format!("命令参数含非法空字符: {e}")))?;
            c_args.push(c_str);
        }
        let ptrs: Vec<*const i8> = c_args.iter().map(|c| c.as_ptr()).collect();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcSetProcessSettingsCmdLine(settings, ptrs.as_ptr(), ptrs.len()) };
        WslcError::check_hr(hr, "设置进程命令行参数失败")?;
        Ok((c_args, ptrs))
    }

    fn apply_env_variables(
        &self,
        settings: &mut WslcProcessSettings,
    ) -> Result<(Vec<CString>, Vec<*const i8>), WslcError> {
        if self.env_variables.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        let mut c_envs = Vec::with_capacity(self.env_variables.len());
        for env in &self.env_variables {
            let c_str = CString::new(env.as_str())
                .map_err(|e| WslcError::NulError(format!("环境变量含非法空字符: {e}")))?;
            c_envs.push(c_str);
        }
        let ptrs: Vec<*const i8> = c_envs.iter().map(|c| c.as_ptr()).collect();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcSetProcessSettingsEnvVariables(settings, ptrs.as_ptr(), ptrs.len()) };
        WslcError::check_hr(hr, "设置进程环境变量失败")?;
        Ok((c_envs, ptrs))
    }

    /// 转换为保全生命周期的底层 C 进程设置
    pub(crate) fn build_raw_settings(self) -> Result<RetainedProcessSettings, WslcError> {
        let mut settings = WslcProcessSettings::default();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcInitProcessSettings(&mut settings) };
        WslcError::check_hr(hr, "初始化进程配置失败")?;

        let working_dir = if let Some(ref dir) = self.working_directory {
            let c_dir = CString::new(dir.as_str())
                .map_err(|e| WslcError::NulError(format!("工作目录含非法空字符: {e}")))?;
            let hr =
                // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
                // 出参为合法的可写指针，不涉及未定义行为。
                unsafe { WslcSetProcessSettingsWorkingDirectory(&mut settings, c_dir.as_ptr()) };
            WslcError::check_hr(hr, "设置进程工作目录失败")?;
            Some(c_dir)
        } else {
            None
        };

        let (cmd_args, cmd_ptrs) = self.apply_cmd_line(&mut settings)?;
        let (env_vars, env_ptrs) = self.apply_env_variables(&mut settings)?;

        if self.flags != 0 {
            // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
            // 出参为合法的可写指针，不涉及未定义行为。
            let hr = unsafe { WslcSetProcessSettingsFlags(&mut settings, self.flags) };
            WslcError::check_hr(hr, "设置进程标志位失败")?;
        }

        if let ProcessCallbackMode::Registered { callbacks, context } = self.callback_mode {
            // SAFETY: err_msg 为官方 _Outptr_opt_result_z_ 输出参数，
            // 其所有权在本行交由 RAII 包装接管并自动释放。
            let hr = unsafe {
                WslcSetProcessSettingsCallbacks(&mut settings, &callbacks, context as *mut c_void)
            };
            WslcError::check_hr(hr, "设置进程回调函数失败")?;
        }

        Ok(RetainedProcessSettings {
            settings,
            _working_dir: working_dir,
            _cmd_args: cmd_args,
            _cmd_ptrs: cmd_ptrs,
            _env_vars: env_vars,
            _env_ptrs: env_ptrs,
        })
    }

    /// 在运行中的容器中派生新进程
    pub fn spawn(self, container: &WslcContainerHandle) -> Result<WslcProcessHandle, WslcError> {
        let stream_state = self.stream_state.clone();
        let cmd_repr = format!("{:?}", self.cmd_line);
        // 句柄创建路径**必须**建立线程级 COM 套间：官方 SDK 在
        // WslcCreateSession / WslcOpenContainer / WslcCreateContainerProcess
        // 内部会回调至本进程，未初始化套间时返回 CO_E_NOTINITIALIZED。
        // 此处不可依赖句柄自带的进程级租约——那只保证已建立句柄的 RPC 绑定
        // 有效，不提供调用入口所需的套间上下文。
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut retained = self.build_raw_settings()?;
        let mut raw_process = WslcProcess::NULL;
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe {
            WslcCreateContainerProcess(
                container.as_raw(),
                &mut retained.settings,
                &mut raw_process,
                &mut err_msg,
            )
        };

        // SAFETY: err_msg 为官方 _Outptr_opt_result_z_ 输出参数，
        // 其所有权在本行交由 RAII 包装接管并自动释放。
        unsafe {
            WslcError::check(hr, err_msg)?;
        }

        if raw_process.is_null() {
            return Err(WslcError::InvalidHandle);
        }

        log::info!("容器内新进程派生成功，命令行: {}", cmd_repr);

        // 取租约失败时该句柄会被回收并返回错误，不会返回一个析构即崩溃的对象
        WslcProcessHandle::try_from_raw_with_stream(raw_process, container.clone(), stream_state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::AsyncRecvError;

    #[test]
    fn test_streaming_io_establishes_two_bounded_channels() {
        let builder = ProcessBuilder::new().command(&["/bin/sh", "-c", "echo hello"]);
        let mut streams = builder.with_streaming_io().1;

        // 两路字节流已建立且暂无可读数据，丢弃计数初始为零
        assert!(matches!(
            streams.stdout.try_recv(),
            Err(AsyncRecvError::Empty)
        ));
        assert!(matches!(
            streams.stderr.try_recv(),
            Err(AsyncRecvError::Empty)
        ));
        assert_eq!(streams.stdout_dropped_bytes(), 0);
        assert_eq!(streams.stderr_dropped_bytes(), 0);
    }

    #[test]
    fn test_streaming_io_early_stream_drop_does_not_break_builder() {
        let builder = ProcessBuilder::new().command(&["/bin/sh", "-c", "echo hello"]);
        let (builder, streams) = builder.with_streaming_io();
        // 提前 drop 接收端不应破坏 builder 内部状态
        drop(streams);
        assert!(builder.build_raw_settings().is_ok());
    }

    #[test]
    fn test_builder_chaining_completes_without_error() {
        let builder = ProcessBuilder::new()
            .command(&["/bin/env"])
            .working_directory("/tmp")
            .env("A", "1")
            .expect("环境变量名合法")
            .env("B", "2")
            .expect("环境变量名合法")
            .enable_stdin(true)
            .enable_stdin(false);
        // 链式配置后仍能生成合法的底层进程设置
        assert!(builder.build_raw_settings().is_ok());
    }

    /// 环境变量名含等号必须被拒绝
    ///
    /// 环境变量以 `key=value` 单串交付官方，键名含 `=` 会让官方按首个等号
    /// 切分，把本属键名的部分误并入值。键名若来自外部输入，注入者可借此
    /// 覆盖容器内的 PATH、LD_PRELOAD 等，构成环境变量注入面。
    #[test]
    fn test_env_key_with_equal_sign_is_rejected() {
        let builder = ProcessBuilder::new().command(&["/bin/env"]);
        match builder.env("PATH=/evil", "/bin") {
            Err(WslcError::InvalidConfiguration(_)) => {}
            Ok(_) => panic!("预期返回 InvalidConfiguration，实际却接受了该键名"),
            Err(other) => panic!("预期返回 InvalidConfiguration，实际为: {other}"),
        }
    }

    /// 空环境变量名同样必须被拒绝，否则会生成 `=value` 这类无键名条目
    #[test]
    fn test_empty_env_key_is_rejected() {
        let builder = ProcessBuilder::new().command(&["/bin/env"]);
        assert!(builder.env("", "1").is_err());
    }

    /// 值中含等号是合法的（如某些配置串），不得误伤
    #[test]
    fn test_equal_sign_in_env_value_is_accepted() {
        let builder = ProcessBuilder::new()
            .command(&["/bin/env"])
            .env("CONFIG", "a=b=c")
            .expect("值含等号应被接受");
        assert!(builder.build_raw_settings().is_ok());
    }

    #[test]
    fn test_nul_byte_in_arguments_is_rejected() {
        let builder = ProcessBuilder::new().command(&["/bin/sh", "-c", "bad\0arg"]);
        match builder.build_raw_settings() {
            Err(WslcError::NulError(_)) => {}
            Ok(_) => panic!("预期返回 NulError，实际却构建成功"),
            Err(other) => panic!("预期返回 NulError，实际为: {other}"),
        }
    }
}
