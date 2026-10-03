<div align="center">

<img src="assets/logo-badge.svg" width="360" alt="WSLCSDK" style="margin-bottom: 14px;" />

**微软官方 Microsoft WSL Containers (WSLC) SDK 的 Rust 惯用安全异步封装生态**

<p align="center">
  <a href="README.md">English</a> | <b>简体中文</b>
</p>

[![Rust](https://img.shields.io/badge/Rust-2024_Edition-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows_11-blue.svg)]()
[![WSL](https://img.shields.io/badge/WSL-2.9.3+%20%7C%203.0.1+-brightgreen.svg)]()

</div>

本项目基于微软官方发布的 **`Microsoft.WSL.Containers` (版本 3.0.1 GA)** 构建，为开发者提供现代化、内存安全且面向生产环境的 Rust 接口，无需安装 Docker Desktop 即可在 Windows 上原生运行并管理基于 WSL 2.0 微虚拟机的 Linux 容器。

---

## 工作区架构设计

本仓库采用标准的 Cargo Workspace 架构进行组织，实现职责清晰的分层隔离：

```text
wslcsdk-rs/
├── crates/
│   ├── wslcsdk-sys/     # 原始 1:1 C-ABI FFI 绑定层 (版本 3.0.1)
│   └── wslcsdk/         # 惯用安全 Rust 封装层 (版本 0.3.0)
│       └── examples/    # 经过编译验证的可运行示例
├── Cargo.toml           # 根工作区配置
├── LICENSE              # MIT 开源许可证
├── README.md            # 默认英文文档
└── README_zh.md         # 简体中文文档
```

| Crate | 版本 | 描述 |
| :--- | :--- | :--- |
| [`wslcsdk-sys`](crates/wslcsdk-sys) | `3.0.1` | 严格 1:1 映射微软官方 `wslcsdk.h` 头文件全部 C 接口；内置 MSVC `/DELAYLOAD` 延迟加载及 `x64` / `arm64` 预编译导入库。 |
| [`wslcsdk`](crates/wslcsdk) | `0.3.0` | 拥有 RAII 生命周期管理、双层强类型错误体系、与运行时解耦的流式 I/O 管道及非阻塞扩展的高级安全 Rust 抽象。 |

---

## 核心特性（真实代码对齐）

- **严格的 RAII 资源释放保证**：底层原生句柄（`WslcSession`、`WslcContainer`、`WslcProcess`、`WslcCrashDumpSubscription`）全部封装于 Rust 守卫对象中，离开作用域时自动调用 C-ABI 析构，杜绝资源泄漏。
- **流式进程 I/O**：`ProcessBuilder::with_streaming_io` 通过原生 C 回调跳板（Trampoline）将容器进程的标准输出（`stdout`）与标准错误（`stderr`）实时推送到有界通道中，彻底解决 Windows 同步匿名管道无法接入异步事件循环的缺陷。公开的接收端（`ProcessStreams` / `AsyncReceiver`）与具体异步运行时解耦，不会把第三方运行时类型泄漏进你的公开接口。
- **双层强类型错误模型**：`WslcDomainError` 与官方头文件的 19 个具名 HRESULT 一一对应，而基础设施类失败（`Hresult`、`Io`、`JsonError`、`TaskJoin` 等）保留在外层 `WslcError`。两层均标记 `#[non_exhaustive]`，且未识别的标准 HRESULT（如 `E_INVALIDARG`）会按名称呈现，不再是笼统且误导的通用描述。
- **异步运行时安全卸载**：提供专用的非阻塞异步扩展方法（`build_async`、`start_async`、`stop_async`、`pull_image_async` 等），自动将耗时 C-ABI 调度至 `tokio::task::spawn_blocking` 阻塞线程池，并自动完成 COM 多线程套间（MTA）的初始化。
- **VHDX 磁盘卷管理**：完整支持创建、挂载及管理专用 VHDX 存储卷，支持配置所属用户（UID）、用户组（GID）、容量大小与读写权限。

---

## 宿主运行环境要求

- **操作系统**：Windows 11
- **WSL 运行时**：WSL 2.9.3+ 或 3.0.1+（宿主机需启用“虚拟机平台”与 WSL 特性）
- **支持的目标编译架构**：`x86_64-pc-windows-msvc` 或 `aarch64-pc-windows-msvc`
- **Rust 工具链**：Rust 1.98.1+ (Edition 2024)

---

## 快速上手

在项目的 `Cargo.toml` 中引入依赖：

```toml
[dependencies]
wslcsdk = "0.3.0"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

完整且经过编译验证的示例位于 [`crates/wslcsdk/examples`](crates/wslcsdk/examples)，
下文代码片段均摘自这些文件。

### 1. 创建会话与容器生命周期管理

```rust,no_run
use wslcsdk::{ContainerBuilder, SessionBuilder, WslcContainerState, WslcPortProtocol, WslcSignal};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 创建会话；默认存储目录为 %LOCALAPPDATA%\wslc\sessions\<name>
    let session = SessionBuilder::new_default("my-session")?
        .cpu_count(4)
        .memory_mb(4096)
        .build()?;

    // 2. 配置并构建容器
    let container = ContainerBuilder::new("docker.io/library/alpine:latest")
        .name("alpine-test")
        .auto_remove(true)
        .add_port_mapping(8080, 80, WslcPortProtocol::Tcp)
        .build(&session)?;

    // 3. 启动容器并查询其状态
    container.start(false)?;
    println!("容器启动成功，唯一标识 ID: {}", container.id()?);
    println!(
        "是否处于运行状态: {}",
        container.state()? == WslcContainerState::RUNNING
    );

    // 4. 优雅停止并清理会话（因配置了 auto_remove(true)，容器已在 stop 时自动删除）
    container.stop(WslcSignal::Sigterm, 10)?;
    session.terminate()?;

    Ok(())
}
```

### 2. 执行容器内命令并异步流式读取输出

```rust,no_run
use wslcsdk::{ContainerBuilder, ProcessBuilder, SessionBuilder, WslcSignal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = SessionBuilder::new_default("demo-session")?.build()?;
    let container = ContainerBuilder::new("alpine:latest").build(&session)?;
    container.start(false)?;

    // 声明带异步流式 I/O 的进程构建器
    let (builder, mut streams) = ProcessBuilder::new()
        .command(&["/bin/sh", "-c", "echo '来自 WSL 容器的问候!' && ls -la"])
        .with_streaming_io();

    let process = builder.spawn(&container)?;
    println!("进程已派生，PID: {}", process.pid()?);

    // 先消费完标准输出再等待退出：官方要求退出回调在 IO 冲刷完成后才触发
    while let Some(chunk) = streams.stdout.recv().await {
        print!("{}", String::from_utf8_lossy(&chunk));
    }

    // 等待进程退出状态码
    let exit_code = streams.wait_exit().await?;
    println!("进程执行完毕，退出状态码: {}", exit_code);

    container.stop(WslcSignal::Sigterm, 5)?;
    container.delete(false)?;
    session.terminate()?;

    Ok(())
}
```

### 3. 非阻塞异步操作示例

```rust,no_run
use wslcsdk::{ContainerBuilder, SessionBuilder, WslcSignal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 异步创建会话与容器，不阻塞 Tokio 异步工作线程
    let session = SessionBuilder::new_default("async-session")?
        .build_async()
        .await?;

    let container = ContainerBuilder::new("alpine:latest")
        .name("async-container")
        .build_async(&session)
        .await?;

    container.start_async(false).await?;
    println!("容器状态: {:?}", container.state()?);

    container.stop_async(WslcSignal::Sigterm, 5).await?;
    container.delete_async(false).await?;
    session.terminate_async().await?;

    Ok(())
}
```

---

## 质量门禁与本地验证

本项目在持续集成中执行严格的零警告质量门禁：

```bash
# 统一代码风格检查
cargo fmt --all -- --check

# 静态分析与 Lint 检查
cargo clippy --all-targets -- -D warnings

# 全量回归测试（同时编译 examples/ 下的全部示例）
cargo test --all-targets

# 文档构建（警告视为错误）
cargo doc --all-features --no-deps
```

### Lint 配置说明

`wslcsdk` crate 在 crate 级启用了以下 lint：

| Lint | 级别 | 作用 |
| --- | --- | --- |
| `unsafe_op_in_unsafe_fn` | deny | 强制 `unsafe fn` 体内逐处显式书写 unsafe 块，使函数签名的安全承诺与实现保持一致 |
| `undocumented_unsafe_blocks` | warn | 每个 unsafe 块都必须携带 `// SAFETY:` 论证 |
| `missing_safety_doc` | warn | 每个 `pub unsafe fn` 都必须文档化其 `# Safety` 契约 |
| `rust_2018_idioms` | warn | 捕获省略生命周期、裸trait 对象等旧式写法 |
| `missing_docs` | warn |公开的结构体字段、枚举变体、模块与方法必须带文档注释 |
| `missing_debug_implementations` | warn | 公开类型须实现 `Debug`，便于日志与断言输出 |
| `unused_qualifications` | warn | 强制以 `use` 显式引入，而非内联全限定路径 |

---

## 开源协议

本项目采用 [MIT License](LICENSE) 开源许可证。
