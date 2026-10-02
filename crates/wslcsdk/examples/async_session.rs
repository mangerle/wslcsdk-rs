//! 异步扩展 API 示例
//!
//! 演示把阻塞的 C ABI 调用卸载到阻塞线程池的异步方法，避免阻塞异步运行时工作线程。
//!
//! 运行前提：宿主机已安装 WSL Containers 支持组件（`wslcsdk.dll` 可加载）。
//!
//! 运行方式：`cargo run --example async_session`

use wslcsdk::{ContainerBuilder, SessionBuilder, WslcSignal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 异步创建会话
    let session = SessionBuilder::new_default("example-async")?
        .cpu_count(2)
        .build_async()
        .await?;

    // 异步创建容器
    let container = ContainerBuilder::new("alpine:latest")
        .name("async-container")
        .build_async(&session)
        .await?;

    // 异步启动并查询状态
    container.start_async(false).await?;
    println!("容器状态: {:?}", container.state()?);

    // 异步列出会话内的镜像
    let images = session.list_images_async().await?;
    println!("会话内镜像数量: {}", images.len());

    // 异步清理
    container.stop_async(WslcSignal::Sigterm, 5).await?;
    container.delete_async(false).await?;
    session.terminate_async().await?;
    Ok(())
}
