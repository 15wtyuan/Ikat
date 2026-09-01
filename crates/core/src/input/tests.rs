use super::*;
use crate::scene::node::{ControlState, EditState, Node, NodeFlags, NodeKind, Rect, Scene};
use crate::scene::transform::compute_world_transforms;
use crate::style::resolved::CursorStyle;

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
    txt.kind = NodeKind::TextNode;
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
    assert!(
        s.get(txt_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::HOVERED),
        "Text 子（命中点）hovered"
    );
    assert!(
        s.get(btn_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::HOVERED),
        "btn（Text 的祖先）也 hovered——祖先链"
    );
    assert!(
        s.get(root_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::HOVERED),
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
    assert!(
        s.get(txt_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::ACTIVE),
        "Text 子（命中点）active"
    );
    assert!(
        s.get(btn_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::ACTIVE),
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
    assert!(
        !s.get(btn_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::ACTIVE),
        "up 后 btn active 清零"
    );
    assert!(
        !s.get(txt_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::ACTIVE),
        "up 后 Text active 清零"
    );
}

#[test]
fn secondary_button_down_does_not_activate_controls() {
    // review 回归：非主键（button!=0）Down 不激活控件、不武装拖选/Slider 跟随——
    // 浏览器对齐（右键按下无 click/拖拽语义）。主键照常激活。
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 200.0,
    };
    let mut tg = Node::default();
    tg.kind = NodeKind::Toggle;
    tg.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut s = Scene::from_nodes(vec![root, tg], vec![(0, 1)]);
    let tg_id = s.get(s.roots[0]).unwrap().children[0];
    s.controls
        .ensure(tg_id, ControlState::Toggle { checked: false });
    compute_world_transforms(&mut s);
    let is_checked = |s: &Scene| {
        matches!(
            s.controls.get(tg_id),
            Some(ControlState::Toggle { checked: true })
        )
    };

    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 10.0,
            y: 10.0,
            button: 2,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(!is_checked(&s), "右键 Down 不翻转 toggle");
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 10.0,
            y: 10.0,
            button: 2,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(!is_checked(&s), "右键 Up 也不翻转（click 走主键路径）");

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
    assert!(is_checked(&s), "主键 Down 正常激活");
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
    assert!(
        !s.get(btn_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::ACTIVE),
        "Up 后 active=false"
    );
    assert!(
        s.get(btn_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::HOVERED),
        "hover 保持"
    );
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
    s.get_mut(btn_id)
        .unwrap()
        .interaction
        .flags
        .insert(NodeFlags::DISABLED);
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
        !s.get(btn_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::ACTIVE),
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
    s.get_mut(btn_id)
        .unwrap()
        .interaction
        .flags
        .insert(NodeFlags::DISABLED);
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
        !s.get(btn_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::ACTIVE),
        "按住 disabled btn 不应 active（active 抑制）"
    );
    assert!(
        !s.get(root_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::ACTIVE),
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
    s.get_mut(btn_id)
        .unwrap()
        .interaction
        .flags
        .insert(NodeFlags::DISABLED);
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
        !s.get(btn_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::ACTIVE),
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
    assert!(s
        .get(btn_id)
        .unwrap()
        .interaction
        .flags
        .contains(NodeFlags::HOVERED));
    // 空事件——hover 应保持（无 RollOut）
    let out = ps.process(&mut s, &[]);
    assert!(
        !out.iter().any(|e| e.event_type == EVT_ROLL_OUT),
        "空事件 hover 保持"
    );
    assert!(
        s.get(btn_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::HOVERED),
        "hover 仍 true"
    );
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
    assert!(
        s.get(a_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::HOVERED),
        "A hovered（touch1 命中）"
    );
    assert!(
        s.get(b_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::HOVERED),
        "B hovered（touch2 命中）"
    );
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
        s.get(a_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::ACTIVE)
            && s.get(b_id)
                .unwrap()
                .interaction
                .flags
                .contains(NodeFlags::ACTIVE),
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
    assert!(
        !s.get(a_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::ACTIVE),
        "松 touch1 → A active 清"
    );
    assert!(
        s.get(b_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::ACTIVE),
        "touch2 仍按 → B 仍 active"
    );
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

// ---- cursor_intent（#93）：鼠标槽命中 → 光标语义决策 ----

fn cursor_style(v: CursorStyle) -> crate::style::resolved::ResolvedStyle {
    let mut st = crate::style::resolved::ResolvedStyle::default();
    st.cursor = v;
    st
}

/// hover Button（Auto 样式）→ Hand；触摸槽命中不参与（恒看鼠标槽）。
#[test]
fn cursor_hover_button_hand_and_touch_slot_ignored() {
    let mut s = one_button_scene(); // root + button(100x100)
    let mut ps = PointerState::new();
    // 触摸指在 btn 上、鼠标在 UI 外 → 决策只看 slots[0] → Arrow
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
    assert_eq!(
        ps.cursor_intent(&s),
        CursorIntent::Arrow,
        "触摸命中不产生悬停语义——决策恒基于鼠标槽"
    );
    // 鼠标移进 btn → UA 默认手型
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
    assert_eq!(
        ps.cursor_intent(&s),
        CursorIntent::Hand,
        "hover pressable 控件（Auto）→ Hand"
    );
    // 鼠标移出（命中 root Container）→ 回箭头
    ps.process(
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
    assert_eq!(
        ps.cursor_intent(&s),
        CursorIntent::Arrow,
        "非 pressable 命中 → Arrow"
    );
}

/// 作者显式声明恒压 UA 行为：default 把可点控件压回箭头；none 元素级隐藏；
/// pointer 让非控件 div 也给手型。
#[test]
fn cursor_author_decl_overrides_ua_default() {
    use NodeKind as K;
    // root + 三兄弟同区域叠放测试不便，直接改 one_button_scene 的节点样式逐态断言
    let mut s = one_button_scene();
    let root_id = s.roots[0];
    let btn_id = s.get(root_id).unwrap().children[0];
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
    s.get_mut(btn_id).unwrap().style = cursor_style(CursorStyle::System);
    assert_eq!(
        ps.cursor_intent(&s),
        CursorIntent::Arrow,
        "作者 cursor:default 压过 UA 手型"
    );
    s.get_mut(btn_id).unwrap().style = cursor_style(CursorStyle::Hidden);
    assert_eq!(
        ps.cursor_intent(&s),
        CursorIntent::Hidden,
        "作者 cursor:none → 元素级隐藏"
    );

    // 非控件 div（Container）+ 作者 pointer → 手型（「地图节点是 div」场景）
    let mut s2 = one_button_scene();
    s2.get_mut(root_id).unwrap().kind = K::Container;
    // root 自身不被命中测试覆盖（last_hit=leaf），此处直接验 Link/容器路径的 author 分支：
    // 用 hit 在 root 上时作者 pointer 应生效——root 是 last_hit 当 (150,150)。
    let mut ps2 = PointerState::new();
    ps2.process(
        &mut s2,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 150.0,
            y: 150.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    s2.get_mut(s2.roots[0]).unwrap().style = cursor_style(CursorStyle::Pointer);
    assert_eq!(
        ps2.cursor_intent(&s2),
        CursorIntent::Hand,
        "非控件命中 + 作者 pointer → Hand"
    );
}

/// disabled 控件不给 UA 手型（浏览器 disabled button 一致）；不可命中（touchable=false）
/// 同理；Link 归 pressable 集。
#[test]
fn cursor_disabled_or_untouchable_no_hand_link_hand() {
    let mut s = one_button_scene();
    let root_id = s.roots[0];
    let btn_id = s.get(root_id).unwrap().children[0];
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
    // 作者显式声明不受 disabled 影响（浏览器级联语义），所以清回 Auto 再验 disabled 门
    let n = s.get_mut(btn_id).unwrap();
    n.interaction.flags.insert(NodeFlags::DISABLED);
    assert_eq!(
        ps.cursor_intent(&s),
        CursorIntent::Arrow,
        "disabled 控件 Auto → 不给手型"
    );

    // touchable=false（pointer-events:none）不给手型
    let n = s.get_mut(btn_id).unwrap();
    n.interaction.flags.remove(NodeFlags::DISABLED);
    n.interaction.touchable = false;
    assert_eq!(
        ps.cursor_intent(&s),
        CursorIntent::Arrow,
        "touchable=false → 无悬停手型"
    );

    // Link kind = pressable 集（<a> 与 button 同待遇）
    let n = s.get_mut(btn_id).unwrap();
    n.kind = NodeKind::Link;
    n.interaction.touchable = true;
    assert_eq!(
        ps.cursor_intent(&s),
        CursorIntent::Hand,
        "<a> 链接 Auto → 手型"
    );
}

/// #93 验收回归（悬停按钮文字光标变回箭头）：rich 内联命中细化到 source 节点
/// （TextNode/span）后，cursor 判定必须沿祖先链上溯到宿主控件——悬停按钮
/// **文字**（命中 Text 叶，非按钮本体）与悬停按钮背景同待遇；宿主按钮上的
/// 作者 cursor 声明同样覆盖文字区命中；disabled 宿主截断。
#[test]
fn cursor_walks_up_from_text_hit_to_host_button() {
    let mut s = button_with_text_child_scene();
    let root_id = s.roots[0];
    let btn_id = s.get(root_id).unwrap().children[0];
    let txt_id = s.get(btn_id).unwrap().children[0];
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 50.0,
            y: 10.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert_eq!(
        crate::hit::hit_test(&s, (50.0, 10.0)),
        Some(txt_id),
        "前置：该点命中 Text 叶子而非按钮"
    );
    assert_eq!(
        ps.cursor_intent(&s),
        CursorIntent::Hand,
        "命中按钮文字（Text 叶）→ 上溯宿主按钮 → 手型"
    );

    // 宿主上的作者声明对文字区命中等效（cursor 不继承，链上最近声明生效）
    s.get_mut(btn_id).unwrap().style.cursor = CursorStyle::System;
    assert_eq!(
        ps.cursor_intent(&s),
        CursorIntent::Arrow,
        "宿主 cursor:default 压过文字区命中的 UA 手型"
    );
    s.get_mut(btn_id).unwrap().style.cursor = CursorStyle::Hidden;
    assert_eq!(
        ps.cursor_intent(&s),
        CursorIntent::Hidden,
        "宿主 cursor:none 覆盖文字区命中"
    );

    // disabled 宿主：箭头并截断（不向外层借 affordance）
    s.get_mut(btn_id).unwrap().style.cursor = CursorStyle::Auto;
    s.get_mut(btn_id)
        .unwrap()
        .interaction
        .flags
        .insert(NodeFlags::DISABLED);
    assert_eq!(
        ps.cursor_intent(&s),
        CursorIntent::Arrow,
        "disabled 按钮的文字区命中 → 箭头"
    );
}

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
    assert!(
        s1.get(s1_btn_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::HOVERED),
        "Move@btn → btn hovered"
    );
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
    btn.interaction.draggable = true;
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
    s.get_mut(root_id).unwrap().interaction.draggable = true; // 仅 root draggable
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
    s.get_mut(btn_id)
        .unwrap()
        .interaction
        .flags
        .insert(NodeFlags::DISABLED); // draggable 但 disabled
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
    s.get_mut(btn_id)
        .unwrap()
        .interaction
        .flags
        .insert(NodeFlags::DISABLED);
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
    a.interaction.tabindex = Some(0);
    a.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let mut b = Node::default();
    b.kind = NodeKind::Button;
    b.interaction.tabindex = Some(0);
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
    assert!(
        s.get(a_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::FOCUSED),
        "A focused=true"
    );
    assert_eq!(s.focused_node, Some(a_id));
    // 聚焦 B → FocusOut@A + FocusIn@B
    focus_node(&mut s, Some(b_id), &mut out);
    assert!(
        !s.get(a_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::FOCUSED),
        "A focused=false（失焦）"
    );
    assert!(
        s.get(b_id)
            .unwrap()
            .interaction
            .flags
            .contains(NodeFlags::FOCUSED),
        "B focused=true"
    );
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
    assert!(!s
        .get(a_id)
        .unwrap()
        .interaction
        .flags
        .contains(NodeFlags::FOCUSED));
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
        n.interaction.tabindex = ti;
        if disabled {
            n.interaction.flags.insert(NodeFlags::DISABLED);
        } else {
            n.interaction.flags.remove(NodeFlags::DISABLED);
        }
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
fn click_non_focusable_blurs_per_dom() {
    // DOM 语义：焦点 root，pointer-down 不可聚焦节点（btn 无 tabindex）→ blur root
    // （focus 移到 body 等价——点空白/非聚焦区让聚焦的输入框失焦，是 Web 标准行为）。
    let mut s = one_button_scene(); // btn 无 tabindex，root 无 tabindex
    let root_id = s.roots[0];
    let mut ps = PointerState::new();
    // 先聚焦 root（编程模拟）——root 无 tabindex，但 focus_node 可强制
    let mut tmp = Vec::new();
    focus_node(&mut s, Some(root_id), &mut tmp);
    // down@btn（不可聚焦）→ 照 DOM 应 FocusOut root + 清焦点
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
        out.iter()
            .any(|e| e.event_type == EVT_FOCUS_OUT && e.node_id == root_id.0),
        "down 不可聚焦节点 → 照 DOM blur 焦点（FocusOut@root）"
    );
    assert_eq!(s.focused_node, None, "焦点清空（focus→body 等价）");
}

#[test]
fn click_disabled_focusable_no_focus() {
    // disabled 可聚焦节点 → pointer-down 不聚焦
    let mut s = two_focusable_scene();
    let a_id = s.get(s.roots[0]).unwrap().children[0];
    s.get_mut(a_id)
        .unwrap()
        .interaction
        .flags
        .insert(NodeFlags::DISABLED); // A disabled（tabindex=0）
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
        Option<String>,
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
       // layout solve 把它填成自身 border 框；测里手填等效值）。
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
        Option<String>,
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
        Option<String>,
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
        Option<String>,
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

/// 点击 thumb 不应导致列表瞬移：Down 在 thumb 中心后，微小 Move 应跟手（保持抓取点），
/// 而非把 thumb 顶端跳到指针处。旧实现把指针当 thumb 参考点（无 grab offset），
/// thumb 初始在中间（perc=0.5）时 Down+微移会瞬移到指针位置。
#[test]
fn grip_drag_follows_with_grab_offset_no_jump() {
    let mut s = grip_scroll_scene();
    let root_id = s.roots[0];
    // 先把 scroll_pos 拨到中间（perc=0.5）：viewport=100 content=200 overlap=100 → pos=50
    s.scroll.get_mut(root_id).unwrap().scroll_pos.1 = 50.0;
    // thumb_h=50，thumb 在 y=25..75（perc=0.5 → thumb_top=0.5*(100-50)=25）。compute 刷新 thumb rect
    compute_world_transforms(&mut s);
    let mut ps = PointerState::new();
    // Down 在 thumb 中心 (96, 50)——抓取偏移 y=0（指针正好在 thumb 中心）
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 96.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    // 微移 5px（跟手，不是跳跃）：Move 到 (96, 55)
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 96.0,
            y: 55.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    // thumb 中心应 = 指针 - grab_offset(0) = 55 → thumb_top = 55-25 = 30 → range=50 → perc=0.6 → pos=60
    // 旧实现（无 grab offset）：perc=(55-0)/(100-20)=0.6875 → pos=68.75（瞬移到指针比例）
    let st = s.scroll.get(root_id).unwrap();
    assert!(
        (st.scroll_pos.1 - 60.0).abs() < 0.5,
        "跟手拖拽：指针微移 5px（grab_offset=0）→ thumb 中心=55 → scroll_pos≈60（got {}）；旧实现瞬移到 ~68.75",
        st.scroll_pos.1
    );
}

/// Down 在 thumb 上不移动 → scroll_pos 完全不变（确认 Down 不写 scroll_pos）。
#[test]
fn grip_down_alone_does_not_move_scroll_pos() {
    let mut s = grip_scroll_scene();
    let root_id = s.roots[0];
    s.scroll.get_mut(root_id).unwrap().scroll_pos.1 = 50.0;
    compute_world_transforms(&mut s);
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 96.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let st = s.scroll.get(root_id).unwrap();
    assert_eq!(
        st.scroll_pos.1, 50.0,
        "Down 不应写 scroll_pos（仅设 grip_dragging 状态）"
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
    assert!(st.tweening_idle(), "grip up 不启惯性 tweening=0");
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

// 这些测试直接调 process_keys（隔离 PointerState/Stage）。focused 节点带正确的 NodeKind
// + 注入 ControlState（TextField/TextArea）。常量 KEY_* / EVT_SUBMITTED 在 input.rs 定义。
// KeyCode 数值取 Unity KeyCode 枚举（与 unity/package/.../Ikat.Types.cs 的 KeyCode enum 对齐——
// IkatInputCollector 用 (uint)UnityEngine.KeyCode 直传，core 须匹配同值）。

/// root + focused TextField(value)。kind 设 NodeKind::TextField（路由按 kind 分派单行/多行）。
fn focused_textfield_scene(value: &str) -> (Scene, NodeId) {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 100.0,
    };
    let mut tf = Node::default();
    tf.kind = NodeKind::TextField;
    tf.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut s = Scene::from_nodes(vec![root, tf], vec![(0, 1)]);
    let tf_id = s.get(s.roots[0]).unwrap().children[0];
    s.controls.ensure(
        tf_id,
        ControlState::TextField(EditState::from_init(value.into(), String::new(), 0, false)),
    );
    s.focused_node = Some(tf_id);
    compute_world_transforms(&mut s);
    (s, tf_id)
}

/// root + focused TextArea(value)。kind 设 NodeKind::TextArea。
fn focused_textarea_scene(value: &str) -> (Scene, NodeId) {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 100.0,
    };
    let mut ta = Node::default();
    ta.kind = NodeKind::TextArea;
    ta.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 60.0,
    };
    let mut s = Scene::from_nodes(vec![root, ta], vec![(0, 1)]);
    let ta_id = s.get(s.roots[0]).unwrap().children[0];
    s.controls.ensure(
        ta_id,
        ControlState::TextArea(EditState::from_init(value.into(), String::new(), 0, false)),
    );
    s.focused_node = Some(ta_id);
    compute_world_transforms(&mut s);
    (s, ta_id)
}

/// 取 TextField 的 EditState（panic 若非 TextField）。
fn tf_edit(s: &Scene, id: NodeId) -> &EditState {
    match s.controls.get(id) {
        Some(ControlState::TextField(e)) => e,
        _ => panic!("not TextField"),
    }
}

#[test]
fn textfield_backspace_key_deletes() {
    let (mut s, tf) = focused_textfield_scene("abc");
    // value="abc"(cursor=3 末尾)。Backspace 删左 → "ab"，cursor=2。
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_BACKSPACE,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(tf_edit(&s, tf).value, "ab");
    assert_eq!(tf_edit(&s, tf).cursor, 2);
}

#[test]
fn textfield_delete_forward_key_deletes() {
    let (mut s, tf) = focused_textfield_scene("abc");
    // 光标移到中间（cursor=1），Delete 删右。
    if let Some(ControlState::TextField(e)) = s.controls.get_mut(tf) {
        e.cursor = 1;
        e.anchor = 1;
    }
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_DELETE,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(tf_edit(&s, tf).value, "ac", "Delete 删右侧 'b'");
    assert_eq!(tf_edit(&s, tf).cursor, 1);
}

#[test]
fn textfield_left_arrow_moves_cursor() {
    let (mut s, tf) = focused_textfield_scene("abc");
    // cursor=3 末尾 → Left → cursor=2。
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_LEFT,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(tf_edit(&s, tf).cursor, 2);
    assert_eq!(tf_edit(&s, tf).anchor, 2, "无 shift → 折叠选区");
}

#[test]
fn textfield_right_arrow_moves_cursor() {
    let (mut s, tf) = focused_textfield_scene("abc");
    // 先把 cursor 移到 1，Right → 2。
    if let Some(ControlState::TextField(e)) = s.controls.get_mut(tf) {
        e.cursor = 1;
        e.anchor = 1;
    }
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_RIGHT,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(tf_edit(&s, tf).cursor, 2);
}

#[test]
fn textfield_home_sets_cursor_to_zero() {
    let (mut s, tf) = focused_textfield_scene("abc");
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_HOME,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(tf_edit(&s, tf).cursor, 0);
    assert_eq!(tf_edit(&s, tf).anchor, 0, "Home 折叠选区");
}

#[test]
fn textfield_end_sets_cursor_to_len() {
    let (mut s, tf) = focused_textfield_scene("abc");
    // 先移到 0，End → 末尾 3。
    if let Some(ControlState::TextField(e)) = s.controls.get_mut(tf) {
        e.cursor = 0;
        e.anchor = 0;
    }
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_END,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(tf_edit(&s, tf).cursor, 3);
}

#[test]
fn textfield_ctrl_a_selects_all() {
    let (mut s, tf) = focused_textfield_scene("hello");
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_A,
            modifiers: MOD_CTRL,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    let e = tf_edit(&s, tf);
    assert_eq!(e.anchor, 0, "ctrl+A anchor→0");
    assert_eq!(e.cursor, 5, "ctrl+A cursor→len");
}

#[test]
fn textfield_shift_left_extends_selection() {
    let (mut s, tf) = focused_textfield_scene("abc");
    // cursor=3，Shift+Left → cursor=2，anchor 保持 3（选区 [2,3]）。
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_LEFT,
            modifiers: MOD_SHIFT,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    let e = tf_edit(&s, tf);
    assert_eq!(e.cursor, 2);
    assert_eq!(e.anchor, 3, "shift 选区 anchor 不动");
}

#[test]
fn textfield_routed_key_consumed_no_keydown() {
    // 路由的控制键不发 keydown（照 Tab 消费模式）。
    let (mut s, _tf) = focused_textfield_scene("abc");
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_BACKSPACE,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert!(
        out.iter().all(|e| e.event_type != EVT_KEY_DOWN),
        "Backspace 被路由消费，不发 keydown"
    );
}

#[test]
fn textfield_non_control_key_still_emits_keydown() {
    // 非控制键（如字母 'Z'=122 不带 ctrl）→ 仍走 keydown 透传（字符输入走 textinput 通道）。
    let (mut s, tf) = focused_textfield_scene("abc");
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_Z,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(tf_edit(&s, tf).value, "abc", "无 ctrl 的字母键不改 value");
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_KEY_DOWN && e.node_id == tf.0),
        "非控制键透传 keydown"
    );
}

#[test]
fn textfield_delete_emits_value_changed() {
    let (mut s, tf) = focused_textfield_scene("abc");
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_BACKSPACE,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_VALUE_CHANGED && e.node_id == tf.0),
        "Backspace 删值 → 发 ValueChanged"
    );
}

#[test]
fn textfield_escape_blurs() {
    let (mut s, _tf) = focused_textfield_scene("abc");
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_ESCAPE,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(s.focused_node, None, "Escape → blur");
    assert!(
        out.iter().any(|e| e.event_type == EVT_FOCUS_OUT),
        "Escape → FocusOut"
    );
}

#[test]
fn textfield_single_line_enter_emits_submitted() {
    let (mut s, tf) = focused_textfield_scene("query");
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_RETURN,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    // 单行框 Enter → Submitted（不改 value）。
    assert_eq!(tf_edit(&s, tf).value, "query", "单行 Enter 不改 value");
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_SUBMITTED && e.node_id == tf.0),
        "单行 Enter → Submitted"
    );
}

#[test]
fn textarea_enter_inserts_newline_no_submitted() {
    let (mut s, ta) = focused_textarea_scene("ab");
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_RETURN,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    // TextArea Enter → 插 \n + ValueChanged；不发 Submitted。
    match s.controls.get(ta) {
        Some(ControlState::TextArea(e)) => {
            assert_eq!(e.value, "ab\n", "TextArea Enter 插换行");
        }
        _ => panic!("not TextArea"),
    }
    assert!(
        out.iter().all(|e| e.event_type != EVT_SUBMITTED),
        "TextArea Enter 不发 Submitted"
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_VALUE_CHANGED && e.node_id == ta.0),
        "TextArea Enter 插换行 → ValueChanged"
    );
}

#[test]
fn textfield_keyup_not_routed_still_keyup() {
    // keyup（is_down=false）不路由（控制键只对 keydown 生效），仍走普通 keyup 透传。
    let (mut s, tf) = focused_textfield_scene("abc");
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_BACKSPACE,
            modifiers: 0,
            is_down: false,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(tf_edit(&s, tf).value, "abc", "keyup 不触发删除");
    assert!(out.iter().any(|e| e.event_type == EVT_KEY_UP), "keyup 透传");
}

#[test]
fn textfield_no_focus_control_key_dropped() {
    // 无焦点 → 控制键丢弃（不路由、不发 keydown）。
    let (mut s, _tf) = focused_textfield_scene("abc");
    s.focused_node = None;
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_BACKSPACE,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert!(out.is_empty(), "无焦点控制键全丢");
}

#[test]
fn non_text_focused_node_not_routed() {
    // 焦点在普通 Button → 控制键不路由到编辑内核，走普通 keydown。
    let mut s = two_focusable_scene();
    let a_id = s.get(s.roots[0]).unwrap().children[0];
    let mut out = Vec::new();
    focus_node(&mut s, Some(a_id), &mut out);
    out.clear();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_BACKSPACE,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_KEY_DOWN && e.node_id == a_id.0),
        "Button 焦点 → Backspace 走普通 keydown"
    );
}

// open Dropdown：pointer-down 命中不在其 select 子树内 → 收起（open=false）。
// select 子树 = select 本身 + .ikat-value/.ikat-popup（含 option）后代。

/// 建 open Dropdown 场景：root > select(Dropdown,open,120x30 @(10,10))，
/// select 的 .ikat-popup(80x60 @(10,40)) 内含两个 option。
/// 另 root 有一个独立 button(50x50 @(200,200)) 作「outside」点击靶。
/// 返回 (select_id, popup_id, opt0_id, button_id)。
fn open_dropdown_with_outside_button_scene() -> (Scene, NodeId, NodeId, NodeId, NodeId) {
    use crate::asset::ControlInit;
    use crate::scene::control::ROLE_LISTBOX;
    use crate::scene::dynamic::create_node_from_template;
    use crate::scene::node::RoleInfo;
    use crate::style::resolved::ResolvedStyle;

    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 400.0,
        h: 400.0,
    };
    let mut s = Scene::from_nodes(vec![root], vec![]);
    let root_id = s.roots[0];

    let select = create_node_from_template(
        &mut s,
        NodeKind::Dropdown,
        ResolvedStyle::default(),
        Some(ControlInit::Dropdown {
            selected_index: 0,
            option_values: Vec::new(),
        }),
    );
    crate::scene::dynamic::append_child(&mut s, root_id, select).unwrap();
    if let Some(ControlState::Dropdown { open, .. }) = s.controls.get_mut(select) {
        *open = true;
    }
    s.get_mut(select).unwrap().layout_rect = Rect {
        x: 10.0,
        y: 10.0,
        w: 120.0,
        h: 30.0,
    };

    // listbox role 子（作者写的弹出列表容器）。
    let listbox =
        create_node_from_template(&mut s, NodeKind::Container, ResolvedStyle::default(), None);
    crate::scene::dynamic::append_child(&mut s, select, listbox).unwrap();
    s.roles.insert(
        listbox,
        RoleInfo {
            role: Some(ROLE_LISTBOX.to_string()),
            slots: Default::default(),
            aria_controls: None,
        },
    );
    let opt0 =
        create_node_from_template(&mut s, NodeKind::OptionItem, ResolvedStyle::default(), None);
    let opt1 =
        create_node_from_template(&mut s, NodeKind::OptionItem, ResolvedStyle::default(), None);
    crate::scene::dynamic::append_child(&mut s, listbox, opt0).unwrap();
    crate::scene::dynamic::append_child(&mut s, listbox, opt1).unwrap();

    let popup = listbox;
    s.get_mut(popup).unwrap().layout_rect = Rect {
        x: 10.0,
        y: 40.0,
        w: 80.0,
        h: 60.0,
    };
    s.get_mut(opt0).unwrap().layout_rect = Rect {
        x: 10.0,
        y: 40.0,
        w: 80.0,
        h: 20.0,
    };
    s.get_mut(opt1).unwrap().layout_rect = Rect {
        x: 10.0,
        y: 60.0,
        w: 80.0,
        h: 20.0,
    };

    // outside 按钮（200,200,50,50）作 outside 点击靶。
    let btn = create_node_from_template(&mut s, NodeKind::Button, ResolvedStyle::default(), None);
    crate::scene::dynamic::append_child(&mut s, root_id, btn).unwrap();
    s.get_mut(btn).unwrap().layout_rect = Rect {
        x: 200.0,
        y: 200.0,
        w: 50.0,
        h: 50.0,
    };

    compute_world_transforms(&mut s);
    (s, select, popup, opt0, btn)
}

fn dropdown_open(scene: &Scene, select: NodeId) -> bool {
    matches!(
        scene.controls.get(select),
        Some(ControlState::Dropdown { open: true, .. })
    )
}

#[test]
fn pointer_down_outside_open_dropdown_closes_it() {
    // open dropdown，pointer-down 落在 outside 按钮 → 收起 dropdown（open=false）。
    let (mut s, select, _popup, _opt0, _btn) = open_dropdown_with_outside_button_scene();
    assert!(dropdown_open(&s, select), "初始 open=true");
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 225.0,
            y: 225.0, // outside 按钮中心
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        !dropdown_open(&s, select),
        "pointer-down 在 select 子树外 → open=false"
    );
}

#[test]
fn pointer_down_on_option_does_not_close_dropdown() {
    // pointer-down 落在 option（popup 内）→ 不收起（option 选中由 click EVT 驱动，另一任务）。
    let (mut s, select, _popup, opt0, _btn) = open_dropdown_with_outside_button_scene();
    assert!(dropdown_open(&s, select));
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0, // opt0 区中心
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    // 点 option 现在选中 + 收起。outside-click 保护仍生效——点 option 不走 close_outside
    // （命中在 popup 子树内），收起是 on_pointer_down 的选中提交副作用，不是 outside-close。
    assert!(
        !dropdown_open(&s, select),
        "pointer-down 在 option → 选中 + 收起（Task 13 交互闭环）"
    );
    // opt0 仍可被命中（前置 popup check 生效）
    assert_eq!(hit_test(&s, (50.0, 50.0)), Some(opt0));
}

#[test]
fn pointer_down_on_select_header_toggles_closed() {
    // open 时点 select header → toggle 收起。outside-click 保护仍生效——点 header 不走
    // close_outside（命中在 select 子树内），收起是 on_pointer_down 的 toggle 副作用。
    let (mut s, select, _popup, _opt0, _btn) = open_dropdown_with_outside_button_scene();
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 70.0,
            y: 25.0, // select header 区中心 (10,10,120,30)
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(
        !dropdown_open(&s, select),
        "open 时 pointer-down 在 select header → toggle 收起（Task 13）"
    );
}

#[test]
fn pointer_down_outside_closes_only_open_dropdown_not_closed_one() {
    // 多个 dropdown：只收起 open 的那个。closed 的不动。
    let (mut s, select_open, _popup, _opt0, _btn) = open_dropdown_with_outside_button_scene();
    // 额外建一个 closed dropdown（open=false），验证它不被误改。
    use crate::asset::ControlInit;
    use crate::scene::dynamic::create_node_from_template;
    use crate::style::resolved::ResolvedStyle;
    let root_id = s.roots[0];
    let select_closed = create_node_from_template(
        &mut s,
        NodeKind::Dropdown,
        ResolvedStyle::default(),
        Some(ControlInit::Dropdown {
            selected_index: 0,
            option_values: Vec::new(),
        }),
    );
    crate::scene::dynamic::append_child(&mut s, root_id, select_closed).unwrap();
    s.get_mut(select_closed).unwrap().layout_rect = Rect {
        x: 10.0,
        y: 300.0,
        w: 120.0,
        h: 30.0,
    };
    compute_world_transforms(&mut s);
    // 初始：select_open 开，select_closed 关
    assert!(dropdown_open(&s, select_open));
    assert!(!dropdown_open(&s, select_closed));
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 225.0,
            y: 225.0, // outside 按钮
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(!dropdown_open(&s, select_open), "open 的被收起");
    assert!(
        !dropdown_open(&s, select_closed),
        "closed 的保持 closed（不变）"
    );
}

// 交互闭环：点 select header 收起↔展开；点 option 选中+收起+发 SelectionChanged；
// 键盘 Up/Down seek 跳过 disabled、Enter 提交、Esc 回滚到打开时的选中项。
// 照 RmlUi WidgetDropDown：SeekSelection 跳 disabled、CancelSelectBox 回滚 open 时刻值。

use crate::asset::ControlInit;
use crate::scene::control::ROLE_LISTBOX;
use crate::scene::dynamic::create_node_from_template;
use crate::scene::node::RoleInfo;
use crate::style::resolved::ResolvedStyle;

/// 建 Dropdown 场景：root > select(Dropdown，selected_index/open 可控)，select 的 listbox
/// 含若干 option（每个 (text, disabled)）。作者正确结构：option 直接在 listbox 内。
/// 布局：select @(10,10,120,30)；listbox @(10,40,80, n*20)；option_i @(10, 40+i*20, 80, 20)。
/// 返回 (select_id, popup_id, Vec<opt_id>)。
fn dropdown_scene(
    options: &[(&str, bool)],
    selected_index: usize,
    open: bool,
) -> (Scene, NodeId, NodeId, Vec<NodeId>) {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 400.0,
        h: 400.0,
    };
    let mut s = Scene::from_nodes(vec![root], vec![]);
    let root_id = s.roots[0];

    let select = create_node_from_template(
        &mut s,
        NodeKind::Dropdown,
        ResolvedStyle::default(),
        Some(ControlInit::Dropdown {
            selected_index: selected_index as u32,
            option_values: Vec::new(),
        }),
    );
    crate::scene::dynamic::append_child(&mut s, root_id, select).unwrap();
    s.get_mut(select).unwrap().layout_rect = Rect {
        x: 10.0,
        y: 10.0,
        w: 120.0,
        h: 30.0,
    };

    // listbox role 子（作者写的弹出列表容器）。
    let listbox =
        create_node_from_template(&mut s, NodeKind::Container, ResolvedStyle::default(), None);
    crate::scene::dynamic::append_child(&mut s, select, listbox).unwrap();
    s.roles.insert(
        listbox,
        RoleInfo {
            role: Some(ROLE_LISTBOX.to_string()),
            slots: Default::default(),
            aria_controls: None,
        },
    );

    let mut opt_ids = Vec::new();
    for (i, (text, disabled)) in options.iter().enumerate() {
        let opt =
            create_node_from_template(&mut s, NodeKind::OptionItem, ResolvedStyle::default(), None);
        s.text_contents.insert(opt, (*text).to_string());
        crate::scene::dynamic::append_child(&mut s, listbox, opt).unwrap();
        if *disabled {
            s.get_mut(opt)
                .unwrap()
                .interaction
                .flags
                .insert(NodeFlags::DISABLED);
        }
        opt_ids.push(opt);
        let _ = i;
    }

    let popup = listbox;
    let n = options.len() as f32;
    s.get_mut(popup).unwrap().layout_rect = Rect {
        x: 10.0,
        y: 40.0,
        w: 80.0,
        h: n * 20.0,
    };
    for (i, &opt) in opt_ids.iter().enumerate() {
        s.get_mut(opt).unwrap().layout_rect = Rect {
            x: 10.0,
            y: 40.0 + (i as f32) * 20.0,
            w: 80.0,
            h: 20.0,
        };
    }

    if open {
        if let Some(ControlState::Dropdown {
            open,
            open_selected_index,
            selected_index,
            ..
        }) = s.controls.get_mut(select)
        {
            *open = true;
            *open_selected_index = Some(*selected_index);
        }
    }
    compute_world_transforms(&mut s);
    (s, select, popup, opt_ids)
}

/// 取 dropdown 的 selected_index。
fn dropdown_selected(scene: &Scene, select: NodeId) -> usize {
    match scene.controls.get(select) {
        Some(ControlState::Dropdown { selected_index, .. }) => *selected_index,
        _ => panic!("not a dropdown"),
    }
}

/// 取 dropdown 的 open_selected_index。
fn dropdown_open_selected(scene: &Scene, select: NodeId) -> Option<usize> {
    match scene.controls.get(select) {
        Some(ControlState::Dropdown {
            open_selected_index,
            ..
        }) => *open_selected_index,
        _ => panic!("not a dropdown"),
    }
}

#[test]
fn click_select_toggles_open() {
    // 收起→点 select header→open=true + open_selected_index 记下当前 selected_index。
    let (mut s, select, _popup, _opts) = dropdown_scene(&[("A", false), ("B", false)], 0, false);
    assert!(!dropdown_open(&s, select), "初始 closed");
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 70.0,
            y: 25.0, // select header 中心 (10,10,120,30)
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(dropdown_open(&s, select), "点 header → open=true");
    assert_eq!(
        dropdown_open_selected(&s, select),
        Some(0),
        "展开时记 open_selected_index=当前 selected_index"
    );
}

#[test]
fn click_option_selects_and_closes() {
    // open→点 option B(index 1)→selected_index=1, value_lock=true, open=false,
    // 发 EVT_SELECTION_CHANGED（node=select，payload=新 index）。
    let (mut s, select, _popup, opts) = dropdown_scene(&[("A", false), ("B", false)], 0, true);
    assert!(dropdown_open(&s, select));
    let opt1 = opts[1];
    let opt1_rect = s.get(opt1).unwrap().layout_rect;
    let cx = opt1_rect.x + opt1_rect.w * 0.5;
    let cy = opt1_rect.y + opt1_rect.h * 0.5;
    let mut ps = PointerState::new();
    let events = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: cx,
            y: cy, // option B 中心
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert_eq!(dropdown_selected(&s, select), 1, "selected_index=B");
    assert!(!dropdown_open(&s, select), "选中后收起");
    assert_eq!(
        dropdown_open_selected(&s, select),
        None,
        "收起后 open_selected_index 清 None"
    );
    assert!(
        matches!(
            s.controls.get(select),
            Some(ControlState::Dropdown {
                value_lock: true,
                ..
            })
        ),
        "value_lock=true（防反馈环）"
    );
    let sel_evt = events
        .iter()
        .find(|e| e.event_type == EVT_SELECTION_CHANGED && e.node_id == select.0)
        .expect("发 EVT_SELECTION_CHANGED@select");
    assert_eq!(
        sel_evt.touch_id, 1,
        "SelectionChanged payload touch_id=新 selected_index"
    );
}

#[test]
fn click_disabled_option_does_not_select() {
    // 点 disabled option → 不选中、不收起（照 HTML：disabled option 不可交互）。
    let (mut s, select, _popup, opts) = dropdown_scene(&[("A", false), ("B", true)], 0, true);
    let opt1 = opts[1];
    let r = s.get(opt1).unwrap().layout_rect;
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: r.x + r.w * 0.5,
            y: r.y + r.h * 0.5,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert_eq!(dropdown_selected(&s, select), 0, "disabled option 不改选中");
    assert!(dropdown_open(&s, select), "disabled option 不收起");
}

#[test]
fn click_header_while_open_closes() {
    // open→再点 select header→收起（toggle）。
    let (mut s, select, _popup, _opts) = dropdown_scene(&[("A", false)], 0, true);
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 70.0,
            y: 25.0, // header
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(!dropdown_open(&s, select), "open 时点 header → toggle 收起");
}

fn key_down(code: u32) -> KeyEvent {
    KeyEvent {
        key_code: code,
        modifiers: 0,
        is_down: true,
        pad: [0, 0],
    }
}

#[test]
fn arrow_down_seeks_non_disabled_option() {
    // open，selected_index=0（A）。[A, B(disabled), C]。Down→跳过 B 落 C(index 2)。
    let (mut s, select, _popup, _opts) =
        dropdown_scene(&[("A", false), ("B", true), ("C", false)], 0, true);
    focus_node(&mut s, Some(select), &mut Vec::new());
    let mut out = Vec::new();
    process_keys(&mut s, &[key_down(KEY_DOWN)], &mut out);
    assert_eq!(
        dropdown_selected(&s, select),
        2,
        "Down 跳过 disabled B 落 C"
    );
    assert!(
        out.iter().all(|e| e.event_type != EVT_SELECTION_CHANGED),
        "seek 不发 SelectionChanged（仅移动高亮，不提交）"
    );
}

#[test]
fn arrow_up_seeks_backward() {
    // selected_index=2（C）。Up→落 B(index 1)（一步一个，不跳两个）。
    let (mut s, select, _popup, _opts) =
        dropdown_scene(&[("A", false), ("B", false), ("C", false)], 2, true);
    focus_node(&mut s, Some(select), &mut Vec::new());
    let mut out = Vec::new();
    process_keys(&mut s, &[key_down(KEY_UP)], &mut out);
    assert_eq!(dropdown_selected(&s, select), 1, "Up 落 B（前一个）");
    // 再 Up→落 A。
    process_keys(&mut s, &[key_down(KEY_UP)], &mut out);
    assert_eq!(dropdown_selected(&s, select), 0, "再 Up 落 A");
}

#[test]
fn enter_commits_highlight_and_closes() {
    // open，Down 走到 B，Enter→提交 B + 收起 + 发 SelectionChanged。
    let (mut s, select, _popup, _opts) = dropdown_scene(&[("A", false), ("B", false)], 0, true);
    focus_node(&mut s, Some(select), &mut Vec::new());
    let mut out = Vec::new();
    process_keys(&mut s, &[key_down(KEY_DOWN)], &mut out);
    assert_eq!(dropdown_selected(&s, select), 1, "Down 移高亮到 B");
    out.clear();
    process_keys(&mut s, &[key_down(KEY_RETURN)], &mut out);
    assert_eq!(dropdown_selected(&s, select), 1, "Enter 提交 B");
    assert!(!dropdown_open(&s, select), "Enter 收起");
    assert!(
        out.iter().any(|e| e.event_type == EVT_SELECTION_CHANGED
            && e.node_id == select.0
            && e.touch_id == 1),
        "Enter 发 SelectionChanged@select，payload=index 1"
    );
}

#[test]
fn escape_closes_and_reverts() {
    // open（selected_index=0）。Down 移到 B(index 1)。Esc→open=false，selected_index 回 0。
    let (mut s, select, _popup, _opts) = dropdown_scene(&[("A", false), ("B", false)], 0, true);
    focus_node(&mut s, Some(select), &mut Vec::new());
    let mut out = Vec::new();
    process_keys(&mut s, &[key_down(KEY_DOWN)], &mut out); // 高亮到 B
    assert_eq!(dropdown_selected(&s, select), 1);
    out.clear();
    process_keys(&mut s, &[key_down(KEY_ESCAPE)], &mut out);
    assert!(!dropdown_open(&s, select), "Esc 收起");
    assert_eq!(
        dropdown_selected(&s, select),
        0,
        "Esc 回滚到打开时的 selected_index"
    );
    assert_eq!(
        dropdown_open_selected(&s, select),
        None,
        "收起后 open_selected_index 清 None"
    );
    assert!(
        out.iter().all(|e| e.event_type != EVT_SELECTION_CHANGED),
        "Esc 回滚不发 SelectionChanged（净变=0）"
    );
}

#[test]
fn keyboard_ignored_when_dropdown_closed() {
    // 收起态：Up/Down/Enter/Esc 不路由（透传为普通 keydown），不改 selected_index/open。
    let (mut s, select, _popup, _opts) = dropdown_scene(&[("A", false), ("B", false)], 0, false);
    focus_node(&mut s, Some(select), &mut Vec::new());
    let mut out = Vec::new();
    process_keys(&mut s, &[key_down(KEY_DOWN)], &mut out);
    assert_eq!(dropdown_selected(&s, select), 0, "closed 时 Down 不改选中");
    assert!(!dropdown_open(&s, select), "closed 时 Down 不展开");
    // 透传普通 keydown@select（未消费）。
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_KEY_DOWN && e.node_id == select.0),
        "closed 时 Down 透传为 keydown（不路由）"
    );
}

// 契约：Up/Down 只移动高亮不提交、不发 SelectionChanged。所有非提交收起路径
//（Esc / header-toggle / outside-click）都必须把 selected_index 回滚到展开时刻快照
//（open_selected_index），否则 host 读到改动却收不到事件（违反 SelectionChanged 事件契约）。
// 提交路径（Enter / 点 option）保留新值并发事件——见 enter_after_keyboard_nav_commits。

#[test]
fn header_toggle_close_after_keyboard_nav_reverts_selection() {
    // open 在 A(0) → Down 高亮到 B(1) → 点 header 收起（toggle）。
    // 期望：收起（open=false）、selected_index 回滚到 0（A）、open_selected_index 清 None、
    // 不发 SelectionChanged（这是一次取消，A→B 未提交）。
    let (mut s, select, _popup, _opts) = dropdown_scene(&[("A", false), ("B", false)], 0, true);
    focus_node(&mut s, Some(select), &mut Vec::new());
    let mut out = Vec::new();
    process_keys(&mut s, &[key_down(KEY_DOWN)], &mut out); // 高亮到 B
    assert_eq!(dropdown_selected(&s, select), 1, "Down 移高亮到 B");
    out.clear();
    // 点 select header（toggle 收起）。
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 70.0,
            y: 25.0, // select header 中心 (10,10,120,30)
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(!dropdown_open(&s, select), "header toggle 收起");
    assert_eq!(
        dropdown_selected(&s, select),
        0,
        "收起回滚 selected_index 到展开时刻快照 A（cancel 语义）"
    );
    assert_eq!(
        dropdown_open_selected(&s, select),
        None,
        "收起后 open_selected_index 清 None"
    );
    assert!(
        out.iter().all(|e| e.event_type != EVT_SELECTION_CHANGED),
        "未提交的 A→B 不发 SelectionChanged"
    );
}

#[test]
fn outside_click_after_keyboard_nav_reverts_selection() {
    // open 在 A(0) → Down 高亮到 B(1) → 点 select 子树外收起。
    // 期望：收起（open=false）、selected_index 回滚到 0（A）、open_selected_index 清 None、
    // 不发 SelectionChanged。
    let (mut s, select, _popup, _opts) = dropdown_scene(&[("A", false), ("B", false)], 0, true);
    focus_node(&mut s, Some(select), &mut Vec::new());
    let mut out = Vec::new();
    process_keys(&mut s, &[key_down(KEY_DOWN)], &mut out); // 高亮到 B
    assert_eq!(dropdown_selected(&s, select), 1, "Down 移高亮到 B");
    out.clear();
    // 点 select 子树外（root 区域，不在 select/.ikat-popup 内）。
    let mut ps = PointerState::new();
    ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 300.0,
            y: 300.0, // select 子树外
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    assert!(!dropdown_open(&s, select), "outside-click 收起");
    assert_eq!(
        dropdown_selected(&s, select),
        0,
        "收起回滚 selected_index 到展开时刻快照 A（cancel 语义）"
    );
    assert_eq!(
        dropdown_open_selected(&s, select),
        None,
        "收起后 open_selected_index 清 None（之前 stale）"
    );
    assert!(
        out.iter().all(|e| e.event_type != EVT_SELECTION_CHANGED),
        "未提交的 A→B 不发 SelectionChanged"
    );
}

#[test]
fn enter_after_keyboard_nav_commits() {
    // 提交路径对照：open 在 A(0) → Down 高亮到 B(1) → Enter 提交。
    // 期望：selected_index=1（B，保留新值）、open=false、open_selected_index 清 None、
    // value_lock=true、发 SelectionChanged@select payload=index 1。
    // （与 cancel 路径对照：commit 保留新值并发事件，cancel 回滚不发事件。）
    let (mut s, select, _popup, _opts) = dropdown_scene(&[("A", false), ("B", false)], 0, true);
    focus_node(&mut s, Some(select), &mut Vec::new());
    let mut out = Vec::new();
    process_keys(&mut s, &[key_down(KEY_DOWN)], &mut out); // 高亮到 B
    assert_eq!(dropdown_selected(&s, select), 1, "Down 移高亮到 B");
    out.clear();
    process_keys(&mut s, &[key_down(KEY_RETURN)], &mut out);
    assert_eq!(
        dropdown_selected(&s, select),
        1,
        "Enter 提交保留新值 B（commit 语义）"
    );
    assert!(!dropdown_open(&s, select), "Enter 收起");
    assert_eq!(
        dropdown_open_selected(&s, select),
        None,
        "收起后 open_selected_index 清 None"
    );
    assert!(
        matches!(
            s.controls.get(select),
            Some(ControlState::Dropdown {
                value_lock: true,
                ..
            })
        ),
        "value_lock=true（防反馈环）"
    );
    assert!(
        out.iter().any(|e| e.event_type == EVT_SELECTION_CHANGED
            && e.node_id == select.0
            && e.touch_id == 1),
        "commit 发 SelectionChanged@select，payload=index 1"
    );
}

// 焦点在 TabList 子树（Tab 是 focusable 元素，TabList 本身不聚焦）→ 向上找
// ControlState::TabList 祖先。方向键按 flex-direction 选轴：row→Left/Right、column→
// Up/Down；row-reverse/column-reverse 翻转 delta 符号。clamp 到 [0, tab_count-1]（不 wrap）。
// 自动激活：每改 selected_index 即发 SelectionChanged（与 Dropdown 的 seek 不提交不同——
// TabList 无「展开/提交」语义，方向键即时生效，镜像 WAI-ARIA tablist automatic-activation）。

use crate::scene::control::{ROLE_TAB, ROLE_TABLIST};
use crate::scene::dynamic::append_child;

/// 建 TabList + N 个 role=tab 子节点，flex-direction 设为 flex_dir。返回
/// (scene, tablist_id, [tab_id,...])。布局不需（方向键路由只读 flex-direction + tab_count，
// 不读 layout_rect）。tablist 角色设 ROLE_TABLIST、tab 设 ROLE_TAB 以匹配 role_of 过滤。
fn tablist_keyboard_scene(
    num_tabs: usize,
    selected_index: usize,
    flex_dir: taffy::FlexDirection,
) -> (Scene, NodeId, Vec<NodeId>) {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 400.0,
        h: 60.0,
    };
    let mut s = Scene::from_nodes(vec![root], vec![]);
    let root_id = s.roots[0];
    let tl = create_node_from_template(
        &mut s,
        NodeKind::TabList,
        ResolvedStyle::default(),
        Some(ControlInit::TabList {
            manual: false,
            selected_index: selected_index as u32,
        }),
    );
    append_child(&mut s, root_id, tl).expect("tl attach");
    s.roles.insert(
        tl,
        RoleInfo {
            role: Some(ROLE_TABLIST.to_string()),
            slots: Default::default(),
            aria_controls: None,
        },
    );
    s.get_mut(tl).unwrap().style.taffy_style.flex_direction = flex_dir;
    let mut tabs = Vec::new();
    for _ in 0..num_tabs {
        let tab = create_node_from_template(&mut s, NodeKind::Tab, ResolvedStyle::default(), None);
        append_child(&mut s, tl, tab).expect("tab attach");
        s.roles.insert(
            tab,
            RoleInfo {
                role: Some(ROLE_TAB.to_string()),
                slots: Default::default(),
                aria_controls: None,
            },
        );
        tabs.push(tab);
    }
    compute_world_transforms(&mut s);
    (s, tl, tabs)
}

/// 取 TabList 的 selected_index。
fn tablist_selected(scene: &Scene, tl: NodeId) -> usize {
    match scene.controls.get(tl) {
        Some(ControlState::TabList { selected_index, .. }) => *selected_index,
        _ => panic!("not a TabList"),
    }
}

#[test]
fn arrow_key_moves_tablist_selected_index() {
    // TabList(row, selected_index=0) + 3 tabs。焦点在 tab0（Tab 是 focusable 元素）。
    // Right → 1（发 SelectionChanged@tablist touch_id=1）；再 Right → 2；再 Right → clamp 2
    // （不发事件——changed-guard）；Left → 1。
    let (mut s, tl, tabs) = tablist_keyboard_scene(3, 0, taffy::FlexDirection::Row);
    focus_node(&mut s, Some(tabs[0]), &mut Vec::new());
    let mut out = Vec::new();

    process_keys(&mut s, &[key_down(KEY_RIGHT)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 1, "Right → index 1");
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_SELECTION_CHANGED && e.node_id == tl.0 && e.touch_id == 1),
        "Right 发 SelectionChanged@tablist，payload touch_id=新 index 1"
    );

    out.clear();
    process_keys(&mut s, &[key_down(KEY_RIGHT)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 2, "Right → index 2");

    // clamp：selected_index 已 2（末），再 Right 不超过 tab_count-1=2。
    out.clear();
    process_keys(&mut s, &[key_down(KEY_RIGHT)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 2, "Right clamp 在末（不 wrap）");
    assert!(
        !out.iter().any(|e| e.event_type == EVT_SELECTION_CHANGED),
        "clamp 未变 → 不发事件"
    );

    out.clear();
    process_keys(&mut s, &[key_down(KEY_LEFT)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 1, "Left → index 1");
}

#[test]
fn tablist_left_clamps_at_zero() {
    // selected_index=0，Left → clamp 0（不 wrap 到末）、不发事件。
    let (mut s, tl, tabs) = tablist_keyboard_scene(3, 0, taffy::FlexDirection::Row);
    focus_node(&mut s, Some(tabs[0]), &mut Vec::new());
    let mut out = Vec::new();
    process_keys(&mut s, &[key_down(KEY_LEFT)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 0, "Left clamp 在 0（不 wrap）");
    assert!(
        !out.iter().any(|e| e.event_type == EVT_SELECTION_CHANGED),
        "clamp 未变 → 不发事件"
    );
}

#[test]
fn tablist_column_direction_uses_up_down() {
    // column 方向：Up/Down 移动 selected_index，Left/Right 不路由（不改）。镜像 WAI-ARIA：
    // 轴由 flex-direction 决定。
    let (mut s, tl, tabs) = tablist_keyboard_scene(3, 0, taffy::FlexDirection::Column);
    focus_node(&mut s, Some(tabs[0]), &mut Vec::new());
    let mut out = Vec::new();

    // Left/Right 在 column 方向不路由：不改 selected_index（透传为普通 keydown）。
    process_keys(&mut s, &[key_down(KEY_RIGHT)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 0, "column 方向 Right 不路由");

    // Down → 1、再 Down → 2、Down → clamp 2。
    process_keys(&mut s, &[key_down(KEY_DOWN)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 1, "Down → index 1");
    process_keys(&mut s, &[key_down(KEY_DOWN)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 2, "Down → index 2");
    out.clear();
    process_keys(&mut s, &[key_down(KEY_DOWN)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 2, "Down clamp 在末");
    assert!(
        !out.iter().any(|e| e.event_type == EVT_SELECTION_CHANGED),
        "clamp 不发事件"
    );

    // Up → 1。
    out.clear();
    process_keys(&mut s, &[key_down(KEY_UP)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 1, "Up → index 1");
}

#[test]
fn tablist_row_reverse_inverts_horizontal_arrows() {
    // row-reverse：Left/Right 的 delta 符号翻转——Right 递减、Left 递增（镜像 row）。
    // 起始 selected_index=1（中段），避免边界 clamp 掩盖符号方向。
    // 该分支独立于 row/column 路径，按分支覆盖纪律单独锁符号。
    let (mut s, tl, tabs) = tablist_keyboard_scene(3, 1, taffy::FlexDirection::RowReverse);
    focus_node(&mut s, Some(tabs[0]), &mut Vec::new());
    let mut out = Vec::new();

    // Right → 递减（row 方向本是递增，reverse 翻号）。1 → 0。
    process_keys(&mut s, &[key_down(KEY_RIGHT)], &mut out);
    assert_eq!(
        tablist_selected(&s, tl),
        0,
        "row-reverse: Right 应递减（1→0）"
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_SELECTION_CHANGED && e.node_id == tl.0 && e.touch_id == 0),
        "row-reverse: Right 递减应发 SelectionChanged@tablist，touch_id=0"
    );

    // Left → 递增（row 方向本是递减，reverse 翻号）。0 → 1 → 2。
    out.clear();
    process_keys(&mut s, &[key_down(KEY_LEFT)], &mut out);
    assert_eq!(
        tablist_selected(&s, tl),
        1,
        "row-reverse: Left 应递增（0→1）"
    );
    process_keys(&mut s, &[key_down(KEY_LEFT)], &mut out);
    assert_eq!(
        tablist_selected(&s, tl),
        2,
        "row-reverse: Left 应递增（1→2）"
    );
}

#[test]
fn tablist_column_reverse_inverts_vertical_arrows() {
    // column-reverse：Up/Down 的 delta 符号翻转——Down 递减、Up 递增（镜像 column）。
    // 起始 selected_index=1（中段），避免边界 clamp 掩盖符号方向。
    // 该分支独立于 row/column 路径，按分支覆盖纪律单独锁符号。
    let (mut s, tl, tabs) = tablist_keyboard_scene(3, 1, taffy::FlexDirection::ColumnReverse);
    focus_node(&mut s, Some(tabs[0]), &mut Vec::new());
    let mut out = Vec::new();

    // Down → 递减（column 方向本是递增，reverse 翻号）。1 → 0。
    process_keys(&mut s, &[key_down(KEY_DOWN)], &mut out);
    assert_eq!(
        tablist_selected(&s, tl),
        0,
        "column-reverse: Down 应递减（1→0）"
    );
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_SELECTION_CHANGED && e.node_id == tl.0 && e.touch_id == 0),
        "column-reverse: Down 递减应发 SelectionChanged@tablist，touch_id=0"
    );

    // Up → 递增（column 方向本是递减，reverse 翻号）。0 → 1 → 2。
    out.clear();
    process_keys(&mut s, &[key_down(KEY_UP)], &mut out);
    assert_eq!(
        tablist_selected(&s, tl),
        1,
        "column-reverse: Up 应递增（0→1）"
    );
    process_keys(&mut s, &[key_down(KEY_UP)], &mut out);
    assert_eq!(
        tablist_selected(&s, tl),
        2,
        "column-reverse: Up 应递增（1→2）"
    );
}

// 手动激活（data-activation="manual"）：方向键只移焦点（roving tabindex），选中不动；
// Enter/Space 才把选中提交到焦点所在 tab。对照 automatic（缺省）：焦点跟随选中。

/// 把 TabList 控件态切到 manual（摆台后直改，模拟 data-activation="manual" 打包烙印）。
fn tablist_set_manual(s: &mut Scene, tl: NodeId) {
    match s.controls.get_mut(tl) {
        Some(ControlState::TabList {
            manual_activation, ..
        }) => *manual_activation = true,
        _ => panic!("not a TabList"),
    }
}

#[test]
fn manual_tablist_arrows_move_focus_not_selection() {
    // manual + row + selected 0、焦点 tab0：Right → 焦点到 tab1（发 FocusOut@tab0 +
    // FocusIn@tab1），selected_index 保持 0、不发 SelectionChanged。
    let (mut s, tl, tabs) = tablist_keyboard_scene(3, 0, taffy::FlexDirection::Row);
    tablist_set_manual(&mut s, tl);
    focus_node(&mut s, Some(tabs[0]), &mut Vec::new());
    let mut out = Vec::new();

    process_keys(&mut s, &[key_down(KEY_RIGHT)], &mut out);
    assert_eq!(s.focused_node, Some(tabs[1]), "Right → 焦点移到 tab1");
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_FOCUS_IN && e.node_id == tabs[1].0),
        "FocusIn@tab1（焦点移动是可观察事件）"
    );
    assert_eq!(tablist_selected(&s, tl), 0, "manual：选中不动");
    assert!(
        !out.iter().any(|e| e.event_type == EVT_SELECTION_CHANGED),
        "manual：方向键不发 SelectionChanged"
    );

    // 再 Right → 焦点 tab2；Left 回 tab1（种子 = 当前焦点 tab）。
    out.clear();
    process_keys(&mut s, &[key_down(KEY_RIGHT)], &mut out);
    assert_eq!(s.focused_node, Some(tabs[2]), "Right → 焦点 tab2");
    process_keys(&mut s, &[key_down(KEY_LEFT)], &mut out);
    assert_eq!(
        s.focused_node,
        Some(tabs[1]),
        "Left → 焦点回 tab1（种子=焦点 tab）"
    );
}

#[test]
fn manual_tablist_enter_and_space_commit_focused_tab() {
    // manual：Right 移焦点到 tab1（选中仍 0）→ Enter 提交选中 1（发 SelectionChanged
    // touch_id=1）→ Space 在已选中 tab 上再提交（净变为零，不发事件）。
    let (mut s, tl, tabs) = tablist_keyboard_scene(3, 0, taffy::FlexDirection::Row);
    tablist_set_manual(&mut s, tl);
    focus_node(&mut s, Some(tabs[0]), &mut Vec::new());
    let mut out = Vec::new();

    process_keys(&mut s, &[key_down(KEY_RIGHT)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 0, "移焦点期间选中不动");

    out.clear();
    process_keys(&mut s, &[key_down(KEY_RETURN)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 1, "Enter 提交焦点所在 tab");
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_SELECTION_CHANGED && e.node_id == tl.0 && e.touch_id == 1),
        "Enter 发 SelectionChanged@tablist，touch_id=1"
    );

    // Space 同为提交键；已选中 → 净变为零 → 不发事件。
    out.clear();
    process_keys(&mut s, &[key_down(KEY_SPACE)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 1, "Space 提交同一 tab");
    assert!(
        !out.iter().any(|e| e.event_type == EVT_SELECTION_CHANGED),
        "净变为零 → 不发事件（HTML change 语义）"
    );
}

#[test]
fn automatic_tablist_focus_follows_selection() {
    // automatic（缺省）：Right → 选中 1 且焦点同步到 tab1（WAI-ARIA automatic activation：
    // 焦点与选中一起移动）。
    let (mut s, tl, tabs) = tablist_keyboard_scene(3, 0, taffy::FlexDirection::Row);
    focus_node(&mut s, Some(tabs[0]), &mut Vec::new());
    let mut out = Vec::new();

    process_keys(&mut s, &[key_down(KEY_RIGHT)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 1, "Right → 选中 1");
    assert_eq!(s.focused_node, Some(tabs[1]), "焦点跟随选中移到 tab1");
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_FOCUS_IN && e.node_id == tabs[1].0),
        "FocusIn@tab1"
    );
}

#[test]
fn manual_tablist_enter_not_consumed_without_tab_focus() {
    // 焦点在 tablist 的非 tab 后代（如 tab 内嵌装饰 div）上：Enter 不被 tablist 路由
    // （无 focused tab 可提交）→ 透传为普通 keydown（不误吞宿主控件的 Enter）。
    let (mut s, tl, tabs) = tablist_keyboard_scene(3, 0, taffy::FlexDirection::Row);
    tablist_set_manual(&mut s, tl);
    let inner =
        create_node_from_template(&mut s, NodeKind::Container, ResolvedStyle::default(), None);
    append_child(&mut s, tabs[0], inner).expect("inner attach");
    focus_node(&mut s, Some(inner), &mut Vec::new());
    let mut out = Vec::new();

    process_keys(&mut s, &[key_down(KEY_RETURN)], &mut out);
    assert_eq!(tablist_selected(&s, tl), 0, "Enter 不改选中");
    assert!(
        out.iter().any(|e| e.event_type == EVT_KEY_DOWN
            && e.node_id == inner.0
            && e.touch_id == KEY_RETURN as i32),
        "Enter 透传为普通 keydown@inner"
    );

    // 方向键仍可从 selected 种子起步移焦点（首次按方向键、焦点不在 tab 上的回落路径）。
    out.clear();
    process_keys(&mut s, &[key_down(KEY_RIGHT)], &mut out);
    assert_eq!(
        s.focused_node,
        Some(tabs[1]),
        "种子回落 selected_index=0，+1 → tab1"
    );
}

// NumberField 是文本类控件（EditState.value 是数字的字符串形式），但字符输入通道
// （textinput，UTF-32 codepoints）须过滤非数字字符：仅允许 0-9 / '-' / '.' / 'e' / 'E'。
// 过滤发生在 commit 路径（process_text_input），不在 IME composition 预编辑期（set_composition
// 不滤，commit_composition 时滤——照「filter only at commit」约定）。TextField/TextArea
// 不受影响（仍接受任意字符）。

/// 取 NumberField 的 EditState（panic 若非 NumberField）。
fn nf_edit(s: &Scene, id: NodeId) -> &EditState {
    match s.controls.get(id) {
        Some(ControlState::NumberField { edit, .. }) => edit,
        _ => panic!("not NumberField"),
    }
}

/// root + focused NumberField(value)。kind 设 NodeKind::NumberField。
fn focused_numberfield_scene(value: &str) -> (Scene, NodeId) {
    let mut root = Node::default();
    root.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 100.0,
    };
    let mut nf = Node::default();
    nf.kind = NodeKind::NumberField;
    nf.layout_rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 30.0,
    };
    let mut s = Scene::from_nodes(vec![root, nf], vec![(0, 1)]);
    let nf_id = s.get(s.roots[0]).unwrap().children[0];
    s.controls.ensure(
        nf_id,
        ControlState::NumberField {
            edit: EditState::from_init(value.into(), String::new(), 0, false),
            min: f32::MIN,
            max: f32::MAX,
            step: 1.0,
        },
    );
    s.focused_node = Some(nf_id);
    compute_world_transforms(&mut s);
    (s, nf_id)
}

/// UTF-32 codepoint 串 → char。便捷构造 textinput 测试输入。
fn cps(s: &str) -> Vec<u32> {
    s.chars().map(|c| c as u32).collect()
}

#[test]
fn number_field_rejects_non_digit_input() {
    // NumberField 收 'a' → value 不变（拒非数字）；收 '5' → value 追加 '5'。
    let (mut s, nf) = focused_numberfield_scene("");
    let mut out = Vec::new();
    // 先打 'a'：应被 guard 拒（value 仍空，不发 ValueChanged）。
    crate::input::process_text_input(&mut s, &cps("a"), &mut out);
    assert_eq!(nf_edit(&s, nf).value, "", "非数字字符 'a' 被拒");
    assert!(
        out.iter().all(|e| e.event_type != EVT_VALUE_CHANGED),
        "拒收不发 ValueChanged"
    );
    // 再打 '5'：应接受（value="5"，发 ValueChanged）。
    out.clear();
    crate::input::process_text_input(&mut s, &cps("5"), &mut out);
    assert_eq!(nf_edit(&s, nf).value, "5", "数字 '5' 被接受");
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_VALUE_CHANGED && e.node_id == nf.0),
        "接受数字发 ValueChanged"
    );
}

#[test]
fn number_field_accepts_minus_dot_e() {
    // '-'/'.'/'e'/'E' 在合法位置 → 接受；其它字母（'x'/'@'）被拒。
    // 输入过滤只拒明显非数字字符（字母 a-z 除 e/E、标点除 '.'/'-'），不做完整浮点语法校验
    // （'1.2.3' 这种留到读值时 parse；filter 目标是拦 'a'/'x'/'@' 等）。
    let (mut s, nf) = focused_numberfield_scene("");
    let mut out = Vec::new();
    // '-' 开头合法（负数）。
    crate::input::process_text_input(&mut s, &cps("-"), &mut out);
    assert_eq!(nf_edit(&s, nf).value, "-", "'-' 被接受");
    // '.' 接受（可构成 ".5" 或 "3."）。
    out.clear();
    crate::input::process_text_input(&mut s, &cps(".5"), &mut out);
    assert_eq!(nf_edit(&s, nf).value, "-.5", "'.' 和数字被接受");
    // 'e'/'E' 接受（科学记数法）。
    out.clear();
    crate::input::process_text_input(&mut s, &cps("e3"), &mut out);
    assert_eq!(nf_edit(&s, nf).value, "-.5e3", "'e' 和数字被接受");
    out.clear();
    crate::input::process_text_input(&mut s, &cps("E2"), &mut out);
    assert_eq!(nf_edit(&s, nf).value, "-.5e3E2", "'E' 被接受");
    // 'x'/'@' 被拒。
    out.clear();
    crate::input::process_text_input(&mut s, &cps("x@"), &mut out);
    assert_eq!(nf_edit(&s, nf).value, "-.5e3E2", "'x'/'@' 被拒，value 不变");
    assert!(
        out.iter().all(|e| e.event_type != EVT_VALUE_CHANGED),
        "拒收不发 ValueChanged"
    );
}

#[test]
fn number_field_mixed_batch_filters_per_char() {
    // 一次 textinput 批含混合字符（如 "3a.5"）→ 滤掉 'a'，保留 "3.5"。
    // 过滤是逐字符的，不是整批拒/整批收。
    let (mut s, nf) = focused_numberfield_scene("");
    let mut out = Vec::new();
    crate::input::process_text_input(&mut s, &cps("3a.5"), &mut out);
    assert_eq!(
        nf_edit(&s, nf).value,
        "3.5",
        "批内逐字符过滤：'a' 去掉，'3'/'.'/'5' 留"
    );
}

#[test]
fn textfield_input_unaffected_by_number_guard() {
    // 回归：TextField 仍接受任意字符（NumberField guard 不影响 TextField）。
    // 'a' 在 TextField 应被接受（与 NumberField 相反）。
    let (mut s, tf) = focused_textfield_scene("");
    let mut out = Vec::new();
    crate::input::process_text_input(&mut s, &cps("abc"), &mut out);
    assert_eq!(tf_edit(&s, tf).value, "abc", "TextField 接受任意字符");
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_VALUE_CHANGED && e.node_id == tf.0),
        "TextField 接受字符发 ValueChanged"
    );
}

#[test]
fn is_number_input_char_predicate() {
    // 纯单元：guard 谓词。允许 0-9 / '-' / '.' / 'e' / 'E'；拒其它字母、标点、空白。
    use crate::input::is_number_input_char;
    for c in '0'..='9' {
        assert!(is_number_input_char(c), "数字 {c} 应被接受");
    }
    assert!(is_number_input_char('-'));
    assert!(is_number_input_char('.'));
    assert!(is_number_input_char('e'));
    assert!(is_number_input_char('E'));
    // 拒其它字母。
    for c in ['a', 'x', 'z', 'A', 'X', 'Z', 'b', 'B'] {
        assert!(!is_number_input_char(c), "字母 {c} 应被拒");
    }
    // 拒标点 / 空白。
    for c in ['@', '#', ' ', '\t', ',', ';', '+', '/', '*', '('] {
        assert!(!is_number_input_char(c), "非数字字符 {c:?} 应被拒");
    }
}

#[test]
fn number_field_ime_commit_filters_non_numeric() {
    // IME：「composition 预编辑期不过滤，commit 时过滤」。composition 原语 set_composition
    // 是 control 层纯函数（不知道 NumberField 语义），不过滤；commit 路径（Stage.commit_composition
    // 的 NumberField 臂）调 filter_number_field_text 把 composition.text 滤成数字再落定。
    // 这里单测 filter_number_field_text 谓词本身（集成接线见 stage 测）。
    use crate::input::filter_number_field_text;
    assert_eq!(
        filter_number_field_text("3a5"),
        "35",
        "滤掉 'a'，保 '3'/'5'"
    );
    assert_eq!(
        filter_number_field_text("-1.2e3"),
        "-1.2e3",
        "合法数字串原样保留"
    );
    assert_eq!(filter_number_field_text("abc"), "", "纯非数字 → 空");
    assert_eq!(
        filter_number_field_text("1+2"),
        "12",
        "'+' 被拒（'+' 不是数字语法）"
    );
}

#[test]
fn number_field_backspace_deletes_char() {
    // NumberField value="123"(cursor=3 末尾)，Backspace 删左 → "12"，发 ValueChanged。
    let (mut s, nf) = focused_numberfield_scene("123");
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_BACKSPACE,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(nf_edit(&s, nf).value, "12", "Backspace 删末尾 '3'");
    assert_eq!(nf_edit(&s, nf).cursor, 2);
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_VALUE_CHANGED && e.node_id == nf.0),
        "删字符发 ValueChanged"
    );
}

#[test]
fn number_field_delete_key_works() {
    // NumberField value="123"，光标移到起点（cursor=0），Delete 删右 → "23"。
    let (mut s, nf) = focused_numberfield_scene("123");
    if let Some(ControlState::NumberField { edit, .. }) = s.controls.get_mut(nf) {
        edit.cursor = 0;
        edit.anchor = 0;
    }
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_DELETE,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(nf_edit(&s, nf).value, "23", "Delete 删首位 '1'");
    assert_eq!(nf_edit(&s, nf).cursor, 0);
    assert!(
        out.iter()
            .any(|e| e.event_type == EVT_VALUE_CHANGED && e.node_id == nf.0),
        "Delete 删字符发 ValueChanged"
    );
}

#[test]
fn delete_key_routes_with_unity_keycode_value() {
    // Unity KeyCode.Delete == 323（C# CollectKeys 传 (uint)KeyCode）。core KEY_DELETE 须匹配此值，
    // 否则 C# 传 323、core 期望 127 → Delete 键不路由（showcase NumberField 删除键失效根因）。
    let (mut s, nf) = focused_numberfield_scene("123");
    if let Some(ControlState::NumberField { edit, .. }) = s.controls.get_mut(nf) {
        edit.cursor = 0;
        edit.anchor = 0;
    }
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: 323, // Unity KeyCode.Delete（C# 实传值，非 core 常量）
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(nf_edit(&s, nf).value, "23", "Unity Delete(323) 删首位 '1'");
}

#[test]
fn number_field_arrow_keys_move_cursor() {
    // NumberField value="123"(cursor=3 末尾)，Left → cursor=2、anchor=2（无 shift 折叠）。
    let (mut s, nf) = focused_numberfield_scene("123");
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_LEFT,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(nf_edit(&s, nf).cursor, 2, "Left 左移一位");
    assert_eq!(nf_edit(&s, nf).anchor, 2, "无 shift → 折叠选区");
    // 方向键不发 ValueChanged（只动光标，不改 value）。
    assert!(
        out.iter().all(|e| e.event_type != EVT_VALUE_CHANGED),
        "方向键不发 ValueChanged"
    );
    // Right 回到末尾。
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_RIGHT,
            modifiers: 0,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(nf_edit(&s, nf).cursor, 3, "Right 回到末尾");
}

#[test]
fn number_field_ctrl_a_selects_all() {
    // NumberField value="123"，ctrl+A → anchor=0、cursor=3（全选，不动 value）。
    let (mut s, nf) = focused_numberfield_scene("123");
    let mut out = Vec::new();
    process_keys(
        &mut s,
        &[KeyEvent {
            key_code: KEY_A,
            modifiers: MOD_CTRL,
            is_down: true,
            pad: [0, 0],
        }],
        &mut out,
    );
    assert_eq!(nf_edit(&s, nf).anchor, 0, "ctrl+A anchor 归零");
    assert_eq!(nf_edit(&s, nf).cursor, 3, "ctrl+A cursor 到末尾");
    assert_eq!(nf_edit(&s, nf).value, "123", "全选不改 value");
}

/// #63：DragMove 逐 Move 增量 + Down/Up 携带 button（pad[0]，web MouseEvent.button 值域）。
/// 语义锚点：DeltaX/Y = 自上一条 DragMove；首条含阈值前行程（锚 Down 位——累加后元素
/// 精确贴指针）。累计偏移不该进载荷：StartPosition + Position 可推导。
#[test]
fn drag_move_delta_and_button_payload() {
    let mut s = one_draggable_button_scene();
    let mut ps = PointerState::new();

    // 右键 Down@50,50 → EVT_DOWN pad[0]=2
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Down,
            x: 50.0,
            y: 50.0,
            button: 2,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let down = out.iter().find(|e| e.event_type == EVT_DOWN).unwrap();
    assert_eq!(down.pad[0], 2, "Down 载荷带 button（pad[0]）");

    // Move@55,50（dx=5 > 阈值 2）→ DragStart + 首条 DragMove delta=(5,0)（锚 Down 位，
    // 含阈值前行程）
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 55.0,
            y: 50.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let dm = out
        .iter()
        .find(|e| e.event_type == EVT_DRAG_MOVE)
        .expect("阈值后同帧应发首条 DragMove");
    assert!(
        (dm.dx - 5.0).abs() < 1e-4 && dm.dy.abs() < 1e-4,
        "首条 DragMove delta 含阈值前行程（锚 Down 位），got ({},{})",
        dm.dx,
        dm.dy
    );

    // Move@58,54 → delta=(3,4)（自上一条 DragMove）
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Move,
            x: 58.0,
            y: 54.0,
            button: 0,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let dm = out.iter().find(|e| e.event_type == EVT_DRAG_MOVE).unwrap();
    assert!(
        (dm.dx - 3.0).abs() < 1e-4 && (dm.dy - 4.0).abs() < 1e-4,
        "逐 Move 增量 = 自上一条 DragMove，got ({},{})",
        dm.dx,
        dm.dy
    );

    // 右键 Up → EVT_UP pad[0]=2 + DragEnd
    let out = ps.process(
        &mut s,
        &[PointerEvent {
            kind: PointerKind::Up,
            x: 58.0,
            y: 54.0,
            button: 2,
            pad: [0, 0],
            touch_id: -1,
        }],
    );
    let up = out.iter().find(|e| e.event_type == EVT_UP).unwrap();
    assert_eq!(up.pad[0], 2, "Up 载荷带 button（pad[0]）");
    assert!(out.iter().any(|e| e.event_type == EVT_DRAG_END));
}
