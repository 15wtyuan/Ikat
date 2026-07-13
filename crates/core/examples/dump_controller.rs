//! 诊断 v1.5 Controller PlayMode bug：实例化 page_controller，dump 各节点
//! display/color/controller selected_index，触发 set_selected_index 看切换。
//!
//! 定位 [data-page] 显隐、选中态 color 继承、transition 的 core 侧真相。
//!
//! 用法：cargo run -p loomgui_core --example dump_controller
use loomgui_core::asset::read_package;
use loomgui_core::scene::node::NodeKind;
use loomgui_core::stage::Stage;
use std::env;

fn main() {
    let pkg_path = env::args().nth(1).unwrap_or_else(|| {
        "unity/showcase-unity/Assets/StreamingAssets/showcase.pkg.bin".to_string()
    });
    let bytes = std::fs::read(&pkg_path).unwrap_or_else(|e| panic!("read {pkg_path}: {e}"));
    let pkg = read_package(&bytes).expect("read_package");
    assert!(
        pkg.components.contains_key("page_controller"),
        "pkg missing page_controller"
    );

    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let mut s = Stage::new((1080.0, 1920.0)).expect("Stage::new");
    s.register_font("DejaVu", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    s.load_package("showcase", &bytes).expect("load_package");
    let root = s
        .create_root(
            "div",
            "width:1080px;height:1920px;background-color:#1a1d2e;flex-direction:column",
        )
        .expect("create_root");
    let page = s
        .instantiate("showcase", "page_controller")
        .expect("instantiate");
    s.append_child(root, page).expect("append_child");

    // 初次 tick：初次 cascade（cascaded_once false→true）+ layout。
    let _ = s.tick_and_render();
    dump_state(
        &s,
        "INITIAL (期望 tab=0: panel-0 flex, panel-1/2 none; tabbtn-0 选中色)",
    );

    let tab_mount = s.get_controller(page, "tab").expect("tab mount found");
    println!("\n>>> set_selected_index(tab, 1)");
    s.set_selected_index(tab_mount, 1);
    let _ = s.tick_and_render();
    dump_state(
        &s,
        "AFTER tab=1 (期望 panel-0 none, panel-1 flex; tabbtn-1 选中色)",
    );
}

fn dump_state(s: &Stage, label: &str) {
    let scene = s.scene.as_ref().expect("scene");
    println!("\n======== {label} ========");

    println!("[controllers]");
    let mut ctrls: Vec<_> = scene.controllers.iter().collect();
    ctrls.sort_by_key(|(k, _)| k.0);
    for (mount, c) in &ctrls {
        let n = scene.get(**mount);
        let name = n
            .and_then(|n| n.data_controller.clone())
            .unwrap_or_default();
        let id = n.and_then(|n| n.id_attr.clone()).unwrap_or_default();
        println!(
            "  mount_idx={:<6} id={:<14} controller={:<10} selected={}",
            mount.0 >> 12,
            id,
            name,
            c.selected_index
        );
    }

    println!("[nodes]  (base=打包期 base_style.display, cur=rematch 后 display)");
    let keywords = [
        "panel",
        "tabbtn",
        "dialog",
        "opanel",
        "obtn",
        "sbtn",
        "sub-panel",
        "b-icon",
        "hover-demo",
    ];
    let mut rows: Vec<_> = scene
        .nodes
        .values()
        .filter(|n| {
            let id_ok = n
                .id_attr
                .as_deref()
                .map(|i| i == "hover-demo")
                .unwrap_or(false);
            let class_ok = n.classes.iter().any(|c| keywords.contains(&c.as_str()));
            id_ok || class_ok
        })
        .collect();
    rows.sort_by_key(|n| scene.node_sort_keys.get(n.id.index()).copied().unwrap_or(0));
    for n in &rows {
        let label = n.id_attr.clone().unwrap_or_else(|| n.classes.join("."));
        let base_d = format!("{:?}", n.base_style.taffy_style.display);
        let cur_d = format!("{:?}", n.style.taffy_style.display);
        let r = n.layout_rect;
        let kind = format!("{:?}", n.kind);
        println!(
            "  [{label:>18}] base={:<5} cur={:<5} kind={:<9} rect=({:>5.0},{:>5.0},{:>4.0},{:>4.0})",
            base_d, cur_d, kind, r.x, r.y, r.w, r.h
        );
    }

    // 重点：display:none 节点的【后代】layout_rect（CSS 语义：display:none 整子树不渲染。
    // 若后代 layout_rect 非 0，它们会进 frame.nodes 被 Unity 渲染——根因所在）。
    println!("[display:none 节点的后代 layout_rect]");
    let none_parents: Vec<_> = scene
        .nodes
        .values()
        .filter(|n| matches!(n.style.taffy_style.display, taffy::Display::None))
        .map(|n| {
            (
                n.id,
                n.id_attr.clone().unwrap_or_else(|| n.classes.join(".")),
            )
        })
        .collect();
    for (root_id, root_label) in &none_parents {
        let mut stack: Vec<_> = vec![*root_id];
        let mut depth = 0usize;
        while let Some(nid) = stack.pop() {
            let n = match scene.get(nid) {
                Some(n) => n,
                None => continue,
            };
            let r = n.layout_rect;
            let lbl = n.id_attr.clone().unwrap_or_else(|| n.classes.join("."));
            println!(
                "  [root={root_label:>10}] depth={depth} [{lbl:>16}] kind={:<9} rect=({:>5.0},{:>5.0},{:>4.0},{:>4.0})",
                format!("{:?}", n.kind),
                r.x, r.y, r.w, r.h,
            );
            // 子入栈（保持顺序：逆序 push）
            let mut children: Vec<_> = scene
                .nodes
                .values()
                .filter(|c| c.parent == Some(nid))
                .collect();
            children.sort_by_key(|c| c.id.0);
            for c in children.into_iter().rev() {
                stack.push(c.id);
            }
            depth += 1;
        }
    }

    // Text 节点 color 继承：选中态 tab-btn 的文字"页1"是子 Text 节点。
    // 验 Text 是否继承父 Container 的 color（CSS color 是继承属性）。
    println!("[text 节点 color（验继承：选中 tab 文字该深色）]");
    let mut texts: Vec<_> = scene
        .nodes
        .values()
        .filter(|n| matches!(n.kind, NodeKind::Text { .. }))
        .collect();
    texts.sort_by_key(|n| scene.node_sort_keys.get(n.id.index()).copied().unwrap_or(0));
    for n in &texts {
        let content = match &n.kind {
            NodeKind::Text { content } => content.clone(),
            _ => String::new(),
        };
        let short: String = content.chars().take(12).collect();
        let parent_lbl = n
            .parent
            .and_then(|p| scene.get(p))
            .and_then(|p| p.id_attr.clone().or_else(|| p.classes.first().cloned()))
            .unwrap_or_default();
        let pcol = n
            .parent
            .and_then(|p| scene.get(p))
            .map(|p| p.style.color)
            .unwrap_or([0.0; 4]);
        let col = n.style.color;
        println!(
            "  parent={:<14} text={:<16} parent.color=({:.2},{:.2},{:.2}) self.color=({:.2},{:.2},{:.2})",
            parent_lbl, short, pcol[0], pcol[1], pcol[2], col[0], col[1], col[2]
        );
    }
}
