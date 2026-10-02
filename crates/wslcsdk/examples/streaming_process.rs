//! 流式捕获容器进程标准输出与标准错误的示例
//!
//! 演示通过原生回调跳板把进程输出实时推送到有界异步通道，并消费退出码。
//!
//! 运行前提：宿主机已安装 WSL Containers 支持组件（`wslcsdk.dll` 可加载）。
//!
//! 运行方式：`cargo run --example streaming_process`

use wslcsdk::{ContainerBuilder, ProcessBuilder, SessionBuilder, WslcSignal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = SessionBuilder::new_default("example-streaming")?.build()?;
    let container = ContainerBuilder::new("alpine:latest").build(&session)?;
    container.start(false)?;

    // 启用流式 I/O：返回配置完毕的建造者与两路字节流接收端
    let (builder, mut streams) = ProcessBuilder::new()
        .command(&["/bin/sh", "-c", "echo '来自 WSL 容器的问候' && ls -la /"])
        .working_directory("/")
        .env("EXAMPLE_ENV", "wslcsdk")?
        .with_streaming_io();

    let process = builder.spawn(&container)?;
    println!("进程已派生，PID: {}", process.pid()?);

    // 先消费完标准输出，再等待退出：官方要求退出回调在 IO 冲刷完成后才触发
    while let Some(chunk) = streams.stdout.recv().await {
        print!("{}", String::from_utf8_lossy(&chunk));
    }

    while let Some(chunk) = streams.stderr.recv().await {
        eprint!("{}", String::from_utf8_lossy(&chunk));
    }

    let exit_code = streams.wait_exit().await?;
    println!("进程退出码: {exit_code}");

    // 消费能力不足时，通道会丢弃数据以保证背压，此处显式提示便于排查
    let dropped = streams.stdout_dropped_bytes() + streams.stderr_dropped_bytes();
    if dropped > 0 {
        eprintln!(
            "警告: 因消费不及时共丢弃 {dropped} 字节输出，可改用 with_streaming_io_capacity 增大缓冲"
        );
    }

    container.stop(WslcSignal::Sigterm, 5)?;
    container.delete(false)?;
    session.terminate()?;
    Ok(())
}
