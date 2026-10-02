use std::env;
use std::path::PathBuf;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let arch_dir = match target_arch.as_str() {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => panic!("WSLC SDK 不支持的目标架构: {other}，仅支持 x86_64 与 aarch64"),
    };

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let lib_dir = manifest_dir.join("native").join("lib").join(arch_dir);

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=wslcsdk");

    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "msvc" {
        println!("cargo:rustc-link-arg=/DELAYLOAD:wslcsdk.dll");
        println!("cargo:rustc-link-lib=delayimp");
    }

    let dll_source = lib_dir.join("wslcsdk.dll");
    if dll_source.exists() {
        let Ok(out_dir) = env::var("OUT_DIR") else {
            return;
        };
        let out_path = PathBuf::from(out_dir);
        let copy_dll = |dest: PathBuf| {
            if let Err(e) = std::fs::copy(&dll_source, &dest) {
                println!(
                    "cargo:warning=复制 DLL 失败 (来源: {}, 目标: {}): {e}",
                    dll_source.display(),
                    dest.display()
                );
            }
        };

        copy_dll(out_path.join("wslcsdk.dll"));

        if let Some(target_profile_dir) = out_path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
        {
            copy_dll(target_profile_dir.join("wslcsdk.dll"));
            let deps_dir = target_profile_dir.join("deps");
            if deps_dir.exists() {
                copy_dll(deps_dir.join("wslcsdk.dll"));
            }
        }
    }

    println!("cargo:rerun-if-changed=native/lib/{arch_dir}/wslcsdk.lib");
    println!("cargo:rerun-if-changed=native/lib/{arch_dir}/wslcsdk.dll");
    println!("cargo:rerun-if-changed=native/include/wslcsdk.h");
}
