//! 控件视觉子节点注入：instantiate 时给控件节点追加框架内部 `.loom-*` 子节点。
//!
//! 控件即容器模型——ProgressBar 注入 `.loom-fill`，Slider 注入 `track > fill` + thumb
//! （结构：slider → [track, thumb]，track → [fill]），Toggle/RadioButton 注入 `.loom-check`。
//! 这些子节点只携带保留 class（无 id_attr），绝不污染用户 id 命名空间（`Get<T>` 作用域查找
//! 不会误命中框架内部节点）。子节点是普通 Container（div），display 默认按 schema 铺底，
//! 与用户手写 `<div class="loom-fill">` 实例化结果一致。

use crate::input::{EventRecord, EVT_CHANGE_COMMITTED, EVT_CHECKED_CHANGED, EVT_VALUE_CHANGED};
use crate::scene::dynamic::{append_child, create_node, set_inline_override, set_user_transform};
use crate::scene::node::{ControlState, NodeFlags, NodeId, NodeKind, Scene};
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

/// 显示变换：PasswordField 掩码（'•' × 字符数）。其他 kind 原样。
///
/// 掩码保持字符串长度不变，每个 UTF-8 字符映射为一个 '•'（U+2022），
/// 使密码框渲染为等长圆点行（掩码不透露密码长度之外的任何信息）。
/// composition 期间不掩码（Task 13 处理——此阶段 value 不含 composition 字符）。
pub fn transform_display_value(kind: NodeKind, value: &str) -> String {
    match kind {
        NodeKind::PasswordField => value.chars().map(|_| '•').collect(),
        _ => value.to_string(),
    }
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
            // slider → [track, thumb]；track → [fill]。
            // thumb 用 position:absolute 脱离 flex 流（不占 track 的兄弟排列位），锚定 slider
            // （slider 设 position:relative 作 absolute containing block）。thumb 的 left:0/
            // top:0 定位到 slider content 左上（= track 左端，因 track flex-grow 占满 content）；
            // 沿 track 的水平滑动由 sync_control_visuals 走 user_transform（高频、不触发 solve）。
            // 参考 RmlUi WidgetSlider：bar 为 non-DOM 手动 SetOffset，不参与 box 流。
            let _ = set_inline_override(scene, id, "position:relative");
            let track = make_child(scene, TRACK);
            let thumb = make_child(scene, THUMB);
            append_child(scene, id, track).expect("fresh child has no parent");
            append_child(scene, id, thumb).expect("fresh child has no parent");
            let _ = set_inline_override(scene, thumb, "position:absolute;left:0;top:0");
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

/// 在 layout 阶段提前 measure 文本控件的显示文本 TextLayout，写入 `scene.text_layouts`。
///
/// 正常文本节点的 TextLayout 在 render 阶段 lazily 计算（`unwrap_or_else(measure_text)`），
/// 但文本控件需要在 render 前就拿到 TextLayout：光标命中测试（Task 7）需 glyph 位置、
/// 光标几何（Task 12）需行高/基线，都依赖已算好的 TextLayout。
///
/// 在 solve 后调用——此时 `layout_rect.w` 已就位（content width = rect.w - border - padding），
/// `ControlState` 已在之前步骤同步（值/placeholder 在 sync_control_visuals 无关——TextField
/// 视觉同步委托给 TextField beam，此处直接用 `transform_display_value` 取显示文本）。
///
/// 写入的 TextLayout 不含 border/padding 偏移——偏移由 render 阶段统一 `bake_content_offset`，
/// 与正常 TextNode 路径（solve 测原始，render 烤偏移）保持一致。placeholder 场景（value 为空）：
/// 跳过缓存（continue），render 阶段的 lazy fallback 会用 placeholder 重测。
/// 布局阶段 measure 只是预热缓存：光标命中/几何 Task 依赖 TextLayout 在 render 前就位。
pub fn measure_text_controls(scene: &mut Scene, fonts: &crate::text::layout::FontTable) {
    let ids: Vec<NodeId> = scene
        .controls
        .0
        .iter()
        .filter(|(_, s)| matches!(s, ControlState::TextField(_) | ControlState::TextArea(_)))
        .map(|(&id, _)| id)
        .collect();
    for id in ids {
        let Some(n) = scene.get(id) else {
            continue;
        };
        let Some(ControlState::TextField(e) | ControlState::TextArea(e)) = scene.controls.get(id)
        else {
            continue;
        };
        let display = transform_display_value(n.kind, &e.value);
        // value 为空时跳过缓存——render 阶段会用 placeholder 重测（空串 TextLayout 零高，
        // 缓存后 render 的 unwrap_or_else 不触发，placeholder 无法显示）。
        if display.is_empty() {
            continue;
        }
        let s = &n.style;
        let stack = fonts.stack_for(s.font_family.as_deref());
        let off_left = crate::render::resolve_lp(s.taffy_style.border.left)
            + crate::render::resolve_lp(s.taffy_style.padding.left);
        let off_right = crate::render::resolve_lp(s.taffy_style.border.right)
            + crate::render::resolve_lp(s.taffy_style.padding.right);
        let content_w = (n.layout_rect.w - off_left - off_right).max(0.0);
        let layout = crate::text::layout::measure_text(
            &display,
            s.font_size,
            s.line_height,
            s.letter_spacing,
            s.text_align,
            s.white_space_nowrap,
            Some(content_w),
            &stack,
            s.color,
            crate::text::rich::weight_from_font_weight(s.font_weight),
        );
        scene.text_layouts[id.index()] = Some(layout);
    }
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
/// - Slider thumb：`pct` → thumb 的 `user_transform.translate` = `(track_w - thumb_w) × pct`
///   （水平，扣自身宽的可滑动距离）+ `(track_h - thumb_h)/2`（垂直居中）。渲染/命中层位移，
///   不触发 solve；track/thumb 几何取上一帧 solve 的 layout_rect，1 帧滞后同 hit_test 标准）。
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
                // thumb 沿 track 滑动。thumb 已 position:absolute（inject 设），不参与 flex
                // 排列，其 layout_rect 锁在 slider content 左上（= track 左端）。水平位移走
                // user_transform（渲染/命中层，不触发 solve，供高频拖拽每帧写）。公式对齐
                // RmlUi PositionBar：可滑动距离 = track_w - thumb_w（扣自身宽），位置 = 该距离 × pct。
                // 垂直方向把 thumb 居中到 track 中心（thumb 绝对定位后 align-items 不生效）。
                if let Some(thumb) = find_child_by_class(scene, id, THUMB) {
                    let (track_w, track_h) = scene
                        .get(track)
                        .map(|n| (n.layout_rect.w, n.layout_rect.h))
                        .unwrap_or((0.0, 0.0));
                    let (thumb_w, thumb_h) = scene
                        .get(thumb)
                        .map(|n| (n.layout_rect.w, n.layout_rect.h))
                        .unwrap_or((0.0, 0.0));
                    let traversable = (track_w - thumb_w).max(0.0);
                    let center_y = (track_h - thumb_h) / 2.0;
                    let _ = set_user_transform(
                        scene,
                        thumb,
                        NodeTransform {
                            translate: [traversable * pct, center_y],
                            ..Default::default()
                        },
                    );
                }
            }
        }
        // TextField/TextArea: visual sync delegated to TextField beam (cursor blink,
        // selection highlight, composition underline — not yet implemented).
        ControlState::TextField(_) | ControlState::TextArea(_) => {}
    }
}

// ── 控件指针交互 ──────────────────────────────────────────────────
//
// Toggle/Radio 在 pointer-down 翻转/互斥选中；Slider 在 down→move→up 期间拖拽改 value。
// 这些函数是纯逻辑（读 ControlState + track 几何，写 side table），由 PointerState::process
// 在 Down/Move/Up 臂调用（命中控件时）。独立于事件仲裁——只改控件状态，不产事件。

/// 从命中节点向上找最近的控件节点。命中常落在控件的内部视觉子节点（.loom-thumb/.loom-fill）
/// 上，需向上追溯到控件本身（控件是顶层 control 节点，其 loom-* 子节点不是控件）。
/// 无命中 / 链上无控件 → None。
pub fn find_control_at(scene: &Scene, hit: Option<NodeId>) -> Option<NodeId> {
    let mut cur = hit;
    while let Some(id) = cur {
        if scene.controls.get(id).is_some() {
            return Some(id);
        }
        cur = scene.get(id).and_then(|n| n.parent);
    }
    None
}

/// Slider 是否占据指针手势（拖拽期间需抑制祖先 scroll）。仅未禁用的 Slider 为真——
/// Toggle/Radio 点击瞬时完成不占手势；disabled 控件不拦截指针（照 HTML：disabled input
/// 不接受交互），故 disabled Slider 不抑制祖先 scroll（否则按下后 scroll 仲裁被清却无人处理，
/// 用户滚不动）。PointerState 据此决定是否抑制 scroll 候选。
pub fn occupies_gesture(scene: &Scene, id: NodeId) -> bool {
    let is_slider = matches!(scene.controls.get(id), Some(ControlState::Slider { .. }));
    let disabled = scene
        .get(id)
        .is_some_and(|n| n.interaction.flags.contains(NodeFlags::DISABLED));
    is_slider && !disabled
}

/// 指针按下命中控件 → 更新控件状态。返回产生的事件（空 Vec=未命中/未处理）。
///
/// - Toggle：翻转 checked → 产 EVT_CHECKED_CHANGED（pad[0]=新值）。
/// - Radio：同名组互斥——全树找同 name 的其它 radio 置 checked=false，本 radio 置 true
///   → 产 EVT_CHECKED_CHANGED（仅新选中那个，pad[0]=1；照 HTML 只对选中项发 change）。
/// - Slider：置 dragging=true + 按 pos 重算 value（track 几何取上一帧 solve，1 帧滞后，同 hit_test）。
///   value 实际变化时产 EVT_VALUE_CHANGED（x=新值）。
/// - Progress：无交互（空）。
///
/// disabled 控件不响应（照 HTML：disabled input 不接受点击）。pos 仅 Slider 用。
pub fn on_pointer_down(scene: &mut Scene, id: NodeId, pos: [f32; 2]) -> Vec<EventRecord> {
    let mut out = Vec::new();
    // disabled 控件不响应交互。
    if scene
        .get(id)
        .is_some_and(|n| n.interaction.flags.contains(NodeFlags::DISABLED))
    {
        return out;
    }
    let Some(state) = scene.controls.get(id).cloned() else {
        return out;
    };
    match state {
        ControlState::Toggle { checked } => {
            scene
                .controls
                .ensure(id, ControlState::Toggle { checked: !checked });
            out.push(EventRecord {
                node_id: id.0,
                event_type: EVT_CHECKED_CHANGED,
                click_count: 0,
                pad: [checked_to_u8(!checked), 0],
                touch_id: 0,
                x: 0.0,
                y: 0.0,
            });
        }
        ControlState::Radio { name, .. } => {
            select_radio(scene, id, name, &mut out);
        }
        ControlState::Slider { .. } => {
            // 先置 dragging=true，再按 pos 重算 value（track 几何取上一帧 solve）。
            if let Some(ControlState::Slider { dragging, .. }) = scene.controls.get_mut(id) {
                *dragging = true;
            }
            if let Some(v) = slider_pos_to_value(scene, id, pos) {
                set_slider_value(scene, id, v, &mut out);
            }
        }
        // TextField/TextArea: pointer-down focus + caret placement delegated to
        // TextField beam (not yet implemented).
        ControlState::TextField(_) | ControlState::TextArea(_) => {}
        ControlState::Progress { .. } => {}
    }
    out
}

/// 指针移动。仅 Slider 拖拽中（dragging=true）跟随指针重算 value → value 变化时产
/// EVT_VALUE_CHANGED；其它情况返空。PointerState Move 臂在 control_target 存在时调用
/// （函数内部自检 dragging，安全）。
pub fn on_pointer_move(scene: &mut Scene, id: NodeId, pos: [f32; 2]) -> Vec<EventRecord> {
    let mut out = Vec::new();
    let dragging = matches!(
        scene.controls.get(id),
        Some(ControlState::Slider { dragging: true, .. })
    );
    if !dragging {
        return out;
    }
    if let Some(v) = slider_pos_to_value(scene, id, pos) {
        set_slider_value(scene, id, v, &mut out);
    }
    out
}

/// 指针松手。Slider 清 dragging（结束本次拖拽）+ 产 EVT_CHANGE_COMMITTED（x=最终值，
/// 仅当本次确实在拖拽）；其它控件返空。PointerState Up/Canceled 臂调用。
pub fn on_pointer_up(scene: &mut Scene, id: NodeId) -> Vec<EventRecord> {
    let mut out = Vec::new();
    let prev = scene.controls.get(id).cloned();
    if let Some(ControlState::Slider {
        value, dragging, ..
    }) = prev
    {
        if dragging {
            if let Some(ControlState::Slider { dragging: d, .. }) = scene.controls.get_mut(id) {
                *d = false;
            }
            out.push(EventRecord {
                node_id: id.0,
                event_type: EVT_CHANGE_COMMITTED,
                click_count: 0,
                pad: [0, 0],
                touch_id: 0,
                x: value,
                y: 0.0,
            });
        }
    }
    out
}

/// bool → EventRecord.pad[0] 载荷编码（0=false / 1=true）。语义由 EVT_CHECKED_CHANGED 消费方约定。
fn checked_to_u8(b: bool) -> u8 {
    if b {
        1
    } else {
        0
    }
}

/// 选 Radio：同名组互斥。全树找同 name 的其它 radio 置 checked=false，本 radio 置 true。
/// HTML 语义：radio 按 name 分组（跨 DOM 层级，不限兄弟），同组至多一个选中。
/// 事件只对新选中项产 EVT_CHECKED_CHANGED（pad[0]=1），照 HTML 只对 change 的那一项发。
fn select_radio(scene: &mut Scene, id: NodeId, name: String, out: &mut Vec<EventRecord>) {
    // 先收集同组其它 radio 的 NodeId（避免边遍历边改 HashMap）。
    let others: Vec<NodeId> = scene
        .controls
        .iter()
        .filter_map(|(nid, s)| match s {
            ControlState::Radio { name: n, .. } if nid != id && n == &name => Some(nid),
            _ => None,
        })
        .collect();
    for oid in others {
        if let Some(ControlState::Radio { checked, .. }) = scene.controls.get_mut(oid) {
            *checked = false;
        }
    }
    if let Some(ControlState::Radio { checked, .. }) = scene.controls.get_mut(id) {
        *checked = true;
    }
    out.push(EventRecord {
        node_id: id.0,
        event_type: EVT_CHECKED_CHANGED,
        click_count: 0,
        pad: [checked_to_u8(true), 0],
        touch_id: 0,
        x: 0.0,
        y: 0.0,
    });
}

/// Slider pos→value：指针 x 投到 track 的 layout_rect，映射到 [min,max]，step 量化 + clamp。
/// track 几何取 loom-track 子节点的 layout_rect（上一帧 solve 写入，1 帧滞后，同 hit_test 标准）。
/// track 未注入 / 宽度退化（≤0）/ 节点非 Slider / min>max（畸形配置，正常路径 instantiate 已 sanitize）→ None（调用方 no-op）。
fn slider_pos_to_value(scene: &Scene, slider: NodeId, pos: [f32; 2]) -> Option<f32> {
    let (min, max, step) = match scene.controls.get(slider)? {
        ControlState::Slider { min, max, step, .. } => (*min, *max, *step),
        _ => return None,
    };
    // 防御：instantiate 已 sanitize min≤max，但 FFI 或外部注入可能破坏不变量。
    // clamp(min,max) 在 min>max 时 panic（FFI 路径不可 panic），此处守卫。
    if min > max {
        return None;
    }
    let track = find_child_by_class(scene, slider, TRACK)?;
    let lr = scene.get(track)?.layout_rect;
    if lr.w <= 0.0 {
        return None;
    }
    let ratio = ((pos[0] - lr.x) / lr.w).clamp(0.0, 1.0);
    let raw = min + ratio * (max - min);
    let v = if step > 0.0 {
        min + ((raw - min) / step).round() * step
    } else {
        raw
    };
    Some(v.clamp(min, max))
}

/// 写 Slider 的 value（clamp 到 [min,max]，保留 dragging/step）。value 实际变化时产
/// EVT_VALUE_CHANGED（x=新值）—— 用精确 != 防 no-change（同 pos → 同量化值 → 不发误报事件）。
/// 非 Slider / 无槽 → no-op。
fn set_slider_value(scene: &mut Scene, id: NodeId, value: f32, out: &mut Vec<EventRecord>) {
    if let Some(ControlState::Slider {
        value: v, min, max, ..
    }) = scene.controls.get_mut(id)
    {
        // 防御：clamp(min,max) 在 min>max 时 panic。instantiate + FFI setter 已维持
        // min≤max，但此处是 FFI 指针路径下游，纵深守卫保 FFI no-panic 不变量。
        let (lo, hi) = if *min <= *max {
            (*min, *max)
        } else {
            (*max, *min)
        };
        let clamped = value.clamp(lo, hi);
        if *v != clamped {
            *v = clamped;
            out.push(EventRecord {
                node_id: id.0,
                event_type: EVT_VALUE_CHANGED,
                click_count: 0,
                pad: [0, 0],
                touch_id: 0,
                x: clamped,
                y: 0.0,
            });
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

    // ── sync_control_visuals（状态 → 子节点 inline style） ──
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
        // thumb 位置走 transform（set_user_transform），本测只验 fill width。
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

    // ── 控件指针交互（on_pointer_down/move/up） ──
    //
    // 直接调交互函数验逻辑（隔离 PointerState 仲裁）：Toggle 翻转、Radio 同名组互斥、
    // Slider 拖拽改 value + step 量化。track 几何手动设（解耦 solve：测试不把 slider 入 roots，
    // solve 不触达 track，故手动写 layout_rect，同 slider_thumb_positioned_by_transform 模式）。

    use crate::scene::node::Rect;

    /// 建一个带 ControlInit 的 Radio（name 分组）。
    fn make_radio(scene: &mut Scene, name: &str, checked: bool) -> NodeId {
        create_node_from_template(
            scene,
            NodeKind::RadioButton,
            ResolvedStyle::default(),
            Some(ControlInit::Radio {
                checked,
                name: name.into(),
            }),
        )
    }

    /// 手动设 slider 的 loom-track layout_rect（解耦 solve：测试不把 slider 入 roots，solve 不触达）。
    fn set_track_rect(scene: &mut Scene, slider: NodeId, x: f32, y: f32, w: f32, h: f32) {
        let track = find_child_by_class(scene, slider, TRACK).expect("slider has track");
        scene.get_mut(track).unwrap().layout_rect = Rect { x, y, w, h };
    }

    #[test]
    fn toggle_click_flips_checked() {
        let mut scene = Scene::default();
        let id = make_toggle(&mut scene, false);
        let events = on_pointer_down(&mut scene, id, [0.0, 0.0]);
        assert!(!events.is_empty(), "toggle down is handled");
        assert!(matches!(
            scene.controls.get(id),
            Some(ControlState::Toggle { checked: true })
        ));
    }

    #[test]
    fn toggle_click_flips_back_to_unchecked() {
        let mut scene = Scene::default();
        let id = make_toggle(&mut scene, true);
        on_pointer_down(&mut scene, id, [0.0, 0.0]);
        assert!(matches!(
            scene.controls.get(id),
            Some(ControlState::Toggle { checked: false })
        ));
    }

    #[test]
    fn radio_click_mutually_exclusive() {
        let mut scene = Scene::default();
        let a = make_radio(&mut scene, "g", false);
        let b = make_radio(&mut scene, "g", false);
        // 选 a
        on_pointer_down(&mut scene, a, [0.0, 0.0]);
        assert!(matches!(
            scene.controls.get(a),
            Some(ControlState::Radio { checked: true, .. })
        ));
        // 选 b → a 应取消（同 name 互斥）
        on_pointer_down(&mut scene, b, [0.0, 0.0]);
        assert!(matches!(
            scene.controls.get(a),
            Some(ControlState::Radio { checked: false, .. })
        ));
        assert!(matches!(
            scene.controls.get(b),
            Some(ControlState::Radio { checked: true, .. })
        ));
    }

    #[test]
    fn radio_different_names_are_independent() {
        // 不同 name 的 radio 互不影响（HTML：radio 按 name 分组，不按 DOM 层级）。
        let mut scene = Scene::default();
        let a = make_radio(&mut scene, "g1", false);
        let b = make_radio(&mut scene, "g2", false);
        on_pointer_down(&mut scene, a, [0.0, 0.0]);
        on_pointer_down(&mut scene, b, [0.0, 0.0]);
        // 两个都选中（不同组，不互斥）
        assert!(matches!(
            scene.controls.get(a),
            Some(ControlState::Radio { checked: true, .. })
        ));
        assert!(matches!(
            scene.controls.get(b),
            Some(ControlState::Radio { checked: true, .. })
        ));
    }

    #[test]
    fn slider_drag_changes_value() {
        let mut scene = Scene::default();
        let id = make_slider(&mut scene, 50.0, 0.0, 100.0);
        set_track_rect(&mut scene, id, 0.0, 0.0, 200.0, 20.0);
        // 按下在 track 中间（pos.x=100 → value=50），拖到 75%（pos.x=150 → value=75）
        on_pointer_down(&mut scene, id, [100.0, 10.0]);
        on_pointer_move(&mut scene, id, [150.0, 10.0]);
        let v = match scene.controls.get(id) {
            Some(ControlState::Slider { value, .. }) => *value,
            _ => 0.0,
        };
        assert!((v - 75.0).abs() < 1.0, "expected ~75, got {v}");
    }

    #[test]
    fn slider_value_step_quantized() {
        // step=10 → value 落在 10 的倍数。pos.x=73 (track_w=100) → raw=73 → 量化 70。
        let mut scene = Scene::default();
        let id = create_node_from_template(
            &mut scene,
            NodeKind::Slider,
            ResolvedStyle::default(),
            Some(ControlInit::Slider {
                value: 50.0,
                min: 0.0,
                max: 100.0,
                step: 10.0,
            }),
        );
        set_track_rect(&mut scene, id, 0.0, 0.0, 100.0, 20.0);
        on_pointer_down(&mut scene, id, [73.0, 10.0]);
        let v = match scene.controls.get(id) {
            Some(ControlState::Slider { value, .. }) => *value,
            _ => 0.0,
        };
        assert!((v - 70.0).abs() < 0.01, "expected 70 (step=10), got {v}");
    }

    #[test]
    fn slider_down_sets_dragging_up_clears() {
        let mut scene = Scene::default();
        let id = make_slider(&mut scene, 50.0, 0.0, 100.0);
        set_track_rect(&mut scene, id, 0.0, 0.0, 200.0, 20.0);
        assert!(matches!(
            scene.controls.get(id),
            Some(ControlState::Slider {
                dragging: false,
                ..
            })
        ));
        on_pointer_down(&mut scene, id, [100.0, 10.0]);
        assert!(matches!(
            scene.controls.get(id),
            Some(ControlState::Slider { dragging: true, .. })
        ));
        on_pointer_up(&mut scene, id);
        assert!(matches!(
            scene.controls.get(id),
            Some(ControlState::Slider {
                dragging: false,
                ..
            })
        ));
    }

    #[test]
    fn slider_move_ignored_when_not_dragging() {
        // 未先 down（dragging=false）直接 move → value 不变。
        let mut scene = Scene::default();
        let id = make_slider(&mut scene, 50.0, 0.0, 100.0);
        set_track_rect(&mut scene, id, 0.0, 0.0, 200.0, 20.0);
        on_pointer_move(&mut scene, id, [150.0, 10.0]);
        let v = match scene.controls.get(id) {
            Some(ControlState::Slider { value, .. }) => *value,
            _ => 0.0,
        };
        assert!(
            (v - 50.0).abs() < 0.01,
            "value unchanged without down, got {v}"
        );
    }

    #[test]
    fn slider_value_clamped_to_range() {
        // pos 超出 track 左边界 → ratio clamp 0 → value=min=0。
        let mut scene = Scene::default();
        let id = make_slider(&mut scene, 50.0, 0.0, 100.0);
        set_track_rect(&mut scene, id, 0.0, 0.0, 100.0, 20.0);
        on_pointer_down(&mut scene, id, [-50.0, 10.0]);
        let v = match scene.controls.get(id) {
            Some(ControlState::Slider { value, .. }) => *value,
            _ => 0.0,
        };
        assert!((v - 0.0).abs() < 0.01, "clamped to min, got {v}");
    }

    #[test]
    fn on_pointer_down_noop_for_non_control() {
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::Container);
        assert!(
            on_pointer_down(&mut scene, id, [0.0, 0.0]).is_empty(),
            "non-control produces no events"
        );
    }

    // ── 畸形配置 panic 回归（clamp no-panic 不变量）──
    //
    // ControlInit 的 min/max/value 来自 HTML 属性，无 schema 约束。下游 clamp(min,max) 在
    // min>max 时 debug 断言 abort；FFI 路径 panic = 杀宿主进程。instantiate sanitize +
    // 指针路径守卫保证任何畸形配置都不 panic。这些测试锁住该不变量。

    #[test]
    fn malformed_slider_min_gt_max_does_not_panic_on_interaction() {
        // <input type=range min=100 max=0>：instantiate sanitize 成 min=0(取max),max=0，
        // 指针 down/move/up 全程不 panic（slider_pos_to_value 的 min>max 守卫 + set_slider_value
        // 的 (lo,hi) 守卫双保险）。
        let mut scene = Scene::default();
        let id = create_node_from_template(
            &mut scene,
            NodeKind::Slider,
            ResolvedStyle::default(),
            Some(ControlInit::Slider {
                value: 50.0,
                min: 100.0,
                max: 0.0,
                step: 1.0,
            }),
        );
        set_track_rect(&mut scene, id, 0.0, 0.0, 100.0, 20.0);
        // 不 panic 即过；dragging 仍置（交互被处理）。
        let _ = on_pointer_down(&mut scene, id, [50.0, 10.0]);
        let _ = on_pointer_move(&mut scene, id, [80.0, 10.0]);
        let _ = on_pointer_up(&mut scene, id);
        // sanitize 后 min≤max：min 被 clamp 到 max（0≤0）。
        assert!(
            matches!(scene.controls.get(id), Some(ControlState::Slider { min, max, .. }) if min <= max),
            "sanitize 保证 min<=max"
        );
    }

    #[test]
    fn malformed_progress_negative_max_sanitized() {
        // <progress max="-5">：instantiate sanitize max 到 0（≥0），value clamp 进 [0,0]=0。
        // 下游 sync_control_visuals 的 (value/max).clamp 不 panic（max>0 守卫已生效）。
        let mut scene = Scene::default();
        let id = create_node_from_template(
            &mut scene,
            NodeKind::ProgressBar,
            ResolvedStyle::default(),
            Some(ControlInit::Progress {
                value: 30.0,
                max: -5.0,
                indeterminate: false,
            }),
        );
        // sync 不 panic（max=0 时 pct=0.0 走 max>0 else 臂）。
        sync_control_visuals(&mut scene, id);
        match scene.controls.get(id) {
            Some(ControlState::Progress { value, max, .. }) => {
                assert!(*max >= 0.0, "max sanitized to >=0, got {max}");
                assert!((value - 0.0).abs() < 1e-6, "value clamped into [0,0]");
            }
            _ => panic!("progress state exists"),
        }
    }

    #[test]
    fn find_control_at_walks_to_ancestor() {
        // 命中控件的 loom-thumb 子节点 → 向上找到 Slider 控件本身。
        let mut scene = Scene::default();
        let id = make_slider(&mut scene, 50.0, 0.0, 100.0);
        let thumb = find_child_by_class(&scene, id, THUMB).expect("slider has thumb");
        assert_eq!(find_control_at(&scene, Some(thumb)), Some(id));
        assert_eq!(find_control_at(&scene, Some(id)), Some(id));
        assert_eq!(find_control_at(&scene, None), None);
    }

    #[test]
    fn find_control_at_skips_non_control_chain() {
        // 命中非控件叶子 → 链上无控件 → None。
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::Container);
        assert_eq!(find_control_at(&scene, Some(id)), None);
    }

    #[test]
    fn occupies_gesture_only_for_slider() {
        let mut scene = Scene::default();
        let slider = make_slider(&mut scene, 0.0, 0.0, 100.0);
        let toggle = make_toggle(&mut scene, false);
        let radio = make_radio(&mut scene, "g", false);
        let progress = make_progress(&mut scene, 0.0, 100.0);
        assert!(occupies_gesture(&scene, slider));
        assert!(!occupies_gesture(&scene, toggle));
        assert!(!occupies_gesture(&scene, radio));
        assert!(!occupies_gesture(&scene, progress));
    }

    #[test]
    fn occupies_gesture_false_for_disabled_slider() {
        // disabled Slider 不占据手势 → 不抑制祖先 scroll（照 HTML：disabled input 不接受交互）。
        // 坑：旧实现对所有 Slider 返 true，按下后 scroll 仲裁被清却无人处理 → 用户滚不动。
        let mut scene = Scene::default();
        let slider = make_slider(&mut scene, 0.0, 0.0, 100.0);
        assert!(occupies_gesture(&scene, slider), "enabled slider 占据手势");
        scene
            .get_mut(slider)
            .unwrap()
            .interaction
            .flags
            .insert(NodeFlags::DISABLED);
        assert!(
            !occupies_gesture(&scene, slider),
            "disabled slider 不占据手势（不抑制 scroll）"
        );
    }

    // ── 控件事件出口（CheckedChanged / ValueChanged / ChangeCommitted） ──
    //
    // 控件交互产生 EventRecord，随 PointerState::process 的 out 流出。直接调交互函数捕获
    // 返回的 Vec<EventRecord> 验事件载荷（隔离 process 仲裁）。payload 复用 EventRecord
    // 现有字段：Toggle/Radio 的 pad[0]=bool，Slider 的 x=value（ABI 不变）。

    use crate::input::{EVT_CHANGE_COMMITTED, EVT_CHECKED_CHANGED, EVT_VALUE_CHANGED};

    #[test]
    fn toggle_click_emits_checked_changed() {
        // false→true：产一条 EVT_CHECKED_CHANGED，pad[0]=1。
        let mut scene = Scene::default();
        let id = make_toggle(&mut scene, false);
        let events = on_pointer_down(&mut scene, id, [0.0, 0.0]);
        let hits: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == EVT_CHECKED_CHANGED && e.node_id == id.0)
            .collect();
        assert_eq!(hits.len(), 1, "exactly one CheckedChanged for the toggle");
        assert_eq!(hits[0].pad[0], 1, "pad[0]=1 means checked=true");
    }

    #[test]
    fn toggle_uncheck_emits_false_payload() {
        // true→false：pad[0]=0（验双向载荷编码，不只发 true）。
        let mut scene = Scene::default();
        let id = make_toggle(&mut scene, true);
        let events = on_pointer_down(&mut scene, id, [0.0, 0.0]);
        let hit = events
            .iter()
            .find(|e| e.event_type == EVT_CHECKED_CHANGED && e.node_id == id.0)
            .expect("emits CheckedChanged");
        assert_eq!(hit.pad[0], 0, "pad[0]=0 means checked=false");
    }

    #[test]
    fn radio_click_emits_checked_changed() {
        // 选 radio：产一条 EVT_CHECKED_CHANGED，仅对新选中项，pad[0]=1。
        let mut scene = Scene::default();
        let a = make_radio(&mut scene, "g", false);
        let events = on_pointer_down(&mut scene, a, [0.0, 0.0]);
        let hits: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == EVT_CHECKED_CHANGED && e.node_id == a.0)
            .collect();
        assert_eq!(
            hits.len(),
            1,
            "exactly one CheckedChanged for selected radio"
        );
        assert_eq!(hits[0].pad[0], 1);
        // 未选中的同组 radio 不产事件（照 HTML 只对新选中项发 change）。
    }

    #[test]
    fn slider_drag_emits_value_changed() {
        // down→move 改 value：move 产 EVT_VALUE_CHANGED，x=新值。
        let mut scene = Scene::default();
        let id = make_slider(&mut scene, 50.0, 0.0, 100.0);
        set_track_rect(&mut scene, id, 0.0, 0.0, 200.0, 20.0);
        let _ = on_pointer_down(&mut scene, id, [100.0, 10.0]); // value=50，无变化→不发
        let events = on_pointer_move(&mut scene, id, [150.0, 10.0]); // value→75
        let hit = events
            .iter()
            .find(|e| e.event_type == EVT_VALUE_CHANGED && e.node_id == id.0)
            .expect("emits ValueChanged on drag");
        assert!(
            (hit.x - 75.0).abs() < 1.0,
            "x carries new value ~75, got {}",
            hit.x
        );
    }

    #[test]
    fn slider_no_spurious_value_changed_on_no_change() {
        // value 未变（down 命中现值位置）→ 不产 ValueChanged（防误报事件）。
        let mut scene = Scene::default();
        let id = make_slider(&mut scene, 50.0, 0.0, 100.0);
        set_track_rect(&mut scene, id, 0.0, 0.0, 200.0, 20.0);
        let events = on_pointer_down(&mut scene, id, [100.0, 10.0]); // pos→value=50，与现值同
        assert!(
            events.iter().all(|e| e.event_type != EVT_VALUE_CHANGED),
            "no ValueChanged when value unchanged"
        );
    }

    #[test]
    fn slider_up_emits_change_committed() {
        // down→move→up：up 产 EVT_CHANGE_COMMITTED，x=最终值。
        let mut scene = Scene::default();
        let id = make_slider(&mut scene, 50.0, 0.0, 100.0);
        set_track_rect(&mut scene, id, 0.0, 0.0, 200.0, 20.0);
        let _ = on_pointer_down(&mut scene, id, [100.0, 10.0]);
        let _ = on_pointer_move(&mut scene, id, [160.0, 10.0]); // value→80
        let events = on_pointer_up(&mut scene, id);
        let hit = events
            .iter()
            .find(|e| e.event_type == EVT_CHANGE_COMMITTED && e.node_id == id.0)
            .expect("emits ChangeCommitted on up after drag");
        assert!(
            (hit.x - 80.0).abs() < 1.0,
            "x carries final value ~80, got {}",
            hit.x
        );
    }

    #[test]
    fn slider_up_without_drag_emits_nothing() {
        // 未 down（dragging=false）直接 up → 不产 ChangeCommitted（非拖拽不提交）。
        let mut scene = Scene::default();
        let id = make_slider(&mut scene, 50.0, 0.0, 100.0);
        let events = on_pointer_up(&mut scene, id);
        assert!(
            events.iter().all(|e| e.event_type != EVT_CHANGE_COMMITTED),
            "no ChangeCommitted without a drag"
        );
    }
}
