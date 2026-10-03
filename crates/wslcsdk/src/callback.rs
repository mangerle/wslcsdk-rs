//! C 回调上下文的统一访问入口
//!
//! 官方 SDK 的多处接口要求调用方以 `void*` 形式移交回调上下文。本模块把
//! 「解引用该指针并调用 Rust 闭包」这一公共骨架收敛到一处，供各处 C 跳板复用。
//!
//! # 为何要有这一层
//!
//! 镜像拉取进度、镜像推送进度与组件安装进度三处跳板，骨架完全相同：
//! 判空 → 捕获 panic → 取 `Mutex` 锁 → 调用闭包。三份拷贝意味着「跨 FFI
//! 边界必须 `catch_unwind`」「并发回调必须串行化」这两条硬约束要在三个
//! 地方各自被想起一次——漏掉任何一处都是未定义行为或数据竞争。
//!
//! # 上下文的内存布局约定
//!
//! 上下文一律为 `Box<Mutex<F>>` 经 `Box::into_raw` 拆出的裸指针。以
//! `Mutex` 包裹是为了在官方从多线程并发派发时，把对闭包的调用串行化：
//! 各处跳板只要求 `F: Send`（而非 `Sync`），闭包内部状态不能被假定为
//! 单线程访问，这一点与 [`crate::session::WslcSessionHandle::register_crash_dump_callback`]
//! 采用的防护等级一致。

use core::ffi::c_void;

/// 在回调上下文中串行化地执行闭包，返回结果；上下文为空或调用失败时返回 `None`
///
/// # Safety
///
/// `context` 必须是由 `Box::into_raw(Box::new(Mutex::new(f)))` 拆出的、
/// 指向 `Mutex<F>` 的有效指针，且 `F` 必须与拆出时的类型严格一致。
/// 在官方停止回调之前，该内存必须保持有效——按错误类型回收会造成内存损坏。
pub(crate) unsafe fn invoke_callback<F, R>(
    context: *mut c_void,
    invoke: impl FnOnce(&mut F) -> R,
) -> Option<R> {
    if context.is_null() {
        return None;
    }

    // 跨 FFI 边界必须捕获 panic：unwind 穿过 C 栈帧属未定义行为。
    // 官方要求回调尽快返回，故此处只取锁执行一次调用，不做任何阻塞等待。
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: 调用方保证 context 非空且指向 Mutex<F>，
        // 类型与 Box::into_raw 拆出时严格一致。
        let mutex = unsafe { &*(context as *const std::sync::Mutex<F>) };
        match mutex.lock() {
            Ok(mut callback) => Some(invoke(&mut callback)),
            // 锁中毒：上一次回调 panic 过，此时不再触碰可能已不一致的闭包状态
            Err(_) => None,
        }
    }))
    // catch_unwind 自身返回 Result<Option<R>, _>；panic 时按「调用未发生」处理
    .unwrap_or(None)
}
