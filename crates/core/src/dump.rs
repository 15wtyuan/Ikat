//! 整树 JSON dump（调试用）。
use crate::scene::node::{Node, NodeId, NodeKind, Scene};

/// JSON 字符串转义：处理 `"` → `\"`、`\` → `\\`、控制字符 → `\uXXXX`。
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04X}", c as u32)),
            _ => out.push(ch),
        }
    }
    out
}

/// NodeKind → 浏览器侧 `tagName.toLowerCase()` 对应 tag 串（rect-diff 配对语义）。
/// 与 `dump_scene_json` 的诊断 tag 映射**不同**（TextNode: `#text` vs `span`；ListView:
/// `ul` vs `div`；CustomElement: `custom` vs `div`）——本函数服务于浏览器 rect 配对
/// （TextNode 在浏览器 `querySelectorAll('body *')` 无元素，diff.mjs 按 `#text` 过滤），
/// 诊断 dump 保留自己的近似映射。全部 21 kind 由单测断言覆盖（防漂移）。
pub fn kind_to_html_tag(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Container => "div",
        NodeKind::TextNode => "#text",
        NodeKind::TextElement => "span",
        NodeKind::Button => "button",
        NodeKind::Image => "img",
        NodeKind::TextField
        | NodeKind::NumberField
        | NodeKind::Slider
        | NodeKind::Toggle
        | NodeKind::RadioButton => "input",
        NodeKind::TextArea => "textarea",
        NodeKind::Dropdown => "select",
        NodeKind::OptionItem => "option",
        NodeKind::ProgressBar => "progress",
        NodeKind::ListView => "ul",
        NodeKind::ListItem => "li",
        NodeKind::Slot => "slot",
        NodeKind::CustomElement => "custom",
        NodeKind::Template => "template",
        NodeKind::TabList => "div",
        NodeKind::Tab => "button",
    }
}

/// 整树 JSON：每节点 {node_id, parent, tag, id, classes, kind, layout, world_matrix, visible}。
/// 文本节点附 `"text"` 块（font-size / 行高（乘数标记，#65 类 `line-height:26` 被当
/// 26 倍乘数一眼可见）/ 行数 / 每行宽）；滚动容器附 `"scroll"` 块（viewport/content/
/// overlap/pos/物理，#64 类 overlap=0 秒判）。均为增量字段——rect-diff 的
/// normalize-dump-scene 只读已知字段，加字段向后安全。
pub fn dump_scene_json(scene: &Scene) -> String {
    let mut s = String::from("[");
    for (i, n) in scene.nodes.values().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let (tag, kind_str): (&'static str, String) = match n.kind {
            NodeKind::Container => ("div", "Container".into()),
            NodeKind::TextNode => ("span", "TextNode".into()),
            NodeKind::TextElement => ("span", "TextElement".into()),
            NodeKind::Button => ("button", "Button".into()),
            NodeKind::Image => ("img", "Image".into()),
            NodeKind::TextField => ("input", "TextField".into()),
            NodeKind::NumberField => ("input", "NumberField".into()),
            NodeKind::Slider => ("input", "Slider".into()),
            NodeKind::Toggle => ("input", "Toggle".into()),
            NodeKind::RadioButton => ("input", "RadioButton".into()),
            NodeKind::TextArea => ("textarea", "TextArea".into()),
            NodeKind::Dropdown => ("select", "Dropdown".into()),
            NodeKind::OptionItem => ("option", "OptionItem".into()),
            NodeKind::ProgressBar => ("progress", "ProgressBar".into()),
            NodeKind::ListView => ("div", "ListView".into()),
            NodeKind::ListItem => ("li", "ListItem".into()),
            NodeKind::Slot => ("slot", "Slot".into()),
            NodeKind::CustomElement => ("div", "CustomElement".into()),
            NodeKind::Template => ("template", "Template".into()),
            NodeKind::TabList => ("div", "TabList".into()),
            NodeKind::Tab => ("button", "Tab".into()),
        };
        let id = json_escape(n.id_attr.as_deref().unwrap_or(""));
        let classes = n
            .classes
            .iter()
            .map(|c| json_escape(c))
            .collect::<Vec<_>>()
            .join(" ");
        // world_transforms / anim 按 slotmap idx 索引（idx 从 1 起，数组长 N+1）。
        // bounds guard：未对齐时 fallback IDENTITY / (false, None)。
        let wm = if n.id.index() < scene.world_transforms.len() {
            &scene.world_transforms[n.id.index()]
        } else {
            &crate::transform::IDENTITY
        };
        // 诊断：附 anim.transform 是否 Some + opacity 值，定位 tween 是否真写进 anim。
        let (anim_tr, anim_op) = match scene.anim.get(n.id) {
            Some(a) => (a.transform.is_some(), a.opacity),
            None => (false, None),
        };
        let op_str = anim_op
            .map(|v| format!("{:.3}", v))
            .unwrap_or_else(|| "null".into());
        // 文本 resolved 块：solve/render 产出的断行结果（text_layouts 槽）。行高字符串
        // 用乘数形（"26.00x"）——显式 px 也换算成乘数展示，异常倍数直读可见。
        let text_seg = scene
            .text_layouts
            .get(n.id.index())
            .and_then(|o| o.as_ref())
            .map(|tl| {
                let eff = n.style.effective_line_height();
                let lh = if eff > 0.0 {
                    format!("{eff:.2}x")
                } else {
                    "normal".into()
                };
                let lh_px = if eff > 0.0 {
                    n.style.font_size * eff
                } else {
                    0.0
                };
                let widths: Vec<String> =
                    tl.lines.iter().map(|l| format!("{:.1}", l.width)).collect();
                format!(
                    r#","text":{{"font_size":{:.1},"line_height":"{}","line_height_px":{:.1},"lines":{},"line_widths":[{}],"text_w":{:.1},"text_h":{:.1}}}"#,
                    n.style.font_size,
                    lh,
                    lh_px,
                    tl.lines.len(),
                    widths.join(","),
                    tl.text_width,
                    tl.text_height
                )
            })
            .unwrap_or_default();
        // 滚动几何块：运行时 scroll 表（无槽 = 非滚动容器，不附）。
        let scroll_seg = scene
            .scroll
            .get(n.id)
            .map(|s| {
                format!(
                    r#","scroll":{{"viewport":[{:.1},{:.1}],"content":[{:.1},{:.1}],"overlap":[{:.1},{:.1}],"pos":[{:.1},{:.1}],"tweening":[{},{}]}}"#,
                    s.viewport_size.0,
                    s.viewport_size.1,
                    s.content_size.0,
                    s.content_size.1,
                    s.overlap.0,
                    s.overlap.1,
                    s.scroll_pos.0,
                    s.scroll_pos.1,
                    s.tweening[0],
                    s.tweening[1]
                )
            })
            .unwrap_or_default();
        s.push_str(&format!(
            r#"{{"node_id":{},"parent":{},"tag":"{}","id":"{}","classes":"{}","kind":"{}","layout":{{"x":{},"y":{},"w":{},"h":{}}},"world_matrix":[{},{},{},{},{},{}],"anim_tr":{},"anim_op":{},"visible":{}{text}{scroll}}}"#,
            n.id.0, n.parent.map(|p| p.0.to_string()).unwrap_or("-1".into()),
            // CustomElement：tag 用 custom_tag 字面值（rect-diff 与浏览器侧 hyphen 原文配对），无字面值退逆映射。
            match n.kind {
                NodeKind::CustomElement => {
                    json_escape(n.custom_tag.as_deref().unwrap_or(tag))
                }
                _ => tag.to_string(),
            },
            id, classes, kind_str,
            n.layout_rect.x, n.layout_rect.y, n.layout_rect.w, n.layout_rect.h,
            wm[0], wm[1], wm[2], wm[3], wm[4], wm[5],
            anim_tr, op_str,
            true, // visible：无独立 visible 字段，恒 true（clip/touchable 另列）
            text = text_seg,
            scroll = scroll_seg,
        ));
    }
    s.push(']');
    s
}

/// 人类可读树视图（F8 dump 附录；AI 代理是第一读者）：每节点一行
/// `tag#id.class (x,y,w,h)` + 文本/滚动关键 resolved 值 + 文本内容摘要，
/// ASCII 树缩进表父子。`filter` = Some(s)：只出 id/class 含 s 的节点子树
/// （大 UI 不再全量肉眼扫；祖先与后代都命中时只打祖先子树）。None = roots 全量。
pub fn dump_scene_tree(scene: &Scene, filter: Option<&str>) -> String {
    let matches = |n: &Node| match filter {
        Some(f) => {
            n.id_attr.as_deref().is_some_and(|id| id.contains(f))
                || n.classes.iter().any(|c| c.contains(f))
        }
        None => true,
    };
    // 命中集合（NodeId -> bool 经 0 值 vs 节点数比较不必，直接 HashSet）。
    let mut hit: std::collections::HashSet<NodeId> = scene
        .nodes
        .iter()
        .filter(|(_, n)| matches(n))
        .map(|(k, _)| NodeId::from_key(k))
        .collect();
    // 剔除「祖先也命中」的节点——其子树已含在祖先子树里。
    let dead: Vec<NodeId> = hit
        .iter()
        .copied()
        .filter(|&id| {
            let mut p = scene.get(id).and_then(|n| n.parent);
            while let Some(anc) = p {
                if hit.contains(&anc) {
                    return true;
                }
                p = scene.get(anc).and_then(|n| n.parent);
            }
            false
        })
        .collect();
    for id in dead {
        hit.remove(&id);
    }
    let mut roots: Vec<NodeId> = if filter.is_some() {
        hit.into_iter().collect()
    } else {
        scene.roots.clone()
    };
    // 命中子树多棵时按 NodeId 稳定排序（NodeId 无 Ord derive，按内部 u64 排）。
    roots.sort_by_key(|id| id.0);
    if roots.is_empty() {
        return "(no matching nodes)".into();
    }
    let mut out = String::new();
    for (i, root) in roots.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        write_subtree(scene, root, "", true, &mut out);
    }
    out
}

/// 递归打子树：`last` = 本层是否末位（└─ vs ├─，前缀纵深 │ 续行）。
fn write_subtree(scene: &Scene, id: &NodeId, prefix: &str, last: bool, out: &mut String) {
    let Some(n) = scene.get(*id) else {
        return;
    };
    let tag = match n.kind {
        NodeKind::CustomElement => n.custom_tag.as_deref().unwrap_or("div"),
        _ => kind_tag(n.kind),
    };
    let id_part = n.id_attr.as_deref().unwrap_or("");
    let classes = n.classes.join(".");
    let sel = if classes.is_empty() {
        format!("{tag}#{id_part}")
    } else {
        format!("{tag}#{id_part}.{classes}")
    };
    let r = &n.layout_rect;
    let (rx, ry, rw, rh) = (r.x, r.y, r.w, r.h);
    let mut line = format!("{sel} ({rx:.0},{ry:.0},{rw:.0},{rh:.0})");
    // 文本关键值：font/行高乘数/行数 + 内容摘要（哪段文字断几行一眼可对）。
    if let Some(tl) = scene
        .text_layouts
        .get(n.id.index())
        .and_then(|o| o.as_ref())
    {
        let eff = n.style.effective_line_height();
        let lh = if eff > 0.0 {
            format!("{eff:.2}x")
        } else {
            "normal".into()
        };
        line.push_str(&format!(
            " font={:.0} lh={lh} lines={}",
            n.style.font_size,
            tl.lines.len()
        ));
        if let Some(content) = scene.text_contents.get(&n.id) {
            let snippet: String = content.chars().take(24).collect();
            if !snippet.is_empty() {
                let ell = if content.chars().count() > 24 {
                    "…"
                } else {
                    ""
                };
                line.push_str(&format!(" \"{snippet}{ell}\""));
            }
        }
    }
    if let Some(s) = scene.scroll.get(n.id) {
        line.push_str(&format!(
            " scroll[vp {:.0}x{:.0} ct {:.0}x{:.0} ov {:.0}x{:.0} pos {:.0},{:.0} tw {},{}]",
            s.viewport_size.0,
            s.viewport_size.1,
            s.content_size.0,
            s.content_size.1,
            s.overlap.0,
            s.overlap.1,
            s.scroll_pos.0,
            s.scroll_pos.1,
            s.tweening[0],
            s.tweening[1]
        ));
    }
    out.push_str(prefix);
    out.push_str(if last { "└─ " } else { "├─ " });
    out.push_str(&line);
    out.push('\n');
    let child_prefix = format!("{prefix}{}  ", if last { ' ' } else { '│' });
    let children = &n.children;
    for (i, c) in children.iter().enumerate() {
        write_subtree(scene, c, &child_prefix, i == children.len() - 1, out);
    }
}

/// NodeKind → 树视图 tag（诊断映射，与 dump_scene_json 的 tag 列一致）。
fn kind_tag(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Container => "div",
        NodeKind::TextNode | NodeKind::TextElement => "span",
        NodeKind::Button | NodeKind::Tab => "button",
        NodeKind::Image => "img",
        NodeKind::TextField
        | NodeKind::NumberField
        | NodeKind::Slider
        | NodeKind::Toggle
        | NodeKind::RadioButton => "input",
        NodeKind::TextArea => "textarea",
        NodeKind::Dropdown => "select",
        NodeKind::OptionItem => "option",
        NodeKind::ProgressBar => "progress",
        NodeKind::ListView => "div",
        NodeKind::ListItem => "li",
        NodeKind::Slot => "slot",
        NodeKind::CustomElement => "div",
        NodeKind::Template => "template",
        NodeKind::TabList => "div",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::node::{Node, Rect, Scene};

    #[test]
    fn dump_has_node_fields() {
        let mut n = Node::default();
        n.id_attr = Some("root".into());
        n.classes = vec!["main".into()];
        n.layout_rect = Rect {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
        };
        // from_nodes 分配 slotmap id（首节点 = NodeId((1<<12)|1)）；world_transforms 空 → IDENTITY。
        let s = Scene::from_nodes(vec![n], vec![]);
        let json = dump_scene_json(&s);
        assert!(json.contains(r#""id":"root""#), "含 id");
        assert!(json.contains(r#""classes":"main""#), "含 classes");
        assert!(json.contains(r#""x":1"#), "含 layout.x");
        assert!(json.contains(r#""y":2"#), "含 layout.y");
        assert!(json.contains(r#""w":3"#), "含 layout.w");
        assert!(
            json.contains(r#""world_matrix":[1,0,0,1,0,0]"#),
            "identity world_matrix"
        );
    }

    #[test]
    fn dump_escapes_quotes_in_id() {
        let mut n = Node::default();
        n.id_attr = Some("a\"b".into());
        let s = Scene::from_nodes(vec![n], vec![]);
        let json = dump_scene_json(&s);
        assert!(
            json.contains(r#""id":"a\"b""#),
            "id 中的引号被转义：{}",
            json
        );
    }

    #[test]
    fn kind_to_html_tag_matches_browser_pairing_semantics() {
        assert_eq!(kind_to_html_tag(NodeKind::Container), "div");
        assert_eq!(kind_to_html_tag(NodeKind::TextNode), "#text");
        assert_eq!(kind_to_html_tag(NodeKind::TextElement), "span");
        assert_eq!(kind_to_html_tag(NodeKind::Button), "button");
        assert_eq!(kind_to_html_tag(NodeKind::Image), "img");
        assert_eq!(kind_to_html_tag(NodeKind::TextField), "input");
        assert_eq!(kind_to_html_tag(NodeKind::NumberField), "input");
        assert_eq!(kind_to_html_tag(NodeKind::Slider), "input");
        assert_eq!(kind_to_html_tag(NodeKind::Toggle), "input");
        assert_eq!(kind_to_html_tag(NodeKind::RadioButton), "input");
        assert_eq!(kind_to_html_tag(NodeKind::TextArea), "textarea");
        assert_eq!(kind_to_html_tag(NodeKind::Dropdown), "select");
        assert_eq!(kind_to_html_tag(NodeKind::OptionItem), "option");
        assert_eq!(kind_to_html_tag(NodeKind::ProgressBar), "progress");
        assert_eq!(kind_to_html_tag(NodeKind::ListView), "ul");
        assert_eq!(kind_to_html_tag(NodeKind::ListItem), "li");
        assert_eq!(kind_to_html_tag(NodeKind::Slot), "slot");
        assert_eq!(kind_to_html_tag(NodeKind::CustomElement), "custom");
        assert_eq!(kind_to_html_tag(NodeKind::Template), "template");
        assert_eq!(kind_to_html_tag(NodeKind::TabList), "div");
        assert_eq!(kind_to_html_tag(NodeKind::Tab), "button");
    }

    /// #85：文本节点附 resolved 块（font/行高乘数/行宽），滚动容器附几何块。
    #[test]
    fn dump_json_has_text_and_scroll_blocks() {
        let mut root = Node::default();
        root.id_attr = Some("root".into());
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 600.0,
        };
        let mut text = Node::default();
        text.kind = NodeKind::TextElement;
        text.id_attr = Some("body".into());
        text.classes = vec!["desc".into()];
        // #65 形异常：line-height 26（无单位乘数）→ dump 展示 "26.00x"。
        text.style.line_height = 26.0;
        text.style.font_size = 16.0;
        let mut s = Scene::from_nodes(vec![root, text], vec![(0, 1)]);
        let text_id = s.roots[0];
        let child = s.get(text_id).unwrap().children[0];
        // 填 text_layouts 槽（solve 产物，这里手工塞两条行）。
        {
            let idx = child.index();
            while s.text_layouts.len() <= idx {
                s.text_layouts.push(None);
            }
            s.text_layouts[idx] = Some(crate::text::layout::TextLayout {
                text_width: 300.0,
                text_height: 832.0,
                lines: vec![
                    crate::text::layout::Line {
                        y: 0.0,
                        height: 416.0,
                        baseline: 400.0,
                        width: 300.0,
                        runs: vec![],
                    },
                    crate::text::layout::Line {
                        y: 416.0,
                        height: 416.0,
                        baseline: 800.0,
                        width: 280.0,
                        runs: vec![],
                    },
                ],
                images: vec![],
                run_rects: vec![],
            });
        }
        // 填 scroll 槽（refresh_content_sizes 产物）。
        {
            let st = s.scroll.ensure(text_id);
            st.viewport_size = (400.0, 600.0);
            st.content_size = (400.0, 1200.0);
            st.overlap = (0.0, 600.0);
            st.scroll_pos = (0.0, 120.0);
        }
        let json = dump_scene_json(&s);
        assert!(
            json.contains(r#""line_height":"26.00x""#),
            "行高乘数直读：{json}"
        );
        assert!(json.contains(r#""line_height_px":416.0"#));
        assert!(json.contains(r#""lines":2"#), "行数");
        assert!(json.contains(r#""line_widths":[300.0,280.0]"#));
        assert!(
            json.contains(r#""scroll":{"viewport":[400.0,600.0]"#),
            "滚动几何行"
        );
        assert!(json.contains(r#""overlap":[0.0,600.0]"#));
    }

    /// #85：树视图 + 子树过滤（id/class 命中只出该子树；祖先后代都命中去重）。
    #[test]
    fn dump_tree_renders_and_filters() {
        let mut root = Node::default();
        root.id_attr = Some("root".into());
        root.layout_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 600.0,
        };
        let mut panel = Node::default();
        panel.id_attr = Some("panel".into());
        panel.classes = vec!["card".into()];
        panel.layout_rect = Rect {
            x: 8.0,
            y: 8.0,
            w: 200.0,
            h: 100.0,
        };
        let mut leaf = Node::default();
        leaf.id_attr = Some("leaf".into());
        leaf.layout_rect = Rect {
            x: 8.0,
            y: 8.0,
            w: 50.0,
            h: 20.0,
        };
        let s = Scene::from_nodes(vec![root, panel, leaf], vec![(0, 1), (1, 2)]);
        let tree = dump_scene_tree(&s, None);
        assert!(tree.contains("div#root (0,0,400,600)"), "root 行：{tree}");
        assert!(
            tree.contains("└─ div#panel.card"),
            "子树前缀 + 选择器：{tree}"
        );
        assert!(tree.contains("    └─ div#leaf"), "孙层缩进：{tree}");

        // 过滤 id="panel" → 只出 panel 子树（root 不含、leaf 在子树内含）。
        let filtered = dump_scene_tree(&s, Some("panel"));
        assert!(
            !filtered.contains("div#root"),
            "过滤后 root 不出：{filtered}"
        );
        assert!(filtered.contains("div#panel"), "{filtered}");
        assert!(filtered.contains("div#leaf"), "子树整体保留：{filtered}");

        // 过滤 class="card" 同样命中 panel。
        assert!(dump_scene_tree(&s, Some("card")).contains("div#panel"));

        // 无命中 → 提示串。
        assert_eq!(dump_scene_tree(&s, Some("zzz")), "(no matching nodes)");
    }
}
