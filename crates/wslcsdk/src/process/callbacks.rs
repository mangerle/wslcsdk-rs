//! C 回调跳板与丢弃统计
//!
//! 本模块是唯一与 C 侧直接交互的部位：官方在 IO 数据到达与进程退出时
//! 回调此处，通过整数流 ID 在全局注册表中定位 Rust 侧状态。
//!
//! 三条硬约束贯穿本模块：
//! 1. **跨 FFI 边界必须捕获 panic**——unwind 穿过 C 栈帧属未定义行为；
//! 2. **上下文为整数而非裸指针**——使 `ProcessBuilder` 天然满足 `Send + Sync`，
//!    无需 `unsafe impl`；
//! 3. **回调不得阻塞**——IO 数据经有界通道 `try_send` 投递，满了即丢弃并计数。

use super::stream::{StreamState, stream_registry};
use core::ffi::c_void;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use wslcsdk_sys::types::WslcProcessIOHandle;

/// 累加丢弃字节数，并仅在首次丢弃时输出一次告警
///
/// 本函数处于 C 回调的高频路径上，刻意不做逐次告警以避免日志轰炸
/// (AGENTS.md §5 严禁高频热点日志轰炸)；完整累计值另由 `StreamState` 析构时汇总。
pub(super) fn record_dropped(state: &StreamState, is_stdout: bool, bytes: u64) {
    let (counter, direction) = if is_stdout {
        (&state.stdout_dropped_bytes, "标准输出")
    } else {
        (&state.stderr_dropped_bytes, "标准错误")
    };

    let previous = counter.fetch_add(bytes, Ordering::Relaxed);
    if previous == 0 {
        log::warn!(
            "流式通道消费不及时，已开始丢弃数据以维持背压，流 ID: {}，方向: {}，本次丢弃 {} 字节",
            state.id,
            direction,
            bytes
        );
    }
}

pub(super) unsafe extern "system" fn stream_io_trampoline(
    io_handle: WslcProcessIOHandle,
    data: *const u8,
    data_bytes: u32,
    context: *mut c_void,
) {
    if context.is_null() || data.is_null() || data_bytes == 0 {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let stream_id = context as usize as u64;
        // 即查即放：取出强引用后立刻结束对注册表的借用，
        // 不让分片锁守卫跨后续的通道操作或 FFI 边界存活
        let state_arc = stream_registry()
            .get(&stream_id)
            .and_then(|weak| weak.upgrade());

        // 若 Rust 接收端与句柄均已被提前释放，安全放弃处理，杜绝 UAF 悬垂指针访问
        let Some(state) = state_arc else {
            return;
        };

        // 依据 C 侧回传的流类型选定目标通道
        let is_stdout = io_handle == WslcProcessIOHandle::STDOUT;
        let sender_slot = if is_stdout {
            &state.stdout_tx
        } else if io_handle == WslcProcessIOHandle::STDERR {
            &state.stderr_tx
        } else {
            // 官方尚未定义该流类型，安全忽略
            return;
        };

        // 仅在 try_send 期间持锁：该操作不会阻塞，两路各用一把锁故互不干扰
        let Ok(guard) = sender_slot.lock() else {
            return;
        };
        let Some(sender) = guard.as_ref() else {
            // 通道已被退出回调关闭，此后到达的数据无处可去，直接忽略
            return;
        };

        // SAFETY: 指针有效且指向至少 len 个元素，所有元素均已初始化，
        // 生命周期由调用方保证覆盖本次借用。
        let slice = unsafe { std::slice::from_raw_parts(data, data_bytes as usize) };
        let chunk = slice.to_vec();

        if let Err(mpsc::error::TrySendError::Full(dropped)) = sender.try_send(chunk) {
            record_dropped(&state, is_stdout, dropped.len() as u64);
        }
        // 显式关闭守卫，确保在跨 FFI 边界前释放锁
        drop(guard);
    }));
}

pub(super) unsafe extern "system" fn stream_exit_trampoline(exit_code: i32, context: *mut c_void) {
    if context.is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let stream_id = context as usize as u64;
        // 即查即放：取出强引用后立刻结束对注册表的借用，
        // 不让分片锁守卫跨后续的通道操作或 FFI 边界存活
        let state_arc = stream_registry()
            .get(&stream_id)
            .and_then(|weak| weak.upgrade());

        if let Some(state) = state_arc {
            // 官方保证退出回调在 IO 冲刷完成后才触发，此时关闭字节流通道
            // 不会丢失尾部数据
            state.close_io_channels();

            if let Ok(mut guard) = state.exit_tx.lock()
                && let Some(tx) = guard.take()
            {
                let _ = tx.send(exit_code);
            }
        }
    }));
}
