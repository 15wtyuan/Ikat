fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    // 生成到 OUT_DIR（Rust 测试/编译用）。正式分发到引擎后端由 xtask 管:
    //   cargo run -p xtask -- sync-bindings
    // csbindgen 只扫显式列入的文件——lib.rs 按职责拆成子模块后，每个含
    // #[no_mangle] extern fn 的模块文件都必须 input_extern_file 进链，否则绑定
    // 静默漏生成。此清单与 crates/xtask/src/bindings.rs 的互为镜像，改清单两处同步。
    csbindgen::Builder::default()
        .input_extern_file("src/lib.rs")
        .input_extern_file("src/stage.rs")
        .input_extern_file("src/host.rs")
        .input_extern_file("src/frame.rs")
        .input_extern_file("src/events.rs")
        .input_extern_file("src/node_getters.rs")
        .input_extern_file("src/node_setters.rs")
        .input_extern_file("src/controls.rs")
        .input_extern_file("src/text.rs")
        .input_extern_file("src/scroll.rs")
        .input_extern_file("src/animation.rs")
        .input_extern_file("src/list.rs")
        .input_extern_file("src/resources.rs")
        .input_extern_file("src/style_sheet.rs")
        .csharp_dll_name("yio_ffi_c")
        .csharp_namespace("Yio.Bindings")
        .csharp_class_name("Native")
        .csharp_use_function_pointer(false)
        .generate_csharp_file(format!("{}/YioBindings.cs", out_dir))
        .expect("csbindgen csharp gen");
}
