use super::*;
use crate::scene::node::NodeKind;

#[test]
fn create_node_and_append_builds_tree() {
    let mut s = Stage::new_for_test();
    let root = s.create_root("div", "width:100px;height:100px").unwrap();
    let child = s.create_node("div", "width:50px;height:50px").unwrap();
    s.append_child(root, child).unwrap();
    let sc = s.scene.as_ref().unwrap();
    assert_eq!(sc.roots, vec![root]);
    assert_eq!(sc.get(root).unwrap().children, vec![child]);
    assert_eq!(sc.get(child).unwrap().parent, Some(root));
    // CSS 应用生效：base_style width 100px
    use taffy::style::Dimension;
    assert!(matches!(
        sc.get(root).unwrap().base_style.taffy_style.size.width,
        Dimension::Length(100.0)
    ));
}

#[test]
fn set_text_changes_content_and_marks_dirty() {
    let mut s = Stage::new_for_test();
    let t = s.create_node("span", "").unwrap();
    // create_node 时 Text 节点 dirty_text=true，先清掉验 set_text 重标
    s.scene.as_mut().unwrap().get_mut(t).unwrap().dirty_text = false;
    s.set_text(t, "hello").unwrap();
    let sc = s.scene.as_ref().unwrap();
    assert!(sc.get(t).unwrap().dirty_text);
    match &sc.get(t).unwrap().kind {
        NodeKind::Text { content } => assert_eq!(content, "hello"),
        _ => panic!("expected Text"),
    }
}

#[test]
fn set_style_changes_base_style() {
    let mut s = Stage::new_for_test();
    let n = s.create_node("div", "").unwrap();
    s.set_style(n, "background-color:#ff0000").unwrap();
    let bg = s
        .scene
        .as_ref()
        .unwrap()
        .get(n)
        .unwrap()
        .base_style
        .background_color;
    assert_eq!(bg, Some([1.0, 0.0, 0.0, 1.0]));
}

#[test]
fn remove_child_detaches_but_keeps_node() {
    let mut s = Stage::new_for_test();
    let root = s.create_root("div", "").unwrap();
    let child = s.create_node("div", "").unwrap();
    s.append_child(root, child).unwrap();
    s.remove_child(root, child).unwrap();
    let sc = s.scene.as_ref().unwrap();
    assert!(sc.get(root).unwrap().children.is_empty());
    assert!(
        sc.get(child).unwrap().parent.is_none(),
        "child 变孤立但仍存活"
    );
    assert!(sc.get(child).is_some());
}

/// 动态建树后 tick_and_render 正确渲染（layout solve 每帧从零建 taffy 树，自动跟进结构变更）。
/// 核心不变量：动态建的树经完整管线（solve+compute+render）不 panic，frame 产出。
/// 注：merge_meshes 会把同 DrawState 的 Mesh 节点合并 → frame.nodes.len() 可小于节点数，
/// 故只断言 frame 非空 + 至少一个 Mesh 含几何（证明渲染吃到动态建的树）。
#[test]
fn dynamic_tree_tick_and_render_does_not_panic() {
    let mut s = Stage::new_for_test();
    let root = s.create_root("div", "width:200px;height:200px").unwrap();
    let child = s
        .create_node("div", "width:100px;height:100px;background-color:#00ff00")
        .unwrap();
    s.append_child(root, child).unwrap();
    // 完整管线跑一遍：solve 建 taffy 树 + compute_world_transforms + render
    let frame = s.tick_and_render();
    // frame 非空 + 至少一个 Mesh 含顶点（root/child 合并后仍应有几何）
    assert!(!frame.nodes.is_empty(), "动态建的树应渲染出节点");
    let has_mesh = frame.nodes.iter().any(|rn| {
            matches!(&rn.payload, crate::render::node::NodePayload::Mesh { verts, .. } if !verts.is_empty())
        });
    assert!(has_mesh, "应有含几何的 Mesh 节点（动态树渲染产出）");
    // 再 tick 一帧（dirty 标志清后稳定，仍不 panic）
    s.tick_and_render();
}

/// set_text 后 tick_and_render 重算文本（dirty_text → render 重测）。
#[test]
fn set_text_then_tick_renders() {
    let mut s = Stage::new_for_test();
    let t = s.create_node("span", "width:100px;height:20px").unwrap();
    s.set_text(t, "hi").unwrap();
    let frame = s.tick_and_render();
    // span 节点应进 frame
    assert!(!frame.nodes.is_empty());
}

/// create_node 拒绝未知 tag。
#[test]
fn create_node_rejects_unknown_tag() {
    let mut s = Stage::new_for_test();
    assert!(s.create_node("ul", "").is_err());
}

/// insert_before 中间插入经 Stage API。
#[test]
fn stage_insert_before_middle() {
    let mut s = Stage::new_for_test();
    let root = s.create_root("div", "").unwrap();
    let a = s.create_node("div", "").unwrap();
    let b = s.create_node("div", "").unwrap();
    let c = s.create_node("div", "").unwrap();
    s.append_child(root, a).unwrap();
    s.append_child(root, b).unwrap();
    s.insert_before(root, c, a).unwrap();
    let sc = s.scene.as_ref().unwrap();
    assert_eq!(sc.get(root).unwrap().children, vec![c, a, b]);
}
