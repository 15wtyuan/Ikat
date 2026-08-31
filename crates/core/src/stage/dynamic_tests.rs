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
    assert_eq!(
        sc.get(root).unwrap().base_style.taffy_style.size.width,
        Dimension::length(100.0)
    );
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
        NodeKind::TextNode => assert_eq!(sc.text_contents.get(&t).unwrap(), "hello"),
        _ => panic!("expected Text"),
    }
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

/// #110 视口字号：solve 期按当帧 root_size 解析，resolved px 沿继承链向下传
/// （子未声明 → 拷父解析值）；resize 后下帧跟随重解析。
#[test]
fn viewport_font_size_resolves_and_inherits() {
    let mut s = Stage::new_for_test(); // root 200×200
    let root = s.create_root("div", "font-size:2vmin").unwrap();
    let child = s.create_node("span", "").unwrap();
    s.append_child(root, child).unwrap();
    s.tick_and_render();
    let sc = s.scene.as_ref().unwrap();
    // 2vmin @ (200,200) = 2% × 200 = 4px；子未声明 → 继承父 resolved px
    assert_eq!(sc.get(root).unwrap().style.font_size, 4.0);
    assert_eq!(sc.get(child).unwrap().style.font_size, 4.0);
    // resize → vmin 分母变 → 下帧重解析 + 重继承
    s.set_root_size(400.0, 100.0).unwrap();
    s.tick_and_render();
    let sc = s.scene.as_ref().unwrap();
    assert_eq!(sc.get(root).unwrap().style.font_size, 2.0); // 2% × min(400,100)
    assert_eq!(sc.get(child).unwrap().style.font_size, 2.0);
}

/// #110 env(safe-area-inset-*)：Stage 注入值经 rematch/propagate + solve 全链生效——
/// padding 通道驱动子布局位移，inset 通道驱动绝对定位；改 inset 下帧跟随。
#[test]
fn env_safe_inset_flows_through_layout() {
    let mut s = Stage::new_for_test(); // root 200×200
    let root = s
        .create_root(
            "div",
            "position:relative;padding-top:env(safe-area-inset-top);width:100px;height:100px",
        )
        .unwrap();
    let kid = s.create_node("div", "width:10px;height:10px").unwrap();
    s.append_child(root, kid).unwrap();
    s.set_safe_insets([20.0, 0.0, 0.0, 0.0]).unwrap();
    s.tick_and_render();
    let sc = s.scene.as_ref().unwrap();
    assert_eq!(sc.get(kid).unwrap().layout_rect.y, 20.0);

    // 绝对定位 inset 通道：top 锚到 safe inset
    let abs = s
        .create_node(
            "div",
            "position:absolute;top:env(safe-area-inset-top);width:5px;height:5px",
        )
        .unwrap();
    s.append_child(root, abs).unwrap();
    s.tick_and_render();
    let sc = s.scene.as_ref().unwrap();
    assert_eq!(sc.get(abs).unwrap().layout_rect.y, 20.0);

    // inset 变更 → 下帧跟随（无需重声明）
    s.set_safe_insets([50.0, 0.0, 0.0, 0.0]).unwrap();
    s.tick_and_render();
    let sc = s.scene.as_ref().unwrap();
    assert_eq!(sc.get(kid).unwrap().layout_rect.y, 50.0);
    assert_eq!(sc.get(abs).unwrap().layout_rect.y, 50.0);
}

/// #110 视口字号 + env 通道的字号继承边界：子自有 px 声明不被父视口值覆盖
/// （set-ness 由 apply_css 记 bit）。
#[test]
fn viewport_font_size_child_own_px_decl_wins() {
    let mut s = Stage::new_for_test();
    let root = s.create_root("div", "font-size:4vmin").unwrap();
    let child = s.create_node("span", "font-size:12px").unwrap();
    s.append_child(root, child).unwrap();
    s.tick_and_render();
    let sc = s.scene.as_ref().unwrap();
    assert_eq!(sc.get(root).unwrap().style.font_size, 8.0); // 4% × 200
    assert_eq!(sc.get(child).unwrap().style.font_size, 12.0); // 自有声明胜
}

/// #110 视口 padding/gap：solve 建树期换算覆写 taffy 副本（布局位移即证据）。
#[test]
fn viewport_padding_gap_flow_into_layout() {
    let mut s = Stage::new_for_test();
    let root = s
        .create_root(
            "div",
            "padding:5vmin;row-gap:2vmin;width:100px;height:100px",
        )
        .unwrap();
    let a = s.create_node("div", "width:10px;height:10px").unwrap();
    let b = s.create_node("div", "width:10px;height:10px").unwrap();
    s.append_child(root, a).unwrap();
    s.append_child(root, b).unwrap();
    s.tick_and_render();
    let sc = s.scene.as_ref().unwrap();
    // root 200×200 默认 flex? div→Block。padding 5vmin=10px；子 y 起 10。
    assert_eq!(sc.get(a).unwrap().layout_rect.y, 10.0);
    assert_eq!(sc.get(a).unwrap().layout_rect.x, 10.0);
}
