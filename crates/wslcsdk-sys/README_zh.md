# wslcsdk-sys

<p align="center">
  <a href="README.md">English</a> | <b>简体中文</b>
</p>

微软官方 Microsoft.WSL.Containers (WSLC) SDK 的原始低级别 FFI 绑定（裸 C-ABI 映射）。

基于微软官方发布的 **`Microsoft.WSL.Containers` (版本 3.0.1 GA)**。

---

## 概述

本 crate 提供对 `wslcsdk.dll` 中导出的全部 63 个 C 函数签名以及底层数据结构、枚举与常量的 1:1 映射：

- **动态加载支持**：针对 MSVC 工具链内置 `/DELAYLOAD:wslcsdk.dll` 与 `delayimp` 配置；
- **跨架构适配**：内置 x64 与 arm64 架构预编译导入库（`.lib`）与动态库（`.dll`）；
- **全量 API 契约**：完整 C-ABI 契约可参阅 [API_REGISTRY.md](API_REGISTRY.md)。

---

## 使用建议

普通应用程序或服务开发，**强烈建议使用高级安全封装库 [`wslcsdk`](https://crates.io/crates/wslcsdk)**，该库提供了 RAII 句柄管理、强类型错误以及非阻塞异步操作。

如果你确需直接使用裸 FFI 接口：

```toml
[dependencies]
wslcsdk-sys = "3.0.1"
```

---

## 开源协议

本项目采用 [MIT License](../../LICENSE) 许可证。
