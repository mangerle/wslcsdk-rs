# wslcsdk

<p align="center">
  <a href="README.md">English</a> | <b>简体中文</b>
</p>

微软官方 Microsoft WSL Containers (WSLC) SDK 的 Rust 惯用安全异步封装库。

基于微软官方发布的 **`Microsoft.WSL.Containers` (版本 3.0.1 GA)** 构建，提供类型安全、内存安全及异步友好的现代 Rust 接口。

---

## 核心特性

- **严格的 RAII 资源管理**：会话、容器、进程与崩溃转储订阅句柄在析构时自动调用底层 C-ABI 释放。
- **Tokio 异步流式进程 I/O**：通过 C 回调跳板拦截标准输出与标准错误并推入 Tokio 有界通道，规避 Windows 同步匿名管道缺陷。
- **强类型领域错误**：将全部微软专有 HRESULT 错误码与宽字符描述映射为强类型领域错误枚举。
- **非阻塞异步扩展**：提供以 `_async` 结尾的异步方法，自动将耗时调用调度至 `tokio::task::spawn_blocking` 并自动初始化 COM MTA。
- **存储卷与镜像管理**：内置 VHDX 存储卷配置与镜像拉取、列出与删除抽象。

---

## 依赖配置

在 `Cargo.toml` 中添加：

```toml
[dependencies]
wslcsdk = "0.1.0"
tokio = { version = "1", features = ["full"] }
```

> **兼容性说明**：`wslcsdk 0.1.x` 基于底层 `wslcsdk-sys 3.0.1`，要求 Windows 10/11 宿主机并安装 WSL 2.9.3+ 或 3.0.1+。

---

## 使用示例

```rust,no_run
use wslcsdk::{ContainerBuilder, SessionBuilder, WslcSignal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 初始化并构建会话
    let session = SessionBuilder::new("my-session", r"C:\wslc\storage")
        .cpu_count(4)
        .memory_mb(4096)
        .build()?;

    // 2. 配置并构建容器
    let container = ContainerBuilder::new("docker.io/library/alpine:latest")
        .name("alpine-demo")
        .auto_remove(true)
        .build(&session)?;

    // 3. 启动容器
    container.start(false)?;
    println!("容器 ID: {}", container.id()?);

    // 4. 停止并删除容器
    container.stop(WslcSignal::Sigterm, 10)?;
    container.delete(false)?;

    Ok(())
}
```

---

## 开源协议

本项目采用 [MIT License](../../LICENSE) 许可证。
