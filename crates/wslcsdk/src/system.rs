use crate::error::{WslcDomainError, WslcError};
use core::ffi::c_void;
use wslcsdk_sys::types::{WslcComponentFlags, WslcInstallOptions, WslcVersion};
use wslcsdk_sys::*;

/// WSLC 系统服务门面
///
/// 全部方法均为关联函数，不持有状态。
#[derive(Debug, Clone, Copy, Default)]
pub struct WslcSystem;

impl WslcSystem {
    /// 探测 `wslcsdk.dll` 是否可加载
    ///
    /// **该结果不代表运行时已就绪**：宿主可能存在该 DLL 却缺少 WSL
    /// 虚拟机平台、WSL 包，或 SDK 版本过旧。因此本方法只作为
    /// `build.rs` `/DELAYLOAD` 链接策略的崩溃防护——DLL 不可用时，
    /// 任何后续调用都会在首次触达延迟加载时触发 SEH(0xC06D007E)，
    /// 必须先行拦下。
    ///
    /// 真正的就绪判定见 [`ensure_sdk_available`](Self::ensure_sdk_available)。
    pub fn is_sdk_available() -> bool {
        let dll_name: Vec<u16> = "wslcsdk.dll"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: 库名为已 NUL 结尾的宽字符串缓冲区，
        // 标志位组合合法，加载失败时返回空句柄并由调用方处理。
        let handle = unsafe {
            windows_sys::Win32::System::LibraryLoader::LoadLibraryExW(
                dll_name.as_ptr(),
                std::ptr::null_mut(),
                windows_sys::Win32::System::LibraryLoader::LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        };
        if !handle.is_null() {
            // SAFETY: 句柄来自同一次成功的 LoadLibraryExW 且已被判非空，
            // 期间未发生其他释放，不存在双重释放。
            unsafe {
                windows_sys::Win32::Foundation::FreeLibrary(handle);
            }
            true
        } else {
            false
        }
    }

    /// 确保 `wslcsdk.dll` 可加载
    ///
    /// 作为所有官方调用的前置守卫：官方函数均以 delayload 方式链接，
    /// DLL 缺失时会在调用瞬间抛出 SEH 而非返回错误码。故凡要触达官方
    /// 接口之处（如 [`get_version`](Self::get_version)）必须先经本方法。
    ///
    /// 该守卫只回答「库能否加载」，不判断运行时组件是否齐备——
    /// 后者由 [`ensure_sdk_available`](Self::ensure_sdk_available) 负责。
    fn ensure_dll_loadable() -> Result<(), WslcError> {
        if Self::is_sdk_available() {
            Ok(())
        } else {
            Err(WslcDomainError::SdkUpdateNeeded(
                "宿主机系统中未检测到可用的 wslcsdk.dll 运行时，请先安装 WSL Containers 支持组件"
                    .to_string(),
            )
            .into())
        }
    }

    /// 确保 WSLC 运行时真正就绪，否则返回友好领域错误
    ///
    /// 判据分两层，缺一不可：
    ///
    /// 1. [`is_sdk_available`](Self::is_sdk_available) —— `wslcsdk.dll`
    ///    可加载。此层是delayload 的崩溃防护：DLL 缺失时调用官方接口
    ///    会抛SEH(0xC06D007E) 而非返回错误码，必须先行拦下。
    /// 2. [`get_missing_components`](Self::get_missing_components) 的
    ///    官方判定 —— DLL 存在不等于运行时可用：宿主可能缺少 WSL
    ///    虚拟机平台或 WSL 包，SDK 版本也可能过旧。仅靠第1 层会把这些
    ///    情况误判为「就绪」，使调用方跳过前置门禁、直至撞上后续 RPC
    ///    调用的晦涩失败码，丧失本函数提供的友好诊断价值。
    ///
    /// 第 2 层刻意复用官方接口而非自行推断版本号：`WSLC_COMPONENT_FLAG_SDK_NEEDS_UPDATE`
    /// 是官方为「SDK 需升级」这一状态专设的标志位，自行比较版本号
    /// 无法表达该语义。
    ///
    /// # Errors
    ///
    /// DLL 不可加载、或官方组件检查判定运行时未就绪时返回
    /// [`WslcDomainError::SdkUpdateNeeded`]。
    pub fn ensure_sdk_available() -> Result<(), WslcError> {
        Self::ensure_dll_loadable()?;

        // 官方接口同样需合法的 COM 上下文：本方法是多个创建路径的前置门禁
        // （如 `SessionBuilder::build`、`WslcClientBuilder::build`），
        // 那些路径会在其后自行初始化套间，此处不能假定调用方已就绪。
        let _com_guard = crate::com::try_initialize_mta()?;

        // DLL 可载后再问官方「还缺什么」。此处刻意不走
        // get_missing_components()，因该方法自身以本方法的 DLL 守卫为
        // 前置条件，直接复用会造成无限递归；故只复用其底层的官方调用。
        let mut missing = 0u32;
        // SAFETY: 入参为已初始化且存活期覆盖本次调用的本地缓冲区，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcGetMissingComponents(&mut missing) };
        WslcError::check_hr(hr, "检测缺失组件失败")?;

        if missing & WSLC_COMPONENT_FLAG_SDK_NEEDS_UPDATE != 0 {
            return Err(WslcDomainError::SdkUpdateNeeded(format!(
                "WSLC 运行时组件未就绪，缺失组件标志位: 0x{missing:08X}；\
                 请先安装 WSL Containers 支持组件或升级 WSL"
            ))
            .into());
        }

        Ok(())
    }

    /// 获取当前系统安装的 WSLC 运行时版本
    ///
    /// # Errors
    ///
    /// `wslcsdk.dll` 不可加载，或 COM 套间初始化失败时返回错误。
    pub fn get_version() -> Result<WslcVersion, WslcError> {
        // 只要求 DLL 可加载：版本查询本身正是判断运行时状态的手段之一，
        // 若在此要求组件齐备，缺失组件的宿主将永远查不到版本号
        Self::ensure_dll_loadable()?;
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut ver = WslcVersion::default();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcGetVersion(&mut ver) };
        WslcError::check_hr(hr, "获取 WSLC 版本失败")?;
        Ok(ver)
    }

    /// 检测当前宿主机缺失的组件依赖
    ///
    /// # Errors
    ///
    /// `wslcsdk.dll` 不可加载，或 COM 套间初始化失败时返回错误。
    pub fn get_missing_components() -> Result<WslcComponentFlags, WslcError> {
        // 同get_version：组件检查自身即用于判定就绪状态，
        // 不可反过来要求运行时已就绪，否则缺失组件的宿主永远查不出结果
        Self::ensure_dll_loadable()?;
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut missing = 0u32;
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcGetMissingComponents(&mut missing) };
        WslcError::check_hr(hr, "检测缺失组件失败")?;
        Ok(missing)
    }

    /// 安装缺失组件与依赖
    ///
    /// # 参数
    /// - `components`: 需安装的组件标志位
    /// - `options`: 安装选项 (如修复重装)
    /// - `on_progress`: 可选进度回调函数: `Fn(组件, 当前步, 总步数)`
    ///
    /// # Errors
    /// 当 COM 上下文初始化失败、系统拒绝提权或安装底层组件报错时返回对应的领域错误。
    pub fn install_with_dependencies<F>(
        components: WslcComponentFlags,
        options: WslcInstallOptions,
        on_progress: Option<F>,
    ) -> Result<(), WslcError>
    where
        F: FnMut(WslcComponentFlags, u32, u32) + Send,
    {
        // 只要求 DLL 可加载：本方法的职责正是补齐缺失组件，
        // 若要求运行时已就绪，则组件缺失的宿主永远无法调用它，
        // 形成「必须先就绪才能修复不就绪」的死锁
        Self::ensure_dll_loadable()?;
        let _com_guard = crate::com::try_initialize_mta()?;
        unsafe extern "system" fn progress_trampoline<F>(
            component: WslcComponentFlags,
            progress_steps: u32,
            total_steps: u32,
            context: *mut c_void,
        ) where
            F: FnMut(WslcComponentFlags, u32, u32),
        {
            if !context.is_null() {
                // SAFETY: 上下文指针由 Box::into_raw 分配并包装于 Mutex 中，
                // 保证多线程并发回调时具备互斥同步保障。
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    let mutex = &*(context as *const std::sync::Mutex<F>);
                    if let Ok(mut callback) = mutex.lock() {
                        callback(component, progress_steps, total_steps);
                    }
                }));
            }
        }

        let (cb, ctx) = match on_progress {
            Some(f) => {
                let boxed = Box::into_raw(Box::new(std::sync::Mutex::new(f)));
                (Some(progress_trampoline::<F> as _), boxed as *mut c_void)
            }
            None => (None, std::ptr::null_mut()),
        };

        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcInstallWithDependencies(components, options, cb, ctx) };
        if !ctx.is_null() {
            // SAFETY: ctx 在本函数开头由 Box::into_raw 分配，调用结束立即安全回收
            let _ = unsafe { Box::from_raw(ctx as *mut std::sync::Mutex<F>) };
        }
        WslcError::check_hr(hr, "安装依赖组件失败")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sdk_probe_never_panics() {
        // 无论宿主机是否安装了 WSLC 运行时，探测接口都必须安全返回而非崩溃，
        // 且失败时必须是明确的领域错误或 HRESULT 错误
        match WslcSystem::get_version() {
            Ok(version) => {
                assert!(version.major > 0 || version.minor > 0 || version.revision > 0);
            }
            Err(WslcError::Domain(WslcDomainError::SdkUpdateNeeded(_))) => {}
            Err(WslcError::Hresult(code, _)) => assert_ne!(code, 0),
            Err(other) => panic!("预期返回 SdkUpdateNeeded 或 Hresult 错误，实际为: {other:?}"),
        }
    }

    #[test]
    fn test_missing_components_probe_never_panics() {
        match WslcSystem::get_missing_components() {
            Ok(flags) => {
                // 返回值为官方组件标志位的组合，合法取值必须落在已知位掩码内
                let known = WSLC_COMPONENT_FLAG_VIRTUAL_MACHINE_PLATFORM
                    | WSLC_COMPONENT_FLAG_WSL_PACKAGE
                    | WSLC_COMPONENT_FLAG_SDK_NEEDS_UPDATE;
                assert_eq!(flags & !known, 0, "返回了未定义的组件标志位: {flags}");
            }
            Err(WslcError::Domain(WslcDomainError::SdkUpdateNeeded(_))) => {}
            Err(WslcError::Hresult(code, _)) => assert_ne!(code, 0),
            Err(other) => panic!("预期返回 SdkUpdateNeeded 或 Hresult 错误，实际为: {other:?}"),
        }
    }

    /// 就绪判定必须严格强于 DLL 探测，且两者在 DLL 缺失时保持一致
    ///
    /// 原用例名为 `test_ensure_sdk_available_agrees_with_probe`，断言两者
    /// 结果**完全相等**。该断言本身即缺陷的固化：`ensure_sdk_available`
    /// 当时只是 `is_sdk_available` 的同义包装，二者同源故自然相等——
    /// 一旦就绪判定按修复方案增强（叠加官方组件检查），二者即不再等价。
    ///
    /// 现断言的正确不变量是**单向蕴含**：DLL 不可加载时必然不就绪；
    /// 而 DLL 可加载时既可能就绪、也可能因组件缺失而不就绪。
    #[test]
    fn test_readiness_is_stricter_than_dll_probe() {
        let dll_loadable = WslcSystem::is_sdk_available();
        let ensured = WslcSystem::ensure_sdk_available();

        if !dll_loadable {
            // DLL 都载不进来，必然不具备运行条件
            assert!(
                ensured.is_err(),
                "DLL 不可加载时 ensure_sdk_available 必须返回错误，实际为 Ok"
            );
            assert!(
                matches!(
                    ensured,
                    Err(WslcError::Domain(WslcDomainError::SdkUpdateNeeded(_)))
                ),
                "DLL 缺失应归入 SdkUpdateNeeded 领域错误，实际为: {ensured:?}"
            );
        } else {
            // DLL 可加载：就绪与否取决于官方组件检查，两种结果都属合法，
            // 但若判定为不就绪，错误必须来自组件层而非其他基础设施
            if let Err(e) = &ensured {
                assert!(
                    matches!(e, WslcError::Domain(WslcDomainError::SdkUpdateNeeded(_))),
                    "DLL 可加载时的失败只应源于组件未就绪，实际为: {e:?}"
                );
            }
        }
    }
}
