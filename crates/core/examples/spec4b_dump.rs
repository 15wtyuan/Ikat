//! 诊断：编码机复现 Unity PlayMode 的 core solve，定位视觉 bug 根因层。
//!
//! dump 所有节点 layout_rect + img css size + text metrics，对照用户报告的 5 个视觉问题：
//!   问题1: header 没顶最上面 + 间距拉大（card-1 Unity 报 Y=317，期望 ~118）
//!   问题2: tofu 问号字符（font fallback / glyph）
//!   问题4: 卡片图片左右压扁（img css.w/h vs layout_rect.w/h）
//!   问题5a: Buy 字没居中（button text text_align）
//! 若编码机复现 card-1 Y=317 → core layout bug；若 Y≈118 → Unity 后端渲染错位。
//!
//! `--json <out>` 模式：DFS 先序 dump 全节点 layout_rect → JSON，
//! 结构与 showcase/scripts/rect-diff/browser-rect.mjs 对齐
//! （`{domIndex, tag, id, classes, x, y, w, h}`），供 rect-diff 比对浏览器基线。
//! DFS 序与浏览器 `querySelectorAll('body *')` 的 DOM 序近似——核心侧含 TextNode 等
//! 非元素节点，配对以 id 为主（domIndex 仅作辅助索引）。

use ikat_core::dump::kind_to_html_tag;
use ikat_core::scene::dynamic::append_child;
use ikat_core::scene::node::{Node, NodeId, NodeKind, Scene};
use ikat_core::stage::Stage;

fn main() {
    let json_out = parse_json_out_arg();

    let root = env!("CARGO_MANIFEST_DIR");
    let pkg_path = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/ui/spec4b-acceptance.pkg.bin",
        root
    );
    let font_default = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/fonts/LXGWWenKai.ttf.bytes",
        root
    );
    let font_fallback = format!(
        "{}/../../unity/showcase-unity/Assets/Bundles/fonts/DejaVuSans.ttf.bytes",
        root
    );

    let pkg = std::fs::read(&pkg_path).expect("read pkg");
    let mut s = Stage::new((1280.0, 720.0)).expect("Stage::new");
    s.register_font("LXGWWenKai", std::fs::read(&font_default).unwrap(), true)
        .unwrap();
    s.register_font("DejaVuSans", std::fs::read(&font_fallback).unwrap(), false)
        .unwrap();
    s.set_fallback_families(&["DejaVuSans".to_string()]);
    s.set_image_sizes(&[
        ("res/icons/item-wand.png".to_string(), 128, 128),
        ("res/icons/item-chest.png".to_string(), 128, 128),
    ]);
    s.load_package("spec4b-acceptance", &pkg)
        .expect("load_package");
    let root_id = s.create_root("div", "").expect("create_root");
    let inst = s
        .instantiate("spec4b-acceptance", "spec4b-acceptance")
        .expect("instantiate");
    {
        let scene = s.scene.as_mut().unwrap();
        append_child(scene, root_id, inst).expect("append_child");
    }
    let _frame = s.tick_and_render();
    let scene = s.scene.as_ref().unwrap();

    println!("stage=1280x720 scene_nodes={}", scene.nodes.len());
    println!(
        "{:<5} {:<6} {:<14} {:<20} {:>7} {:>7} {:>7} {:>7}  extra",
        "nid", "kind", "id", "class", "x", "y", "w", "h"
    );
    for n in scene.nodes.values() {
        print_diagnostic_row(n, scene, &s);
    }

    if let Some(out_path) = json_out {
        let dfs = collect_dfs(scene, root_id);
        let nodes_json: Vec<serde_json::Value> = dfs
            .iter()
            .enumerate()
            .map(|(i, nid)| {
                let n = scene.get(*nid).expect("DFS node must exist");
                let r = n.layout_rect;
                serde_json::json!({
                    "domIndex": i,
                    "tag": kind_to_html_tag(n.kind),
                    "id": n.id_attr.clone(),
                    "classes": n.classes.clone(),
                    "x": r.x,
                    "y": r.y,
                    "w": r.w,
                    "h": r.h,
                })
            })
            .collect();
        let json_str = serde_json::to_string_pretty(&nodes_json).expect("serialize json");
        std::fs::write(&out_path, json_str).expect("write json");
        eprintln!(
            "wrote {} DFS nodes (root={}) -> {}",
            dfs.len(),
            root_id.index(),
            out_path
        );
    }
}

/// 从 `std::env::args` 解析 `--json <path>`。无该参 → None。
/// 不接 clap（零新依赖，CLI 表面极小，手写足够）。
fn parse_json_out_arg() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--json" {
            return Some(args.next().expect("--json requires a <path> argument"));
        }
    }
    None
}

/// DFS 先序收集节点 id（含 root）。与浏览器 `body *` 的 DOM 序同源——子树按子节点出现顺序
/// 递归展开。核心 Scene 只存 `Node.children: Vec<NodeId>`，无需父→子索引构建。
fn collect_dfs(scene: &Scene, root: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    collect_dfs_rec(scene, root, &mut out);
    out
}

fn collect_dfs_rec(scene: &Scene, id: NodeId, out: &mut Vec<NodeId>) {
    out.push(id);
    // 拷 children 出去再递归——避开 scene.nodes 的不可变借用跨递归调用。
    let children: Vec<NodeId> = scene
        .get(id)
        .map(|n| n.children.clone())
        .unwrap_or_default();
    for c in children {
        collect_dfs_rec(scene, c, out);
    }
}

/// 诊断表行打印（原 main 体内逻辑，抽出以便 main 留在顶部可读）。
fn print_diagnostic_row(n: &Node, scene: &Scene, s: &Stage) {
    let r = n.layout_rect;
    let id = n.id_attr.clone().unwrap_or_default();
    let kind = match n.kind {
        NodeKind::Container => "div",
        NodeKind::Image => "img",
        NodeKind::Button => "btn",
        NodeKind::TextElement => "span",
        NodeKind::TextNode => "#text",
        NodeKind::TabList => "tablist",
        NodeKind::Tab => "tab",
        NodeKind::Tree => "tree",
        NodeKind::TreeItem => "treeitem",
        _ => "?",
    };
    let class = n.classes.join(",");
    let mut extra = String::new();
    if matches!(n.kind, NodeKind::Image) {
        let src = scene.image_srcs.get(&n.id).cloned().unwrap_or_default();
        extra.push_str(&format!("src={:?}", src));
    }
    if matches!(n.kind, NodeKind::TextNode) {
        let content = scene.text_contents.get(&n.id).cloned().unwrap_or_default();
        let st = &n.style;
        extra.push_str(&format!(
            "text={:?} fam={:?} size={:?} lh={:?} align={:?}",
            content.chars().take(16).collect::<String>(),
            st.font_family,
            st.font_size,
            st.line_height,
            st.text_align
        ));
        // tofu 调查：每个非空白 char 的 glyph_index（gid 0/None = .notdef = tofu）。
        // 走 default font stack（fam=None → default + fallback）。空白 text 跳过。
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            let probe = trimmed.chars().next().unwrap();
            let probes: Vec<char> = trimmed.chars().take(4).collect();
            let mut glyph_dbg = String::from(" glyphs[");
            for ch in probes {
                let fam_str = st.font_family.as_deref();
                let host = s.host.borrow();
                let stack = host.fonts.stack_for(fam_str);
                let (font, _font_id) = stack.pick(ch);
                let gid = font.face.glyph_index(ch);
                let gid_n = gid.map(|g| g.0).unwrap_or(0);
                let src_label = if std::ptr::eq(
                    font as *const _,
                    host.fonts.select(Some("LXGWWenKai")) as *const _,
                ) {
                    "LXGW"
                } else if std::ptr::eq(
                    font as *const _,
                    host.fonts.select(Some("DejaVuSans")) as *const _,
                ) {
                    "DejaVu"
                } else {
                    "?"
                };
                glyph_dbg.push_str(&format!(" {}=gid{}({})", ch, gid_n, src_label));
            }
            glyph_dbg.push_str(&format!(" ] first_probe={:?}", probe));
            extra.push_str(&glyph_dbg);
        }
    }
    println!(
        "{:<5} {:<6} {:<14} {:<20} {:>7.1} {:>7.1} {:>7.1} {:>7.1}  {}",
        n.id.index(),
        kind,
        id,
        class,
        r.x,
        r.y,
        r.w,
        r.h,
        extra
    );
}
