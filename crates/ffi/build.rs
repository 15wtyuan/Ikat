fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    // 生成到 OUT_DIR（Rust 测试/编译用）。正式分发到引擎后端由 xtask 管:
    //   cargo run -p xtask -- sync-bindings
    csbindgen::Builder::default()
        .input_extern_file("src/lib.rs")
        .csharp_dll_name("loomgui_ffi_c")
        .csharp_namespace("LoomGUI.Bindings")
        .csharp_class_name("Native")
        .csharp_use_function_pointer(false)
        .generate_csharp_file(format!("{}/LoomGUIBindings.cs", out_dir))
        .expect("csbindgen csharp gen");
}
