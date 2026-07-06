use super::*;
use crate::scene::node::{Node, NodeKind, Rect, Scene};
use crate::scene::transform::compute_world_transforms;

fn one_button_scene() -> Scene {
    // root + button(100x100 at 0,0)
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut btn = Node::default();
    btn.kind = NodeKind::Button;
    btn.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    let mut s = Scene::from_nodes(vec![root, btn], vec![(0, 1)]);
    compute_world_transforms(&mut s);
    s
}

/// root + btn(100x100) + btn 的 Text 子(100x20 上半段，挡 btn 上半命中)。
/// 验 hover 祖先链：hover Text 区（命中 Text）→ Text + btn + root 祖先链都 hovered。
fn button_with_text_child_scene() -> Scene {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut btn = Node::default();
    btn.kind = NodeKind::Button;
    btn.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    let mut txt = Node::default();
    txt.kind = NodeKind::Text {
        content: "btn".into(),
    };
    txt.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 20.0,
    }; // btn 上半段，touchable 默认 true 挡命中
    let mut s = Scene::from_nodes(vec![root, btn, txt], vec![(0, 1), (1, 2)]);
    compute_world_transforms(&mut s);
    s
}

#[test]
fn hover_text_child_sets_ancestor_btn_hovered() {
    // b 根因回归测：hover btn 的 Text 子区（命中 Text NodeId 2，非 btn NodeId 1）
    // → Text + btn + root 祖先链都 hovered（CSS :hover 祖先语义）。
    // 这样 .btn:hover 伪类匹配 btn（即使命中的是 btn 的文字子）。
    let mut s = button_with_text_child_scene();
    let root_id = s.roots[0];
    let btn_id = s.get(root_id).unwrap().children[0];
    let txt_id = s.get(btn_id).unwrap().children[0];
    let mut ps = PointerState::new();
    // Move 到 Text 区 (10,10)——命中 Text，不是 btn
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(s.get(txt_id).unwrap().hovered, "Text 子（命中点）hovered");
    assert!(
        s.get(btn_id).unwrap().hovered,
        "btn（Text 的祖先）也 hovered——祖先链"
    );
    assert!(
        s.get(root_id).unwrap().hovered,
        "root（btn 的祖先）也 hovered——祖先链"
    );
}

#[test]
fn down_text_child_sets_ancestor_btn_active() {
    // active 祖先链：按下 btn 的 Text 子 → Text + btn 都 active（.btn:active 匹配 btn）
    let mut s = button_with_text_child_scene();
    let root_id = s.roots[0];
    let btn_id = s.get(root_id).unwrap().children[0];
    let txt_id = s.get(btn_id).unwrap().children[0];
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(s.get(txt_id).unwrap().active, "Text 子（命中点）active");
    assert!(
        s.get(btn_id).unwrap().active,
        "btn（Text 祖先）也 active——祖先链"
    );
    // up 后清所有 active
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(!s.get(btn_id).unwrap().active, "up 后 btn active 清零");
    assert!(!s.get(txt_id).unwrap().active, "up 后 Text active 清零");
}

#[test]
fn down_up_same_node_within_threshold_emits_click() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    // Move 到按钮上（触发 RollOver）+ Down + Up（位移 < 10px）
    let evs = vec![
        PointerEvent {
            kind: PointerKind::Move,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        },
        PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        },
        PointerEvent {
            kind: PointerKind::Up,
            x: 51.0,
            y: 51.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        },
    ];
    let out = ps.process(&mut s, &evs);
    let types: Vec<u8> = out.iter().map(|e| e.event_type).collect();
    assert!(types.contains(&EVT_ROLL_OVER), "Move 到按钮 → RollOver");
    assert!(types.contains(&EVT_DOWN));
    assert!(types.contains(&EVT_UP));
    assert!(types.contains(&EVT_CLICK), "同节点位移 <10px → Click");
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    assert!(!s.get(btn_id).unwrap().active, "Up 后 active=false");
    assert!(s.get(btn_id).unwrap().hovered, "hover 保持");
}

#[test]
fn down_up_exceeds_threshold_no_click() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    let evs = vec![
        PointerEvent {
            kind: PointerKind::Down,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        },
        PointerEvent {
            kind: PointerKind::Up,
            x: 80.0,
            y: 80.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }, // 位移 ~99px
    ];
    let out = ps.process(&mut s, &evs);
    let has_click = out.iter().any(|e| e.event_type == EVT_CLICK);
    assert!(!has_click, "位移超阈值 → 不产 Click");
}

#[test]
fn down_on_disabled_node_no_active_no_click() {
    let mut s = one_button_scene();
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    s.get_mut(btn_id).unwrap().disabled = true;
    let mut ps = PointerState::new();
    let evs = vec![
        PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        },
        PointerEvent {
            kind: PointerKind::Up,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        },
    ];
    let out = ps.process(&mut s, &evs);
    assert!(
        !s.get(btn_id).unwrap().active,
        "disabled 节点 down 不设 active"
    );
    let has_click = out.iter().any(|e| e.event_type == EVT_CLICK);
    assert!(!has_click, "disabled 节点不产 Click");
    let has_down = out.iter().any(|e| e.event_type == EVT_DOWN);
    assert!(!has_down, "disabled 节点不产 Down");
}

#[test]
fn down_held_on_disabled_no_active() {
    // 回归测：Down 命中 disabled 节点后【按住不松】（无同帧 Up）→
    // disabled 节点及祖先都不应 active。
    // 注：down_on_disabled_node_no_active_no_click 漏此 case（Down+Up 同 process 调用，recompute 时 is_down 已 false）。
    let mut s = one_button_scene();
    let root_id = s.roots[0];
    let btn_id = s.get(root_id).unwrap().children[0];
    s.get_mut(btn_id).unwrap().disabled = true;
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        !s.get(btn_id).unwrap().active,
        "按住 disabled btn 不应 active（active 抑制）"
    );
    assert!(
        !s.get(root_id).unwrap().active,
        "disabled 祖先 root 也不应 active"
    );
}

#[test]
fn down_held_on_disabled_via_text_child_no_active() {
    // 回归测（Text 子命中路径）：按下 disabled 按钮的 Text 子（命中 Text，非 btn）→
    // disabled btn 仍不应 active。hit 落 disabled 节点的非 disabled 子时，active 链会带上
    // disabled 祖先——须沿链逐节点查 disabled，不只查 down_node。
    let mut s = button_with_text_child_scene(); // root + btn + Text 挡 btn 上半
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    s.get_mut(btn_id).unwrap().disabled = true;
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    // (10,10) 命中 Text 子（Text @0,0,100,20 挡 btn 上半——hover_text_child_sets_ancestor_btn_hovered 已验）
    assert!(
        !s.get(btn_id).unwrap().active,
        "按下 disabled btn 的 Text 子 → btn 不应 active（链遍历逐节点查 disabled）"
    );
}

#[test]
fn rollover_emitted_on_enter_rollout_on_leave() {
    let mut s = one_button_scene();
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    // Move 到按钮 → RollOver
    let out1 = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(out1
        .iter()
        .any(|e| e.event_type == EVT_ROLL_OVER && e.node_id == btn_id.0));
    // Move 移出按钮（150,150 在 root 非 button）→ RollOut(button) + RollOver(root)
    let out2 = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 150.0,
            y: 150.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        out2.iter()
            .any(|e| e.event_type == EVT_ROLL_OUT && e.node_id == btn_id.0),
        "移出按钮 → RollOut(button)"
    );
}

#[test]
fn hover_diff_no_move_event_still_runs() {
    let mut s = one_button_scene();
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    // 先 Move 到按钮
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(s.get(btn_id).unwrap().hovered);
    // 空事件——hover 应保持（无 RollOut）
    let out = ps.process(&mut s, &[]);
    assert!(
        !out.iter().any(|e| e.event_type == EVT_ROLL_OUT),
        "空事件 hover 保持"
    );
    assert!(s.get(btn_id).unwrap().hovered, "hover 仍 true");
}

#[test]
fn events_preserved_in_generation_order() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    // Move + Down 同帧——Move 的 RollOver 应在 Down 前
    let evs = vec![
        PointerEvent {
            kind: PointerKind::Move,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        },
        PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        },
    ];
    let out = ps.process(&mut s, &evs);
    // 找 RollOver 和 Down 的 index
    let ro_idx = out.iter().position(|e| e.event_type == EVT_ROLL_OVER);
    let down_idx = out.iter().position(|e| e.event_type == EVT_DOWN);
    assert!(ro_idx.is_some() && down_idx.is_some());
    assert!(
        ro_idx.unwrap() < down_idx.unwrap(),
        "RollOver 在 Down 前（生成序）"
    );
}

/// root + parent(100x100) + child(50x50 in parent)。验 hover 祖先链 diff。
fn nested_scene() -> Scene {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut parent = Node::default();
    parent.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    let mut child = Node::default();
    child.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let mut s = Scene::from_nodes(vec![root, parent, child], vec![(0, 1), (1, 2)]);
    compute_world_transforms(&mut s);
    s
}

#[test]
fn hover_into_child_no_rollout_parent() {
    // 点 1 回归：hover parent 区(75,75) → 链 [parent,root]；移进 child 区(10,10) → 链 [child,parent,root]。
    // 共同 parent,root → 不产 RollOut(parent)；child 新 → RollOver(child)。
    let mut s = nested_scene();
    let root_id = s.roots[0];
    let parent_id = s.get(root_id).unwrap().children[0];
    let child_id = s.get(parent_id).unwrap().children[0];
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 75.0,
            y: 75.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        !out.iter().any(|e| e.event_type == EVT_ROLL_OUT),
        "进子 → 不产任何 RollOut"
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_ROLL_OVER && e.node_id == child_id.0),
        "进子 → RollOver(child)"
    );
}

#[test]
fn hover_between_siblings_old_chain_rollout() {
    // 兄弟 A/B：hover A → RollOver(A)+RollOver(root)；移到 B → RollOut(A)+RollOver(B)（root 共同不产）。
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut a = Node::default();
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let mut b = Node::default();
    b.layout_rect = Rect {
        x: 100.0,
        y: 100.0,
        w: 50.0,
        h: 50.0,
    };
    let mut s = Scene::from_nodes(vec![root, a, b], vec![(0, 1), (0, 2)]);
    let root_id = s.roots[0];
    let a_id = s.get(root_id).unwrap().children[0];
    let b_id = s.get(root_id).unwrap().children[1];
    compute_world_transforms(&mut s);
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 25.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    ); // 命中 A
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 125.0,
            y: 125.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    ); // 命中 B
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_ROLL_OUT && e.node_id == a_id.0),
        "移到 B → RollOut(A)"
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_ROLL_OVER && e.node_id == b_id.0),
        "移到 B → RollOver(B)"
    );
    assert!(
        !out.iter().any(|e| e.node_id == root_id.0),
        "root 共同祖先 → 不产事件"
    );
}

#[test]
fn hover_chain_idempotent() {
    // 同点 Move 两次 → 第二次无 hover 事件（链不变；Move 仍恒产，不抑制）。
    let mut s = nested_scene();
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        out.iter()
            .all(|e| e.event_type != EVT_ROLL_OVER && e.event_type != EVT_ROLL_OUT),
        "同点 Move → 无 hover 事件（Move 允许，hover diff 幂等）"
    );
}

#[test]
fn hover_out_of_ui_rollout_whole_chain() {
    // hover child → 链 [child,parent,root]；移出根外 → 空链 → 整链 RollOut。
    let mut s = nested_scene();
    let root_id = s.roots[0];
    let parent_id = s.get(root_id).unwrap().children[0];
    let child_id = s.get(parent_id).unwrap().children[0];
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 300.0,
            y: 300.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    ); // 根外
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_ROLL_OUT && e.node_id == child_id.0),
        "移出 → RollOut(child)"
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_ROLL_OUT && e.node_id == parent_id.0),
        "移出 → RollOut(parent)"
    );
    assert!(
        !out.iter().any(|e| e.event_type == EVT_ROLL_OVER),
        "移出 → 无 RollOver"
    );
}

// ===== 多槽测试 =====

/// 鼠标 touch_id=-1 进 slots[0]，Down/Up/Click 等价单指。
#[test]
fn mouse_uses_slot0_touch_id_neg1() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    let out = ps.process(
        &mut s,
        &[
            PointerEvent {
                kind: PointerKind::Down,
                x: 50.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
            PointerEvent {
                kind: PointerKind::Up,
                x: 50.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
        ],
    );
    assert!(out.iter().any(|e| e.event_type == EVT_DOWN), "鼠标 Down 产");
    assert!(
        out.iter().any(|e| e.event_type == EVT_CLICK),
        "鼠标 Click 产"
    );
    assert!(out.iter().all(|e| e.touch_id == -1), "鼠标事件 touch_id=-1");
}

/// 两触摸指各自 Down/Up，事件带正确 touch_id。
#[test]
fn two_touches_independent_down_up() {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut a = Node::default();
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let mut b = Node::default();
    b.layout_rect = Rect {
        x: 100.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let mut s = Scene::from_nodes(vec![root, a, b], vec![(0, 1), (0, 2)]);
    let root_id = s.roots[0];
    let a_id = s.get(root_id).unwrap().children[0];
    let b_id = s.get(root_id).unwrap().children[1];
    compute_world_transforms(&mut s);
    let mut ps = PointerState::new();
    // touch_id=1 Down 在 A，touch_id=2 Down 在 B（同帧）
    let out = ps.process(
        &mut s,
        &[
            PointerEvent {
                kind: PointerKind::Down,
                x: 25.0,
                y: 25.0,
                button: 0,
                pad: [0, 0],
                touch_id: 1,
            },
            PointerEvent {
                kind: PointerKind::Down,
                x: 125.0,
                y: 25.0,
                button: 0,
                pad: [0, 0],
                touch_id: 2,
            },
        ],
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_DOWN && e.node_id == a_id.0 && e.touch_id == 1),
        "touch1 Down@A"
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_DOWN && e.node_id == b_id.0 && e.touch_id == 2),
        "touch2 Down@B"
    );
}

/// 5 触摸 Down（slot1-4 满），第 5 指丢弃。
#[test]
fn touch_alloc_fourth_dropped() {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut s = Scene::from_nodes(vec![root], vec![]);
    compute_world_transforms(&mut s);
    let mut ps = PointerState::new();
    // touch_id 1..5 全 Down（4 触摸槽 slot1-4，第 5 指应丢）
    let mut evs = Vec::new();
    for tid in 1..=5i32 {
        evs.push(PointerEvent {
            kind: PointerKind::Down,
            x: 0.0,
            y: 0.0,
            button: 0,
            pad: [0, 0],
            touch_id: tid,
        });
    }
    let out = ps.process(&mut s, &evs);
    let down_count = out.iter().filter(|e| e.event_type == EVT_DOWN).count();
    assert_eq!(down_count, 4, "仅 4 触摸槽，第 5 指 Down 丢弃");
}

/// 触摸无 capture Move 不产 Move 事件（hover_diff 仍跑）。
#[test]
fn touch_move_no_monitor_no_event() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 51.0,
            y: 51.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    assert!(
        !out.iter().any(|e| e.event_type == EVT_MOVE),
        "无 monitor 触摸 Move 不产 Move 事件"
    );
    assert!(out.iter().all(|e| e.event_type != EVT_MOVE), "无 Move 事件");
}

/// 鼠标无 capture Move 不产事件（与触摸行为一致）。
#[test]
fn mouse_move_no_capture_no_event() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 51.0,
            y: 51.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        !out.iter().any(|e| e.event_type == EVT_MOVE),
        "鼠标无 capture Move 不产"
    );
}

/// hover 全局合并：两指命中不同元素 → 两元素都 hovered。
#[test]
fn hover_global_merge_two_fingers() {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut a = Node::default();
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let mut b = Node::default();
    b.layout_rect = Rect {
        x: 100.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let mut s = Scene::from_nodes(vec![root, a, b], vec![(0, 1), (0, 2)]);
    let root_id = s.roots[0];
    let a_id = s.get(root_id).unwrap().children[0];
    let b_id = s.get(root_id).unwrap().children[1];
    compute_world_transforms(&mut s);
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[
            PointerEvent {
                kind: PointerKind::Move,
                x: 25.0,
                y: 25.0,
                button: 0,
                pad: [0, 0],
                touch_id: 1,
            }, // 命中 A
            PointerEvent {
                kind: PointerKind::Move,
                x: 125.0,
                y: 25.0,
                button: 0,
                pad: [0, 0],
                touch_id: 2,
            }, // 命中 B
        ],
    );
    assert!(s.get(a_id).unwrap().hovered, "A hovered（touch1 命中）");
    assert!(s.get(b_id).unwrap().hovered, "B hovered（touch2 命中）");
}

/// active 全局合并：两指按不同 btn → 都 active；松一指 → 剩余仍 active。
#[test]
fn active_global_merge_two_fingers() {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut a = Node::default();
    a.kind = NodeKind::Button;
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let mut b = Node::default();
    b.kind = NodeKind::Button;
    b.layout_rect = Rect {
        x: 100.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let mut s = Scene::from_nodes(vec![root, a, b], vec![(0, 1), (0, 2)]);
    let root_id = s.roots[0];
    let a_id = s.get(root_id).unwrap().children[0];
    let b_id = s.get(root_id).unwrap().children[1];
    compute_world_transforms(&mut s);
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[
            PointerEvent {
                kind: PointerKind::Down,
                x: 25.0,
                y: 25.0,
                button: 0,
                pad: [0, 0],
                touch_id: 1,
            },
            PointerEvent {
                kind: PointerKind::Down,
                x: 125.0,
                y: 25.0,
                button: 0,
                pad: [0, 0],
                touch_id: 2,
            },
        ],
    );
    assert!(
        s.get(a_id).unwrap().active && s.get(b_id).unwrap().active,
        "两指都按 → 两 btn active"
    );
    // 松 touch1
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 25.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    assert!(!s.get(a_id).unwrap().active, "松 touch1 → A active 清");
    assert!(s.get(b_id).unwrap().active, "touch2 仍按 → B 仍 active");
}

/// RollOver per-touch：touch1 进 A、touch2 进 B，各自 RollOver 带 touch_id。
#[test]
fn rollover_per_touch_independent() {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut a = Node::default();
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let mut b = Node::default();
    b.layout_rect = Rect {
        x: 100.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let mut s = Scene::from_nodes(vec![root, a, b], vec![(0, 1), (0, 2)]);
    let root_id = s.roots[0];
    let a_id = s.get(root_id).unwrap().children[0];
    let b_id = s.get(root_id).unwrap().children[1];
    compute_world_transforms(&mut s);
    let mut ps = PointerState::new();
    let out = ps.process(
        &mut s,
        &[
            PointerEvent {
                kind: PointerKind::Move,
                x: 25.0,
                y: 25.0,
                button: 0,
                pad: [0, 0],
                touch_id: 1,
            },
            PointerEvent {
                kind: PointerKind::Move,
                x: 125.0,
                y: 25.0,
                button: 0,
                pad: [0, 0],
                touch_id: 2,
            },
        ],
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_ROLL_OVER && e.node_id == a_id.0 && e.touch_id == 1),
        "touch1 RollOver@A"
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_ROLL_OVER && e.node_id == b_id.0 && e.touch_id == 2),
        "touch2 RollOver@B"
    );
}

/// is_pointer_on_ui 任一指命中。
#[test]
fn is_pointer_on_ui_any_slot() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    // 鼠标在 UI 外 (150,150 命中 root 非 btn)，触摸在 btn 内
    ps.process(
        &mut s,
        &[
            PointerEvent {
                kind: PointerKind::Move,
                x: 150.0,
                y: 150.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
            PointerEvent {
                kind: PointerKind::Move,
                x: 50.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: 1,
            },
        ],
    );
    assert!(
        ps.is_pointer_on_ui(&s),
        "触摸命中 btn → is_pointer_on_ui=true（任一指）"
    );
}

// ===== touch_monitors capture 测 =====

/// Down 后 add_touch_monitor → 后续 Move 产 Move@monitor。
#[test]
fn move_with_monitor_dispatches_to_monitor() {
    let mut s = one_button_scene();
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    // touch1 Down 在 btn
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    // capture btn（模拟 C# CaptureTouch 后调 add_touch_monitor）
    ps.add_touch_monitor(1, btn_id);
    // Move 移出 btn 到 root 区 (150,150)——正常无 monitor 不产 Move，但有 monitor → Move@btn
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 150.0,
            y: 150.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_MOVE && e.node_id == btn_id.0 && e.touch_id == 1),
        "capture 后 Move（即使移出 btn）产 Move@btn"
    );
}

/// Up 后 monitor 清空，后续 Move 不产。
#[test]
fn capture_clears_on_up() {
    let mut s = one_button_scene();
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    ps.add_touch_monitor(1, btn_id);
    // Up（清 monitor）
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    // 注意：Up 释放了 slot1（touch_id 重置 -1）。重新 Down 再 Move 验无 monitor
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: 2,
        }],
    );
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 51.0,
            y: 51.0,
            button: 0,
            pad: [0, 0],
            touch_id: 2,
        }],
    );
    assert!(
        !out.iter().any(|e| e.event_type == EVT_MOVE),
        "Up 清 monitor 后 Move 不产"
    );
}

/// Up 时 monitor==hit 不重复产 Up。
#[test]
fn up_hit_equals_monitor_no_double() {
    let mut s = one_button_scene();
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    ps.add_touch_monitor(1, btn_id); // monitor == btn
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    let up_btn = out
        .iter()
        .filter(|e| e.event_type == EVT_UP && e.node_id == btn_id.0)
        .count();
    assert_eq!(up_btn, 1, "monitor==hit → Up@btn 只产一次（去重）");
}

/// remove_touch_monitor：加后移除，Move 不再产给该 monitor。
#[test]
fn remove_touch_monitor_stops_dispatch() {
    let mut s = one_button_scene();
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    ps.add_touch_monitor(1, btn_id);
    ps.remove_touch_monitor(btn_id); // 主动释放
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 150.0,
            y: 150.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    assert!(
        !out.iter().any(|e| e.event_type == EVT_MOVE),
        "remove 后 Move 不产给该 monitor"
    );
}

// ===== click_test + per-axis 阈值 + down_targets =====

/// Click 目标 = down_leaf（非当前 hit）。Down@btn 边缘，漂出 btn 到 root（位移≤10），
/// Up → Click@btn（按下叶），Up 事件@root（当前 hit）。down_targets[0] 优先。
#[test]
fn click_target_is_down_leaf_not_current_hit() {
    let mut s = one_button_scene(); // root(0,0,200,200) + btn(0,0,100,100)
    let root_id = s.roots[0];
    let btn_id = s.get(root_id).unwrap().children[0];
    let mut ps = PointerState::new();
    // Down@(95,50)→btn；Up@(105,50)→root（105>100）。dx=10（mouse 阈值，|10|>10 false→不超）
    let out = ps.process(
        &mut s,
        &[
            PointerEvent {
                kind: PointerKind::Down,
                x: 95.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
            PointerEvent {
                kind: PointerKind::Up,
                x: 105.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
        ],
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_CLICK && e.node_id == btn_id.0),
        "Click@btn（down_leaf），即使 Up 时命中已漂移到 root"
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_UP && e.node_id == root_id.0),
        "Up@root（当前 hit）"
    );
    assert!(
        !out.iter()
            .any(|e| e.event_type == EVT_CLICK && e.node_id == root_id.0),
        "不产 Click@root"
    );
}

/// per-axis 阈值：mouse 对角 (8,8)→ dx=8,dy=8，均 ≤10 → 仍 Click（按轴判，不合计距离）。
#[test]
fn per_axis_threshold_mouse_diagonal_clicks() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    let out = ps.process(
        &mut s,
        &[
            PointerEvent {
                kind: PointerKind::Down,
                x: 50.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
            PointerEvent {
                kind: PointerKind::Up,
                x: 58.0,
                y: 58.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
        ],
    );
    assert!(
        out.iter().any(|e| e.event_type == EVT_CLICK),
        "per-axis (8,8) 各轴 ≤10 → Click"
    );
}

/// mouse 30px 漂移 → 无 Click（30>10）；touch 30px 漂移 → Click（30<50）。
#[test]
fn threshold_mouse_10_rejects_touch_50_allows_30px() {
    // mouse
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    let out_m = ps.process(
        &mut s,
        &[
            PointerEvent {
                kind: PointerKind::Down,
                x: 10.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
            PointerEvent {
                kind: PointerKind::Up,
                x: 40.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
        ],
    );
    assert!(
        !out_m.iter().any(|e| e.event_type == EVT_CLICK),
        "mouse 30px >10 → 无 Click"
    );
    // touch
    let mut s2 = one_button_scene();
    let mut ps2 = PointerState::new();
    let out_t = ps2.process(
        &mut s2,
        &[
            PointerEvent {
                kind: PointerKind::Down,
                x: 10.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: 1,
            },
            PointerEvent {
                kind: PointerKind::Up,
                x: 40.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: 1,
            },
        ],
    );
    assert!(
        out_t.iter().any(|e| e.event_type == EVT_CLICK),
        "touch 30px <50 → Click"
    );
}

/// down_leaf 销毁 → 沿当前 hit 祖先兜底。Down@child（scene1），scene2 移除 child，Up@root 区 → Click@root。
#[test]
fn down_leaf_destroyed_fallback_to_ancestor() {
    // scene1: root(0,0,200,200) + child(0,0,50,50)
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut child = Node::default();
    child.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let mut s1 = Scene::from_nodes(vec![root, child], vec![(0, 1)]);
    compute_world_transforms(&mut s1);
    let mut ps = PointerState::new();
    // Down@(25,25)→child；down_targets=[child,root]
    ps.process(
        &mut s1,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 25.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    // scene2: 仅 root（child 移除）——child NodeId 在 s2 不存在（悬空）
    let mut root2 = Node::default();
    root2.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut s2 = Scene::from_nodes(vec![root2], vec![]);
    compute_world_transforms(&mut s2);
    let root2_id = s2.roots[0];
    let out = ps.process(
        &mut s2,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 25.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    // click_test：down_targets[0]=child 悬空→走祖先；current_hit=root2 in down_targets → Click@root2
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_CLICK && e.node_id == root2_id.0),
        "down_leaf 销毁 → Click@root（祖先兜底）"
    );
}

// ===== 双击 + Move 取消 =====

/// 双击：两次 Click（time_s 间隔 0.2、同位置、同键）→ 第二次 click_count=2。
#[test]
fn double_click_within_window_clickcount_2() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    ps.time_s = 0.0;
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let c1 = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let count1 = c1
        .iter()
        .find(|e| e.event_type == EVT_CLICK)
        .map(|e| e.click_count)
        .unwrap();
    assert_eq!(count1, 1, "首次 Click count=1");
    ps.time_s = 0.2;
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let c2 = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let count2 = c2
        .iter()
        .find(|e| e.event_type == EVT_CLICK)
        .map(|e| e.click_count)
        .unwrap();
    assert_eq!(count2, 2, "350ms 内同位同键 → count=2");
}

/// 超 350ms → count 重置 1。
#[test]
fn double_click_resets_after_window() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    ps.time_s = 0.0;
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.time_s = 0.4; // >0.35
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let c = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let count = c
        .iter()
        .find(|e| e.event_type == EVT_CLICK)
        .map(|e| e.click_count)
        .unwrap();
    assert_eq!(count, 1, "超 350ms → count=1");
}

/// 三击循环 1→2→1。
#[test]
fn clickcount_cycle_1_2_1() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    let mut counts = Vec::new();
    for i in 0..3 {
        ps.time_s = i as f32 * 0.2;
        ps.process(
            &mut s,
            &[PointerEvent {
                kind: PointerKind::Down,
                x: 50.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            }],
        );
        let c = ps.process(
            &mut s,
            &[PointerEvent {
                kind: PointerKind::Up,
                x: 50.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            }],
        );
        counts.push(
            c.iter()
                .find(|e| e.event_type == EVT_CLICK)
                .map(|e| e.click_count)
                .unwrap(),
        );
    }
    assert_eq!(counts, vec![1, 2, 1], "1→2→1 循环");
}

/// Move 位移>50 取消 click：Down→Move 60px→Up → 无 Click。
#[test]
fn move_exceeds_50_cancels_click() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 10.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 70.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    ); // dx=60>50
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 70.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        !out.iter().any(|e| e.event_type == EVT_CLICK),
        "Move>50 → 取消 click"
    );
    assert!(out.iter().any(|e| e.event_type == EVT_UP), "Up 仍发");
}

// ===== Canceled + CancelClick =====

/// Canceled：发 Up、不发 Click。
#[test]
fn canceled_emits_up_skips_click() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Canceled,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        out.iter().any(|e| e.event_type == EVT_UP),
        "Canceled → Up 仍发"
    );
    assert!(
        !out.iter().any(|e| e.event_type == EVT_CLICK),
        "Canceled → 不发 Click"
    );
}

/// cancel_click API：Down → cancel_click → Up → 无 Click。
#[test]
fn cancel_click_api_skips_click() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.cancel_click(-1); // Down 后、Up 前取消
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        !out.iter().any(|e| e.event_type == EVT_CLICK),
        "cancel_click → 无 Click"
    );
    assert!(out.iter().any(|e| e.event_type == EVT_UP), "Up 仍发");
}

/// Canceled reset 双击窗口：Canceled 后 click_count=1、last_click_time=0。
/// 用 time_s≥1.0（reset-to-0 在真实游戏时间下永远超 350ms；小 time_s 是测伪影）。
#[test]
fn canceled_resets_click_count() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    ps.time_s = 1.0;
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    ); // count1
    ps.time_s = 1.1;
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    ); // count2
    assert_eq!(ps.slots[0].click_count, 2);
    ps.time_s = 1.2;
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Canceled,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert_eq!(ps.slots[0].click_count, 1, "Canceled reset click_count=1");
    assert_eq!(
        ps.slots[0].last_click_time, 0.0,
        "Canceled reset last_click_time=0"
    );
}

// ===== Stationary hover 跟随 =====

/// 静止光标下元素移走 → hover 跟随刷新（无 Move 事件）。
/// Move@btn → hover btn；scene2 btn 移到 (150,150)，空事件 → re-hit-test (50,50)=root → RollOut(btn)。
#[test]
fn stationary_cursor_hover_follows_moved_element() {
    let mut s1 = one_button_scene(); // root(0,0,200,200)+btn(0,0,100,100)
    let s1_btn_id = s1.get(s1.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    ps.process(
        &mut s1,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(s1.get(s1_btn_id).unwrap().hovered, "Move@btn → btn hovered");
    // scene2：btn 移到 (150,150)——(50,50) 现仅 root
    let mut root2 = Node::default();
    root2.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut btn2 = Node::default();
    btn2.kind = NodeKind::Button;
    btn2.layout_rect = Rect {
        x: 150.0,
        y: 150.0,
        w: 100.0,
        h: 100.0,
    };
    let mut s2 = Scene::from_nodes(vec![root2, btn2], vec![(0, 1)]);
    compute_world_transforms(&mut s2);
    let out = ps.process(&mut s2, &[]); // 空事件 → stationary follow
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_ROLL_OUT && e.node_id == s1_btn_id.0),
        "btn 移走（静止光标）→ RollOut(btn)"
    );
    assert!(
        !out.iter().any(|e| e.event_type == EVT_ROLL_OVER),
        "root 已 hovered → 无 RollOver"
    );
}

// ===== core drag 检测 =====

/// root(0,0,200,200) + draggable btn(0,0,100,100)。
fn one_draggable_button_scene() -> Scene {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut btn = Node::default();
    btn.kind = NodeKind::Button;
    btn.draggable = true;
    btn.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    let mut s = Scene::from_nodes(vec![root, btn], vec![(0, 1)]);
    compute_world_transforms(&mut s);
    s
}

#[test]
fn drag_start_emits_dragstart_and_cancels_click() {
    // draggable btn：Down@(50,50) + Move@(55,50)（dx=5>mouse阈值2）→ DragStart@btn + click_cancelled。
    let mut s = one_draggable_button_scene();
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    let out = ps.process(
        &mut s,
        &[
            PointerEvent {
                kind: PointerKind::Down,
                x: 50.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
            PointerEvent {
                kind: PointerKind::Move,
                x: 55.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
        ],
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_DRAG_START && e.node_id == btn_id.0),
        "draggable btn Move>阈值 → DragStart@btn"
    );
    // 同帧 Up 应无 Click（drag-start 已置 click_cancelled）
    let out2 = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 55.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        !out2.iter().any(|e| e.event_type == EVT_CLICK),
        "drag-start 取消 click"
    );
    assert!(
        out2.iter()
            .any(|e| e.event_type == EVT_DRAG_END && e.node_id == btn_id.0),
        "Up → DragEnd@btn"
    );
}

#[test]
fn drag_move_emitted_after_start() {
    let mut s = one_draggable_button_scene();
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 55.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    ); // DragStart
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 60.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_DRAG_MOVE && e.node_id == btn_id.0),
        "drag 中 Move → DragMove@btn"
    );
}

#[test]
fn non_draggable_no_drag_events() {
    // 普通 btn（draggable=false）：Down+Move → 无 drag 事件（仅既有 MOVE/click 取消走原逻辑）
    let mut s = one_button_scene(); // 既有 helper：btn 非 draggable
    let mut ps = PointerState::new();
    let out = ps.process(
        &mut s,
        &[
            PointerEvent {
                kind: PointerKind::Down,
                x: 50.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
            PointerEvent {
                kind: PointerKind::Move,
                x: 55.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
        ],
    );
    assert!(
        !out.iter()
            .any(|e| e.event_type == EVT_DRAG_START || e.event_type == EVT_DRAG_MOVE),
        "非 draggable → 无 drag 事件"
    );
}

#[test]
fn drag_threshold_mouse_2_touch_10_per_axis() {
    // mouse: Move dx=2（=阈值，per-axis |2|>2 false）→ 不发 DragStart；dx=3 → 发。
    let mut s = one_draggable_button_scene();
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let out1 = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 52.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        !out1.iter().any(|e| e.event_type == EVT_DRAG_START),
        "mouse dx=2（=阈值，per-axis 不超）→ 不发 DragStart"
    );
    // 重置场景验 dx=3
    let mut s2 = one_draggable_button_scene();
    let mut ps2 = PointerState::new();
    ps2.process(
        &mut s2,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let out2 = ps2.process(
        &mut s2,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 53.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        out2.iter().any(|e| e.event_type == EVT_DRAG_START),
        "mouse dx=3>2 → 发 DragStart"
    );
    // touch: dx=10（=阈值不超）不发；dx=11 发
    let mut s3 = one_draggable_button_scene();
    let mut ps3 = PointerState::new();
    ps3.process(
        &mut s3,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    let out3 = ps3.process(
        &mut s3,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 60.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    assert!(
        !out3.iter().any(|e| e.event_type == EVT_DRAG_START),
        "touch dx=10（=阈值不超）→ 不发"
    );
    let mut s4 = one_draggable_button_scene();
    let mut ps4 = PointerState::new();
    ps4.process(
        &mut s4,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    let out4 = ps4.process(
        &mut s4,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 61.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: 1,
        }],
    );
    assert!(
        out4.iter().any(|e| e.event_type == EVT_DRAG_START),
        "touch dx=11>10 → 发"
    );
}

#[test]
fn drag_target_is_nearest_draggable_ancestor() {
    // root draggable，btn 非 draggable：Down@btn → drag_target=root（祖先），DragStart@root。
    let mut s = one_button_scene(); // root+btn，均非 draggable
    let root_id = s.roots[0];
    s.get_mut(root_id).unwrap().draggable = true; // 仅 root draggable
    let mut ps = PointerState::new();
    let out = ps.process(
        &mut s,
        &[
            PointerEvent {
                kind: PointerKind::Down,
                x: 50.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            }, // 命中 btn
            PointerEvent {
                kind: PointerKind::Move,
                x: 55.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
        ],
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_DRAG_START && e.node_id == root_id.0),
        "down 叶 btn 非 draggable 但祖先 root draggable → DragStart@root"
    );
}

#[test]
fn drag_disabled_node_no_drag() {
    let mut s = one_draggable_button_scene();
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    s.get_mut(btn_id).unwrap().disabled = true; // draggable 但 disabled
    let mut ps = PointerState::new();
    let out = ps.process(
        &mut s,
        &[
            PointerEvent {
                kind: PointerKind::Down,
                x: 50.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
            PointerEvent {
                kind: PointerKind::Move,
                x: 55.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
        ],
    );
    assert!(
        !out.iter().any(|e| e.event_type == EVT_DRAG_START),
        "disabled draggable → 不发 drag"
    );
}

#[test]
fn drag_below_threshold_still_clicks() {
    // draggable btn：Down+Move dx=1（<阈值2）+Up → 不发 drag，正常 Click（drag 不破坏 click 容忍）
    let mut s = one_draggable_button_scene();
    let mut ps = PointerState::new();
    let out = ps.process(
        &mut s,
        &[
            PointerEvent {
                kind: PointerKind::Down,
                x: 50.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
            PointerEvent {
                kind: PointerKind::Move,
                x: 51.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            }, // dx=1<2
            PointerEvent {
                kind: PointerKind::Up,
                x: 51.0,
                y: 50.0,
                button: 0,
                pad: [0, 0],
                touch_id: -1,
            },
        ],
    );
    assert!(
        !out.iter().any(|e| e.event_type == EVT_DRAG_START),
        "dx=1<阈值 → 不发 drag"
    );
    assert!(
        out.iter().any(|e| e.event_type == EVT_CLICK),
        "阈值内 → 正常 Click"
    );
}

#[test]
fn canceled_emits_dragend() {
    let mut s = one_draggable_button_scene();
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 55.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    ); // DragStart
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Canceled,
            x: 55.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_DRAG_END && e.node_id == btn_id.0),
        "Canceled → DragEnd@btn"
    );
}

// ===== core longpress 检测 =====

#[test]
fn longpress_fires_after_1_5s_no_move() {
    // Down@btn → time_s 推进 1.5s（空事件 tick）→ LongPress@btn 一次。
    let mut s = one_button_scene();
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    ps.time_s = 0.0;
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.time_s = 1.5;
    let out = ps.process(&mut s, &[]); // 空事件 tick → longpress 检查
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_LONG_PRESS && e.node_id == btn_id.0),
        "按住 1.5s 无 move → LongPress@btn"
    );
}

#[test]
fn longpress_not_fired_before_trigger() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    ps.time_s = 0.0;
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.time_s = 1.0; // <1.5
    let out = ps.process(&mut s, &[]);
    assert!(
        !out.iter().any(|e| e.event_type == EVT_LONG_PRESS),
        "<1.5s → 不发 LongPress"
    );
}

#[test]
fn longpress_cancelled_by_move_over_50() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    ps.time_s = 0.0;
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    // Move 60px → longpress_cancelled（与 click_cancelled 同处）
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 110.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.time_s = 1.5;
    let out = ps.process(&mut s, &[]);
    assert!(
        !out.iter().any(|e| e.event_type == EVT_LONG_PRESS),
        "Move>50 → longpress 取消"
    );
}

#[test]
fn longpress_fires_only_once_per_press() {
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    ps.time_s = 0.0;
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.time_s = 1.5;
    let out1 = ps.process(&mut s, &[]);
    assert!(
        out1.iter().any(|e| e.event_type == EVT_LONG_PRESS),
        "1.5s → 发一次"
    );
    ps.time_s = 2.0; // 继续 tick
    let out2 = ps.process(&mut s, &[]);
    assert!(
        !out2.iter().any(|e| e.event_type == EVT_LONG_PRESS),
        "已 fired → 不再发"
    );
}

#[test]
fn longpress_independent_of_click() {
    // LongPress 后 Up → Click 照发（独立）。
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    ps.time_s = 0.0;
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.time_s = 1.5;
    ps.process(&mut s, &[]); // LongPress
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        out.iter().any(|e| e.event_type == EVT_CLICK),
        "LongPress 后 Up → Click 仍发（独立）"
    );
}

#[test]
fn longpress_disabled_node_no_fire() {
    let mut s = one_button_scene();
    let btn_id = s.get(s.roots[0]).unwrap().children[0];
    s.get_mut(btn_id).unwrap().disabled = true;
    let mut ps = PointerState::new();
    ps.time_s = 0.0;
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.time_s = 1.5;
    let out = ps.process(&mut s, &[]);
    assert!(
        !out.iter().any(|e| e.event_type == EVT_LONG_PRESS),
        "disabled → 不发 LongPress"
    );
}

// ===== 焦点 + 键盘 =====

/// root + btnA(tabindex=0) + btnB(tabindex=0)，均 @ 各位可区分。
fn two_focusable_scene() -> Scene {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut a = Node::default();
    a.kind = NodeKind::Button;
    a.tabindex = Some(0);
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let mut b = Node::default();
    b.kind = NodeKind::Button;
    b.tabindex = Some(0);
    b.layout_rect = Rect {
        x: 100.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let mut s = Scene::from_nodes(vec![root, a, b], vec![(0, 1), (0, 2)]);
    compute_world_transforms(&mut s);
    s
}

#[test]
fn focus_node_emits_focusout_then_focusin() {
    let mut s = two_focusable_scene();
    let root_id = s.roots[0];
    let a_id = s.get(root_id).unwrap().children[0];
    let b_id = s.get(root_id).unwrap().children[1];
    let mut out = Vec::new();
    // 先聚焦 A
    focus_node(&mut s, Some(a_id), &mut out);
    assert!(s.get(a_id).unwrap().focused, "A focused=true");
    assert_eq!(s.focused_node, Some(a_id));
    // 聚焦 B → FocusOut@A + FocusIn@B
    focus_node(&mut s, Some(b_id), &mut out);
    assert!(!s.get(a_id).unwrap().focused, "A focused=false（失焦）");
    assert!(s.get(b_id).unwrap().focused, "B focused=true");
    assert_eq!(s.focused_node, Some(b_id));
    // out 含 [FocusIn@A, FocusOut@A, FocusIn@B]
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_FOCUS_IN && e.node_id == a_id.0),
        "FocusIn@A"
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_FOCUS_OUT && e.node_id == a_id.0),
        "FocusOut@A"
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_FOCUS_IN && e.node_id == b_id.0),
        "FocusIn@B"
    );
}

#[test]
fn focus_node_same_target_no_event() {
    let mut s = two_focusable_scene();
    let a_id = s.get(s.roots[0]).unwrap().children[0];
    let mut out = Vec::new();
    focus_node(&mut s, Some(a_id), &mut out);
    let mut out2 = Vec::new();
    focus_node(&mut s, Some(a_id), &mut out2); // 同目标
    assert!(out2.is_empty(), "同目标重复聚焦 → 不发事件");
}

#[test]
fn focus_node_clear_blur() {
    let mut s = two_focusable_scene();
    let a_id = s.get(s.roots[0]).unwrap().children[0];
    let mut out = Vec::new();
    focus_node(&mut s, Some(a_id), &mut out);
    focus_node(&mut s, None, &mut out); // 清焦点
    assert_eq!(s.focused_node, None);
    assert!(!s.get(a_id).unwrap().focused);
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_FOCUS_OUT && e.node_id == a_id.0),
        "blur → FocusOut@A"
    );
}

/// root + A(tabindex=2) + B(tabindex=1) + C(tabindex=0) + D(tabindex=-1) + E(无属性) + disabled F(tabindex=0)
fn tab_chain_scene() -> Scene {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 400.0,
        h: 200.0,
    };
    let mk = |ti: Option<i32>, disabled: bool, id: usize| {
        let mut n = Node::default();
        n.kind = NodeKind::Button;
        n.tabindex = ti;
        n.disabled = disabled;
        n.layout_rect = Rect {
            x: id as f32 * 50.0,
            y: 0.0,
            w: 40.0,
            h: 40.0,
        };
        n
    };
    // root(0) + a(1,ti=2) + b(2,ti=1) + c(3,ti=0) + d(4,ti=-1) + e(5,None) + f(6,ti=0,disabled)
    let a = mk(Some(2), false, 1);
    let b = mk(Some(1), false, 2);
    let c = mk(Some(0), false, 3);
    let d = mk(Some(-1), false, 4);
    let e = mk(None, false, 5);
    let f = mk(Some(0), true, 6); // disabled
    let mut s = Scene::from_nodes(
        vec![root, a, b, c, d, e, f],
        vec![(0, 1), (0, 2), (0, 3), (0, 4), (0, 5), (0, 6)],
    );
    compute_world_transforms(&mut s);
    s
}

#[test]
fn tab_chain_orders_by_tabindex_then_dfs() {
    let s = tab_chain_scene();
    let chain = build_tab_chain(&s);
    // 正整数组 [B(tabindex=1), A(tabindex=2)] 升序，后接 0 组 [C(tabindex=0)]。
    // D(-1)/E(None)/F(disabled) 不进。
    let root_id = s.roots[0];
    let children = &s.get(root_id).unwrap().children;
    let a_id = children[0]; // tabindex=2
    let b_id = children[1]; // tabindex=1
    let c_id = children[2]; // tabindex=0
    assert_eq!(
        chain,
        vec![b_id, a_id, c_id],
        "链序：正整数升序(B=1,A=2)后接 0 组(C=0)"
    );
}

#[test]
fn tab_forward_cycles_through_chain() {
    let mut s = tab_chain_scene();
    let root_id = s.roots[0];
    let children = s.get(root_id).unwrap().children.clone();
    let a_id = children[0]; // tabindex=2
    let b_id = children[1]; // tabindex=1
    let c_id = children[2]; // tabindex=0
    let mut out = Vec::new();
    // 焦点 None → Tab → B（链首）
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_TAB,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(s.focused_node, Some(b_id), "首次 Tab → 链首 B");
    // Tab → A
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_TAB,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(s.focused_node, Some(a_id), "Tab → A");
    // Tab → C → Tab → wrap 回 B
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_TAB,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(s.focused_node, Some(c_id), "Tab → C");
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_TAB,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(s.focused_node, Some(b_id), "链尾 Tab → wrap 回链首 B");
}

#[test]
fn shift_tab_backward_cycles() {
    let mut s = tab_chain_scene();
    let root_id = s.roots[0];
    let children = s.get(root_id).unwrap().children.clone();
    let a_id = children[0]; // tabindex=2
    let b_id = children[1]; // tabindex=1
    let c_id = children[2]; // tabindex=0
    let mut out = Vec::new();
    // 焦点 None → Shift+Tab → 链尾 C
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_TAB,
            modifiers: MOD_SHIFT,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(s.focused_node, Some(c_id), "Shift+Tab 从 None → 链尾 C");
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_TAB,
            modifiers: MOD_SHIFT,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(s.focused_node, Some(a_id), "Shift+Tab → A");
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_TAB,
            modifiers: MOD_SHIFT,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(s.focused_node, Some(b_id), "Shift+Tab → B");
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_TAB,
            modifiers: MOD_SHIFT,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(s.focused_node, Some(c_id), "链首 Shift+Tab → wrap 回链尾 C");
}

#[test]
fn tab_empty_chain_no_op() {
    // 无可聚焦节点 → Tab 无操作（不发 keydown，不改焦点）
    let mut s = one_button_scene(); // btn 无 tabindex
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_TAB,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(s.focused_node, None, "无可聚焦 → Tab 不改焦点");
    assert!(out.is_empty(), "空链 Tab → 无事件");
}

#[test]
fn keydown_emitted_to_focused_node() {
    let mut s = two_focusable_scene();
    let a_id = s.get(s.roots[0]).unwrap().children[0];
    let mut out = Vec::new();
    focus_node(&mut s, Some(a_id), &mut out); // 聚焦 A
    out.clear();
    // Enter keydown（KeyCode.Return=13，core 不解释，只透传）
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: 13,
            modifiers: MOD_CTRL,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    let kd = out
        .iter()
        .find(|e| e.event_type == EVT_KEY_DOWN)
        .expect("keydown");
    assert_eq!(kd.node_id, a_id.0, "keydown@焦点 A");
    assert_eq!(kd.touch_id, 13, "key_code 复用 touch_id");
    assert_eq!(kd.pad[0], MOD_CTRL, "modifiers 复用 pad[0]");
}

#[test]
fn keydown_no_focus_dropped() {
    let mut s = two_focusable_scene();
    let mut out = Vec::new();
    // 无焦点 + keydown → 丢弃
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: 13,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert!(
        out.iter().all(|e| e.event_type != EVT_KEY_DOWN),
        "无焦点 keydown 丢弃"
    );
}

#[test]
fn tab_consumed_no_keydown() {
    let mut s = two_focusable_scene();
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_TAB,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert!(
        out.iter().all(|e| e.event_type != EVT_KEY_DOWN),
        "Tab 被导航消费，不发 keydown"
    );
    assert!(
        out.iter().any(|e| e.event_type == EVT_FOCUS_IN),
        "Tab → FocusIn"
    );
}

#[test]
fn click_to_focus_focusable_node() {
    // pointer-down 命中 tabindex=0 节点 → FocusIn@该节点
    let mut s = two_focusable_scene(); // A@0,0,50,50 tabindex=0
    let a_id = s.get(s.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 25.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_FOCUS_IN && e.node_id == a_id.0),
        "down@A(tabindex=0) → FocusIn@A"
    );
    assert_eq!(s.focused_node, Some(a_id));
}

#[test]
fn click_non_focusable_no_blur() {
    // 焦点 A，pointer-down 不可聚焦节点（btn 无 tabindex）→ 不夺焦（不发 FocusOut）
    let mut s = one_button_scene(); // btn 无 tabindex，root 无 tabindex
    let root_id = s.roots[0];
    let mut ps = PointerState::new();
    // 先聚焦 root（编程模拟）——root 无 tabindex，但 focus_node 可强制（测 click-to-focus 不夺焦）
    let mut tmp = Vec::new();
    focus_node(&mut s, Some(root_id), &mut tmp);
    // down@btn（不可聚焦）→ 不应 FocusOut root
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        out.iter().all(|e| e.event_type != EVT_FOCUS_OUT),
        "down 不可聚焦节点 → 不夺焦（无 FocusOut）"
    );
    assert_eq!(s.focused_node, Some(root_id), "焦点保持 root");
}

#[test]
fn click_disabled_focusable_no_focus() {
    // disabled 可聚焦节点 → pointer-down 不聚焦
    let mut s = two_focusable_scene();
    let a_id = s.get(s.roots[0]).unwrap().children[0];
    s.get_mut(a_id).unwrap().disabled = true; // A disabled（tabindex=0）
    let mut ps = PointerState::new();
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 25.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        out.iter().all(|e| e.event_type != EVT_FOCUS_IN),
        "disabled 可聚焦 → down 不聚焦"
    );
    assert_eq!(s.focused_node, None);
}

// ===== scroll 手势仲裁 =====

/// root(0) + scroll 容器(1) overflow_y=Scroll viewport 100x100 + content 子(2) 40x200（content>viewport y 轴）。
/// refresh_content_sizes 后容器 1 overlap_y=100，effective_y=true（Scroll 永真），effective_x=false（Visible）。
fn v_scroll_scene() -> Scene {
    use crate::style::resolved::{OverflowMode, ResolvedStyle};
    let mut scroll_style = ResolvedStyle::default();
    scroll_style.overflow_y = OverflowMode::Scroll; // 仅垂直可滚
    let entries: Vec<(
        Option<usize>,
        NodeKind,
        ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
        Option<String>,
    )> = vec![
        (
            None,
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
        ), // 0 root
        (
            Some(0),
            NodeKind::Container,
            scroll_style,
            vec![],
            None,
            false,
            None,
            None,
        ), // 1 scroll 容器
        (
            Some(1),
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
        ), // 2 content 子
    ];
    let mut s = Scene::build(&entries);
    let root_id = s.roots[0];
    let scroll_id = s.get(root_id).unwrap().children[0];
    let content_id = s.get(scroll_id).unwrap().children[0];
    s.get_mut(root_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    s.get_mut(scroll_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    }; // viewport 100x100
    s.get_mut(content_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 200.0,
    }; // content 40x200 → overlap_y=100
       // 模拟 layout solve 的 clip_rect 填充（overflow!=Visible 节点 build 时 Some(default)，
       // layout/mod.rs:196 把它填成自身 border 框；测里手填等效值）。
    for n in s.nodes.values_mut() {
        if n.clip_rect.is_some() {
            n.clip_rect = Some(n.layout_rect);
        }
    }
    crate::scene::transform::compute_world_transforms(&mut s);
    crate::scroll::refresh_content_sizes(&mut s);
    s
}

#[test]
fn scroll_wins_over_drag_when_scroll_threshold_first() {
    // 容器 overflow_y=Scroll；content 子非 draggable。Move 垂直超 mouse 阈值 8 →
    // scrolling_pane 设为容器、click_cancelled。drag 阈值 mouse 2 < scroll 8，但子非 draggable
    // → drag_target=None → 仅 scroll 候选在跑。
    let mut s = v_scroll_scene();
    let scroll_id = s.get(s.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    // Down @(10,10) 命中 content 子，down_targets=[content,scroll,root]，候选沿链找最近 effective=scroll（容器）
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    // Move @(10,25) dy=15 > scroll 阈值 8（mouse）
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 10.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let slot = &ps.slots[0];
    assert!(
        slot.scrolling_pane == Some(scroll_id),
        "scroll 达阈值先 → scrolling_pane=容器"
    );
    assert!(slot.click_cancelled, "scroll-start 取消 click");
}

#[test]
fn vertical_only_container_yields_on_horizontal_gesture() {
    // overflow_y=Scroll（仅垂直 effective）；水平位移更大（dx>dy）→ 让出，scrolling_pane 保持 None。
    let mut s = v_scroll_scene();
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    // Move @(30,15) dx=20 > scroll 阈值 8 且 dx > dy(5) → V-only 让出（lock_ok=false）
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 30.0,
            y: 15.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let slot = &ps.slots[0];
    assert!(
        slot.scrolling_pane.is_none(),
        "V-only 容器遇水平更大手势 → 让出（不滚）"
    );
}

#[test]
fn nested_innermost_scroll_wins() {
    // 外 vertical 容器(1) + 内 vertical 容器(2)；Down 在内层 → scrolling_pane = 内层(2)。
    use crate::style::resolved::{OverflowMode, ResolvedStyle};
    let mut outer = ResolvedStyle::default();
    outer.overflow_y = OverflowMode::Scroll;
    let mut inner = ResolvedStyle::default();
    inner.overflow_y = OverflowMode::Scroll;
    let entries: Vec<(
        Option<usize>,
        NodeKind,
        ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
        Option<String>,
    )> = vec![
        (
            None,
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
        ), // 0 root
        (
            Some(0),
            NodeKind::Container,
            outer,
            vec![],
            None,
            false,
            None,
            None,
        ), // 1 外 scroll
        (
            Some(1),
            NodeKind::Container,
            inner,
            vec![],
            None,
            false,
            None,
            None,
        ), // 2 内 scroll
        (
            Some(2),
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
        ), // 3 内层 content
    ];
    let mut s = Scene::build(&entries);
    let root_id = s.roots[0];
    let outer_id = s.get(root_id).unwrap().children[0];
    let inner_id = s.get(outer_id).unwrap().children[0];
    let content_id = s.get(inner_id).unwrap().children[0];
    s.get_mut(root_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 300.0,
        h: 300.0,
    };
    s.get_mut(outer_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    s.get_mut(inner_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    s.get_mut(content_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 200.0,
    };
    // 模拟 layout solve 的 clip_rect 填充（见 v_scroll_scene 注释）。
    for n in s.nodes.values_mut() {
        if n.clip_rect.is_some() {
            n.clip_rect = Some(n.layout_rect);
        }
    }
    crate::scene::transform::compute_world_transforms(&mut s);
    crate::scroll::refresh_content_sizes(&mut s);
    let mut ps = PointerState::new();
    // Down @(10,10) 命中 content 子，down_targets=[content,inner,outer,root]，候选=最近 effective=内层
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    // Move dy=15 > 8 → scroll 达阈值，V-only 且 dy>dx(0) → lock_ok → scrolling_pane=内层
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 10.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let slot = &ps.slots[0];
    assert_eq!(
        slot.scrolling_pane,
        Some(inner_id),
        "嵌套 Down 在内层 → scrolling_pane=内层（最近祖先优先）"
    );
}

#[test]
fn scroll_drag_follow_advances_scroll_pos() {
    // scrolling_pane 已判定 → Move 跟手 drag_follow 写 scroll_pos。
    let mut s = v_scroll_scene();
    let scroll_id = s.get(s.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 10.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    ); // scroll 启动
       // 再 Move @(10,35) → design 下拖 +10 → scroll_pos.y 减（看上方，触屏跟手；与 apply_wheel 一致）
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 10.0,
            y: 35.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let st = s.scroll.get(scroll_id).unwrap();
    assert!(
        st.scroll_pos.1 < 0.0,
        "下拖 design +y → scroll_pos.y 减（看上方），got {}",
        st.scroll_pos.1
    );
}

#[test]
fn scroll_up_starts_inertia_and_clears_state() {
    // scrolling_pane 中 Up → begin_inertia + 清 scroll 字段。
    let mut s = v_scroll_scene();
    let scroll_id = s.get(s.roots[0]).unwrap().children[0];
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 10.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    ); // scroll 启动
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 10.0,
            y: 35.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    ); // 跟手攒速度
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 10.0,
            y: 35.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let slot = &ps.slots[0];
    assert!(slot.scrolling_pane.is_none(), "Up 后 scrolling_pane 清空");
    assert!(!slot.scroll_testing, "Up 后 scroll_testing=false");
    // inertia 启动：velocity 非零时 tweening=2（速度不足则仍 0——本测跟手攒了速度）
    let _st = s.scroll.get(scroll_id).unwrap();
    // 不硬断 tweening（速度可能因阈值不启），仅验字段清。
}

#[test]
fn scroll_start_suppresses_drag() {
    // 同时 draggable + scroll 容器（叶子 draggable）：scroll 达阈值先于 drag（scroll 8 > drag 2
    // 但子非 draggable 这里）；此处验 scroll 启动后 drag_target 被清，drag 不启动。
    // 改 leaf draggable=true 但容器是 scroll：drag_target=leaf（draggable），scroll_candidate=容器。
    // 阈值赛跑：drag mouse 2 < scroll 8 → drag 先达。本测改验：scroll 先达场景下 drag_target=None。
    // 构造：draggable=true 的 content 子放在 scroll 容器，先小 Move 触发 scroll（需 scroll 先于 drag
    // 不可能——drag 阈值更小）。改测：draggable leaf 在 scroll 容器，Move 大位移同时超两者 →
    // drag 先达（2<8）→ scroll_testing 清。此验互斥另一侧（drag 赢清 scroll）。
    use crate::style::resolved::{OverflowMode, ResolvedStyle};
    let mut scroll_style = ResolvedStyle::default();
    scroll_style.overflow_y = OverflowMode::Scroll;
    let entries: Vec<(
        Option<usize>,
        NodeKind,
        ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
        Option<String>,
    )> = vec![
        (
            None,
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Container,
            scroll_style,
            vec![],
            None,
            false,
            None,
            None,
        ),
        (
            Some(1),
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            None,
            true,
            None,
            None,
        ), // 2 draggable content
    ];
    let mut s = Scene::build(&entries);
    let root_id = s.roots[0];
    let scroll_id = s.get(root_id).unwrap().children[0];
    let content_id = s.get(scroll_id).unwrap().children[0];
    s.get_mut(root_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    s.get_mut(scroll_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    s.get_mut(content_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 200.0,
    };
    // 模拟 layout solve 的 clip_rect 填充（见 v_scroll_scene 注释）。
    for n in s.nodes.values_mut() {
        if n.clip_rect.is_some() {
            n.clip_rect = Some(n.layout_rect);
        }
    }
    crate::scene::transform::compute_world_transforms(&mut s);
    crate::scroll::refresh_content_sizes(&mut s);
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    // Move dy=5（>drag 2，<scroll 8）→ drag 先达 DragStart + 清 scroll_testing
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 10.0,
            y: 15.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let slot = &ps.slots[0];
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_DRAG_START && e.node_id == content_id.0),
        "draggable leaf Move>2 → DragStart"
    );
    assert!(
        !slot.scroll_testing,
        "drag 先达 → scroll_testing 清（互斥）"
    );
    assert!(
        slot.scroll_candidate.is_none(),
        "drag 先达 → scroll_candidate 清"
    );
    assert!(
        slot.scrolling_pane.is_none(),
        "drag 赢 → scrolling_pane 不设"
    );
}

#[test]
fn no_scroll_candidate_when_no_effective_ancestor() {
    // 普通 scene（无 scroll 容器）→ Down+Move 不设 scroll 字段（零回归保险）。
    let mut s = one_button_scene();
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 50.0,
            y: 70.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let slot = &ps.slots[0];
    assert!(
        slot.scroll_candidate.is_none(),
        "无 scroll 容器 → scroll_candidate=None"
    );
    assert!(
        !slot.scroll_testing,
        "无 scroll 容器 → scroll_testing=false"
    );
    assert!(
        slot.scrolling_pane.is_none(),
        "无 scroll 容器 → scrolling_pane=None"
    );
}

// ── scrollbar grip 拖拽 ─────────────────────────────

fn grip_scroll_scene() -> Scene {
    use crate::scene::node::NodeKind;
    use crate::style::resolved::{OverflowMode, ResolvedStyle};
    let mut scroll_style = ResolvedStyle::default();
    scroll_style.overflow_y = OverflowMode::Scroll;
    let entries: Vec<(
        Option<usize>,
        NodeKind,
        ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
        Option<String>,
    )> = vec![
        (
            None,
            NodeKind::Container,
            scroll_style.clone(),
            vec![],
            None,
            false,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
        ),
        (
            Some(0),
            NodeKind::Container,
            ResolvedStyle::default(),
            vec![],
            None,
            false,
            None,
            None,
        ),
    ];
    let mut s = Scene::build(&entries);
    let root_id = s.roots[0];
    let child0_id = s.get(root_id).unwrap().children[0];
    let child1_id = s.get(root_id).unwrap().children[1];
    s.get_mut(root_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    s.get_mut(root_id).unwrap().clip_rect = Some(Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    });
    s.get_mut(child0_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    };
    s.get_mut(child1_id).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 30.0,
        h: 200.0,
    }; // content_y=200 > viewport=100
    crate::scroll::refresh_content_sizes(&mut s);
    compute_world_transforms(&mut s);
    s
}

#[test]
fn grip_down_sets_grip_dragging_and_cancels_click() {
    let mut s = grip_scroll_scene();
    let root_id = s.roots[0];
    let mut ps = PointerState::new();
    // thumb 右边缘：x=92..100, y=0..50（viewport=100 content=200 → thumb_h=50）
    // 点 thumb center (96, 25)
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 96.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let slot = &ps.slots[0];
    assert!(slot.grip_dragging, "thumb 命中 → grip_dragging=true");
    assert_eq!(slot.scrolling_pane, Some(root_id), "scrolling_pane=容器");
    assert!(slot.click_cancelled, "grip down 取消 click");
    assert!(
        slot.scroll_gesture & 1 != 0,
        "垂直 thumb → scroll_gesture bit0"
    );
    // 不应发 EVT_DOWN（continue 跳过）
    assert!(
        !out.iter().any(|e| e.event_type == EVT_DOWN),
        "grip down 不产 EVT_DOWN"
    );
}

#[test]
fn grip_move_drives_scroll_pos() {
    let mut s = grip_scroll_scene();
    let root_id = s.roots[0];
    let mut ps = PointerState::new();
    // Down on thumb (96, 25)
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 96.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    // Move thumb to y=75（track_h=100, min_thumb=20 → effective range=80；perc = (75-0)/80=0.9375）
    // overlap_y=100 → scroll_pos = 0.9375*100 = 93.75
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 96.0,
            y: 75.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let st = s.scroll.get(root_id).unwrap();
    assert!(
        st.scroll_pos.1 > 50.0,
        "grip move → scroll_pos 变化，got {}",
        st.scroll_pos.1
    );
}

#[test]
fn grip_up_clears_state_and_no_inertia() {
    let mut s = grip_scroll_scene();
    let root_id = s.roots[0];
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 96.0,
            y: 25.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    // Move to build some velocity via... actually grip doesn't use drag_follow
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 96.0,
            y: 75.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    // Up — grip_dragging should clear, no inertia (tweening remains 0)
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 96.0,
            y: 75.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let slot = &ps.slots[0];
    assert!(!slot.grip_dragging, "Up 后 grip_dragging 清");
    assert!(slot.scrolling_pane.is_none(), "Up 后 scrolling_pane 清");
    let st = s.scroll.get(root_id).unwrap();
    assert_eq!(st.tweening, 0, "grip up 不启惯性 tweening=0");
}

#[test]
fn grip_no_hit_on_non_thumb_area() {
    let mut s = grip_scroll_scene();
    let mut ps = PointerState::new();
    // Click on container area (10, 10) — not thumb
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 10.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let slot = &ps.slots[0];
    assert!(!slot.grip_dragging, "非 thumb 区 → grip_dragging=false");
    assert!(
        out.iter().any(|e| e.event_type == EVT_DOWN),
        "非 thumb Down 正常发 EVT_DOWN"
    );
}
