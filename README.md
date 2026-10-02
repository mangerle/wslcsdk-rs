<div align="center">

<img src="assets/logo-badge.svg" width="360" alt="WSLCSDK" style="margin-bottom: 14px;" />

**Idiomatic, safe, and asynchronous Rust binding and abstraction suite for Microsoft WSL Containers (WSLC)**

<p align="center">
  <b>English</b> | <a href="README_zh.md">简体中文</a>
</p>

[![Rust](https://img.shields.io/badge/Rust-2024_Edition-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows_11-blue.svg)]()
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
│       └── examples/    # Compile-verified runnable samples
├── Cargo.toml           # Root workspace configuration
├── LICENSE              # MIT License
├── README.md            # Default documentation (English)
└── README_zh.md         # Chinese documentation (简体中文)
```

| Crate | Version | Description |
| :--- | :--- | :--- |
| [`wslcsdk-sys`](crates/wslcsdk-sys) | `3.0.1` | Raw FFI declarations matching `wslcsdk.h` with MSVC `/DELAYLOAD` and precompiled import libraries for `x64` and `arm64`. |
| [`wslcsdk`](crates/wslcsdk) | `0.1.0` | Safe RAII handles, two-layer typed error model, runtime-agnostic streaming process I/O, and async execution wrappers. |

---

## Key Features (Code-Accurate)

- **Strict RAII Lifetime Management**: All native handles (`WslcSession`, `WslcContainer`, `WslcProcess`, `WslcCrashDumpSubscription`) automatically trigger safe teardown via C-ABI release functions when dropped.
- **Streaming Process I/O**: `ProcessBuilder::with_streaming_io` captures process `stdout` and `stderr` through native C trampolines into bounded channels, bypassing Windows synchronous anonymous pipe bottlenecks. The exposed receivers (`ProcessStreams` / `AsyncReceiver`) are runtime-agnostic, so no third-party async runtime type leaks into your public API.
- **Two-Layer Typed Error Domain**: `WslcDomainError` mirrors the 19 named HRESULTs of the official header one-to-one, while infrastructure failures (`Hresult`, `Io`, `JsonError`, `TaskJoin`, ...) remain in the outer `WslcError`. Both are `#[non_exhaustive]`, and unrecognized standard HRESULTs such as `E_INVALIDARG` are rendered by name instead of a misleading generic message.
- **Async Runtime Offloading**: Dedicated non-blocking extensions (`build_async`, `start_async`, `stop_async`, `pull_image_async`) offload blocking C-ABI calls to `tokio::task::spawn_blocking` while guaranteeing COM MTA initialization.
- **VHDX Storage and Volume Management**: Support for creating, mounting, and managing dedicated VHDX volumes with configurable ownership, permissions, and sizing.

---

## Prerequisites

- **Host Operating System**: Windows 11
- **WSL Runtime**: WSL 2.9.3+ or 3.0.1+ with "Virtual Machine Platform" enabled
- **Supported Target Architectures**: `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`
- **Rust Toolchain**: Rust 1.98.1+ (2024 Edition)

---

## Quick Start

Add `wslcsdk` to your `Cargo.toml`:

```toml
[dependencies]
wslcsdk = "0.1.0"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

Complete, compile-verified samples live in [`crates/wslcsdk/examples`](crates/wslcsdk/examples).
The snippets below are excerpts of those files.

### 1. Creating Sessions and Managing Containers

```rust,no_run
use wslcsdk::{ContainerBuilder, SessionBuilder, WslcContainerState, WslcPortProtocol, WslcSignal};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Create a session; the default storage path is
    //    %LOCALAPPDATA%\wslc\sessions\<name>
    let session = SessionBuilder::new_default("my-session")?
        .cpu_count(4)
        .memory_mb(4096)
        .build()?;

    // 2. Configure a container
    let container = ContainerBuilder::new("docker.io/library/alpine:latest")
        .name("alpine-test")
        .auto_remove(true)
        .add_port_mapping(8080, 80, WslcPortProtocol::Tcp)
        .build(&session)?;

    // 3. Start the container and inspect its state
    container.start(false)?;
    println!("Container started with ID: {}", container.id()?);
    println!(
        "Running: {}",
        container.state()? == WslcContainerState::RUNNING
    );

    // 4. Stop and clean up (container is automatically removed upon stop)
    container.stop(WslcSignal::Sigterm, 10)?;
    session.terminate()?;

    Ok(())
}
```

### 2. Streaming Process Execution

```rust,no_run
use wslcsdk::{ContainerBuilder, ProcessBuilder, SessionBuilder, WslcSignal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = SessionBuilder::new_default("demo-session")?.build()?;
    let container = ContainerBuilder::new("alpine:latest").build(&session)?;
    container.start(false)?;

    // Capture stdout/stderr of a process spawned inside the container
    let (builder, mut streams) = ProcessBuilder::new()
        .command(&["/bin/sh", "-c", "echo 'Hello from WSL Container!' && ls -la"])
        .with_streaming_io();

    let process = builder.spawn(&container)?;
    println!("Process PID: {}", process.pid()?);

    // Consume stdout before awaiting the exit code: the official contract only
    // fires the exit callback once the IO buffers have been flushed
    while let Some(chunk) = streams.stdout.recv().await {
        print!("{}", String::from_utf8_lossy(&chunk));
    }

    let exit_code = streams.wait_exit().await?;
    println!("Process finished with exit code: {}", exit_code);

    container.stop(WslcSignal::Sigterm, 5)?;
    container.delete(false)?;
    session.terminate()?;

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
        .name("async-container")
        .build_async(&session)
        .await?;

    container.start_async(false).await?;
    println!("Container state: {:?}", container.state()?);

    container.stop_async(WslcSignal::Sigterm, 5).await?;
    container.delete_async(false).await?;
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

# Regression testing (also compiles every file under examples/)
cargo test --all-targets

# Documentation build with warnings denied
cargo doc --all-features --no-deps
```

### Lint Configuration

The `wslcsdk` crate enables the following lints at crate level:

| Lint | Level | Purpose |
| --- | --- | --- |
| `unsafe_op_in_unsafe_fn` | deny | Requires explicit `unsafe` blocks inside `unsafe fn`, so the signature's promise and the implementation stay consistent |
| `undocumented_unsafe_blocks` | warn | Every `unsafe` block must carry a `// SAFETY:` justification |
| `missing_safety_doc` | warn | Every `pub unsafe fn` must document its `# Safety` contract |
| `rust_2018_idioms` | warn | Catches elided lifetimes, bare trait objects, and other legacy idioms |
| `missing_docs` | warn | Public struct fields, enum variants, modules, and methods must be documented |
| `missing_debug_implementations` | warn | Public types must implement `Debug` for logging and assertions |
| `unused_qualifications` | warn | Enforces explicit `use` imports over inline full paths |

---

## License

This project is licensed under the [MIT License](LICENSE).
