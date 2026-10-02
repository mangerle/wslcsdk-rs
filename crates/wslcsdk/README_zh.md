# wslcsdk

<p align="center">
  <a href="README.md">English</a> | <b>简体中文</b>
</p>

微软官方 Microsoft WSL Containers (WSLC) SDK 的 Rust 惯用安全异步封装库。

基于微软官方发布的 **`Microsoft.WSL.Containers` (版本 3.0.1 GA)** 构建，提供类型安全、内存安全及异步友好的现代 Rust 接口。

---

## 核心特性

- **严格的 RAII 资源管理**：会话、容器、进程与崩溃转储订阅句柄在析构时自动调用底层 C-ABI 释放。
- **流式进程 I/O**：通过 C 回调跳板拦截标准输出与标准错误并推入有界通道，规避 Windows 同步匿名管道缺陷；对外暴露的接收端与具体异步运行时解耦，不会把第三方运行时类型泄漏进公开接口。
- **双层强类型错误**：`WslcDomainError` 与官方头文件的 19 个具名 HRESULT 一一对应，基础设施类失败保留在外层 `WslcError`；未识别的标准 HRESULT 按名称呈现，而非笼统的通用描述。
- **非阻塞异步扩展**：提供以 `_async` 结尾的异步方法，自动将耗时调用调度至 `tokio::task::spawn_blocking` 并自动初始化 COM MTA。
- **存储卷与镜像管理**：内置 VHDX 存储卷配置与镜像拉取、列出与删除抽象。

---

## 依赖配置

在 `Cargo.toml` 中添加：

```toml
[dependencies]
wslcsdk = "0.2.0"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

> **兼容性说明**：`wslcsdk 0.2.x` 基于底层 `wslcsdk-sys 3.0.1`，要求 Windows 11 宿主机并安装 WSL 2.9.3+ 或 3.0.1+。

---

## 使用示例

```rust,no_run
use wslcsdk::{WslcClient, WslcSignal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 初始化统一客户端 (自动完成环境安全探测、创建默认会话并接管 COM MTA 守护)
    let client = WslcClient::builder().session_name("my-session").build()?;

    // 2. 建造者已绑定客户端的会话，build() 无需再传会话句柄
    let container = client
        .create_container("docker.io/library/alpine:latest")
        .name("alpine-demo")
        .auto_remove(true)
        .build()?;

    // 3. 启动容器
    container.start(false)?;

    // 4. 停止并删除容器
    container.stop(WslcSignal::Sigterm, 10)?;
    container.delete(false)?;

    Ok(())
}
```

---

## 开源协议

本项目采用 [MIT License](../../LICENSE) 许可证。
