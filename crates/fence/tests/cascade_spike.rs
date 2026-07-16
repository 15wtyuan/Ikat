//! Spec-1 (阶段 S spike) 验收门：div/span HTML → <style> cascade → rect/语义。
//!
//! throwaway mini-bridge（标 ponytail）：fence ParsedTemplate.tree(IrTree) → core Scene。
//! 生产 IrTree→新 NodeKind 桥是 Spec-3 ②，本测试用最小映射（div→Container、文本→Text）。
use loomgui_core::layout::{solve, ImageSizeTable};
use loomgui_core::scene::node::{NodeKind, Scene};
use loomgui_core::style::dynamic::rematch_pseudo_classes;
use loomgui_core::style::resolved::ResolvedStyle;
use loomgui_core::text::layout::FontTable;
use loomgui_fence::{parse_template, IrNodeKind};
use std::collections::HashMap;

    /// ponytail: throwaway mini-bridge for spike; replaced by production bridge (Spec-3 ②) on new enum.
    /// 把 fence ParsedTemplate 的 IrTree 折叠成 core Scene：div→Container、文本叶子→TextNode。
    fn bridge(html: &str) -> Scene {
    let parsed = parse_template(html, "spike.html");
    assert!(
        parsed.diagnostics.is_empty(),
        "fence diagnostics: {:?}",
        parsed.diagnostics
    );

    let tree = &parsed.tree;
    // 给每个 Element IrNode 分配一个 Scene 节点 index（DFS 前序，跳过 Text IrNode）。
    // ponytail: 简化——假设测试 HTML 元素都是 div/span，文本是叶子。
    // SceneEntry = Scene::build 的 entry 元组类型（..., content, src）。
    type SceneEntry = (
        Option<usize>,
        NodeKind,
        ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let mut entries: Vec<SceneEntry> = Vec::new();
    // ir_index → scene_index 映射
    let mut ir_to_scene: std::collections::HashMap<usize, usize> = HashMap::new();

    // DFS 前序遍历 IrTree 元素
    let mut stack: Vec<(usize, Option<usize>)> = tree.roots.iter().map(|r| (r.0, None)).collect();
    while let Some((ir_idx, parent_scene_idx)) = stack.pop() {
        let node = &tree.nodes[ir_idx];
        let IrNodeKind::Element(el) = &node.kind else {
            continue;
        };
        let scene_idx = entries.len();
        ir_to_scene.insert(ir_idx, scene_idx);

        // 收集本元素直接文本子节点为 content
        let mut content = String::new();
        let mut child_elems: Vec<usize> = Vec::new();
        for &child_id in &node.children {
            match &tree.nodes[child_id.0].kind {
                IrNodeKind::Text(t) => content.push_str(t),
                IrNodeKind::Element(_) => child_elems.push(child_id.0),
                // Comment/Doctype 既非文本也非子元素，忽略（mini-bridge 不关心）。
                _ => {}
            }
        }

        let classes: Vec<String> = el
            .attributes
            .iter()
            .find(|a| a.name == "class")
            .map(|a| a.value.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default();
        let id_attr = el
            .attributes
            .iter()
            .find(|a| a.name == "id")
            .map(|a| a.value.clone());

        let kind = match el.tag.as_str() {
            "div" | "main" | "section" | "header" | "footer" | "nav" | "article" | "aside" => {
                NodeKind::Container
            }
            // span/p/h*/strong/em/label/a → TextNode 叶子（content 进 side table）
            _ => NodeKind::TextNode,
        };
        entries.push((
            parent_scene_idx,
            kind,
            ResolvedStyle::default(), // base_style = UA default（spike 无打包期 bake）
            classes,
            id_attr,
            false,
            None,
            None,
            if matches!(kind, NodeKind::TextNode) {
                Some(content)
            } else {
                None
            },
            None,
        ));
        // 子元素入栈（逆序保前序）
        for &ce in child_elems.iter().rev() {
            stack.push((ce, Some(scene_idx)));
        }
    }

    let mut scene = Scene::build(&entries);
    // 规则表喂给现成 cascade 引擎
    scene.dynamic_rules.rules.extend(parsed.dynamic_rules);
    scene
}

#[test]
fn cascade_class_hit_and_font_size_inheritance() {
    // 断言 3（class 命中）+ 断言 2（font-size 继承）。只跑 cascade，不跑 solve（无字体依赖）。
    let html = r#"<style>.par { font-size: 24px } .hit { color: #ff0000 }</style>
        <div class="par"><span class="hit">Hello</span></div>"#;
    let mut scene = bridge(html);

    rematch_pseudo_classes(&mut scene);

    let root = scene.roots[0];
    let span_id = scene.get(root).unwrap().children[0];
    let span = scene.get(span_id).unwrap();
    // 断言 3：.hit class 命中 → color 红
    assert_eq!(
        span.style.color,
        [1.0, 0.0, 0.0, 1.0],
        ".hit class 命中应设 color 红"
    );
    // 断言 2：span 无 font-size 规则 → 继承 .par 的 24px
    assert_eq!(
        span.style.font_size, 24.0,
        "span 该继承 parent .par 的 font-size:24"
    );
}

#[test]
fn layout_rect_and_display_none_pruning() {
    // 断言 1（rect）+ 断言 4（display:none 剪枝）。只用 Container（无 Text），solve 不触发 measure。
    // root flex column 200x200 > [.hidden(display:none w:100 h:50), .vis(w:100 h:50)]
    // .hidden 被剪枝 → .vis 落在 y=0（若没剪枝会落在 y=50）。
    let html = r#"<style>
        .hidden { display: none; width: 100px; height: 50px }
        .vis { width: 100px; height: 50px }
        .root { width: 200px; height: 200px }
    </style>
    <div class="root"><div class="hidden"></div><div class="vis"></div></div>"#;
    let mut scene = bridge(html);

    rematch_pseudo_classes(&mut scene);
    // Container-only 树，无 Text → measure 不触发 → 空字体表即可
    let fonts = FontTable::new();
    let sizes: ImageSizeTable = HashMap::new();
    solve(&mut scene, &fonts, (200.0, 200.0), &sizes);

    let root = scene.roots[0];
    let children = &scene.get(root).unwrap().children;
    // 子节点文档序：.hidden(idx0)、.vis(idx1)
    let vis_id = children[1];
    let vis = scene.get(vis_id).unwrap().layout_rect;
    // 断言 1：.vis 尺寸正确
    assert!(
        (vis.w - 100.0).abs() < 0.5,
        ".vis width 应 100，实际 {}",
        vis.w
    );
    assert!(
        (vis.h - 50.0).abs() < 0.5,
        ".vis height 应 50，实际 {}",
        vis.h
    );
    // 断言 4：.hidden 被 display:none 剪枝 → .vis 落在 y=0（而非 y=50）
    assert!(
        vis.y.abs() < 0.5,
        ".hidden 剪枝后 .vis 应在 y=0，实际 y={}（display:none 未剪枝？）",
        vis.y
    );
}
