//! csbindgen 绑定生成 + 分发。
//! 路径推算: 从 xtask 的 CARGO_MANIFEST_DIR (= crates/xtask) 出发。
//! ffi 源 = ../ffi/src/lib.rs; Unity 目标 = ../../unity/package/Plugins/LoomGUI/Bindings/

use std::path::PathBuf;

fn ffi_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("ffi")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

pub fn sync_bindings() -> Result<(), Box<dyn std::error::Error>> {
    let ffi_lib = ffi_dir().join("src").join("lib.rs");
    if !ffi_lib.exists() {
        return Err(format!("ffi lib.rs not found at {}", ffi_lib.display()).into());
    }

    let unity_target = repo_root()
        .join("unity")
        .join("package")
        .join("Plugins")
        .join("LoomGUI")
        .join("Bindings")
        .join("LoomGUIBindings.cs");

    if let Some(parent) = unity_target.parent() {
        std::fs::create_dir_all(parent)?;
    }

    csbindgen::Builder::default()
        .input_extern_file(&ffi_lib)
        .csharp_dll_name("loomgui_ffi_c")
        .csharp_namespace("LoomGUI.Bindings")
        .csharp_class_name("Native")
        .csharp_use_function_pointer(false)
        .generate_csharp_file(&unity_target)?;

    println!("sync-bindings: Unity -> {}", unity_target.display());

    // TODO(future): cbindgen -> loomgui.h for Godot/Unreal backends

    Ok(())
}
