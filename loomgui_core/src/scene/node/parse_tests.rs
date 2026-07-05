use super::*;
use crate::parse::{css::parse_css, dom::parse_html};
use crate::style::cascade::resolve_styles;

#[test]
fn build_scene_fills_classes_and_id() {
    let html = r#"<div class="a b" id="x"><span class="c">hi</span></div>"#;
    let css = "";
    let tree = parse_html(html).unwrap();
    let sheet = parse_css(css).unwrap();
    let styles = resolve_styles(&tree, &sheet);
    let scene = build_scene(&tree, &styles);
    let root = scene.get(scene.roots[0]).unwrap();
    assert_eq!(root.classes, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(root.id_attr.as_deref(), Some("x"));
    let span = scene.get(root.children[0]).unwrap();
    assert_eq!(span.classes, vec!["c".to_string()]);
}

#[test]
fn find_by_id_attr_returns_node_and_none() {
    // 手搓 Scene（不走 parse）：root(id="root") + btn(id="btn") + Text 子(无 id)。
    // 验：精确匹配返 NodeId；无匹配/空 id → None。
    let entries = vec![
        (
            None,
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            Some("root".to_string()),
            false,
            None,
        ),
        (
            Some(0),
            NodeKind::Button,
            ResolvedStyle::default(),
            vec![],
            Some("btn".to_string()),
            false,
            None,
        ),
        (
            Some(1),
            NodeKind::Text {
                content: "x".into(),
            },
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
        ),
    ];
    let scene = Scene::build(&entries);
    let root_id = scene.roots[0];
    let btn_id = scene.get(root_id).unwrap().children[0];
    assert_eq!(
        scene.find_by_id_attr("btn"),
        Some(btn_id),
        "find btn → btn node"
    );
    assert_eq!(
        scene.find_by_id_attr("root"),
        Some(root_id),
        "find root → root node"
    );
    assert_eq!(scene.find_by_id_attr("missing"), None, "无匹配 → None");
    assert_eq!(scene.find_by_id_attr(""), None, "空 id → None");
}

#[test]
fn builds_div_button_text_image() {
    // img 用属性 src（不是文本）；其它元素覆盖四种 NodeKind。
    let html = r#"<div class="root"><button>OK</button><span>hi</span><img src="logo.png"></div>"#;
    let css = ".root { width: 200px; }";
    let tree = parse_html(html).unwrap();
    let sheet = parse_css(css).unwrap();
    let styles = resolve_styles(&tree, &sheet);
    let scene = build_scene(&tree, &styles);
    let root = scene.get(scene.roots[0]).unwrap();
    assert!(matches!(root.kind, NodeKind::Container));
    assert_eq!(root.children.len(), 3);
    let c0 = root.children[0];
    let c1 = root.children[1];
    let c2 = root.children[2];
    assert!(matches!(scene.get(c0).unwrap().kind, NodeKind::Button));
    let text = scene.get(c1).unwrap();
    match &text.kind {
        NodeKind::Text { content } => assert_eq!(content, "hi"),
        _ => panic!("expected Text"),
    }
    match &scene.get(c2).unwrap().kind {
        NodeKind::Image { src } => assert_eq!(src, "logo.png"),
        _ => panic!("expected Image"),
    }
}

#[test]
fn overflow_hidden_sets_clip_rect_slot() {
    let html = r#"<div></div>"#;
    let css = "div { overflow: hidden; }";
    let tree = parse_html(html).unwrap();
    let sheet = parse_css(css).unwrap();
    let styles = resolve_styles(&tree, &sheet);
    let scene = build_scene(&tree, &styles);
    assert!(scene.get(scene.roots[0]).unwrap().clip_rect.is_some());
}

#[test]
fn image_without_src_falls_back_to_empty() {
    // 缺 src 属性不 panic，降级空串（render 层报缺图）。
    let html = r#"<div><img alt="no src"></div>"#;
    let css = "";
    let tree = parse_html(html).unwrap();
    let sheet = parse_css(css).unwrap();
    let styles = resolve_styles(&tree, &sheet);
    let scene = build_scene(&tree, &styles);
    let root = scene.get(scene.roots[0]).unwrap();
    match &scene.get(root.children[0]).unwrap().kind {
        NodeKind::Image { src } => assert_eq!(src, ""),
        _ => panic!("expected Image"),
    }
}

#[test]
fn text_node_marks_dirty_text_and_clean_leaves_unset() {
    // Text 节点 dirty_text=true；Container dirty_text=false；全部 dirty_mesh=true。
    let html = r#"<div><span>hi</span></div>"#;
    let css = "";
    let tree = parse_html(html).unwrap();
    let sheet = parse_css(css).unwrap();
    let styles = resolve_styles(&tree, &sheet);
    let scene = build_scene(&tree, &styles);
    let root = scene.get(scene.roots[0]).unwrap();
    assert!(root.dirty_mesh);
    assert!(!root.dirty_text); // Container 不脏文本
    let text_id = root.children[0];
    let text = scene.get(text_id).unwrap();
    assert!(text.dirty_mesh);
    assert!(text.dirty_text); // Text 节点脏文本
}

#[test]
fn div_raw_text_becomes_text_child() {
    // div 的裸文本 → Text 子节点（文本是 flex item，参与布局）。
    // 匹配 AI 的 HTML 先验：<div>标题</div> 里的"标题"应可见、参与 flex 排列。
    let html = r#"<div>标题</div>"#;
    let css = "";
    let tree = parse_html(html).unwrap();
    let sheet = parse_css(css).unwrap();
    let styles = resolve_styles(&tree, &sheet);
    let scene = build_scene(&tree, &styles);
    let root = scene.get(scene.roots[0]).unwrap();
    assert!(matches!(root.kind, NodeKind::Container));
    assert_eq!(root.children.len(), 1, "裸文本应产 1 个 Text 子节点");
    let child_id = root.children[0];
    let child = scene.get(child_id).unwrap();
    match &child.kind {
        NodeKind::Text { content } => assert_eq!(content, "标题"),
        other => panic!("expected Text child, got {:?}", other),
    }
    // parent 指向 Container
    assert_eq!(child.parent, Some(scene.roots[0]));
}

#[test]
fn button_raw_text_becomes_text_child() {
    // button 同理：裸文本 → Text 子节点
    let html = r#"<button>确定</button>"#;
    let css = "";
    let tree = parse_html(html).unwrap();
    let sheet = parse_css(css).unwrap();
    let styles = resolve_styles(&tree, &sheet);
    let scene = build_scene(&tree, &styles);
    let btn = scene.get(scene.roots[0]).unwrap();
    assert!(matches!(btn.kind, NodeKind::Button));
    assert_eq!(btn.children.len(), 1);
    match &scene.get(btn.children[0]).unwrap().kind {
        NodeKind::Text { content } => assert_eq!(content, "确定"),
        _ => panic!("expected Text child"),
    }
}

#[test]
fn text_child_inherits_parent_text_fields_resets_size() {
    // Text 子节点应像无 class 的 <span>——继承父 color/font，
    // 但 taffy_style 取 DEFAULT（无固定 size，由测量决定）。
    // 父 .h{height:30px} 不应让文本子也高 30px。
    let html = r#"<div class="h">txt</div>"#;
    let css = r#".h { height: 30px; color: #ff0000; font-size: 20px; }"#;
    let tree = parse_html(html).unwrap();
    let sheet = parse_css(css).unwrap();
    let styles = resolve_styles(&tree, &sheet);
    let scene = build_scene(&tree, &styles);
    let root = scene.get(scene.roots[0]).unwrap();
    let child = scene.get(root.children[0]).unwrap();
    // 继承
    assert_eq!(child.style.color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(child.style.font_size, 20.0);
    // size 不继承：父 height=Length(30)，子 height 应是 Auto（由文本测量决定）
    use taffy::style::Dimension;
    assert!(
        matches!(child.style.taffy_style.size.height, Dimension::Auto),
        "text child height should be Auto (measured), not inherited parent's 30px"
    );
}

#[test]
fn draggable_attr_true_sets_node_draggable() {
    let html = r#"<div><button draggable="true">OK</button></div>"#;
    let css = "";
    let tree = parse_html(html).unwrap();
    let sheet = parse_css(css).unwrap();
    let styles = resolve_styles(&tree, &sheet);
    let scene = build_scene(&tree, &styles);
    let root = scene.get(scene.roots[0]).unwrap();
    // btn 是 root 的子（root 的 Text 子"OK"另算——button 裸文本→Text 子）。
    // 找 button kind 的子：
    let btn = scene
        .nodes
        .values()
        .find(|n| matches!(n.kind, NodeKind::Button))
        .expect("btn");
    assert!(btn.draggable, "draggable=\"true\" → Node.draggable=true");
    assert!(!root.draggable, "root 无 draggable 属性 → false");
}

#[test]
fn draggable_attr_absent_or_false_is_false() {
    let html = r#"<div draggable="false"><button draggable="yes">x</button></div>"#;
    let css = "";
    let tree = parse_html(html).unwrap();
    let sheet = parse_css(css).unwrap();
    let styles = resolve_styles(&tree, &sheet);
    let scene = build_scene(&tree, &styles);
    let root = scene.get(scene.roots[0]).unwrap();
    assert!(!root.draggable, "draggable=\"false\" → false");
    let btn = scene
        .nodes
        .values()
        .find(|n| matches!(n.kind, NodeKind::Button))
        .expect("btn");
    assert!(
        !btn.draggable,
        "draggable=\"yes\"（非 true）→ false（truthy 仅认 true）"
    );
}

#[test]
fn tabindex_attr_parsed() {
    let html = r#"<div><button tabindex="0">a</button><button tabindex="3">b</button><button tabindex="-1">c</button><button tabindex="abc">d</button><button>e</button></div>"#;
    let css = "";
    let tree = parse_html(html).unwrap();
    let sheet = parse_css(css).unwrap();
    let styles = resolve_styles(&tree, &sheet);
    let scene = build_scene(&tree, &styles);
    let btns: Vec<_> = scene
        .nodes
        .values()
        .filter(|n| matches!(n.kind, NodeKind::Button))
        .collect();
    assert_eq!(btns.len(), 5);
    assert_eq!(btns[0].tabindex, Some(0), "tabindex=\"0\" → Some(0)");
    assert_eq!(btns[1].tabindex, Some(3), "tabindex=\"3\" → Some(3)");
    assert_eq!(btns[2].tabindex, Some(-1), "tabindex=\"-1\" → Some(-1)");
    assert_eq!(btns[3].tabindex, None, "tabindex=\"abc\"（非数字）→ None");
    assert_eq!(btns[4].tabindex, None, "无 tabindex 属性 → None");
}
