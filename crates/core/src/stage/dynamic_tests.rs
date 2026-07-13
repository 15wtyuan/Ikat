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

/// Controller 切页 round-trip：get_controller 定位挂载点 → set_selected_index 切页 →
/// get_selected_index 读回。覆盖核心 registry + Stage API 闭环。
#[test]
fn set_selected_index_round_trips() {
    let mut s = Stage::new_for_test();
    let root = s.create_root("div", "").unwrap();
    let mount = s.create_node("div", "").unwrap();
    s.append_child(root, mount).unwrap();
    // 挂载点声明 data-controller="tab"（运行时通常由 instantiate 从模板填，
    // 这里直接写字段模拟——create_node 不暴露 HTML 属性）
    s.scene
        .as_mut()
        .unwrap()
        .get_mut(mount)
        .unwrap()
        .data_controller = Some("tab".to_string());

    // get_controller 在子树内找名为 "tab" 的挂载点
    let found = s.get_controller(root, "tab").expect("应找到 tab 挂载点");
    assert_eq!(found, mount);

    // 初始无条目 → get_selected_index 返 -1
    assert_eq!(s.get_selected_index(mount), -1);

    // 切到第 2 页
    let prev = s.set_selected_index(mount, 2);
    assert_eq!(prev, -1, "首次 set 返 prev=-1（无条目）");
    assert_eq!(s.get_selected_index(mount), 2);

    // 再切到第 0 页，prev 应为 2
    let prev = s.set_selected_index(mount, 0);
    assert_eq!(prev, 2);
    assert_eq!(s.get_selected_index(mount), 0);

    // 切页事件入 pending_controller_events（prev != new 才推）
    let sc = s.scene.as_ref().unwrap();
    assert_eq!(sc.pending_controller_events.len(), 2);
    assert_eq!(
        sc.pending_controller_events[0],
        crate::scene::node::ControllerChangedEvent {
            mount_node: mount.0,
            prev: -1,
            new: 2
        }
    );
    assert_eq!(
        sc.pending_controller_events[1],
        crate::scene::node::ControllerChangedEvent {
            mount_node: mount.0,
            prev: 2,
            new: 0
        }
    );
}

/// set_selected_index 对无效 mount 静默 no-op（不 panic，返 -1）。
/// 覆盖 FFI no-panic 约定：mount 节点不存在 / 未挂 data-controller 都视为无效。
#[test]
fn set_selected_index_invalid_mount_no_op() {
    let mut s = Stage::new_for_test();
    let root = s.create_root("div", "").unwrap();
    // root 未挂 data-controller → set_selected_index 静默返 -1
    let prev = s.set_selected_index(root, 1);
    assert_eq!(prev, -1);
    assert_eq!(s.get_selected_index(root), -1);
    // 无效 NodeId → 同样静默返 -1
    let bogus = crate::scene::node::NodeId(0xFFFF_FFFF);
    assert_eq!(s.set_selected_index(bogus, 1), -1);
    // pending_controller_events 不应被推入
    assert!(s
        .scene
        .as_ref()
        .unwrap()
        .pending_controller_events
        .is_empty());
}

/// get_controller 子树内无匹配 name → None。
#[test]
fn get_controller_no_match_returns_none() {
    let mut s = Stage::new_for_test();
    let root = s.create_root("div", "").unwrap();
    let mount = s.create_node("div", "").unwrap();
    s.append_child(root, mount).unwrap();
    s.scene
        .as_mut()
        .unwrap()
        .get_mut(mount)
        .unwrap()
        .data_controller = Some("tab".to_string());
    // 查 "other" → None
    assert!(s.get_controller(root, "other").is_none());
    // 在 mount 子树内查 "tab"（mount 自身匹配）→ Some(mount)
    assert_eq!(s.get_controller(mount, "tab"), Some(mount));
}

/// rich_link_at pull 查询：命中 fragment rect 返 link_id；越界/非 RichText/无 fragment 返 0。
#[test]
fn rich_link_at_returns_link_id_on_hit() {
    use crate::text::rich::RichFragment;
    let mut s = Stage::new_for_test();
    let node = s.create_node("span", "").unwrap();
    // span → RichText（create_node 按 tag 映射；span 是 RichText 叶）
    {
        let sc = s.scene.as_mut().unwrap();
        let n = sc.get_mut(node).unwrap();
        n.kind = NodeKind::RichText { runs: vec![] };
        n.layout_rect = crate::scene::node::Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 40.0,
        };
    }
    // 填 world_transforms（单位矩阵——world == local）+ rich_fragments
    {
        let sc = s.scene.as_mut().unwrap();
        let idx = node.index();
        sc.world_transforms = vec![crate::transform::IDENTITY; idx + 1];
        sc.rich_fragments = vec![None; idx + 1];
        sc.rich_fragments[idx] = Some(vec![
            RichFragment {
                x: 10.0,
                y: 5.0,
                w: 50.0,
                h: 20.0,
                link_id: 1,
            },
            RichFragment {
                x: 70.0,
                y: 5.0,
                w: 30.0,
                h: 20.0,
                link_id: 2,
            },
        ]);
    }
    // 命中第一个 fragment → link_id=1
    assert_eq!(s.rich_link_at(node, 30.0, 15.0), 1);
    // 命中第二个 fragment → link_id=2
    assert_eq!(s.rich_link_at(node, 80.0, 10.0), 2);
    // 越界（rect 外）→ 0
    assert_eq!(s.rich_link_at(node, 5.0, 5.0), 0);
    assert_eq!(s.rich_link_at(node, 200.0, 5.0), 0);
}

/// rich_link_at 对非 RichText 节点返 0（Container/Text/Image 都不算）。
#[test]
fn rich_link_at_non_rich_text_returns_zero() {
    let mut s = Stage::new_for_test();
    let node = s.create_node("div", "").unwrap(); // div = Container
    assert_eq!(s.rich_link_at(node, 0.0, 0.0), 0);
}

/// rich_link_at 对失效 NodeId 静默返 0（不 panic，FFI no-panic 约定）。
#[test]
fn rich_link_at_invalid_node_returns_zero() {
    let s = Stage::new_for_test();
    let bogus = crate::scene::node::NodeId(0xFFFF_FFFF);
    assert_eq!(s.rich_link_at(bogus, 0.0, 0.0), 0);
}

/// rich_link_at 对世界坐标做反变换：节点有 transform（translate 100,50）时，
/// 世界点 (130, 65) → 本地 (30, 15) 命中 fragment。
#[test]
fn rich_link_at_inverse_transforms_world_point() {
    use crate::text::rich::RichFragment;
    let mut s = Stage::new_for_test();
    let node = s.create_node("span", "").unwrap();
    {
        let sc = s.scene.as_mut().unwrap();
        let n = sc.get_mut(node).unwrap();
        n.kind = NodeKind::RichText { runs: vec![] };
    }
    let idx = node.index();
    {
        let sc = s.scene.as_mut().unwrap();
        // world matrix = translate(100, 50)
        sc.world_transforms = vec![crate::transform::from_translate(100.0, 50.0); idx + 1];
        sc.rich_fragments = vec![None; idx + 1];
        sc.rich_fragments[idx] = Some(vec![RichFragment {
            x: 20.0,
            y: 10.0,
            w: 40.0,
            h: 20.0,
            link_id: 3,
        }]);
    }
    // 世界 (130, 65) → 本地 (30, 15)，落在 fragment (20..60, 10..30) 内 → link_id=3
    assert_eq!(s.rich_link_at(node, 130.0, 65.0), 3);
    // 世界 (110, 55) → 本地 (10, 5)，fragment 外 → 0
    assert_eq!(s.rich_link_at(node, 110.0, 55.0), 0);
}
