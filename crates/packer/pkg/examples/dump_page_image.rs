//! 诊断：完整模拟 Unity 启动流程加载 showcase.pkg.bin → set_image_sizes →
//! instantiate page_image → tick_and_render，dump 所有 bg-image Mesh 的
//! image_path / program / verts / box，验证 core 是否正确产出 bg-image 渲染节点。
//! 区分「core/打包期没产 image_path」vs「Unity 侧 SpriteResolver/texture 断了」。
//! 跑：`cargo run -p yio_pkg --example dump_page_image`

use yio_core::render::node::NodePayload;
use yio_core::stage::Stage;
use yio_pkg::atlas::AtlasManifest;

fn main() {
    let manifest = env!("CARGO_MANIFEST_DIR"); // yio_pkg
    let pkg_path = format!(
        "{}/../../../unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin",
        manifest
    );
    let atlas_path = format!(
        "{}/../../../unity/showcase-unity/Assets/Bundles/atlas/icons.atlas.json",
        manifest
    );

    let pkg_bytes = std::fs::read(&pkg_path).expect("read showcase.pkg.bin");
    let atlas_json = std::fs::read_to_string(&atlas_path).expect("read icons.atlas.json");
    let font_path = format!(
        "{}/../../../showcase_project/res/fonts/LXGWWenKai.ttf",
        manifest
    );

    let mut stage = Stage::new((1080.0, 1920.0)).expect("Stage::new");
    stage
        .register_font(
            "LXGWWenKai",
            std::fs::read(&font_path).expect("read font"),
            true,
        )
        .expect("register_font");

    // 0. 建 root scene（instantiate 前置；模拟 Unity CreateRoot）
    let root = stage.create_root("div", "").expect("create_root");

    // 1. 加载包（模拟 YioStageDriver.LoadPackage）
    stage
        .load_package("showcase", &pkg_bytes)
        .expect("load_package showcase");

    // 2. 从 atlas.json 提取 sprite sizes 灌入 core（模拟 set_image_sizes）
    let atlas: AtlasManifest = serde_json::from_str(&atlas_json).expect("parse AtlasManifest");
    let sizes: Vec<(String, u32, u32)> = atlas
        .sprites
        .iter()
        .map(|(k, e)| (k.clone(), e.orig[0], e.orig[1]))
        .collect();
    println!("=== set_image_sizes: {} sprites（前 5）===", sizes.len());
    for (k, w, h) in sizes.iter().take(5) {
        println!("  {} = {}x{}", k, w, h);
    }
    stage.set_image_sizes(&sizes);

    // 3. 实例化 page_image 组件（组件名 = html stem）挂到 root 下
    let comp = stage
        .instantiate("showcase", "page_image")
        .expect("instantiate page_image");
    stage.append_child(root, comp).expect("append_child");
    println!("=== instantiated page_image, comp NodeId={:?} ===", comp);

    // 4. 渲染
    let frame = stage.tick_and_render();
    println!("=== frame.nodes 总数: {} ===", frame.nodes.len());

    // 5. 打印所有带 image_path 的 Mesh（bg-image 或 <img>）
    println!("=== Mesh 带 image_path（非 font-atlas）===");
    let mut img_count = 0;
    let mut bg_count = 0;
    for rn in &frame.nodes {
        let NodePayload::Mesh {
            verts,
            image_path,
            program,
            colors,
            ..
        } = &rn.payload;
        let Some(p) = image_path.as_deref() else {
            continue;
        };
        if p.starts_with("yio://font-atlas") {
            continue;
        }
        img_count += 1;
        if *program == 2 || *program == 4 {
            bg_count += 1;
        }
        let xmin = verts.iter().map(|v| v[0]).fold(f32::MAX, f32::min);
        let xmax = verts.iter().map(|v| v[0]).fold(f32::MIN, f32::max);
        let ymin = verts.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
        let ymax = verts.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
        let tint = colors.first().copied().unwrap_or([0.0; 4]);
        println!(
            "  path={} program={} verts={} box=({:.0},{:.0},{:.0},{:.0}) tint=[{},{},{},{}]",
            p,
            program,
            verts.len(),
            xmin,
            ymin,
            xmax - xmin,
            ymax - ymin,
            tint[0],
            tint[1],
            tint[2],
            tint[3],
        );
    }
    println!(
        "=== 合计：image Mesh {}（其中 bg-image program=2/4：{}）===",
        img_count, bg_count
    );

    if img_count == 0 {
        println!("!! 警告：core 没产出任何 image Mesh —— bg-image 在 core/打包期就丢了");
    }
}
