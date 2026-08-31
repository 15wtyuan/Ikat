//! 诊断：实例化 showcase 任意页面，dump 全节点 layout_rect + display(flex/block/none)
//! + touchable，定位「Unity 布局与 HTML 不一致」时 bug 在 core/packager 还是 Unity 后端。
//!
//! 用法：cargo run -p ikat_core --example dump_page -- <page-name>
//!   page-name ∈ home/settings/inventory/mail/shop/character/form/lab
//!
//! 重点取证：.root / .sidebar / .main 等关键容器的 display_mode 与 layout_rect，
//! 对照 HTML 预期（settings 应左右布局、home 应纵向 flex 等）。

use ikat_core::dump::kind_to_html_tag;
use ikat_core::scene::dynamic::append_child;
use ikat_core::scene::node::{Node, NodeId, NodeKind, Scene};
use ikat_core::stage::Stage;

fn main() {
    let page = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "home".to_string());

    let json_out = parse_json_out_arg();

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
    // 适配取证（#110）：IKAT_ROOT=WxH 设 root 形状（fit 模式的重排 root，如
    // 1920x1440 = fit-width@4:3 屏）；IKAT_SAFE=t,r,b,l 设 env(safe-area-inset-*)
    // design px（对拍 browser 侧 --safe）。缺省 = 设计分辨率 + 无 inset。
    let root_size = std::env::var("IKAT_ROOT")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?))
        })
        .unwrap_or((1920.0, 1080.0));
    let safe_insets: [f32; 4] = std::env::var("IKAT_SAFE")
        .ok()
        .map(|v| {
            let parts: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            [
                parts.first().copied().unwrap_or(0.0),
                parts.get(1).copied().unwrap_or(0.0),
                parts.get(2).copied().unwrap_or(0.0),
                parts.get(3).copied().unwrap_or(0.0),
            ]
        })
        .unwrap_or([0.0; 4]);
    let mut s = Stage::new(root_size).expect("Stage::new");
    s.set_safe_insets(safe_insets).expect("set_safe_insets");
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
        "========== page={page} stage={}x{} safe={:?} nodes={} ==========",
        root_size.0,
        root_size.1,
        safe_insets,
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
    let mut lr: HashMap<u64, (f32, f32, f32, f32)> = HashMap::new();
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
    let mut matches: HashMap<u64, (f32, f32)> = HashMap::new();
    let wt_len = scene.world_transforms.len();
    println!(
        "  [diag] world_transforms.len()={} nodes.capacity()={}",
        wt_len,
        scene.nodes.capacity()
    );
    // 重复 node_id 检测：同一 node_id 出现多次 → 后者覆盖前者 world_matrix？
    let mut id_count: HashMap<u64, usize> = HashMap::new();
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

    // 渐变参数取证：program=6/7 节点的 grad_params（kind/角度/几何/stops）。
    // Unity 侧视觉不对时，对照此输出定位 core 参数错 vs shader 错。
    println!("\n── 渐变节点 grad_params（program=6/7）──");
    let mut grad_count = 0;
    for rn in &frame.nodes {
        // NodePayload 单一 Mesh 变体（v10 起文本也塌进 mesh），直接解构。
        let ikat_core::render::node::NodePayload::Mesh { program, .. } = &rn.payload;
        if *program == 6 || *program == 7 {
            grad_count += 1;
            let g = &rn.gradient;
            let stops: Vec<String> = g.stops[..g.stop_count.min(8) as usize]
                .iter()
                .map(|s| {
                    format!(
                        "rgba({:.2},{:.2},{:.2},{:.2})@{:.2}",
                        s[0], s[1], s[2], s[3], s[4]
                    )
                })
                .collect();
            match g.kind {
                1 => println!(
                    "    nid={} prog={} radial c=({:.1},{:.1}) r=({:.1},{:.1}) stops=[{}]",
                    rn.node_id,
                    program,
                    g.center[0],
                    g.center[1],
                    g.radii[0],
                    g.radii[1],
                    stops.join(", ")
                ),
                _ => println!(
                    "    nid={} prog={} linear {}deg dir=({:.3},{:.3}) t0={:.1} span={:.4} stops=[{}]",
                    rn.node_id,
                    program,
                    g.angle_deg,
                    g.dir[0],
                    g.dir[1],
                    g.t0,
                    g.inv_span,
                    stops.join(", ")
                ),
            }
        }
    }
    if grad_count == 0 {
        println!("    （无渐变节点）");
    }

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
            let ikat_core::render::node::NodePayload::Mesh { verts, .. } = &rn.payload;
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

    if let Some(out_path) = json_out {
        // DFS from the synthetic Stage root's CHILDREN, skipping the root
        // itself: the root (create_root) has no browser-side counterpart
        // (browser enumerates `body *`, which never includes the Stage
        // wrapper). Including it injects a 1920x1080 plain-div into the
        // `div|` bucket that mispairs with a real browser plain-div and
        // surfaces false huge-rect diffs.
        let dfs = {
            let roots_children: Vec<NodeId> = scene
                .get(root_id)
                .map(|n| n.children.clone())
                .unwrap_or_default();
            let mut v = Vec::new();
            for child in roots_children {
                collect_dfs_rec(scene, child, &mut v);
            }
            v
        };
        let nodes_json: Vec<serde_json::Value> = dfs
            .iter()
            .enumerate()
            .map(|(i, nid)| {
                let n = scene.get(*nid).expect("DFS node must exist");
                let r = n.layout_rect;
                // 渐变节点附带解析后参数（--json 侧取证；rect-diff 主流程不读此字段）。
                let gradient_json = n.style.background_gradient.as_ref().map(|g| {
                    let p = ikat_core::render::gradient::resolve_gradient(g, r.w, r.h);
                    serde_json::json!({
                        "kind": if p.kind == 1 { "radial" } else { "linear" },
                        "angleDeg": p.angle_deg,
                        "dir": p.dir,
                        "t0": p.t0,
                        "invSpan": p.inv_span,
                        "center": p.center,
                        "radii": p.radii,
                        "stops": p.stops[..p.stop_count.min(8) as usize],
                    })
                });
                serde_json::json!({
                    "domIndex": i,
                    // CustomElement 发 custom_tag 字面量（与浏览器侧 tagName 原文配对；
                    // 其余 kind 走 kind_to_html_tag 语义映射）。
                    "tag": n.custom_tag.as_deref().map(str::to_string)
                        .unwrap_or_else(|| kind_to_html_tag(n.kind).to_string()),
                    "id": n.id_attr.clone(),
                    "classes": n.classes.clone(),
                    "x": r.x,
                    "y": r.y,
                    "w": r.w,
                    "h": r.h,
                    "gradient": gradient_json,
                })
            })
            .collect();
        let json_str = serde_json::to_string_pretty(&nodes_json).expect("serialize json");
        std::fs::write(&out_path, json_str).expect("write json");
        eprintln!("wrote {} DFS nodes -> {}", dfs.len(), out_path);
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
        // 0.14 新变体：ikat 不产出（围栏不放 flow-root），诊断输出兜底。
        taffy::style::Display::FlowRoot => "flow-root",
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

/// 从 `std::env::args` 解析 `--json <path>`。无该参 → None。
/// 不接 clap（零新依赖，CLI 表面极小，手写足够）。
fn parse_json_out_arg() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--json" {
            // 用法错误统一 exit 2（与 run-page.sh / diff.mjs 的 2=usage 契约对齐），不 panic。
            return match args.next() {
                Some(path) => Some(path),
                None => {
                    eprintln!("error: --json requires a <path> argument");
                    std::process::exit(2);
                }
            };
        }
    }
    None
}

/// DFS 先序收集 `id` 的后代（不含 `id` 自身）。与浏览器 `body *` 的 DOM 序同源——子树
/// 按子节点出现顺序递归展开。核心 Scene 只存 `Node.children: Vec<NodeId>`，无需父→子
/// 索引构建。调用方选起始层（合成 Stage 根的子节点），使输出与浏览器侧 `body *` 枚举
/// 对齐——Stage 根本身无浏览器对应物，混入会污染 idless 桶配对。
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
