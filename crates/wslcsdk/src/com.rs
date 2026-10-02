//! Windows COM 多线程套间 (MTA) RAII 守卫
//!
//! WSLC 底层依赖 COM/RPC 通信。本模块确保调用线程具备合法的 COM 上下文，
//! 并在离开作用域时按规范执行配对的 CoUninitialize，防止发生 CO_E_NOTINITIALIZED 错误。

use crate::error::WslcError;
use std::marker::PhantomData;
use windows_sys::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows_sys::core::HRESULT;

/// COM 已经初始化为单线程套间 (STA) 时的错误码
const RPC_E_CHANGED_MODE: HRESULT = 0x8001_0106_u32 as HRESULT;

/// COM 多线程套间 RAII 生命周期守卫 (绑定当前线程，禁止跨线程移动)
#[derive(Debug)]
pub struct ComGuard {
    should_uninitialize: bool,
    _not_send: PhantomData<*const ()>,
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.should_uninitialize {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

/// 尝试为当前线程初始化 COM MTA 上下文
///
/// 若当前线程已处于 MTA，返回具备配对析构能力的守卫；
/// 若当前线程已被初始化为 STA（如某些 GUI 线程），则返回 `None`（已有 COM 环境且不可改变）；
/// 若初始化失败则返回错误。
pub fn try_initialize_mta() -> Result<Option<ComGuard>, WslcError> {
    let hr = unsafe { CoInitializeEx(std::ptr::null_mut(), COINIT_MULTITHREADED as u32) };
    match hr {
        // S_OK (0) 或 S_FALSE (1): 初始化成功或已初始化为 MTA，均需对应析构
        0 | 1 => Ok(Some(ComGuard {
            should_uninitialize: true,
            _not_send: PhantomData,
        })),
        // 当前线程已被外部宿主 (如 UI 框架) 初始化为 STA，直接复用既有 COM 环境
        RPC_E_CHANGED_MODE => Ok(None),
        code => Err(WslcError::Win32(
            code as u32,
            "CoInitializeEx 初始化失败".to_string(),
        )),
    }
}

/// 在保证具备合法的 COM MTA 环境内执行操作
pub fn with_mta<F, R>(f: F) -> Result<R, WslcError>
where
    F: FnOnce() -> Result<R, WslcError>,
{
    let _guard = try_initialize_mta()?;
    f()
}
