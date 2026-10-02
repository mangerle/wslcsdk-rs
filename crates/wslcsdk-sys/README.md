# wslcsdk-sys

<p align="center">
  <b>English</b> | <a href="README_zh.md">简体中文</a>
</p>

Raw FFI bindings for the official Microsoft WSL Containers (WSLC) SDK.

Built against Microsoft's official **`Microsoft.WSL.Containers` (Version 3.0.1 GA)**.

---

## Overview

This crate provides 1:1 raw C-ABI declarations for all 63 exported C functions, structures, enums, and constants defined in `wslcsdk.h`:

- **Dynamic Loading Support**: Pre-configured with MSVC `/DELAYLOAD:wslcsdk.dll` and `delayimp` linking.
- **Cross-Architecture**: Bundles precompiled import libraries (`.lib`) and dynamic libraries (`.dll`) for both `x64` and `arm64`.
- **Full API Registry**: Consult [API_REGISTRY.md](API_REGISTRY.md) for the complete list of 63 mapped C functions and contracts.

---

## Recommended Usage

For production application development, **it is strongly recommended to use the safe wrapper crate [`wslcsdk`](https://crates.io/crates/wslcsdk)**, which provides automatic RAII cleanup, strongly typed error models, and asynchronous I/O support.

If you strictly require direct low-level FFI access:

```toml
[dependencies]
wslcsdk-sys = "3.0.1"
```

---

## License

This project is licensed under the [MIT License](../../LICENSE).
