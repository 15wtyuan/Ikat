//! csbindgen 绑定生成 + 分发。
//! 路径推算: 从 xtask 的 CARGO_MANIFEST_DIR (= crates/xtask) 出发。
//! ffi 源 = ../ffi/src/ 下 lib.rs + 各含 extern fn 的子模块; Unity 目标 = ../../unity/package/Plugins/LoomGUI/Bindings/

use crate::paths;
use std::path::PathBuf;

fn ffi_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("ffi")
}

pub fn sync_bindings() -> Result<(), Box<dyn std::error::Error>> {
    let ffi_src = ffi_dir().join("src");
    // csbindgen 只扫显式列入的文件——与 crates/ffi/build.rs 的清单互为镜像，
    // 改清单两处同步（新增含 extern fn 的模块须同时补进两边，否则绑定静默漏生成）。
    let ffi_modules = [
        "lib.rs",
        "stage.rs",
        "frame.rs",
        "events.rs",
        "node_getters.rs",
        "node_setters.rs",
        "controls.rs",
        "text.rs",
        "scroll.rs",
        "animation.rs",
        "list.rs",
        "resources.rs",
    ];
    for m in &ffi_modules {
        let p = ffi_src.join(m);
        if !p.exists() {
            return Err(format!("ffi source file not found at {}", p.display()).into());
        }
    }

    let unity_target = paths::repo_root()
        .join("unity")
        .join("package")
        .join("Plugins")
        .join("LoomGUI")
        .join("Bindings")
        .join("LoomGUIBindings.cs");

    if let Some(parent) = unity_target.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut builder = csbindgen::Builder::default();
    for m in &ffi_modules {
        builder = builder.input_extern_file(ffi_src.join(m));
    }
    builder
        .csharp_dll_name("loomgui_ffi_c")
        .csharp_namespace("LoomGUI.Bindings")
        .csharp_class_name("Native")
        .csharp_use_function_pointer(false)
        .generate_csharp_file(&unity_target)?;

    println!("sync-bindings: Unity -> {}", unity_target.display());

    // TODO(future): cbindgen -> loomgui.h for Godot/Unreal backends

    Ok(())
}
