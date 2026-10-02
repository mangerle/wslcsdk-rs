# Microsoft.WSL.Containers 3.0.1 官方 FFI API 注册表与契约规范

本文档是 `wslc-sys` 针对微软官方发布的 **`Microsoft.WSL.Containers` (版本 3.0.1 GA)** 的全量 C API 原始契约归档记录。用于版本比对、变更追踪以及防止任何 API 遗漏。

---

## 一、 来源与环境元数据

- **官方包名**：`Microsoft.WSL.Containers`
- **版本号**：`3.0.1` (GA 发布日期：2026-09-29)
- **最低环境要求**：Windows 11 / Windows 10 (WSL 2.9.3+ / 3.0.1+)
- **头文件路径**：`native/include/wslcsdk.h` (701 行)
- **动态链接库**：`wslcsdk.dll` (x64 / arm64)
- **导入符号库**：`wslcsdk.lib` (x64 / arm64)
- **调用约定**：`STDAPI` (`__stdcall` / `extern "system"`)
- **错误返回机制**：返回 `HRESULT`（`0x00000000` 表示 `S_OK`）；部分失败路径通过 `_Outptr_opt_result_z_ PWSTR* errorMessage` 返回详细宽字符描述。

---

## 二、 全量 C 函数签名清单 (共 46 个导出函数)

### 1. 安装与版本检测 (Installation API) - 3 个

1. **`WslcGetVersion`**
   - **签名**：`STDAPI WslcGetVersion(_Out_writes_(1) WslcVersion* version);`
   - **功能**：获取 WSLC 运行时主要、次要与修订版本号。
   - **内存责任**：调用方分配 `WslcVersion` 栈内存。
2. **`WslcGetMissingComponents`**
   - **签名**：`STDAPI WslcGetMissingComponents(_Out_ WslcComponentFlags* missingComponents);`
   - **功能**：检测宿主机缺失的组件（虚拟机平台、WSL 软件包、SDK 本身更新需求）。
3. **`WslcInstallWithDependencies`**
   - **签名**：`STDAPI WslcInstallWithDependencies(_In_ WslcComponentFlags components, _In_ WslcInstallOptions options, _In_opt_ WslcInstallCallback progressCallback, _In_opt_ PVOID context);`
   - **功能**：按需安装缺失的依赖组件并提供进度回调。

---

### 2. 会话生命周期与设置 (Session API) - 13 个

4. **`WslcInitSessionSettings`**
   - **签名**：`STDAPI WslcInitSessionSettings(_In_ PCWSTR name, _In_ PCWSTR storagePath, _Out_ WslcSessionSettings* sessionSettings);`
   - **功能**：以会话名称和存储路径初始化配置结构体（72 字节不透明缓冲区）。
5. **`WslcSetSessionSettingsCpuCount`**
   - **签名**：`STDAPI WslcSetSessionSettingsCpuCount(_In_ WslcSessionSettings* sessionSettings, _In_ uint32_t cpuCount);`
   - **功能**：限制分配给该会话虚拟机的 CPU 核心数。
6. **`WslcSetSessionSettingsMemory`**
   - **签名**：`STDAPI WslcSetSessionSettingsMemory(_In_ WslcSessionSettings* sessionSettings, _In_ uint32_t memoryMB);`
   - **功能**：限制分配给该会话虚拟机的内存上限（MB）。
7. **`WslcSetSessionSettingsTimeout`**
   - **签名**：`STDAPI WslcSetSessionSettingsTimeout(_In_ WslcSessionSettings* sessionSettings, _In_ uint32_t timeoutMS);`
   - **功能**：设置空闲自动超时关闭时间（毫秒）。
8. **`WslcSetSessionSettingsVhd`**
   - **签名**：`STDAPI WslcSetSessionSettingsVhd(_In_ WslcSessionSettings* sessionSettings, _In_opt_ const WslcVhdRequirements* vhdRequirements);`
   - **功能**：配置会话专用的根 VHDX 参数。
9. **`WslcSetSessionSettingsFeatureFlags`**
   - **签名**：`STDAPI WslcSetSessionSettingsFeatureFlags(_In_ WslcSessionSettings* sessionSettings, _In_ WslcSessionFeatureFlags flags);`
   - **功能**：配置会话特性（如开启 GPU 加速）。
10. **`WslcCreateSession`**
    - **签名**：`STDAPI WslcCreateSession(_In_ WslcSessionSettings* sessionSettings, _Out_ WslcSession* session, _Outptr_opt_result_z_ PWSTR* errorMessage);`
    - **功能**：创建并激活会话，返回 `WslcSession` 不透明句柄。
    - **内存责任**：失败时若 `errorMessage` 非空，需使用 `CoTaskMemFree` 释放。
11. **`WslcGetSessionTerminationEvent`**
    - **签名**：`STDAPI WslcGetSessionTerminationEvent(_In_ WslcSession session, _Out_ HANDLE* terminationEvent);`
    - **功能**：获取会话终止 Win32 Event 句柄，用于跨线程或异步通知。
12. **`WslcGetSessionTerminationReason`**
    - **签名**：`STDAPI WslcGetSessionTerminationReason(_In_ WslcSession session, _Out_ WslcSessionTerminationReason* reason);`
    - **功能**：获取会话退出的具体原因（正常关机、崩溃等）。
13. **`WslcTerminateSession`**
    - **签名**：`STDAPI WslcTerminateSession(_In_ WslcSession session);`
    - **功能**：强制中断并关闭指定会话。
14. **`WslcReleaseSession`**
    - **签名**：`STDAPI WslcReleaseSession(_In_ WslcSession session);`
    - **功能**：释放会话对象句柄（RAII 必须绑定此接口）。
15. **`WslcRegisterSessionCrashDumpCallback`**
    - **签名**：`STDAPI WslcRegisterSessionCrashDumpCallback(_In_ WslcSession session, _In_ WslcSessionCrashDumpCallback crashDumpCallback, _In_opt_ PVOID crashDumpContext, _Out_ WslcCrashDumpSubscription* subscription, _Outptr_opt_result_z_ PWSTR* errorMessage);`
    - **功能**：注册 Linux 进程崩溃转储监控回调。
16. **`WslcReleaseCrashDumpSubscription`**
    - **签名**：`STDAPI WslcReleaseCrashDumpSubscription(_In_ WslcCrashDumpSubscription subscription);`
    - **功能**：注销并释放崩溃转储订阅句柄。

---

### 3. 容器生命周期管理 (Container Lifecycle API) - 10 个

17. **`WslcInitContainerSettings`**
    - **签名**：`STDAPI WslcInitContainerSettings(_In_ PCSTR imageName, _Out_ WslcContainerSettings* containerSettings);`
    - **功能**：基于基础镜像名称初始化容器设置结构体（104 字节不透明缓冲区）。
18. **`WslcCreateContainer`**
    - **签名**：`STDAPI WslcCreateContainer(_In_ WslcSession session, _In_ const WslcContainerSettings* containerSettings, _Out_ WslcContainer* container, _Outptr_opt_result_z_ PWSTR* errorMessage);`
    - **功能**：在会话内创建容器实例，返回 `WslcContainer` 句柄。
19. **`WslcOpenContainer`**
    - **签名**：`STDAPI WslcOpenContainer(_In_ WslcSession session, _In_z_ PCSTR nameOrId, _Out_ WslcContainer* container, _Outptr_opt_result_z_ PWSTR* errorMessage);`
    - **功能**：通过容器名称、全量 ID 或唯一前缀打开已有容器句柄。
20. **`WslcStartContainer`**
    - **签名**：`STDAPI WslcStartContainer(_In_ WslcContainer container, _In_ WslcContainerStartFlags flags, _Outptr_opt_result_z_ PWSTR* errorMessage);`
    - **功能**：启动指定容器（支持 ATTACH 标志）。
21. **`WslcStopContainer`**
    - **签名**：`STDAPI WslcStopContainer(_In_ WslcContainer container, _In_ WslcSignal signal, _In_ uint32_t timeoutSeconds, _Outptr_opt_result_z_ PWSTR* errorMessage);`
    - **功能**：向容器发送指定信号并在超时秒数内等待停止。
22. **`WslcDeleteContainer`**
    - **签名**：`STDAPI WslcDeleteContainer(_In_ WslcContainer container, _In_ WslcDeleteContainerFlags flags, _Outptr_opt_result_z_ PWSTR* errorMessage);`
    - **功能**：删除容器对象（支持强制 FORCE 标志）。
23. **`WslcReleaseContainer`**
    - **签名**：`STDAPI WslcReleaseContainer(_In_ WslcContainer container);`
    - **功能**：释放容器对象句柄。
24. **`WslcGetContainerID`**
    - **签名**：`STDAPI WslcGetContainerID(_In_ WslcContainer container, _Out_writes_(WSLC_CONTAINER_ID_BUFFER_SIZE) CHAR containerID[WSLC_CONTAINER_ID_BUFFER_SIZE]);`
    - **功能**：获取 64 位十六进制容器唯一哈希 ID。
25. **`WslcGetContainerState`**
    - **签名**：`STDAPI WslcGetContainerState(_In_ WslcContainer container, _Out_ WslcContainerState* state);`
    - **功能**：获取容器当前运行状态枚举（CREATED, RUNNING, EXITED, DELETED）。
26. **`WslcInspectContainer`**
    - **签名**：`STDAPI WslcInspectContainer(_In_ WslcContainer container, _Outptr_result_z_ PSTR* inspectData);`
    - **功能**：获取容器底层 JSON 格式检查快照。
    - **内存责任**：返回的 ANSI 字符串由 `CoTaskMemAlloc` 分配，调用方必须使用 `CoTaskMemFree` 释放。

---

### 4. 容器配置扩展 (Container Settings API) - 9 个

27. **`WslcSetContainerSettingsName`**
    - **签名**：`STDAPI WslcSetContainerSettingsName(_In_ WslcContainerSettings* containerSettings, _In_ PCSTR name);`
28. **`WslcSetContainerSettingsInitProcess`**
    - **签名**：`STDAPI WslcSetContainerSettingsInitProcess(_In_ WslcContainerSettings* containerSettings, _In_ WslcProcessSettings* initProcess);`
29. **`WslcSetContainerSettingsNetworkingMode`**
    - **签名**：`STDAPI WslcSetContainerSettingsNetworkingMode(_In_ WslcContainerSettings* containerSettings, _In_ WslcContainerNetworkingMode networkingMode);`
30. **`WslcSetContainerSettingsHostName`**
    - **签名**：`STDAPI WslcSetContainerSettingsHostName(_In_ WslcContainerSettings* containerSettings, _In_ PCSTR hostName);`
31. **`WslcSetContainerSettingsDomainName`**
    - **签名**：`STDAPI WslcSetContainerSettingsDomainName(_In_ WslcContainerSettings* containerSettings, _In_ PCSTR domainName);`
32. **`WslcSetContainerSettingsFlags`**
    - **签名**：`STDAPI WslcSetContainerSettingsFlags(_In_ WslcContainerSettings* containerSettings, _In_ WslcContainerFlags flags);`
33. **`WslcSetContainerSettingsPortMappings`**
    - **签名**：`STDAPI WslcSetContainerSettingsPortMappings(_In_ WslcContainerSettings* containerSettings, _In_reads_opt_(portMappingCount) const WslcContainerPortMapping* portMappings, _In_ uint32_t portMappingCount);`
34. **`WslcSetContainerSettingsVolumes`**
    - **签名**：`STDAPI WslcSetContainerSettingsVolumes(_In_ WslcContainerSettings* containerSettings, _In_reads_opt_(volumeCount) const WslcContainerVolume* volumes, _In_ uint32_t volumeCount);`
35. **`WslcSetContainerSettingsNamedVolumes`**
    - **签名**：`STDAPI WslcSetContainerSettingsNamedVolumes(_In_ WslcContainerSettings* containerSettings, _In_reads_opt_(namedVolumeCount) const WslcContainerNamedVolume* namedVolumes, _In_ uint32_t namedVolumeCount);`

---

### 5. 进程执行与标准 I/O (Process & Stream API) - 15 个

36. **`WslcInitProcessSettings`**
    - **签名**：`STDAPI WslcInitProcessSettings(_Out_ WslcProcessSettings* processSettings);`
37. **`WslcSetProcessSettingsWorkingDirectory`**
    - **签名**：`STDAPI WslcSetProcessSettingsWorkingDirectory(_In_ WslcProcessSettings* processSettings, _In_ PCSTR workingDirectory);`
38. **`WslcSetProcessSettingsCmdLine`**
    - **签名**：`STDAPI WslcSetProcessSettingsCmdLine(_In_ WslcProcessSettings* processSettings, _In_reads_(argc) PCSTR const* argv, size_t argc);`
39. **`WslcSetProcessSettingsEnvVariables`**
    - **签名**：`STDAPI WslcSetProcessSettingsEnvVariables(_In_ WslcProcessSettings* processSettings, _In_reads_(argc) PCSTR const* key_value, size_t argc);`
40. **`WslcSetProcessSettingsFlags`**
    - **签名**：`STDAPI WslcSetProcessSettingsFlags(_In_ WslcProcessSettings* processSettings, _In_ WslcProcessFlags flags);`
41. **`WslcSetProcessSettingsCallbacks`**
    - **签名**：`STDAPI WslcSetProcessSettingsCallbacks(_In_ WslcProcessSettings* processSettings, _In_ const WslcProcessCallbacks* callbacks, _In_opt_ PVOID context);`
42. **`WslcCreateContainerProcess`**
    - **签名**：`STDAPI WslcCreateContainerProcess(_In_ WslcContainer container, _In_ WslcProcessSettings* newProcessSettings, _Out_ WslcProcess* newProcess, _Outptr_opt_result_z_ PWSTR* errorMessage);`
43. **`WslcGetContainerInitProcess`**
    - **签名**：`STDAPI WslcGetContainerInitProcess(_In_ WslcContainer container, _Out_ WslcProcess* initProcess);`
44. **`WslcSetContainerInitProcessIOCallbacks`**
    - **签名**：`STDAPI WslcSetContainerInitProcessIOCallbacks(_In_ WslcContainer container, _In_ const WslcProcessCallbacks* callbacks, _In_opt_ PVOID context);`
45. **`WslcGetProcessPid`**
    - **签名**：`STDAPI WslcGetProcessPid(_In_ WslcProcess process, _Out_ uint32_t* pid);`
46. **`WslcGetProcessExitEvent`**
    - **签名**：`STDAPI WslcGetProcessExitEvent(_In_ WslcProcess process, _Out_ HANDLE* exitEvent);`
47. **`WslcGetProcessState`**
    - **签名**：`STDAPI WslcGetProcessState(_In_ WslcProcess process, _Out_ WslcProcessState* state);`
48. **`WslcGetProcessExitCode`**
    - **签名**：`STDAPI WslcGetProcessExitCode(_In_ WslcProcess process, _Out_ PINT32 exitCode);`
49. **`WslcSignalProcess`**
    - **签名**：`STDAPI WslcSignalProcess(_In_ WslcProcess process, _In_ WslcSignal signal);`
50. **`WslcGetProcessIOHandle`**
    - **签名**：`STDAPI WslcGetProcessIOHandle(_In_ WslcProcess process, _In_ WslcProcessIOHandle ioHandle, _Out_ HANDLE* handle);`
51. **`WslcReleaseProcess`**
    - **签名**：`STDAPI WslcReleaseProcess(_In_ WslcProcess process);`

---

### 6. 镜像管理与仓库凭据 (Image & Registry API) - 10 个

52. **`WslcListSessionImages`**
    - **签名**：`STDAPI WslcListSessionImages(_In_ WslcSession session, _Outptr_result_buffer_(*count) WslcImageInfo** images, _Out_ uint32_t* count);`
    - **内存责任**：返回的 `images` 数组由 `CoTaskMemFree` 释放。
53. **`WslcPullSessionImage`**
    - **签名**：`STDAPI WslcPullSessionImage(_In_ WslcSession session, _In_ const WslcPullImageOptions* options, _Outptr_opt_result_z_ PWSTR* errorMessage);`
54. **`WslcImportSessionImage`**
    - **签名**：`STDAPI WslcImportSessionImage(_In_ WslcSession session, _In_z_ PCSTR imageName, _In_ HANDLE imageContent, _In_ uint64_t imageContentBytes, _In_opt_ const WslcImportImageOptions* options, _Outptr_opt_result_z_ PWSTR* errorMessage);`
55. **`WslcImportSessionImageFromFile`**
    - **签名**：`STDAPI WslcImportSessionImageFromFile(_In_ WslcSession session, _In_z_ PCSTR imageName, _In_z_ PCWSTR path, _In_opt_ const WslcImportImageOptions* options, _Outptr_opt_result_z_ PWSTR* errorMessage);`
56. **`WslcLoadSessionImage`**
    - **签名**：`STDAPI WslcLoadSessionImage(_In_ WslcSession session, _In_ HANDLE imageContent, _In_ uint64_t imageContentBytes, _In_opt_ const WslcLoadImageOptions* options, _Outptr_opt_result_z_ PWSTR* errorMessage);`
57. **`WslcLoadSessionImageFromFile`**
    - **签名**：`STDAPI WslcLoadSessionImageFromFile(_In_ WslcSession session, _In_z_ PCWSTR path, _In_opt_ const WslcLoadImageOptions* options, _Outptr_opt_result_z_ PWSTR* errorMessage);`
58. **`WslcTagSessionImage`**
    - **签名**：`STDAPI WslcTagSessionImage(_In_ WslcSession session, _In_ const WslcTagImageOptions* options, _Outptr_opt_result_z_ PWSTR* errorMessage);`
59. **`WslcPushSessionImage`**
    - **签名**：`STDAPI WslcPushSessionImage(_In_ WslcSession session, _In_ const WslcPushImageOptions* options, _Outptr_opt_result_z_ PWSTR* errorMessage);`
60. **`WslcDeleteSessionImage`**
    - **签名**：`STDAPI WslcDeleteSessionImage(_In_ WslcSession session, _In_z_ PCSTR nameOrID, _Outptr_opt_result_z_ PWSTR* errorMessage);`
61. **`WslcSessionAuthenticate`**
    - **签名**：`STDAPI WslcSessionAuthenticate(_In_ WslcSession session, _In_z_ PCSTR serverAddress, _In_z_ PCSTR username, _In_z_ PCSTR password, _Outptr_result_z_ PSTR* identityToken, _Out_opt_ WslcIdentityTokenType* tokenType, _Outptr_opt_result_z_ PWSTR* errorMessage);`
    - **内存责任**：返回的 `identityToken` 必须使用 `CoTaskMemFree` 释放。

---

### 7. 存储卷管理 (Storage Volume API) - 2 个

62. **`WslcCreateSessionVhdVolume`**
    - **签名**：`STDAPI WslcCreateSessionVhdVolume(_In_ WslcSession session, _In_ const WslcVhdRequirements* options, _Outptr_opt_result_z_ PWSTR* errorMessage);`
63. **`WslcDeleteSessionVhdVolume`**
    - **签名**：`STDAPI WslcDeleteSessionVhdVolume(_In_ WslcSession session, _In_z_ PCSTR name, _Outptr_opt_result_z_ PWSTR* errorMessage);`

---

## 三、 官方错误码映射表 (19 个具名 HRESULT)

所有错误码基址：`WSLC_E_BASE = 0x0600`，由 `MAKE_HRESULT(SEVERITY_ERROR, FACILITY_ITF, WSLC_E_BASE + N)` 构成：

| 错误码常量名 | 十六进制值 | 对应中文含义与排查指引 |
| :--- | :--- | :--- |
| `WSLC_E_IMAGE_NOT_FOUND` | `0x80040601` | 指定的容器镜像未在本地会话中找到 |
| `WSLC_E_CONTAINER_PREFIX_AMBIGUOUS` | `0x80040602` | 提供的容器 ID 前缀匹配到多个容器，存在歧义 |
| `WSLC_E_CONTAINER_NOT_FOUND` | `0x80040603` | 指定的容器不存在 |
| `WSLC_E_VOLUME_NOT_FOUND` | `0x80040604` | 指定的持久卷未找到 |
| `WSLC_E_CONTAINER_NOT_RUNNING` | `0x80040605` | 容器未处于运行状态 |
| `WSLC_E_CONTAINER_IS_RUNNING` | `0x80040606` | 容器已在运行中 |
| `WSLC_E_SESSION_RESERVED` | `0x80040607` | 会话名称属于系统保留名称，不可使用 |
| `WSLC_E_INVALID_SESSION_NAME` | `0x80040608` | 会话名称非法（包含非法字符或长度越界） |
| `WSLC_E_NETWORK_NOT_FOUND` | `0x80040609` | 网络未找到 |
| `WSLC_E_WU_SEARCH_FAILED` | `0x8004060A` | Windows Update 组件检索失败 |
| `WSLC_E_SDK_UPDATE_NEEDED` | `0x8004060B` | SDK 版本过低，需要升级更新 |
| `WSLC_E_CONTAINER_DISABLED` | `0x8004060C` | 容器功能被系统或组策略禁用 |
| `WSLC_E_REGISTRY_BLOCKED_BY_POLICY` | `0x8004060D` | 镜像仓库访问被安全策略拦截 |
| `WSLC_E_VOLUME_NOT_AVAILABLE` | `0x8004060E` | 存储卷当前不可用或被独占锁定 |
| `WSLC_E_SESSION_NOT_FOUND` | `0x8004060F` | 指定的会话未找到 |
| `WSLC_E_VM_NOT_RUNNING` | `0x80040610` | 后台 WSL 虚拟机未处于运行状态 |
| `WSLC_E_EVENTS_LOST` | `0x80040611` | 事件流溢出导致部分事件丢失 |
| `WSLC_E_EVENT_STREAM_FINISHED` | `0x80040612` | 事件流已正常终止 |
| `WSLC_E_CONTAINER_DELETED` | `0x80040613` | 容器已被删除 |

---

## 四、 核心句柄与不透明结构体规范

1. **`WslcSession`**：Windows HANDLE 别名，代表独立的隔离 VM 与 VHD 存储环境。
2. **`WslcContainer`**：Windows HANDLE 别名，代表会话内的一个容器实例。
3. **`WslcProcess`**：Windows HANDLE 别名，代表容器内执行的一个 Linux 进程。
4. **`WslcCrashDumpSubscription`**：Windows HANDLE 别名，代表崩溃转储监听订阅。
5. **不透明配置缓冲区**：
   - `WslcSessionSettings`：72 字节，8 字节对齐。
   - `WslcContainerSettings`：104 字节，8 字节对齐。
   - `WslcProcessSettings`：72 字节，8 字节对齐。
