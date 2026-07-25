//! 诊断：精确复现 Unity 切页序列，验证 core 是否在「多次切页」后偶现非确定性布局。
//!
//! 用法：cargo run -p loomgui_core --example dump_switch -- <final> [seq...] [--runs N]
//!   序列为页面名列表，按顺序 instantiate→tick→remove，最后一个保留并 dump。
//!   默认 runs=8（每次全新 Stage）。
//!
//! 例：cargo run -p loomgui_core --example dump_switch -- settings home settings home settings
//!     cargo run -p loomgui_core --example dump_switch -- inventory home mail home inventory --runs 12
//!
//! 每页之间 tick 3 帧（模拟 Unity 里页面停留）。对比每次 run 最后一页的关键容器 rect +
//! 渲染节点数 + world_matrix 偏差，若跨 run 不一致 → core 切页偶现（根因在 core）。

use loomgui_core::render::node::NodePayload;
use loomgui_core::scene::dynamic::append_child;
use loomgui_core::stage::Stage;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: dump_switch <page> [page...] [--runs N]");
        std::process::exit(2);
    }
    // 解析 --runs
    let mut runs: usize = 8;
    let mut seq: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--runs" {
            runs = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(8);
            i += 2;
        } else {
            seq.push(args[i].clone());
            i += 1;
        }
    }
    let final_page = seq.last().cloned().expect("at least one page");

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
    let fd = std::fs::read(&font_default).unwrap();
    let ff = std::fs::read(&font_fallback).unwrap();

    println!(
        "========== seq=[{}] final={} runs={} ==========",
        seq.join("→"),
        final_page,
        runs
    );

    let mut prints: Vec<String> = Vec::new();
    for r in 0..runs {
        let fp = run_once(&pkg, &fd, &ff, &seq);
        prints.push(fp.clone());
        println!("[run {}] {}", r, fp);
    }
    let first = &prints[0];
    let all_same = prints.iter().all(|f| f == first);
    println!();
    if all_same {
        println!("✅ 全部 {} 次一致 → core 该序列确定性", runs);
    } else {
        println!("❌ 不一致 → core 切页偶现！列出差异 run：");
        for (i, f) in prints.iter().enumerate() {
            if f != first {
                println!("   run {} ≠ run 0", i);
            }
        }
    }
}

fn run_once(pkg: &[u8], fd: &[u8], ff: &[u8], seq: &[String]) -> String {
    let mut s = Stage::new((1920.0, 1080.0)).expect("Stage::new");
    s.register_font("LXGWWenKai", fd.to_vec(), true).unwrap();
    s.register_font("wqy-microhei", ff.to_vec(), false).unwrap();
    s.set_fallback_families(&["wqy-microhei".to_string()]);
    s.set_image_sizes(&icon_sizes());
    s.load_package("showcase", pkg).expect("load_package");
    let root_id = s.create_root("div", "").expect("create_root");

    let mut cur = None;
    for (idx, page) in seq.iter().enumerate() {
        if let Some(old) = cur.take() {
            s.remove_node(old);
        }
        let inst = s
            .instantiate("showcase", page)
            .unwrap_or_else(|e| panic!("instantiate {page}: {e}"));
        {
            let scene = s.scene.as_mut().unwrap();
            append_child(scene, root_id, inst).expect("append");
        }
        cur = Some(inst);
        // 非最后一页 tick 几帧再切（模拟 Unity 里停留）
        let frames = if idx + 1 < seq.len() { 3 } else { 1 };
        for _ in 0..frames {
            s.tick_and_render();
        }
    }
    let frame = s.tick_and_render();
    let scene = s.scene.as_ref().unwrap();

    // 指纹：关键容器 rect + 渲染节点数 + world_matrix 偏差（排除 merged by-design）
    let mut parts: Vec<String> = Vec::new();
    for n in scene.nodes.values() {
        let key = n.classes.iter().any(|c| {
            matches!(
                c.as_str(),
                "root"
                    | "sidebar"
                    | "main"
                    | "topbar"
                    | "body"
                    | "hero"
                    | "actions"
                    | "nav-grid"
                    | "quick-bar"
                    | "footbar"
            )
        });
        if !key {
            continue;
        }
        let r = n.layout_rect;
        parts.push(format!(
            "{}=({:.0},{:.0},{:.0},{:.0})",
            n.classes.join(","),
            r.x,
            r.y,
            r.w,
            r.h
        ));
    }
    let mut lr = std::collections::HashMap::new();
    for n in scene.nodes.values() {
        lr.insert(n.id.0, (n.layout_rect.x, n.layout_rect.y));
    }
    // 真 bug 计数：顶点 bbox 起点偏离 layout_rect 超过 1px（merged by-design 顶点在绝对坐标=正确）
    let mut bad = 0usize;
    for rn in &frame.nodes {
        if let Some(&(rx, ry)) = lr.get(&rn.node_id) {
            let NodePayload::Mesh { verts, .. } = &rn.payload;
            if verts
                .iter()
                .any(|v| (v[0] - rx).abs() > 1.5 || (v[1] - ry).abs() > 1.5)
            {
                bad += 1;
            }
        }
    }
    parts.push(format!("nodes={}", frame.nodes.len()));
    parts.push(format!("bad={}", bad));
    parts.join(" | ")
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
