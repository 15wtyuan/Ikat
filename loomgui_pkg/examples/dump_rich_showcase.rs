//! 诊断：showcase page_text.html 跑完整管线（含 pkg `desugar_block_divs`），
//! dump 富文本行内图的 sort_key + 相对父 RichText 的位置。
//! `dump_showcase_text`（core example）不跑 desugar，故看不到 RichText/行内图——
//! 本 example 补上 desugar 这一步。
//!
//! 跑：`cargo run -p loomgui_pkg --example dump_rich_showcase`

use loomgui_core::parse::css::parse_css;
use loomgui_core::parse::dom::parse_html;
use loomgui_core::render::node::NodePayload;
use loomgui_core::scene::node::{build_scene, NodeId, NodeKind};
use loomgui_core::stage::Stage;
use loomgui_core::style::cascade::resolve_styles;
use loomgui_core::text::rich::RichKind;

fn main() {
    let manifest = env!("CARGO_MANIFEST_DIR"); // loomgui_pkg
    let html_path = format!(
        "{}/../loomgui_unity/Assets/LoomUI/showcase/page_text.html",
        manifest
    );
    let base_css_path = format!(
        "{}/../loomgui_unity/Assets/LoomUI/showcase/preview/preview-base.css",
        manifest
    );
    let font_path = format!(
        "{}/../loomgui_core/tests/fixtures/wqy-microhei.ttc",
        manifest
    );

    let html = std::fs::read_to_string(&html_path).expect("read html");
    let base_css = std::fs::read_to_string(&base_css_path).expect("read base css");
    let inline_css = extract_style(&html);
    let css = format!("{}\n{}", base_css, inline_css);

    let mut stage = Stage::new((1080.0, 1920.0)).expect("Stage::new");
    stage
        .register_font(
            "wqy-microhei",
            std::fs::read(&font_path).expect("read font"),
            true,
        )
        .unwrap();

    let tree = parse_html(&html).expect("parse_html");
    let sheet = parse_css(&css).expect("parse_css");
    let styles = resolve_styles(&tree, &sheet);
    // 关键：跑 desugar（inline display:block div → rich_runs → RichText）
    let (tree, styles) = loomgui_pkg::desugar_block_divs(tree, styles).expect("desugar");
    stage.tweens.clear();
    stage.prev_node_hashes.clear();
    stage.scene = Some(build_scene(&tree, &styles));

    let frame = stage.tick_and_render();
    let scene = stage.scene.as_ref().expect("scene");

    // RichText 节点 runs 含 Image 的（即 §B B1/B4 等）
    println!("=== scene RichText 节点（含行内图）===");
    let mut rich_with_img = 0;
    for n in scene.nodes.values() {
        if let NodeKind::RichText { runs } = &n.kind {
            let imgs: Vec<String> = runs
                .iter()
                .filter_map(|r| match &r.kind {
                    RichKind::Image { src, w, h, .. } => Some(format!("{}({}x{})", src, w, h)),
                    _ => None,
                })
                .collect();
            if !imgs.is_empty() {
                rich_with_img += 1;
                println!(
                    "  id_attr={:?} layout=({:.0},{:.0},{:.0},{:.0}) images=[{}]",
                    n.id_attr,
                    n.layout_rect.x,
                    n.layout_rect.y,
                    n.layout_rect.w,
                    n.layout_rect.h,
                    imgs.join(", "),
                );
            }
        }
    }
    println!("含行内图的 RichText 节点数：{}", rich_with_img);

    // 行内图 RenderNode：相对父 RichText layout 的位置
    println!("\n=== 行内图 RenderNode（相对父 layout 原点）===");
    for rn in &frame.nodes {
        let NodePayload::Mesh {
            verts, image_path, ..
        } = &rn.payload;
        let Some(p) = image_path.as_deref() else {
            continue;
        };
        if p.starts_with("loomgui://font-atlas") {
            continue;
        }
        let ys: Vec<f32> = verts.iter().map(|v| v[1]).collect();
        let xs: Vec<f32> = verts.iter().map(|v| v[0]).collect();
        let ymin = ys.iter().copied().fold(f32::MAX, f32::min);
        let ymax = ys.iter().copied().fold(f32::MIN, f32::max);
        let xmin = xs.iter().copied().fold(f32::MAX, f32::min);
        let xmax = xs.iter().copied().fold(f32::MIN, f32::max);
        let primary = rn.node_id & 0x00FF_FFFF;
        let parent = scene.get(NodeId(primary));
        let (px, py) = parent
            .map(|n| (n.layout_rect.x, n.layout_rect.y))
            .unwrap_or((0.0, 0.0));
        println!(
            "  node_id={} sort_key={} path={} img_w={:.0} img_h={:.0} rel_x={:.1} rel_top={:.1} rel_bottom={:.1}",
            rn.node_id,
            rn.sort_key,
            p,
            xmax - xmin,
            ymax - ymin,
            xmin - px,
            ymin - py,
            ymax - py,
        );
    }
}

fn extract_style(html: &str) -> String {
    if let Some(start) = html.find("<style>") {
        if let Some(end) = html[start..].find("</style>") {
            return html[start + 7..start + end].to_string();
        }
    }
    String::new()
}
