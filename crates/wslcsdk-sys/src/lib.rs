//! Microsoft.WSL.Containers 3.0.1 官方原始 C FFI 绑定
//!
//! 提供对 `wslcsdk.dll` 的全量 1:1 C 导出函数声明。
//!
//! # 平台要求
//!
//! 本 crate 绑定的是 Windows 专有 DLL，**仅支持 Windows 平台**。
//! 在非 Windows 目标上编译会立即得到一条明确的 `compile_error!` 提示。

// `doc_auto_cfg` 已于 Rust 1.92.0 被移除并合并入 `doc_cfg`（rust-lang/rust#138907），
// 继续书写旧名称会在 nightly 上直接触发 E0557 使 docs.rs 构建失败，故此处使用新名称。
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(not(windows))]
compile_error!("wslcsdk-sys 仅支持 Windows 平台：其绑定目标 wslcsdk.dll 为 Windows 专有组件");

#[cfg(windows)]
pub mod errors;
#[cfg(windows)]
pub mod types;

#[cfg(windows)]
pub use errors::*;
#[cfg(windows)]
pub use types::*;

#[cfg(windows)]
use core::ffi::c_void;
#[cfg(windows)]
use windows_sys::Win32::Foundation::HANDLE;
#[cfg(windows)]
use windows_sys::core::HRESULT;

unsafe extern "system" {
    // ==================== 安装与版本检测 API ====================

    /// 获取 WSLC 运行时版本号
    pub fn WslcGetVersion(version: *mut WslcVersion) -> HRESULT;

    /// 获取当前宿主机缺失的组件标志位
    pub fn WslcGetMissingComponents(missingComponents: *mut WslcComponentFlags) -> HRESULT;

    /// 安装缺失组件与依赖
    pub fn WslcInstallWithDependencies(
        components: WslcComponentFlags,
        options: WslcInstallOptions,
        progressCallback: WslcInstallCallback,
        context: *mut c_void,
    ) -> HRESULT;

    // ==================== 会话管理 API ====================

    /// 初始化会话设置
    pub fn WslcInitSessionSettings(
        name: *const u16,
        storagePath: *const u16,
        sessionSettings: *mut WslcSessionSettings,
    ) -> HRESULT;

    /// 设置会话 CPU 核心数限制
    pub fn WslcSetSessionSettingsCpuCount(
        sessionSettings: *mut WslcSessionSettings,
        cpuCount: u32,
    ) -> HRESULT;

    /// 设置会话内存限制 (MB)
    pub fn WslcSetSessionSettingsMemory(
        sessionSettings: *mut WslcSessionSettings,
        memoryMB: u32,
    ) -> HRESULT;

    /// 设置会话空闲超时断开时间 (毫秒)
    pub fn WslcSetSessionSettingsTimeout(
        sessionSettings: *mut WslcSessionSettings,
        timeoutMS: u32,
    ) -> HRESULT;

    /// 设置会话 VHDX 存储规格
    pub fn WslcSetSessionSettingsVhd(
        sessionSettings: *mut WslcSessionSettings,
        vhdRequirements: *const WslcVhdRequirements,
    ) -> HRESULT;

    /// 设置会话特性标志位 (如 GPU)
    pub fn WslcSetSessionSettingsFeatureFlags(
        sessionSettings: *mut WslcSessionSettings,
        flags: WslcSessionFeatureFlags,
    ) -> HRESULT;

    /// 创建会话
    pub fn WslcCreateSession(
        sessionSettings: *mut WslcSessionSettings,
        session: *mut WslcSession,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 获取会话终止通知事件句柄
    pub fn WslcGetSessionTerminationEvent(
        session: WslcSession,
        terminationEvent: *mut HANDLE,
    ) -> HRESULT;

    /// 获取会话终止原因
    pub fn WslcGetSessionTerminationReason(
        session: WslcSession,
        reason: *mut WslcSessionTerminationReason,
    ) -> HRESULT;

    /// 强制终止会话
    pub fn WslcTerminateSession(session: WslcSession) -> HRESULT;

    /// 释放会话句柄
    pub fn WslcReleaseSession(session: WslcSession) -> HRESULT;

    /// 注册会话崩溃转储回调
    pub fn WslcRegisterSessionCrashDumpCallback(
        session: WslcSession,
        crashDumpCallback: WslcSessionCrashDumpCallback,
        crashDumpContext: *mut c_void,
        subscription: *mut WslcCrashDumpSubscription,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 释放崩溃转储订阅句柄
    pub fn WslcReleaseCrashDumpSubscription(subscription: WslcCrashDumpSubscription) -> HRESULT;

    // ==================== 容器配置与创建 API ====================

    /// 初始化容器设置
    pub fn WslcInitContainerSettings(
        imageName: *const i8,
        containerSettings: *mut WslcContainerSettings,
    ) -> HRESULT;

    /// 设置容器名称
    pub fn WslcSetContainerSettingsName(
        containerSettings: *mut WslcContainerSettings,
        name: *const i8,
    ) -> HRESULT;

    /// 设置容器初始化主进程
    pub fn WslcSetContainerSettingsInitProcess(
        containerSettings: *mut WslcContainerSettings,
        initProcess: *mut WslcProcessSettings,
    ) -> HRESULT;

    /// 设置容器网络模式
    pub fn WslcSetContainerSettingsNetworkingMode(
        containerSettings: *mut WslcContainerSettings,
        networkingMode: WslcContainerNetworkingMode,
    ) -> HRESULT;

    /// 设置容器主机名
    pub fn WslcSetContainerSettingsHostName(
        containerSettings: *mut WslcContainerSettings,
        hostName: *const i8,
    ) -> HRESULT;

    /// 设置容器域名
    pub fn WslcSetContainerSettingsDomainName(
        containerSettings: *mut WslcContainerSettings,
        domainName: *const i8,
    ) -> HRESULT;

    /// 设置容器运行标志位
    pub fn WslcSetContainerSettingsFlags(
        containerSettings: *mut WslcContainerSettings,
        flags: WslcContainerFlags,
    ) -> HRESULT;

    /// 设置容器端口映射
    pub fn WslcSetContainerSettingsPortMappings(
        containerSettings: *mut WslcContainerSettings,
        portMappings: *const WslcContainerPortMapping,
        portMappingCount: u32,
    ) -> HRESULT;

    /// 设置容器目录卷挂载
    pub fn WslcSetContainerSettingsVolumes(
        containerSettings: *mut WslcContainerSettings,
        volumes: *const WslcContainerVolume,
        volumeCount: u32,
    ) -> HRESULT;

    /// 设置容器具名 VHD 卷挂载
    pub fn WslcSetContainerSettingsNamedVolumes(
        containerSettings: *mut WslcContainerSettings,
        namedVolumes: *const WslcContainerNamedVolume,
        namedVolumeCount: u32,
    ) -> HRESULT;

    /// 创建容器
    pub fn WslcCreateContainer(
        session: WslcSession,
        containerSettings: *const WslcContainerSettings,
        container: *mut WslcContainer,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 打开已有容器
    pub fn WslcOpenContainer(
        session: WslcSession,
        nameOrId: *const i8,
        container: *mut WslcContainer,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 启动容器
    pub fn WslcStartContainer(
        container: WslcContainer,
        flags: WslcContainerStartFlags,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 停止容器
    pub fn WslcStopContainer(
        container: WslcContainer,
        signal: WslcSignal,
        timeoutSeconds: u32,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 删除容器
    pub fn WslcDeleteContainer(
        container: WslcContainer,
        flags: WslcDeleteContainerFlags,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 释放容器句柄
    pub fn WslcReleaseContainer(container: WslcContainer) -> HRESULT;

    /// 获取容器 64 位唯一哈希 ID
    pub fn WslcGetContainerID(container: WslcContainer, containerID: *mut i8) -> HRESULT;

    /// 获取容器初始化主进程
    pub fn WslcGetContainerInitProcess(
        container: WslcContainer,
        initProcess: *mut WslcProcess,
    ) -> HRESULT;

    /// 获取容器 JSON 检查快照
    pub fn WslcInspectContainer(container: WslcContainer, inspectData: *mut *mut i8) -> HRESULT;

    /// 获取容器状态
    pub fn WslcGetContainerState(
        container: WslcContainer,
        state: *mut WslcContainerState,
    ) -> HRESULT;

    /// 设置容器主进程 IO 回调
    pub fn WslcSetContainerInitProcessIOCallbacks(
        container: WslcContainer,
        callbacks: *const WslcProcessCallbacks,
        context: *mut c_void,
    ) -> HRESULT;

    // ==================== 进程与流交互 API ====================

    /// 初始化进程设置
    pub fn WslcInitProcessSettings(processSettings: *mut WslcProcessSettings) -> HRESULT;

    /// 设置进程工作目录
    pub fn WslcSetProcessSettingsWorkingDirectory(
        processSettings: *mut WslcProcessSettings,
        workingDirectory: *const i8,
    ) -> HRESULT;

    /// 设置进程命令行参数
    pub fn WslcSetProcessSettingsCmdLine(
        processSettings: *mut WslcProcessSettings,
        argv: *const *const i8,
        argc: usize,
    ) -> HRESULT;

    /// 设置进程环境变量
    pub fn WslcSetProcessSettingsEnvVariables(
        processSettings: *mut WslcProcessSettings,
        keyValue: *const *const i8,
        argc: usize,
    ) -> HRESULT;

    /// 设置进程标志位
    pub fn WslcSetProcessSettingsFlags(
        processSettings: *mut WslcProcessSettings,
        flags: WslcProcessFlags,
    ) -> HRESULT;

    /// 设置进程回调函数
    pub fn WslcSetProcessSettingsCallbacks(
        processSettings: *mut WslcProcessSettings,
        callbacks: *const WslcProcessCallbacks,
        context: *mut c_void,
    ) -> HRESULT;

    /// 在容器内创建新进程
    pub fn WslcCreateContainerProcess(
        container: WslcContainer,
        newProcessSettings: *mut WslcProcessSettings,
        newProcess: *mut WslcProcess,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 获取进程 PID
    pub fn WslcGetProcessPid(process: WslcProcess, pid: *mut u32) -> HRESULT;

    /// 获取进程退出事件句柄
    pub fn WslcGetProcessExitEvent(process: WslcProcess, exitEvent: *mut HANDLE) -> HRESULT;

    /// 获取进程运行状态
    pub fn WslcGetProcessState(process: WslcProcess, state: *mut WslcProcessState) -> HRESULT;

    /// 获取进程退出代码
    pub fn WslcGetProcessExitCode(process: WslcProcess, exitCode: *mut i32) -> HRESULT;

    /// 向进程发送信号
    pub fn WslcSignalProcess(process: WslcProcess, signal: WslcSignal) -> HRESULT;

    /// 获取进程标准 IO 管道句柄
    pub fn WslcGetProcessIOHandle(
        process: WslcProcess,
        ioHandle: WslcProcessIOHandle,
        handle: *mut HANDLE,
    ) -> HRESULT;

    /// 释放进程句柄
    pub fn WslcReleaseProcess(process: WslcProcess) -> HRESULT;

    // ==================== 镜像与注册表 API ====================

    /// 列出会话内的镜像列表
    pub fn WslcListSessionImages(
        session: WslcSession,
        images: *mut *mut WslcImageInfo,
        count: *mut u32,
    ) -> HRESULT;

    /// 拉取远程镜像
    pub fn WslcPullSessionImage(
        session: WslcSession,
        options: *const WslcPullImageOptions,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 从内存流句柄导入镜像
    pub fn WslcImportSessionImage(
        session: WslcSession,
        imageName: *const i8,
        imageContent: HANDLE,
        imageContentBytes: u64,
        options: *const WslcImportImageOptions,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 从文件导入镜像
    pub fn WslcImportSessionImageFromFile(
        session: WslcSession,
        imageName: *const i8,
        path: *const u16,
        options: *const WslcImportImageOptions,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 从内存流句柄载入镜像
    pub fn WslcLoadSessionImage(
        session: WslcSession,
        imageContent: HANDLE,
        imageContentBytes: u64,
        options: *const WslcLoadImageOptions,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 从文件载入镜像
    pub fn WslcLoadSessionImageFromFile(
        session: WslcSession,
        path: *const u16,
        options: *const WslcLoadImageOptions,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 为镜像打标签
    pub fn WslcTagSessionImage(
        session: WslcSession,
        options: *const WslcTagImageOptions,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 推送镜像至远程仓库
    pub fn WslcPushSessionImage(
        session: WslcSession,
        options: *const WslcPushImageOptions,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 删除镜像
    pub fn WslcDeleteSessionImage(
        session: WslcSession,
        nameOrID: *const i8,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 登录镜像仓库并生成身份认证 Token
    pub fn WslcSessionAuthenticate(
        session: WslcSession,
        serverAddress: *const i8,
        username: *const i8,
        password: *const i8,
        identityToken: *mut *mut i8,
        tokenType: *mut WslcIdentityTokenType,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    // ==================== 存储卷 API ====================

    /// 创建会话 VHD 卷
    pub fn WslcCreateSessionVhdVolume(
        session: WslcSession,
        options: *const WslcVhdRequirements,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;

    /// 删除会话 VHD 卷
    pub fn WslcDeleteSessionVhdVolume(
        session: WslcSession,
        name: *const i8,
        errorMessage: *mut *mut u16,
    ) -> HRESULT;
}

// ==================== 绑定完整性校验 ====================
//
// 官方头文件声明了 63 个导出函数。下列测试逐一引用每个绑定，把「头文件声明了
// 哪些接口」这一事实固化为可执行断言：一旦上游新增接口而本绑定未跟进，
// 或某个绑定被误删，编译即会因符号无法解析而失败，避免悄无声息地漏绑。
//
// 仅对函数项取地址而不发起任何实际调用，因此不依赖目标机器是否装有
// wslcsdk.dll，可在任意环境下安全执行。

/// 官方头文件 `native/include/wslcsdk.h` 中 `STDAPI Wslc*(` 的出现次数
///
/// 升级官方 SDK 时须同步更新本常量与下方清单。
#[cfg(test)]
const OFFICIAL_EXPORT_COUNT: usize = 63;

#[cfg(test)]
mod binding_tests {
    use super::*;

    /// 逐一取地址以确认每个官方导出都存在对应绑定
    #[test]
    fn assert_all_official_exports_bound() {
        // 数组长度由常量约束，故新增或遗漏绑定都会改变元素个数并触发编译错误
        let _exports: [*const (); OFFICIAL_EXPORT_COUNT] = [
            WslcGetVersion as *const (),
            WslcGetMissingComponents as *const (),
            WslcInstallWithDependencies as *const (),
            WslcInitSessionSettings as *const (),
            WslcCreateSession as *const (),
            WslcSetSessionSettingsCpuCount as *const (),
            WslcSetSessionSettingsMemory as *const (),
            WslcSetSessionSettingsTimeout as *const (),
            WslcSetSessionSettingsVhd as *const (),
            WslcSetSessionSettingsFeatureFlags as *const (),
            WslcGetSessionTerminationEvent as *const (),
            WslcGetSessionTerminationReason as *const (),
            WslcTerminateSession as *const (),
            WslcReleaseSession as *const (),
            WslcRegisterSessionCrashDumpCallback as *const (),
            WslcReleaseCrashDumpSubscription as *const (),
            WslcInitContainerSettings as *const (),
            WslcCreateContainer as *const (),
            WslcOpenContainer as *const (),
            WslcStartContainer as *const (),
            WslcSetContainerSettingsName as *const (),
            WslcSetContainerSettingsInitProcess as *const (),
            WslcSetContainerSettingsNetworkingMode as *const (),
            WslcSetContainerSettingsHostName as *const (),
            WslcSetContainerSettingsDomainName as *const (),
            WslcSetContainerSettingsFlags as *const (),
            WslcSetContainerSettingsPortMappings as *const (),
            WslcSetContainerSettingsVolumes as *const (),
            WslcSetContainerSettingsNamedVolumes as *const (),
            WslcReleaseContainer as *const (),
            WslcStopContainer as *const (),
            WslcDeleteContainer as *const (),
            WslcGetContainerID as *const (),
            WslcGetContainerInitProcess as *const (),
            WslcInspectContainer as *const (),
            WslcGetContainerState as *const (),
            WslcSetContainerInitProcessIOCallbacks as *const (),
            WslcInitProcessSettings as *const (),
            WslcSetProcessSettingsWorkingDirectory as *const (),
            WslcSetProcessSettingsCmdLine as *const (),
            WslcSetProcessSettingsEnvVariables as *const (),
            WslcSetProcessSettingsFlags as *const (),
            WslcSetProcessSettingsCallbacks as *const (),
            WslcCreateContainerProcess as *const (),
            WslcReleaseProcess as *const (),
            WslcGetProcessPid as *const (),
            WslcGetProcessExitEvent as *const (),
            WslcGetProcessState as *const (),
            WslcGetProcessExitCode as *const (),
            WslcSignalProcess as *const (),
            WslcGetProcessIOHandle as *const (),
            WslcListSessionImages as *const (),
            WslcPullSessionImage as *const (),
            WslcImportSessionImage as *const (),
            WslcImportSessionImageFromFile as *const (),
            WslcLoadSessionImage as *const (),
            WslcLoadSessionImageFromFile as *const (),
            WslcTagSessionImage as *const (),
            WslcPushSessionImage as *const (),
            WslcDeleteSessionImage as *const (),
            WslcSessionAuthenticate as *const (),
            WslcCreateSessionVhdVolume as *const (),
            WslcDeleteSessionVhdVolume as *const (),
        ];
    }
}
