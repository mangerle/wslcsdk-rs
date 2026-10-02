//! WSLC 容器内进程创建、信号分发与标准 I/O 交互
//!
//! 包含子进程建造者、进程 RAII 生命周期句柄、基于 Tokio 的流式 I/O 管道桥接，
//! 以及与 C 侧直接交互的回调跳板。

mod builder;
mod callbacks;
mod handle;
mod stream;

pub use builder::ProcessBuilder;
pub(crate) use builder::RetainedProcessSettings;
pub use handle::WslcProcessHandle;
pub use stream::ProcessStreams;
pub(crate) use stream::{StreamState, setup_streaming_channels};
