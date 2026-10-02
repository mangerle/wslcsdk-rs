# wslcsdk

<p align="center">
  <b>English</b> | <a href="README_zh.md">简体中文</a>
</p>

Idiomatic, safe, and asynchronous Rust SDK for Microsoft WSL Containers (WSLC).

Built against Microsoft's official **`Microsoft.WSL.Containers` (Version 3.0.1 GA)**, this crate provides modern, memory-safe, and production-grade Rust interfaces to manage WSL-native micro-VM containers.

---

## Features

- **Strict RAII Lifetime Management**: Automatically closes native C handles (`WslcSession`, `WslcContainer`, `WslcProcess`, `WslcCrashDumpSubscription`) upon drop.
- **Tokio Streaming Process I/O**: Captures stdout/stderr via native C trampolines and feeds them into bounded asynchronous channels.
- **Typed Error Domain**: Maps Windows HRESULTs and wide-string error messages into typed `WslcError` variants.
- **Non-blocking Async Extensions**: Provides `_async` methods offloaded to `tokio::task::spawn_blocking` with COM MTA automatic initialization.
- **Volume and Image Management**: Built-in VHD volume options and image pull/list/delete abstractions.

---

## Installation

Add this crate to your `Cargo.toml`:

```toml
[dependencies]
wslcsdk = "0.1.0"
tokio = { version = "1", features = ["full"] }
```

> **Compatibility**: `wslcsdk 0.1.x` is built on top of `wslcsdk-sys 3.0.1` and requires Windows 10/11 with WSL 2.9.3+ or 3.0.1+.

---

## Usage Example

```rust,no_run
use wslcsdk::{ContainerBuilder, SessionBuilder, WslcSignal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Create session
    let session = SessionBuilder::new("my-session", r"C:\wslc\storage")
        .cpu_count(4)
        .memory_mb(4096)
        .build()?;

    // 2. Build container
    let container = ContainerBuilder::new("docker.io/library/alpine:latest")
        .name("alpine-demo")
        .auto_remove(true)
        .build(&session)?;

    // 3. Start container
    container.start(false)?;
    println!("Container ID: {}", container.id()?);

    // 4. Stop and delete
    container.stop(WslcSignal::Sigterm, 10)?;
    container.delete(false)?;

    Ok(())
}
```

---

## License

This project is licensed under the [MIT License](../../LICENSE).
