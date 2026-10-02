<div align="center">

<img src="assets/logo-badge.svg" width="360" alt="WSLCSDK" style="margin-bottom: 14px;" />

**Idiomatic, safe, and asynchronous Rust binding and abstraction suite for Microsoft WSL Containers (WSLC)**

<p align="center">
  <b>English</b> | <a href="README_zh.md">简体中文</a>
</p>

[![Rust](https://img.shields.io/badge/Rust-2024_Edition-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows_10%20%7C%20Windows_11-blue.svg)]()
[![WSL](https://img.shields.io/badge/WSL-2.9.3+%20%7C%203.0.1+-brightgreen.svg)]()

</div>

Built against Microsoft's official **`Microsoft.WSL.Containers` (Version 3.0.1 GA)**, this workspace provides modern, memory-safe, and production-grade Rust interfaces to manage WSL-native micro-VM containers without requiring Docker Desktop.

---

## Workspace Structure

This repository is organized as a Cargo workspace with a clear separation of concerns:

```text
wslcsdk-rs/
├── crates/
│   ├── wslcsdk-sys/     # Raw 1:1 C-ABI FFI bindings (Version 3.0.1)
│   └── wslcsdk/         # High-level idiomatic, safe Rust SDK (Version 0.1.0)
├── Cargo.toml           # Root workspace configuration
├── LICENSE              # MIT License
├── README.md            # Default documentation (English)
└── README_zh.md         # Chinese documentation (简体中文)
```

| Crate | Version | Description |
| :--- | :--- | :--- |
| [`wslcsdk-sys`](crates/wslcsdk-sys) | `3.0.1` | Raw FFI declarations matching `wslcsdk.h` with MSVC `/DELAYLOAD` and precompiled import libraries for `x64` and `arm64`. |
| [`wslcsdk`](crates/wslcsdk) | `0.1.0` | Safe RAII handles, typed error models, Tokio streaming process I/O, and async execution wrappers. |

---

## Key Features (Code-Accurate)

- **Strict RAII Lifetime Management**: All native handles (`WslcSession`, `WslcContainer`, `WslcProcess`, `WslcCrashDumpSubscription`) automatically trigger safe teardown via C-ABI release functions when dropped.
- **Tokio Streaming Process I/O**: `ProcessBuilder::with_streaming_io` captures process `stdout` and `stderr` through native C trampolines directly into bounded Tokio MPSC channels, bypassing Windows synchronous anonymous pipe bottlenecks.
- **Strong Typed Error Domain**: Comprehensive `WslcError` mapped via `thiserror` covering official HRESULT error codes (e.g., `WSLC_E_CONTAINER_NOT_FOUND`, `WSLC_E_IMAGE_NOT_FOUND`, `WSLC_E_VM_NOT_RUNNING`) with contextual wide-string error messages.
- **Async Runtime Offloading**: Dedicated non-blocking extensions (`build_async`, `start_async`, `stop_async`, `pull_image_async`) offload blocking C-ABI calls to `tokio::task::spawn_blocking` while guaranteeing COM MTA initialization.
- **VHDX Storage and Volume Management**: Support for creating, mounting, and managing dedicated VHDX volumes with configurable ownership, permissions, and sizing.

---

## Prerequisites

- **Host Operating System**: Windows 11 or Windows 10 (Build 19044+)
- **WSL Runtime**: WSL 2.9.3+ or 3.0.1+ with "Virtual Machine Platform" enabled
- **Supported Target Architectures**: `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`
- **Rust Toolchain**: Rust 1.85+ (2024 Edition)

---

## Quick Start

Add `wslcsdk` to your `Cargo.toml`:

```toml
[dependencies]
wslcsdk = "0.1.0"
tokio = { version = "1", features = ["full"] }
```

### 1. Creating Sessions and Managing Containers

```rust,no_run
use wslcsdk::{ContainerBuilder, SessionBuilder, WslcSignal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize and configure a session
    let session = SessionBuilder::new("my-session", r"C:\wslc\storage")
        .cpu_count(4)
        .memory_mb(4096)
        .build()?;

    // 2. Configure a container
    let container = ContainerBuilder::new("docker.io/library/alpine:latest")
        .name("alpine-test")
        .auto_remove(true)
        .build(&session)?;

    // 3. Start the container
    container.start(false)?;
    println!("Container started with ID: {}", container.id()?);

    // 4. Stop and clean up
    container.stop(WslcSignal::Sigterm, 10)?;
    container.delete(false)?;

    Ok(())
}
```

### 2. Streaming Process Execution with Tokio

```rust,no_run
use wslcsdk::{ContainerBuilder, ProcessBuilder, SessionBuilder};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = SessionBuilder::new_default("demo-session")?.build()?;
    let container = ContainerBuilder::new("ubuntu:latest").build(&session)?;
    container.start(false)?;

    // Configure streaming I/O for container process
    let (builder, mut streams) = ProcessBuilder::new()
        .command(&["/bin/sh", "-c", "echo 'Hello from WSL Container!' && ls -la"])
        .with_streaming_io();

    let _process = builder.spawn(&container)?;

    // Stream stdout asynchronously
    while let Some(chunk) = streams.stdout.recv().await {
        print!("{}", String::from_utf8_lossy(&chunk));
    }

    // Wait for exit code
    let exit_code = streams.wait_exit().await?;
    println!("Process finished with exit code: {}", exit_code);

    Ok(())
}
```

### 3. Asynchronous Extensions

```rust,no_run
use wslcsdk::{ContainerBuilder, SessionBuilder, WslcSignal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = SessionBuilder::new_default("async-session")?
        .build_async()
        .await?;

    let container = ContainerBuilder::new("alpine:latest")
        .build_async(&session)
        .await?;

    container.start_async(false).await?;
    container.stop_async(WslcSignal::Sigterm, 5).await?;
    session.terminate_async().await?;

    Ok(())
}
```

---

## Quality Gates and Verification

This codebase enforces zero-warning compilation and comprehensive unit testing:

```bash
# Code format check
cargo fmt --all -- --check

# Strict linting
cargo clippy --all-targets -- -D warnings

# Regression testing
cargo test --all-targets
```

---

## License

This project is licensed under the [MIT License](LICENSE).
