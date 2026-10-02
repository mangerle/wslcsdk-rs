# wslcsdk

<p align="center">
  <b>English</b> | <a href="README_zh.md">简体中文</a>
</p>

Idiomatic, safe, and asynchronous Rust SDK for Microsoft WSL Containers (WSLC).

Built against Microsoft's official **`Microsoft.WSL.Containers` (Version 3.0.1 GA)**, this crate provides modern, memory-safe, and production-grade Rust interfaces to manage WSL-native micro-VM containers.

---

## Features

- **Strict RAII Lifetime Management**: Automatically closes native C handles (`WslcSession`, `WslcContainer`, `WslcProcess`, `WslcCrashDumpSubscription`) upon drop.
- **Streaming Process I/O**: Captures stdout/stderr via native C trampolines and feeds them into bounded asynchronous channels. The receivers it exposes are runtime-agnostic, so no third-party async runtime type leaks into the public API.
- **Two-Layer Typed Error Domain**: `WslcDomainError` mirrors the 19 named HRESULTs of the official header one-to-one, while infrastructure failures stay in the outer `WslcError`. Unrecognized standard HRESULTs are reported by name instead of a misleading generic message.
- **Non-blocking Async Extensions**: Provides `_async` methods offloaded to `tokio::task::spawn_blocking` with COM MTA automatic initialization.
- **Volume and Image Management**: Built-in VHD volume options and image pull/list/delete abstractions.

---

## Installation

Add this crate to your `Cargo.toml`:

```toml
[dependencies]
wslcsdk = "0.2.0"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

> **Compatibility**: `wslcsdk 0.2.x` is built on top of `wslcsdk-sys 3.0.1` and requires Windows 11 with WSL 2.9.3+ or 3.0.1+.

---

## Usage Example

```rust,no_run
use wslcsdk::{WslcClient, WslcSignal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize unified client (auto environment check, default session, COM MTA lifecycle)
    let client = WslcClient::builder().session_name("my-session").build()?;

    // 2. The builder is already bound to the client's session, so build() takes no argument
    let container = client
        .create_container("docker.io/library/alpine:latest")
        .name("alpine-demo")
        .auto_remove(true)
        .build()?;

    // 3. Start container
    container.start(false)?;

    // 4. Stop and delete
    container.stop(WslcSignal::Sigterm, 10)?;
    container.delete(false)?;

    Ok(())
}
```

---

## License

This project is licensed under the [MIT License](../../LICENSE).
