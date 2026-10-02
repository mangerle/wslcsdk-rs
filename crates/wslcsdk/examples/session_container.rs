//! 会话与容器的完整生命周期示例
//!
//! 演示创建会话、配置并启动容器、查询状态，以及按序清理资源的完整流程。
//!
//! 运行前提：宿主机已安装 WSL Containers 支持组件（`wslcsdk.dll` 可加载）。
//!
//! 运行方式：`cargo run --example session_container`

use wslcsdk::{ContainerBuilder, SessionBuilder, WslcContainerState, WslcPortProtocol, WslcSignal};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 创建会话：默认存储目录位于 %LOCALAPPDATA%\wslc\sessions\<name>
    let session = SessionBuilder::new_default("example-session")?
        .cpu_count(4)
        .memory_mb(4096)
        .build()?;
    println!("会话已创建: {}", session.name());

    // 2. 配置并创建容器
    let container = ContainerBuilder::new("docker.io/library/alpine:latest")
        .name("example-container")
        .auto_remove(true)
        .add_port_mapping(8080, 80, WslcPortProtocol::Tcp)
        .add_volume(r"C:\example-data", "/data", false)
        .build(&session)?;
    println!("容器已创建，ID: {}", container.id()?);

    // 3. 启动容器并查询状态
    container.start(false)?;
    let state = container.state()?;
    println!(
        "容器是否处于运行状态: {}",
        state == WslcContainerState::RUNNING
    );

    // 4. 查看容器检查快照（JSON）
    let inspect = container.inspect()?;
    println!(
        "容器检查快照字段数: {}",
        inspect.as_object().map_or(0, |o| o.len())
    );

    // 5. 停止并清理资源
    // 容器配置了 auto_remove(true)，在 stop 时会自动清理删除，无需再调用 delete
    container.stop(WslcSignal::Sigterm, 10)?;
    session.terminate()?;

    println!("全部资源已释放");
    Ok(())
}
