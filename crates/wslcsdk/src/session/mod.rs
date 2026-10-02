//! WSLC 会话生命周期管理
//!
//! 按关注点分为两个子模块：
//! - `builder`：会话设置建造者，以及字符串到宽字符的转换辅助；
//! - `handle`：会话句柄与崩溃转储订阅的 RAII 生命周期管理。
//!
//! 会话句柄的有效性绑定于建立它的 COM 套间，故句柄均持有进程级 MTA 租约，
//! 详见 `handle` 模块文档。

mod builder;
mod handle;

pub use builder::{SessionBuilder, VhdRequirementsData, default_session_root};
// 路径转换辅助为 crate 内部工具，供容器与镜像模块复用，不参与公开 API 面
pub(crate) use builder::path_to_wide_null;
pub use handle::{CrashDumpSubscription, WslcSessionHandle};
