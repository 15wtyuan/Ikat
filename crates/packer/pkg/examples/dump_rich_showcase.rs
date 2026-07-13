//! 诊断：showcase page_text.html 跑完整管线（含 pkg `desugar_block_divs`），
//! dump 富文本 text_layout（行数/行宽）vs content box（验证换行 + 对齐基准）。
//! `dump_showcase_text`（core example）不跑 desugar，故看不到 RichText——本 example
//! 补 desugar。跑：`cargo run -p loomgui_pkg --example dump_rich_showcase`

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
        "{}/../../../loomgui_unity/Assets/LoomUI/showcase/page_text.html",
        manifest
    );
    let base_css_path = format!(
        "{}/../../../loomgui_unity/Assets/LoomUI/showcase/preview/preview-base.css",
        manifest
    );
    let font_path = format!(
        "{}/../../core/tests/fixtures/wqy-microhei.ttc",
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
    let (tree, styles) = loomgui_pkg::desugar_block_divs(tree, styles).expect("desugar");
    stage.tweens.clear();
    stage.prev_node_hashes.clear();
    stage.scene = Some(build_scene(&tree, &styles));

    let frame = stage.tick_and_render();
    let scene = stage.scene.as_ref().expect("scene");

    println!("=== 超宽节点（box_w > 1080，按 y 排序）===");
    {
        let mut wide: Vec<_> = scene
            .nodes
            .values()
            .filter(|n| n.layout_rect.w > 1080.0)
            .collect();
        wide.sort_by(|a, b| {
            a.layout_rect
                .y
                .partial_cmp(&b.layout_rect.y)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for n in &wide {
            println!(
                "  id={:?} box=({:.0},{:.0},{:.0},{:.0}) flex_shrink={} align_self={:?} flex_dir={:?} overflow=({:?},{:?})",
                n.id_attr,
                n.layout_rect.x,
                n.layout_rect.y,
                n.layout_rect.w,
                n.layout_rect.h,
                n.style.taffy_style.flex_shrink,
                n.style.taffy_style.align_self,
                n.style.taffy_style.flex_direction,
                n.style.overflow_x,
                n.style.overflow_y,
            );
        }
    }

    println!("=== RichText: text_layout vs box ===");
    for n in scene.nodes.values() {
        let NodeKind::RichText { runs } = &n.kind else {
            continue;
        };
        let txt_chars: usize = runs
            .iter()
            .map(|r| match &r.kind {
                RichKind::Text { text } => text.chars().count(),
                _ => 0,
            })
            .sum();
        let layout = scene
            .text_layouts
            .get(n.id.index())
            .and_then(|o| o.as_ref());
        let (n_lines, widest) = layout
            .map(|l| (l.lines.len(), l.text_width))
            .unwrap_or((0, 0.0));
        let snippet: String = runs
            .iter()
            .filter_map(|r| match &r.kind {
                RichKind::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
            .chars()
            .take(14)
            .collect();
        println!(
            "  id={:?} box_w={:.0} chars={} lines={} widest={:.0} flex_shrink={} flex_grow={} align={:?} snip={:?}",
            n.id_attr,
            n.layout_rect.w,
            txt_chars,
            n_lines,
            widest,
            n.style.taffy_style.flex_shrink,
            n.style.taffy_style.flex_grow,
            n.style.text_align,
            snippet,
        );
    }

    // 渲染后 text mesh x 范围（program=1），看是否超 box right
    println!("\n=== RichText 渲染 x 范围（program=1，相对 box）===");
    for rn in &frame.nodes {
        let NodePayload::Mesh { verts, program, .. } = &rn.payload;
        if *program != 1 {
            continue;
        }
        let primary = rn.node_id & 0x00FF_FFFF;
        let Some(parent) = scene.get(NodeId(primary)) else {
            continue;
        };
        if !matches!(parent.kind, NodeKind::RichText { .. }) {
            continue;
        }
        let xmin = verts.iter().map(|v| v[0]).fold(f32::MAX, f32::min);
        let xmax = verts.iter().map(|v| v[0]).fold(f32::MIN, f32::max);
        let box_right = parent.layout_rect.x + parent.layout_rect.w;
        let over = xmax - box_right;
        println!(
            "  snip box_x={:.0} box_right={:.0} text_x=[{:.1},{:.1}] over_box={:.1}",
            parent.layout_rect.x, box_right, xmin, xmax, over,
        );
    }

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
        let py = scene
            .get(NodeId(primary))
            .map(|n| n.layout_rect.y)
            .unwrap_or(0.0);
        println!(
            "  path={} sort_key={} w={:.0} h={:.0} rel_top={:.1} rel_bottom={:.1}",
            p,
            rn.sort_key,
            xmax - xmin,
            ymax - ymin,
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
