use crate::error::WslcError;
use core::ffi::c_void;
use wslcsdk_sys::types::{WslcComponentFlags, WslcInstallOptions, WslcVersion};
use wslcsdk_sys::*;

/// WSLC 系统服务门面
pub struct WslcSystem;

impl WslcSystem {
    /// 安全检测当前宿主机系统是否已就绪 WSLC SDK (wslcsdk.dll 是否存在并可加载)
    pub fn is_sdk_available() -> bool {
        let dll_name: Vec<u16> = "wslcsdk.dll"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe {
            windows_sys::Win32::System::LibraryLoader::LoadLibraryExW(
                dll_name.as_ptr(),
                std::ptr::null_mut(),
                0,
            )
        };
        if !handle.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::FreeLibrary(handle);
            }
            true
        } else {
            false
        }
    }
    /// 获取当前系统安装的 WSLC 运行时版本
    pub fn get_version() -> Result<WslcVersion, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut ver = WslcVersion::default();
        let hr = unsafe { WslcGetVersion(&mut ver) };
        if hr >= 0 {
            Ok(ver)
        } else {
            Err(WslcError::Win32(
                hr as u32,
                "获取 WSLC 版本失败".to_string(),
            ))
        }
    }

    /// 检测当前宿主机缺失的组件依赖
    pub fn get_missing_components() -> Result<WslcComponentFlags, WslcError> {
        let _com_guard = crate::com::try_initialize_mta()?;
        let mut missing = 0u32;
        let hr = unsafe { WslcGetMissingComponents(&mut missing) };
        if hr >= 0 {
            Ok(missing)
        } else {
            Err(WslcError::Win32(hr as u32, "检测缺失组件失败".to_string()))
        }
    }

    /// 安装缺失组件与依赖
    ///
    /// # 参数
    /// - `components`: 需安装的组件标志位
    /// - `options`: 安装选项 (如修复重装)
    /// - `on_progress`: 可选进度回调函数: `Fn(组件, 当前步, 总步数)`
    pub fn install_with_dependencies<F>(
        components: WslcComponentFlags,
        options: WslcInstallOptions,
        mut on_progress: Option<F>,
    ) -> Result<(), WslcError>
    where
        F: FnMut(WslcComponentFlags, u32, u32) + Send + 'static,
    {
        unsafe extern "system" fn progress_trampoline<F>(
            component: WslcComponentFlags,
            progress_steps: u32,
            total_steps: u32,
            context: *mut c_void,
        ) where
            F: FnMut(WslcComponentFlags, u32, u32),
        {
            if !context.is_null() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    let callback = &mut *(context as *mut F);
                    callback(component, progress_steps, total_steps);
                }));
            }
        }

        let (cb, ctx) = match on_progress.as_mut() {
            Some(f) => (
                Some(progress_trampoline::<F> as _),
                f as *mut F as *mut c_void,
            ),
            None => (None, std::ptr::null_mut()),
        };

        let hr = unsafe { WslcInstallWithDependencies(components, options, cb, ctx) };
        if hr >= 0 {
            Ok(())
        } else {
            Err(WslcError::Win32(hr as u32, "安装依赖组件失败".to_string()))
        }
    }
}
