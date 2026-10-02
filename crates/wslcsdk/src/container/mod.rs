//! WSLC 容器设置建造者与容器生命周期控制
//!
//! 包含容器配置建造者、官方结构体编排、端口与挂载模型、以及容器生命周期管理句柄。

mod builder;
mod handle;
mod settings;
mod sockaddr;
mod types;

pub use builder::ContainerBuilder;
pub use handle::WslcContainerHandle;
pub use types::ContainerPortMappingData;
