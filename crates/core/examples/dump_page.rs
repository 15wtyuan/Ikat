//! 诊断：实例化 showcase 任意页面，dump 全节点 layout_rect + display(flex/block/none)
//! + touchable，定位「Unity 布局与 HTML 不一致」时 bug 在 core/packager 还是 Unity 后端。
//!
//! 用法：cargo run -p loomgui_core --example dump_page -- <page-name>
//!   page-name ∈ home/settings/inventory/mail/shop/character/form/lab
//!
//! 重点取证：.root / .sidebar / .main 等关键容器的 display_mode 与 layout_rect，
//! 对照 HTML 预期（settings 应左右布局、home 应纵向 flex 等）。

use loomgui_core::scene::dynamic::append_child;
use loomgui_core::scene::node::{Node, NodeId, NodeKind};
use loomgui_core::stage::Stage;

fn main() {
    let page = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "home".to_string());

    let root = env!("CARGO_MANIFEST_DIR");
    let pkg_path = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin",
        root
    );
    let font_default = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/fonts/LXGWWenKai.ttf.bytes",
        root
    );
    let font_fallback = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/fonts/wqy-microhei.ttc.bytes",
        root
    );

    let pkg = std::fs::read(&pkg_path).expect("read pkg");
    let mut s = Stage::new((1920.0, 1080.0)).expect("Stage::new");
    s.register_font("LXGWWenKai", std::fs::read(&font_default).unwrap(), true)
        .unwrap();
    s.register_font(
        "wqy-microhei",
        std::fs::read(&font_fallback).unwrap(),
        false,
    )
    .unwrap();
    s.set_fallback_families(&["wqy-microhei".to_string()]);
    s.set_image_sizes(&icon_sizes());
    s.load_package("showcase", &pkg).expect("load_package");
    let root_id = s.create_root("div", "").expect("create_root");
    let inst = s
        .instantiate("showcase", &page)
        .unwrap_or_else(|e| panic!("instantiate {page}: {e}"));
    {
        let scene = s.scene.as_mut().unwrap();
        append_child(scene, root_id, inst).expect("append_child");
    }
    let _frame0 = s.tick_and_render(); // warm-up: 控件 sync 写 inline_override，rematch 下帧才进 style
    let frame = s.tick_and_render();
    let scene = s.scene.as_ref().unwrap();

    println!(
        "========== page={page} stage=1920x1080 nodes={} ==========",
        scene.nodes.len()
    );
    println!(
        "{:<5} {:<11} {:<16} {:<22} {:>8} {:>8} {:>8} {:>8}  {:<5} touch  base  eff",
        "nid", "kind", "id", "class", "x", "y", "w", "h", "disp"
    );
    // 收集后排序：按 y 再按 x，方便肉眼对照视觉布局
    let mut rows: Vec<&Node> = scene.nodes.values().collect();
    rows.sort_by(|a, b| {
        a.layout_rect
            .y
            .partial_cmp(&b.layout_rect.y)
            .unwrap()
            .then(a.layout_rect.x.partial_cmp(&b.layout_rect.x).unwrap())
    });
    for n in rows {
        print_row(n);
    }

    println!("\n── 渲染节点 world_matrix vs layout_rect（遍历 frame.nodes，查 blob 不匹配）──");
    // build node_id(index) → layout_rect 索引（scene 侧）
    use std::collections::HashMap;
    let mut lr: HashMap<u32, (f32, f32, f32, f32)> = HashMap::new();
    for n in scene.nodes.values() {
        lr.insert(
            n.id.0,
            (
                n.layout_rect.x,
                n.layout_rect.y,
                n.layout_rect.w,
                n.layout_rect.h,
            ),
        );
    }
    let mut mismatch = 0;
    let mut checked = 0;
    let mut samples = Vec::new();
    let mut orphan = 0;
    let mut matches: HashMap<u32, (f32, f32)> = HashMap::new();
    let wt_len = scene.world_transforms.len();
    println!(
        "  [diag] world_transforms.len()={} nodes.capacity()={}",
        wt_len,
        scene.nodes.capacity()
    );
    // 重复 node_id 检测：同一 node_id 出现多次 → 后者覆盖前者 world_matrix？
    let mut id_count: HashMap<u32, usize> = HashMap::new();
    let mut dup_ids = 0;
    for rn in &frame.nodes {
        let c = id_count.entry(rn.node_id).or_insert(0);
        *c += 1;
        if *c == 2 {
            dup_ids += 1;
        }
    }
    println!(
        "  [diag] frame.nodes={} unique_node_ids={} 重复 node_id 的 {} 个",
        frame.nodes.len(),
        id_count.len(),
        dup_ids
    );
    for rn in &frame.nodes {
        let (tx, ty) = (rn.world_matrix[4], rn.world_matrix[5]);
        if let Some(&(rx, ry, rw, rh)) = lr.get(&rn.node_id) {
            checked += 1;
            let dx = (tx - rx).abs();
            let dy = (ty - ry).abs();
            if dx > 0.5 || dy > 0.5 {
                mismatch += 1;
                // 沿 parent 链回溯，看是否可达 root（孤儿？）
                let nid = NodeId(rn.node_id);
                let mut cur = nid;
                let mut depth = 0;
                let mut reaches_root = false;
                while let Some(n) = scene.get(cur) {
                    match n.parent {
                        Some(p) => {
                            cur = p;
                            depth += 1;
                            if depth > 999 {
                                break;
                            }
                        }
                        None => {
                            reaches_root = scene.roots.contains(&cur);
                            break;
                        }
                    }
                }
                if !reaches_root {
                    orphan += 1;
                }
                if samples.len() < 6 {
                    let idx = nid.index();
                    let wt_at = scene.world_transforms.get(idx);
                    let wm_full = rn.world_matrix;
                    samples.push(format!("nid={} idx={} wm_full=[{:.0},{:.0},{:.0},{:.0},{:.0},{:.0}] wt[idx]={:?} rect=({:.0},{:.0},{:.0},{:.0}) depth={}", rn.node_id, idx, wm_full[0], wm_full[1], wm_full[2], wm_full[3], wm_full[4], wm_full[5], wt_at, rx, ry, rw, rh, depth));
                }
                matches.insert(rn.node_id, (rx, ry));
            }
        }
    }
    println!(
        "  frame 渲染节点 {} 个，检查 {} 个，world_matrix≠layout_rect 的 {} 个（其中孤儿 {} 个）",
        frame.nodes.len(),
        checked,
        mismatch,
        orphan
    );
    for s in &samples {
        println!("    ⚠ {}", s);
    }
    println!(
        "  → {}（0=blob 正确，bug 在 Unity 后端；>0=bug 在 core compute_world_transforms）",
        if mismatch == 0 {
            "blob 正确"
        } else {
            "blob 有误（需查顶点：merged 节点 wm=IDENTITY 但顶点可能已是绝对坐标）"
        }
    );
    // 验证：不匹配节点的 mesh 顶点是否已是绝对坐标（merged by-design）
    println!("  ── 不匹配节点 mesh 顶点采样（验证是否绝对坐标）──");
    for rn in &frame.nodes {
        if matches.contains_key(&rn.node_id)
            && rn.world_matrix[4].abs() < 0.5
            && rn.world_matrix[5].abs() < 0.5
        {
            let loomgui_core::render::node::NodePayload::Mesh { verts, .. } = &rn.payload;
            if !verts.is_empty() {
                let v0 = verts[0];
                let vmin = verts
                    .iter()
                    .fold([f32::MAX; 2], |a, v| [a[0].min(v[0]), a[1].min(v[1])]);
                let vmax = verts
                    .iter()
                    .fold([f32::MIN; 2], |a, v| [a[0].max(v[0]), a[1].max(v[1])]);
                println!("    nid={} wm=({:.0},{:.0}) verts={} 首顶=({:.0},{:.0}) bbox=({:.0},{:.0}~{:.0},{:.0})", rn.node_id, rn.world_matrix[4], rn.world_matrix[5], verts.len(), v0[0], v0[1], vmin[0], vmin[1], vmax[0], vmax[1]);
            }
        }
    }
}

fn print_row(n: &Node) {
    let r = n.layout_rect;
    let id = n.id_attr.clone().unwrap_or_default();
    let class = n.classes.join(",");
    let bg_base = match n.base_style.background_color {
        Some([r_, g, b, a]) => format!("rgba({:.2},{:.2},{:.2},{:.2})", r_, g, b, a),
        None => "-".to_string(),
    };
    let bg_eff = match n.style.background_color {
        Some([r_, g, b, a]) => format!("rgba({:.2},{:.2},{:.2},{:.2})", r_, g, b, a),
        None => "-".to_string(),
    };
    println!(
        "{:<5} {:<11} {:<16} {:<22} {:>8.1} {:>8.1} {:>8.1} {:>8.1}  {:<5} {}  base={} eff={}",
        n.id.index(),
        kind_str(n.kind),
        id,
        class,
        r.x,
        r.y,
        r.w,
        r.h,
        disp_str(n),
        n.interaction.touchable,
        bg_base,
        bg_eff,
    );
}

fn disp_str(n: &Node) -> &'static str {
    match n.style.taffy_style.display {
        taffy::style::Display::None => "none",
        taffy::style::Display::Flex => "flex",
        taffy::style::Display::Block => "block",
        taffy::style::Display::Grid => "grid",
    }
}

fn kind_str(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Container => "div",
        NodeKind::Button => "btn",
        NodeKind::Image => "img",
        NodeKind::TextNode => "#text",
        NodeKind::TextElement => "span",
        _ => "?",
    }
}

fn icon_sizes() -> Vec<(String, u32, u32)> {
    [
        "logo",
        "settings",
        "inventory",
        "mail",
        "shop",
        "character",
        "form",
        "lab",
        "item-chest",
        "weapon",
        "item-gem",
        "skill-eye",
        "item-scroll",
        "item-staff",
    ]
    .iter()
    .map(|n| (format!("res/icons/{}.png", n), 128, 128))
    .collect()
}
