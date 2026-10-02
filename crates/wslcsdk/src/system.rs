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
    /// 安全检测当前宿主机系统是否已就绪 WSLC SDK (wslcsdk.dll 是否存在并可加载)
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

    /// 确保 WSLC SDK 运行时就绪，若缺失则返回友好领域错误
    pub fn ensure_sdk_available() -> Result<(), WslcError> {
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

    /// 获取当前系统安装的 WSLC 运行时版本
    pub fn get_version() -> Result<WslcVersion, WslcError> {
        Self::ensure_sdk_available()?;
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut ver = WslcVersion::default();
        // SAFETY: 入参均为已初始化且存活期覆盖本次调用的本地缓冲区或官方句柄，
        // 出参为合法的可写指针，不涉及未定义行为。
        let hr = unsafe { WslcGetVersion(&mut ver) };
        WslcError::check_hr(hr, "获取 WSLC 版本失败")?;
        Ok(ver)
    }

    /// 检测当前宿主机缺失的组件依赖
    pub fn get_missing_components() -> Result<WslcComponentFlags, WslcError> {
        Self::ensure_sdk_available()?;
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
        Self::ensure_sdk_available()?;
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

    #[test]
    fn test_ensure_sdk_available_agrees_with_probe() {
        let available = WslcSystem::is_sdk_available();
        let ensured = WslcSystem::ensure_sdk_available();
        assert_eq!(
            available,
            ensured.is_ok(),
            "is_sdk_available 与 ensure_sdk_available 的判定结果不一致"
        );
    }
}
