//! 诊断：border 位置偏上取证。加载 showcase.pkg.bin → instantiate page_image →
//! tick_and_render，两路取证：(1) scene 节点找 border_color 接近 #3a3f55 的 .sec-h
//! 候选，打印 ts.border/ts.padding/layout_rect 验 padding-bottom longhand 是否被忽略；
//! (2) frame mesh 打印纯色 mesh box/tint 确认 border mesh 产出。
//! 跑：`cargo run -p loomgui_pkg --example dump_border_pos`

use loomgui_core::render::node::NodePayload;
use loomgui_core::stage::Stage;
use loomgui_pkg::atlas::AtlasManifest;

fn box_of(verts: &[[f32; 2]]) -> (f32, f32, f32, f32) {
    let xmin = verts.iter().map(|v| v[0]).fold(f32::MAX, f32::min);
    let xmax = verts.iter().map(|v| v[0]).fold(f32::MIN, f32::max);
    let ymin = verts.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
    let ymax = verts.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
    (xmin, ymin, xmax - xmin, ymax - ymin)
}

fn main() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let pkg_path = format!(
        "{}/../../../unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin",
        manifest
    );
    let atlas_path = format!(
        "{}/../../../unity/showcase-unity/Assets/Bundles/atlas/icons.atlas.json",
        manifest
    );
    let font_path = format!(
        "{}/../../../showcase_project/res/fonts/LXGWWenKai.ttf",
        manifest
    );

    let pkg_bytes = std::fs::read(&pkg_path).expect("read showcase.pkg.bin");
    let atlas_json = std::fs::read_to_string(&atlas_path).expect("read icons.atlas.json");

    let mut stage = Stage::new((1080.0, 1920.0)).expect("Stage::new");
    stage
        .register_font(
            "LXGWWenKai",
            std::fs::read(&font_path).expect("read font"),
            true,
        )
        .expect("register_font");

    let root = stage.create_root("div", "").expect("create_root");
    stage
        .load_package("showcase", &pkg_bytes)
        .expect("load_package");
    let atlas: AtlasManifest = serde_json::from_str(&atlas_json).expect("parse AtlasManifest");
    let sizes: Vec<(String, u32, u32)> = atlas
        .sprites
        .iter()
        .map(|(k, e)| (k.clone(), e.orig[0], e.orig[1]))
        .collect();
    stage.set_image_sizes(&sizes);
    let comp = stage
        .instantiate("showcase", "page_image")
        .expect("instantiate page_image");
    stage.append_child(root, comp).expect("append_child");
    let frame = stage.tick_and_render();

    println!("=== scene 节点：border_color 接近 #3a3f55（.sec-h 候选）===");
    if let Some(scene) = &stage.scene {
        for n in scene.nodes.values() {
            let Some(c) = n.style.border_color else {
                continue;
            };
            // #3a3f55 = (58,63,85)/255 ≈ (0.227, 0.247, 0.333)
            if (c[0] - 0.227).abs() > 0.02
                || (c[1] - 0.247).abs() > 0.02
                || (c[2] - 0.333).abs() > 0.02
            {
                continue;
            }
            let ts = &n.style.taffy_style;
            let lr = n.layout_rect;
            println!(
                "  node={:?} kind={:?} layout_rect=({:.0},{:.0},{:.0},{:.0}) border_color=({:.3},{:.3},{:.3})",
                n.id, n.kind, lr.x, lr.y, lr.w, lr.h, c[0], c[1], c[2]
            );
            println!("    ts.border  = {:?}", ts.border);
            println!("    ts.padding = {:?}", ts.padding);
        }
    } else {
        println!("  (scene 为空)");
    }

    println!("=== frame 纯色 mesh（border 候选 = 无图且细条；其余纯色背景对照）===");
    for rn in &frame.nodes {
        let NodePayload::Mesh {
            verts,
            image_path,
            colors,
            program,
            ..
        } = &rn.payload;
        if image_path.is_some() || *program != 0 {
            continue;
        }
        let (x, y, w, h) = box_of(verts);
        let tint = colors.first().copied().unwrap_or([0.0; 4]);
        // 检查是否含 border 色 #3a3f55（.sec-h / .card border ring 在 colors[4..]）
        let has_border = colors.iter().any(|c| {
            (c[0] - 0.227).abs() < 0.02
                && (c[1] - 0.247).abs() < 0.02
                && (c[2] - 0.333).abs() < 0.02
        });
        let tag = if has_border {
            "BORDER!"
        } else if h <= 4.0 || w <= 4.0 {
            "BORDER?"
        } else {
            "bg"
        };
        println!(
            "  [{tag}] node={:5} verts={} box=({x:.0},{y:.0},{w:.0},{h:.0}) tint=({:.3},{:.3},{:.3},{:.3})",
            rn.node_id, verts.len(), tint[0], tint[1], tint[2], tint[3],
        );
    }
}
