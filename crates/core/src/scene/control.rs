//! 控件视觉子节点注入：instantiate 时给控件节点追加框架内部 `.loom-*` 子节点。
//!
//! 控件即容器模型——ProgressBar 注入 `.loom-fill`，Slider 注入 `track > fill` + thumb
//! （结构：slider → [track, thumb]，track → [fill]），Toggle/RadioButton 注入 `.loom-check`。
//! 这些子节点只携带保留 class（无 id_attr），绝不污染用户 id 命名空间（`Get<T>` 作用域查找
//! 不会误命中框架内部节点）。子节点是普通 Container（div），display 默认按 schema 铺底，
//! 与用户手写 `<div class="loom-fill">` 实例化结果一致。

use crate::scene::dynamic::{append_child, create_node, set_inline_override, set_user_transform};
use crate::scene::node::{ControlState, NodeId, NodeKind, Scene};
use crate::transform::NodeTransform;

const FILL: &str = "loom-fill";
const TRACK: &str = "loom-track";
const THUMB: &str = "loom-thumb";
const CHECK: &str = "loom-check";

/// 建一个携带单个框架保留 class 的 Container 子节点（div）。
///
/// 无 id_attr：框架内部视觉节点绝不能占用用户 id 命名空间（`Get<T>` 按作用域递归查找，
/// 若内部节点带 id 会误命中、与用户同名 id 冲突）。复用 `create_node`（div → Container，
/// 含 display schema 铺底 + slotmap insert + parallel-array resize），保证注入的子节点
/// 与用户手写 `<div class="loom-*">` 的实例化路径完全一致。
fn make_child(scene: &mut Scene, class: &str) -> NodeId {
    let id = create_node(scene, "div", "").expect("\"div\" is in the fence whitelist");
    scene.get_mut(id).unwrap().classes.push(class.to_string());
    id
}

/// 给控件节点注入框架内部视觉子节点。非控件 NodeKind 为 no-op。
///
/// 在 `create_node_from_template` 填完 `ControlTable` side table 后调用——只有
/// `control_init.is_some()` 的控件节点才进此路径，普通容器/叶子节点不受影响。
///
/// Slider 结构是分层的：slider → [track, thumb]（track 与 thumb 平级），track → [fill]。
/// 故先挂 track+thumb 到 slider，再把 fill 挂到 track 内部。其余控件是单层单子。
/// `append_child` 对全新构造的子节点（无 parent）必成功，`.expect` 仅防逻辑漂移。
pub fn inject_control_children(scene: &mut Scene, id: NodeId, kind: NodeKind) {
    match kind {
        NodeKind::ProgressBar => {
            let fill = make_child(scene, FILL);
            append_child(scene, id, fill).expect("fresh child has no parent");
        }
        NodeKind::Slider => {
            // slider → [track, thumb]；track → [fill]
            let track = make_child(scene, TRACK);
            let thumb = make_child(scene, THUMB);
            append_child(scene, id, track).expect("fresh child has no parent");
            append_child(scene, id, thumb).expect("fresh child has no parent");
            let fill = make_child(scene, FILL);
            append_child(scene, track, fill).expect("fresh child has no parent");
        }
        NodeKind::Toggle | NodeKind::RadioButton => {
            let check = make_child(scene, CHECK);
            append_child(scene, id, check).expect("fresh child has no parent");
        }
        _ => {}
    }
}

/// 在 parent 的直接子节点里按 class 找第一个匹配。无匹配 / parent 不 live → None。
///
/// 框架内部视觉节点（.loom-fill / .loom-track / .loom-check ...）按 class 定位，不靠 id
/// （它们不带 id，绝不污染用户命名空间）。控件结构是单层或两层固定深度（ProgressBar 单子、
/// Slider track > fill），故只查直接子节点即可；不递归（防误深入用户内容区）。
pub fn find_child_by_class(scene: &Scene, parent: NodeId, class: &str) -> Option<NodeId> {
    let children = scene.get(parent)?.children.clone();
    children.into_iter().find(|&cid| {
        scene
            .get(cid)
            .is_some_and(|n| n.classes.iter().any(|c| c == class))
    })
}

/// 把控件状态同步到其框架内部视觉子节点的 inline style。
///
/// 这是状态→视觉的单向桥：上层逻辑改 `ControlState`（交互/Tween/C# API），core 据此
/// 写子节点 inline override。inline 是 HTML 语义最高优先级（> 动态规则 > base_style），
/// 与手写 `<div style="width:70%">` 完全等价——故复用 `set_inline_override` 而非另建并行机制。
///
/// 各控件映射：
/// - ProgressBar / Slider：`value / max` → `.loom-fill` 的 `width:%`（Slider 的 fill 在 track 内）。
/// - Toggle / Radio：`checked` → `.loom-check` 的 `display:flex/none`。
/// - Slider thumb：`pct` → thumb 的 `user_transform.translate.x = track_w * pct`（渲染/命中层
///   位移，不触发 solve；track_w 取上一帧 solve 的 layout_rect，1 帧滞后同 hit_test 标准）。
///
/// 无控件状态（非 control 节点）→ no-op。tick 每帧对所有控件节点调一次（控件稀疏，代价可接受）。
/// 对找不到子节点的控件（结构未注入）静默跳过——防御性，instantiate 保证子节点就位。
pub fn sync_control_visuals(scene: &mut Scene, id: NodeId) {
    let Some(state) = scene.controls.get(id).cloned() else {
        return;
    };
    match state {
        ControlState::Progress { value, max, .. } => {
            let pct = if max > 0.0 {
                (value / max).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if let Some(fill) = find_child_by_class(scene, id, FILL) {
                // width:N% — 用百分比，随 track 宽度自适应（track 尺寸由布局决定）。
                let _ = set_inline_override(scene, fill, &format!("width:{}%", pct * 100.0));
            }
        }
        ControlState::Toggle { checked } | ControlState::Radio { checked, .. } => {
            if let Some(check) = find_child_by_class(scene, id, CHECK) {
                let display = if checked {
                    "display:flex"
                } else {
                    "display:none"
                };
                let _ = set_inline_override(scene, check, display);
            }
        }
        ControlState::Slider {
            value, min, max, ..
        } => {
            let pct = if max > min {
                ((value - min) / (max - min)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            // Slider 结构：slider → [track, thumb]，track → [fill]。
            if let Some(track) = find_child_by_class(scene, id, TRACK) {
                if let Some(fill) = find_child_by_class(scene, track, FILL) {
                    let _ = set_inline_override(scene, fill, &format!("width:{}%", pct * 100.0));
                }
                // thumb 沿 track 滑动：translate.x = track_w * pct。track_w 取 track 的
                // layout_rect.w（上一帧 solve 写入，1 帧滞后——同 hit_test 用上帧 world 的标准模式）。
                // 走 user_transform 而非 inline：这是渲染/命中层位移，不进布局、不触发 solve，
                // 供高频拖拽每帧写一次（下帧 compute_world_transforms 读取）。
                if let Some(thumb) = find_child_by_class(scene, id, THUMB) {
                    let track_w = scene.get(track).map(|n| n.layout_rect.w).unwrap_or(0.0);
                    let _ = set_user_transform(
                        scene,
                        thumb,
                        NodeTransform {
                            translate: [track_w * pct, 0.0],
                            ..Default::default()
                        },
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::dynamic::create_node_from_template;
    use crate::scene::node::{NodeKind, Scene};
    use crate::style::resolved::ResolvedStyle;

    /// 建一个指定 kind 的控件节点（无 control_init，仅验注入结构）。
    fn make_control(scene: &mut Scene, kind: NodeKind) -> NodeId {
        create_node_from_template(scene, kind, ResolvedStyle::default(), None)
    }

    #[test]
    fn progress_injects_fill_child() {
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::ProgressBar);
        inject_control_children(&mut scene, id, NodeKind::ProgressBar);
        let children = scene.get(id).unwrap().children.clone();
        assert_eq!(children.len(), 1, "ProgressBar gets exactly one fill child");
        let fill = scene.get(children[0]).unwrap();
        assert!(fill.classes.iter().any(|c| c == FILL));
        assert_eq!(fill.kind, NodeKind::Container);
    }

    #[test]
    fn slider_injects_track_fill_thumb() {
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::Slider);
        inject_control_children(&mut scene, id, NodeKind::Slider);
        let children = scene.get(id).unwrap().children.clone();
        assert_eq!(children.len(), 2, "Slider gets track + thumb as siblings");
        // children[0] = track
        let track = scene.get(children[0]).unwrap();
        assert!(track.classes.iter().any(|c| c == TRACK));
        assert_eq!(track.kind, NodeKind::Container);
        // children[1] = thumb
        let thumb = scene.get(children[1]).unwrap();
        assert!(thumb.classes.iter().any(|c| c == THUMB));
        assert_eq!(thumb.kind, NodeKind::Container);
        // track → [fill]
        let track_children = track.children.clone();
        assert_eq!(track_children.len(), 1, "track contains the fill");
        let fill = scene.get(track_children[0]).unwrap();
        assert!(fill.classes.iter().any(|c| c == FILL));
        assert_eq!(fill.kind, NodeKind::Container);
        // fill 的 parent 是 track，不是 slider
        assert_eq!(fill.parent, Some(children[0]));
    }

    #[test]
    fn toggle_injects_check() {
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::Toggle);
        inject_control_children(&mut scene, id, NodeKind::Toggle);
        let children = scene.get(id).unwrap().children.clone();
        assert_eq!(children.len(), 1);
        let check = scene.get(children[0]).unwrap();
        assert!(check.classes.iter().any(|c| c == CHECK));
        assert_eq!(check.kind, NodeKind::Container);
    }

    #[test]
    fn radio_injects_check() {
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::RadioButton);
        inject_control_children(&mut scene, id, NodeKind::RadioButton);
        let children = scene.get(id).unwrap().children.clone();
        assert_eq!(children.len(), 1);
        let check = scene.get(children[0]).unwrap();
        assert!(check.classes.iter().any(|c| c == CHECK));
        assert_eq!(check.kind, NodeKind::Container);
    }

    #[test]
    fn non_control_kinds_get_no_children() {
        // Container / Button / Image 不是控件 —— 注入是 no-op。
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::Container);
        inject_control_children(&mut scene, id, NodeKind::Container);
        assert!(scene.get(id).unwrap().children.is_empty());
    }

    #[test]
    fn injected_children_carry_no_id_attr() {
        // 框架内部子节点绝不能带 id（不污染用户 id 命名空间，防 Get<T> 误命中）。
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::ProgressBar);
        inject_control_children(&mut scene, id, NodeKind::ProgressBar);
        for &child in &scene.get(id).unwrap().children {
            assert!(
                scene.get(child).unwrap().id_attr.is_none(),
                "injected child must not carry an id"
            );
        }
    }

    // ── Task 5: sync_control_visuals（状态 → 子节点 inline style） ──
    //
    // 控件状态变后由 core 写子节点 inline style（语义优先级 = HTML inline，最高）。
    // ProgressBar/Slider 写 .loom-fill 的 width:%，Toggle/Radio 切 .loom-check 的 display。
    // 用真实 ControlInit 建 + ControlState 侧表，再调 sync_control_visuals 验子节点 inline_override。

    use crate::asset::ControlInit;
    use crate::style::resolved::DisplayMode;
    use taffy::prelude::Dimension;

    /// 建一个带 ControlInit 的 ProgressBar（state + 注入子节点都就位）。
    fn make_progress(scene: &mut Scene, value: f32, max: f32) -> NodeId {
        create_node_from_template(
            scene,
            NodeKind::ProgressBar,
            ResolvedStyle::default(),
            Some(ControlInit::Progress {
                value,
                max,
                indeterminate: false,
            }),
        )
    }

    /// 建一个带 ControlInit 的 Toggle（checked 决定 check 是否显示）。
    fn make_toggle(scene: &mut Scene, checked: bool) -> NodeId {
        create_node_from_template(
            scene,
            NodeKind::Toggle,
            ResolvedStyle::default(),
            Some(ControlInit::Toggle { checked }),
        )
    }

    /// 建一个带 ControlInit 的 Slider（track > fill + thumb 都注入）。
    fn make_slider(scene: &mut Scene, value: f32, min: f32, max: f32) -> NodeId {
        create_node_from_template(
            scene,
            NodeKind::Slider,
            ResolvedStyle::default(),
            Some(ControlInit::Slider {
                value,
                min,
                max,
                step: 0.0,
            }),
        )
    }

    #[test]
    fn progress_fill_width_reflects_value() {
        // value=70/max=100 → fill inline width = 70%（Dimension::Percent(0.7)）。
        let mut scene = Scene::default();
        let id = make_progress(&mut scene, 70.0, 100.0);
        sync_control_visuals(&mut scene, id);
        let fill = find_child_by_class(&scene, id, FILL).expect("progress has fill child");
        let w = scene
            .get(fill)
            .unwrap()
            .inline_override
            .taffy_style
            .size
            .width;
        assert_eq!(w, Dimension::percent(0.7), "70/100 → width:70%");
        // inline_set 的 width bit 也应被置（set_inline_override OR 进）。
        use crate::style::dynamic::INLINE_WIDTH;
        assert_ne!(
            scene.get(fill).unwrap().inline_set.0 & INLINE_WIDTH,
            0,
            "width bit set in inline_set"
        );
    }

    #[test]
    fn progress_fill_clamped_to_range() {
        // value 超 max → clamp 到 100%；负值 → 0%。防 layout 出现 110% 溢出。
        let mut scene = Scene::default();
        let id = make_progress(&mut scene, 120.0, 100.0);
        sync_control_visuals(&mut scene, id);
        let fill = find_child_by_class(&scene, id, FILL).unwrap();
        assert_eq!(
            scene
                .get(fill)
                .unwrap()
                .inline_override
                .taffy_style
                .size
                .width,
            Dimension::percent(1.0),
            "clamp to 100%"
        );
    }

    #[test]
    fn toggle_check_hidden_when_unchecked() {
        // unchecked → check inline display:none（taffy Display::None + display_mode None）。
        let mut scene = Scene::default();
        let id = make_toggle(&mut scene, false);
        sync_control_visuals(&mut scene, id);
        let check = find_child_by_class(&scene, id, CHECK).expect("toggle has check child");
        let n = scene.get(check).unwrap();
        assert_eq!(
            n.inline_override.taffy_style.display,
            taffy::Display::None,
            "unchecked → display:none"
        );
        assert_eq!(
            n.inline_override.display_mode,
            DisplayMode::None,
            "display_mode also None"
        );
    }

    #[test]
    fn toggle_check_shown_when_checked() {
        // checked → check inline display:flex（可见）。
        let mut scene = Scene::default();
        let id = make_toggle(&mut scene, true);
        sync_control_visuals(&mut scene, id);
        let check = find_child_by_class(&scene, id, CHECK).expect("toggle has check child");
        let n = scene.get(check).unwrap();
        assert_eq!(
            n.inline_override.taffy_style.display,
            taffy::Display::Flex,
            "checked → display:flex"
        );
        assert_eq!(
            n.inline_override.display_mode,
            DisplayMode::Flex,
            "display_mode also Flex"
        );
    }

    #[test]
    fn radio_check_hidden_when_unchecked() {
        // Radio 与 Toggle 共用 check 显示逻辑。
        let mut scene = Scene::default();
        let id = create_node_from_template(
            &mut scene,
            NodeKind::RadioButton,
            ResolvedStyle::default(),
            Some(ControlInit::Radio {
                checked: false,
                name: "g".into(),
            }),
        );
        sync_control_visuals(&mut scene, id);
        let check = find_child_by_class(&scene, id, CHECK).expect("radio has check child");
        assert_eq!(
            scene
                .get(check)
                .unwrap()
                .inline_override
                .taffy_style
                .display,
            taffy::Display::None,
            "unchecked radio → display:none"
        );
    }

    #[test]
    fn slider_fill_width_reflects_value() {
        // Slider: value=25/min=0/max=100 → track 内 fill width = 25%。
        // thumb 位置走 transform（Task 6），本测只验 fill width。
        let mut scene = Scene::default();
        let id = make_slider(&mut scene, 25.0, 0.0, 100.0);
        sync_control_visuals(&mut scene, id);
        let track = find_child_by_class(&scene, id, TRACK).expect("slider has track child");
        let fill = find_child_by_class(&scene, track, FILL).expect("track has fill child");
        assert_eq!(
            scene
                .get(fill)
                .unwrap()
                .inline_override
                .taffy_style
                .size
                .width,
            Dimension::percent(0.25),
            "25/100 → width:25%"
        );
    }

    #[test]
    fn slider_thumb_positioned_by_transform() {
        // value=50/min=0/max=100 → pct=0.5。thumb translate.x = track_w * pct。
        // track_w 取自 track 的 layout_rect.w——运行时由上一帧 solve 写入（1 帧滞后，同
        // hit_test 用上帧 world 的标准模式）。此处手动设，以解耦 layout wiring（make_slider
        // 不入 roots，solve 不会触达），聚焦验 pct→translate 的映射本身。
        let mut scene = Scene::default();
        let id = make_slider(&mut scene, 50.0, 0.0, 100.0);
        let track = find_child_by_class(&scene, id, TRACK).expect("slider has track child");
        scene.get_mut(track).unwrap().layout_rect.w = 200.0;
        sync_control_visuals(&mut scene, id);
        let thumb = find_child_by_class(&scene, id, THUMB).expect("slider has thumb child");
        let tr = scene.get(thumb).unwrap().user_transform;
        let track_w = scene.get(track).unwrap().layout_rect.w;
        let expected = track_w * 0.5;
        assert!(
            (tr.translate[0] - expected).abs() < 1e-4,
            "thumb x = track_w({track_w}) * pct(0.5) = {expected}, got {}",
            tr.translate[0]
        );
        assert!(tr.translate[1].abs() < 1e-4, "thumb y 保持 0");
    }

    #[test]
    fn sync_control_visuals_noop_for_non_control_node() {
        // 非 control 节点（无 ControlState 槽）：sync 是 no-op，不 panic。
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::Container);
        sync_control_visuals(&mut scene, id);
        assert!(scene.get(id).unwrap().children.is_empty());
    }
}
