use super::*;
use crate::input::EVT_SELECTION_CHANGED;
use crate::scene::dynamic::{append_child, create_node_from_template};
use crate::scene::node::{ControlState, EditState, NodeFlags, NodeId, NodeKind, RoleInfo, Scene};
use crate::scene::text_cursor::{cursor_pixel_x, hit_byte_offset, line_byte_ranges};
use crate::style::resolved::ResolvedStyle;

/// 建一个指定 kind 的控件节点（无 control_init，无子节点——控件结构由作者自写）。
fn make_control(scene: &mut Scene, kind: NodeKind) -> NodeId {
    create_node_from_template(scene, kind, ResolvedStyle::default(), None)
}

/// 建一个 Container 子节点、登记 data-slot 进 RoleTable、挂到 parent。
/// 复刻 instantiate 从模板填 RoleTable 的路径（作者写 `<div data-slot="fill">`）。
fn make_slot_child(scene: &mut Scene, parent: NodeId, slot: &str) -> NodeId {
    let id = create_node_from_template(scene, NodeKind::Container, ResolvedStyle::default(), None);
    append_child(scene, parent, id).expect("fresh child has no parent");
    scene.roles.insert(
        id,
        RoleInfo {
            role: None,
            slots: [(slot.to_string(), String::new())].into_iter().collect(),
            attrs: vec![],
        },
    );
    id
}

/// 建一个 Container 子节点、登记 role 进 RoleTable、挂到 parent。
/// 复刻 instantiate 从模板填 RoleTable 的路径（作者写 `<div role="listbox">`）。
fn make_role_child(scene: &mut Scene, parent: NodeId, role: &str) -> NodeId {
    let id = create_node_from_template(scene, NodeKind::Container, ResolvedStyle::default(), None);
    append_child(scene, parent, id).expect("fresh child has no parent");
    scene.roles.insert(
        id,
        RoleInfo {
            role: Some(role.to_string()),
            slots: Default::default(),
            attrs: vec![],
        },
    );
    id
}

#[test]
fn find_child_by_role_matches_direct_child() {
    // combobox 直接子节点里 role=listbox 命中；未登记的 role → None。只查直接子，不递归。
    let mut scene = Scene::default();
    let root = make_control(&mut scene, NodeKind::Container);
    let listbox = make_role_child(&mut scene, root, ROLE_LISTBOX);
    assert_eq!(
        find_child_by_role(&scene, root, ROLE_LISTBOX),
        Some(listbox)
    );
    assert_eq!(find_child_by_role(&scene, root, "combobox"), None);
}

#[test]
fn find_child_by_slot_matches_direct_child() {
    // slider 直接子节点里 data-slot=thumb / data-slot=fill 各自命中（key 存在即命中）。
    let mut scene = Scene::default();
    let root = make_control(&mut scene, NodeKind::Container);
    let fill = make_slot_child(&mut scene, root, SLOT_FILL);
    let thumb = make_slot_child(&mut scene, root, SLOT_THUMB);
    assert_eq!(find_child_by_slot(&scene, root, SLOT_FILL), Some(fill));
    assert_eq!(find_child_by_slot(&scene, root, SLOT_THUMB), Some(thumb));
    assert_eq!(find_child_by_slot(&scene, root, SLOT_VALUE), None);
}

#[test]
fn find_child_by_role_recursive_descends_subtree() {
    // listbox 不是直接子（裹在 wrapper 里）→ 直接查 None，递归查命中。
    let mut scene = Scene::default();
    let combobox = make_control(&mut scene, NodeKind::Container);
    let wrapper = make_control(&mut scene, NodeKind::Container); // 普通 wrapper（无 role/slot）
    append_child(&mut scene, combobox, wrapper).expect("wrapper attach");
    let listbox = make_role_child(&mut scene, wrapper, ROLE_LISTBOX);
    assert_eq!(
        find_child_by_role(&scene, combobox, ROLE_LISTBOX),
        None,
        "直接子查不递归 → wrapper 挡住 listbox"
    );
    assert_eq!(
        find_child_by_role_recursive(&scene, combobox, ROLE_LISTBOX),
        Some(listbox),
        "递归查穿透 wrapper 命中 listbox"
    );
}

#[test]
fn find_child_returns_none_for_dead_parent() {
    // parent 不 live → None（不 panic，`scene.get(parent)?` 早返）。
    let scene = Scene::default();
    assert_eq!(find_child_by_role(&scene, NodeId::INVALID, "x"), None);
    assert_eq!(find_child_by_slot(&scene, NodeId::INVALID, "x"), None);
    assert_eq!(
        find_child_by_role_recursive(&scene, NodeId::INVALID, "x"),
        None
    );
}

// 控件状态变后由 core 按 role/data-slot 定位作者子节点写 inline style（语义优先级 = HTML
// inline，最高）。ProgressBar/Slider 写 fill slot 的 width:%、Slider 写 thumb slot 的
// transform；Dropdown 写 listbox role 的 display + value slot 的文本。Toggle/Radio 不 sync
// （作者用 [aria-checked] CSS）。用真实 ControlInit 建 + ControlState 侧表 + 作者写的
// role/slot 子树（make_slot_child/make_role_child），再调 sync_control_visuals 验子节点 inline_override。

use crate::asset::ControlInit;
use taffy::prelude::Dimension;

/// 建一个带 ControlInit 的 ProgressBar，并附作者写的 `data-slot="fill"` 子节点。
fn make_progress(scene: &mut Scene, value: f32, max: f32) -> NodeId {
    let id = create_node_from_template(
        scene,
        NodeKind::ProgressBar,
        ResolvedStyle::default(),
        Some(ControlInit::Progress {
            value,
            min: 0.0,
            max,
            indeterminate: false,
        }),
    );
    make_slot_child(scene, id, SLOT_FILL);
    id
}

/// 建一个带 ControlInit 的 Toggle（无必需子节点——作者用 [aria-checked] CSS）。
fn make_toggle(scene: &mut Scene, checked: bool) -> NodeId {
    create_node_from_template(
        scene,
        NodeKind::Toggle,
        ResolvedStyle::default(),
        Some(ControlInit::Toggle { checked }),
    )
}

/// 建一个带 ControlInit 的 Slider，并附作者写的 `data-slot="fill"` + `data-slot="thumb"`
/// 兄弟子节点（新结构无 track 中间层）。
fn make_slider(scene: &mut Scene, value: f32, min: f32, max: f32) -> NodeId {
    let id = create_node_from_template(
        scene,
        NodeKind::Slider,
        ResolvedStyle::default(),
        Some(ControlInit::Slider {
            value,
            min,
            max,
            step: 0.0,
        }),
    );
    make_slot_child(scene, id, SLOT_FILL);
    make_slot_child(scene, id, SLOT_THUMB);
    id
}

#[test]
fn progress_fill_width_reflects_value() {
    // value=70/max=100 → fill inline width = 70%（Dimension::Percent(0.7)）。
    let mut scene = Scene::default();
    let id = make_progress(&mut scene, 70.0, 100.0);
    sync_control_visuals(&mut scene, id, 0.0);
    let fill = find_child_by_slot(&scene, id, SLOT_FILL).expect("progress has fill child");
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
fn progress_fill_width_uses_aria_min_domain() {
    // min≠0 → ARIA 填充比例 (value-min)/(max-min)：min=50/max=100/value=75 → 50%，
    // 不是 value/max 的 75%（#97 语义裁决：core 对齐 ARIA 标准）。
    let mut scene = Scene::default();
    let id = create_node_from_template(
        &mut scene,
        NodeKind::ProgressBar,
        ResolvedStyle::default(),
        Some(ControlInit::Progress {
            value: 75.0,
            min: 50.0,
            max: 100.0,
            indeterminate: false,
        }),
    );
    make_slot_child(&mut scene, id, SLOT_FILL);
    sync_control_visuals(&mut scene, id, 0.0);
    let fill = find_child_by_slot(&scene, id, SLOT_FILL).expect("progress has fill child");
    assert_eq!(
        scene
            .get(fill)
            .unwrap()
            .inline_override
            .taffy_style
            .size
            .width,
        Dimension::percent(0.5),
        "(75-50)/(100-50) → width:50%"
    );
}

#[test]
fn progress_fill_clamped_to_range() {
    // value 超 max → clamp 到 100%；负值 → 0%。防 layout 出现 110% 溢出。
    let mut scene = Scene::default();
    let id = make_progress(&mut scene, 120.0, 100.0);
    sync_control_visuals(&mut scene, id, 0.0);
    let fill = find_child_by_slot(&scene, id, SLOT_FILL).unwrap();
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
fn progress_indeterminate_yields_fill_width_to_author_css() {
    // indeterminate=true → 不写 width，且清掉 value 时代写入的 inline width（残留会以
    // inline 优先级压死作者 [aria-indeterminate] 规则——跳过不写不够，必须清 bit）。
    let mut scene = Scene::default();
    let id = make_progress(&mut scene, 70.0, 100.0);
    sync_control_visuals(&mut scene, id, 0.0);
    let fill = find_child_by_slot(&scene, id, SLOT_FILL).unwrap();
    use crate::style::dynamic::INLINE_WIDTH;
    assert_ne!(
        scene.get(fill).unwrap().inline_set.0 & INLINE_WIDTH,
        0,
        "value 时代先写入 width（前置条件）"
    );

    if let Some(ControlState::Progress { indeterminate, .. }) = scene.controls.get_mut(id) {
        *indeterminate = true;
    }
    sync_control_visuals(&mut scene, id, 0.0);
    assert_eq!(
        scene.get(fill).unwrap().inline_set.0 & INLINE_WIDTH,
        0,
        "indeterminate 清 width bit，几何权归作者 CSS"
    );

    // 回到 determinate：恢复每帧写 width。
    if let Some(ControlState::Progress { indeterminate, .. }) = scene.controls.get_mut(id) {
        *indeterminate = false;
    }
    sync_control_visuals(&mut scene, id, 0.0);
    assert_eq!(
        scene
            .get(fill)
            .unwrap()
            .inline_override
            .taffy_style
            .size
            .width,
        Dimension::percent(0.7),
        "退出 indeterminate 恢复 width:70%"
    );
}

#[test]
fn sync_toggle_is_noop_for_children() {
    // Toggle 无必需子节点：作者用 [aria-checked] CSS 表达选中态，core 不再 sync check 子节点。
    // 验 sync 不 panic 且不读写任何子节点 inline（无副作用）。
    let mut scene = Scene::default();
    let id = make_toggle(&mut scene, false);
    // 手动附一个普通子节点（作者可能写图标容器），sync 不应动它。
    let kid = make_control(&mut scene, NodeKind::Container);
    append_child(&mut scene, id, kid).expect("kid attach");
    sync_control_visuals(&mut scene, id, 0.0);
    let n = scene.get(kid).unwrap();
    assert_eq!(
        n.inline_override.taffy_style.display,
        taffy::Display::Flex,
        "toggle sync 不改子节点 display（默认 Flex）"
    );
}

#[test]
fn sync_radio_is_noop_for_children() {
    // Radio 同 Toggle：无 check 子节点 sync。
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
    sync_control_visuals(&mut scene, id, 0.0);
    // 无 panic、无子节点改动即过（radio 无子节点）。
    assert!(scene.get(id).unwrap().children.is_empty());
}

#[test]
fn slider_fill_width_reflects_value() {
    // Slider: value=25/min=0/max=100 → fill slot 的 width = 25%（新结构 fill 是 slider 直接子）。
    // thumb 位置走 transform（set_user_transform），本测只验 fill width。
    let mut scene = Scene::default();
    let id = make_slider(&mut scene, 25.0, 0.0, 100.0);
    sync_control_visuals(&mut scene, id, 0.0);
    let fill = find_child_by_slot(&scene, id, SLOT_FILL).expect("slider has fill child");
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
    // value=50/min=0/max=100 → pct=0.5。thumb translate.x = slider_w * pct（新结构无 track
    // 中间层，几何取 slider 自身 layout_rect）。运行时由上一帧 solve 写入（1 帧滞后，同
    // hit_test 用上帧 world 的标准模式）。此处手动设 slider 的 layout_rect，以解耦 layout
    // wiring（make_slider 不入 roots，solve 不会触达），聚焦验 pct→translate 的映射本身。
    let mut scene = Scene::default();
    let id = make_slider(&mut scene, 50.0, 0.0, 100.0);
    scene.get_mut(id).unwrap().layout_rect.w = 200.0;
    scene.get_mut(id).unwrap().layout_rect.h = 20.0;
    sync_control_visuals(&mut scene, id, 0.0);
    let thumb = find_child_by_slot(&scene, id, SLOT_THUMB).expect("slider has thumb child");
    let tr = scene.get(thumb).unwrap().user_transform;
    let slider_w = scene.get(id).unwrap().layout_rect.w;
    let expected = slider_w * 0.5;
    assert!(
        (tr.translate[0] - expected).abs() < 1e-4,
        "thumb x = slider_w({slider_w}) * pct(0.5) = {expected}, got {}",
        tr.translate[0]
    );
    // thumb 自身宽 0（未设 layout_rect）→ center_y = (20-0)/2 = 10。
    assert!(
        (tr.translate[1] - 10.0).abs() < 1e-4,
        "thumb y 居中到 slider"
    );
}

#[test]
fn slider_thumb_author_positioning_zeroed() {
    // 作者给 thumb 写定位（浏览器直觉：负 top 居中 + margin 微调）与控件位移叠加会
    // 双偏移——sync 必须逐帧归零 inset/margin（class 规则每帧经 rematch 重放）。
    // 尺寸声明不受影响。
    let mut scene = Scene::default();
    let id = make_slider(&mut scene, 50.0, 0.0, 100.0);
    scene.get_mut(id).unwrap().layout_rect.w = 200.0;
    scene.get_mut(id).unwrap().layout_rect.h = 6.0;
    let thumb = find_child_by_slot(&scene, id, SLOT_THUMB).expect("slider has thumb child");
    {
        let tn = scene.get_mut(thumb).unwrap();
        let mut s = ResolvedStyle::default();
        crate::style::mapping::apply_decl(&mut s, "top", "-9px");
        crate::style::mapping::apply_decl(&mut s, "left", "62%");
        crate::style::mapping::apply_decl(&mut s, "margin-top", "-12px");
        crate::style::mapping::apply_decl(&mut s, "width", "24px");
        tn.style = s;
    }
    sync_control_visuals(&mut scene, id, 0.0);
    let tn = scene.get(thumb).unwrap();
    use taffy::style::LengthPercentageAuto;
    assert_eq!(
        tn.style.taffy_style.inset.top,
        LengthPercentageAuto::length(0.0)
    );
    assert_eq!(
        tn.style.taffy_style.inset.left,
        LengthPercentageAuto::length(0.0)
    );
    assert_eq!(
        tn.style.taffy_style.margin.top,
        LengthPercentageAuto::length(0.0)
    );
    // 尺寸/外观保留。
    assert_eq!(
        tn.style.taffy_style.size.width,
        taffy::style::Dimension::length(24.0)
    );
    // 垂直居中 transform 不变（thumb_h=0 未设 → (6-0)/2 = 3）。
    assert!((tn.user_transform.translate[1] - 3.0).abs() < 1e-4);
}

#[test]
fn sync_control_visuals_noop_for_non_control_node() {
    // 非 control 节点（无 ControlState 槽）：sync 是 no-op，不 panic。
    let mut scene = Scene::default();
    let id = make_control(&mut scene, NodeKind::Container);
    sync_control_visuals(&mut scene, id, 0.0);
    assert!(scene.get(id).unwrap().children.is_empty());
}

// combobox 的 selected_index → value slot 显示对应 option 文本；open → listbox role 的
// display:block/none 切换。option 文本取自 option 子树（自身 text_contents 或后代 TextNode）。
// value slot 是 Container，文本落在其内嵌 TextNode（作者写 `<div data-slot=value><span/></div>`）。

/// 建一个带 ControlInit 的 combobox（Dropdown），按 role/data-slot 结构自写：
/// `combobox > [data-slot=value > TextNode, role=listbox > [option...]]`。
/// 模拟作者写 `<div role=combobox><div data-slot=value><span/></div><div role=listbox>
/// <div role=option>A</div>...</div></div>`。reparent 调用复刻生产 Stage::instantiate
/// （option 已在 listbox 内时为 no-op，顺带验证幂等）。
fn make_dropdown_with_options(scene: &mut Scene, option_texts: &[&str], selected: u32) -> NodeId {
    let id = create_node_from_template(
        scene,
        NodeKind::Dropdown,
        ResolvedStyle::default(),
        Some(ControlInit::Dropdown {
            selected_index: selected,
            option_values: Vec::new(),
        }),
    );
    // value slot（含 TextNode 承载选中项文本）。
    let value = make_slot_child(scene, id, SLOT_VALUE);
    let value_text_node =
        create_node_from_template(scene, NodeKind::TextNode, ResolvedStyle::default(), None);
    append_child(scene, value, value_text_node).expect("value text append");
    // listbox role（option 列表容器）。
    let listbox = make_role_child(scene, id, ROLE_LISTBOX);
    for &t in option_texts {
        let opt =
            create_node_from_template(scene, NodeKind::OptionItem, ResolvedStyle::default(), None);
        scene.text_contents.insert(opt, t.to_string());
        append_child(scene, listbox, opt).expect("option append");
    }
    // 与 Stage::instantiate 一致：建完后调 reparent（option 已在 listbox 内 → no-op）。
    reparent_options_into_popup(scene, id);
    id
}

/// 取 value slot 内 TextNode 子节点的文本内容。
fn value_text(scene: &Scene, select: NodeId) -> String {
    let value = find_child_by_slot(scene, select, SLOT_VALUE).expect("value slot present");
    let text_node = scene
        .get(value)
        .unwrap()
        .children
        .iter()
        .find(|&&c| scene.get(c).is_some_and(|n| n.kind == NodeKind::TextNode))
        .copied()
        .expect("value slot has a TextNode child");
    scene
        .text_contents
        .get(&text_node)
        .cloned()
        .unwrap_or_default()
}

#[test]
fn sync_dropdown_shows_selected_option_text_in_value() {
    // selected_index=1 → value slot 文本应是第 2 个 option 的文本（"B"）。
    let mut scene = Scene::default();
    let sel = make_dropdown_with_options(&mut scene, &["A", "B", "C"], 1);
    sync_control_visuals(&mut scene, sel, 0.0);
    assert_eq!(
        value_text(&scene, sel),
        "B",
        "value shows selected option text"
    );
}

#[test]
fn sync_dropdown_value_text_tracks_selected_index_change() {
    // 改 selected_index 后再 sync，value slot 文本随之更新。
    let mut scene = Scene::default();
    let sel = make_dropdown_with_options(&mut scene, &["A", "B", "C"], 0);
    sync_control_visuals(&mut scene, sel, 0.0);
    assert_eq!(value_text(&scene, sel), "A");
    if let Some(ControlState::Dropdown { selected_index, .. }) = scene.controls.get_mut(sel) {
        *selected_index = 2;
    }
    sync_control_visuals(&mut scene, sel, 0.0);
    assert_eq!(value_text(&scene, sel), "C", "re-sync after index change");
}

#[test]
fn sync_dropdown_selected_index_out_of_range_yields_empty() {
    // selected_index 越界（无对应 option）→ value slot 文本为空（不 panic、不残留旧值语义由
    // 调用方保证；此处只验不 panic 且文本被写成空串）。
    let mut scene = Scene::default();
    let sel = make_dropdown_with_options(&mut scene, &["A", "B"], 0);
    if let Some(ControlState::Dropdown { selected_index, .. }) = scene.controls.get_mut(sel) {
        *selected_index = 99;
    }
    sync_control_visuals(&mut scene, sel, 0.0);
    assert_eq!(
        value_text(&scene, sel),
        "",
        "out-of-range index → empty value text"
    );
}

#[test]
fn sync_dropdown_option_text_from_child_text_node() {
    // option 文本不在 option 自身的 text_contents，而在其后代 TextNode 里——收集须递归。
    let mut scene = Scene::default();
    let id = create_node_from_template(
        &mut scene,
        NodeKind::Dropdown,
        ResolvedStyle::default(),
        Some(ControlInit::Dropdown {
            selected_index: 0,
            option_values: Vec::new(),
        }),
    );
    // 作者结构：value slot + listbox role。
    let value = make_slot_child(&mut scene, id, SLOT_VALUE);
    let value_text_node = create_node_from_template(
        &mut scene,
        NodeKind::TextNode,
        ResolvedStyle::default(),
        None,
    );
    append_child(&mut scene, value, value_text_node).expect("value text append");
    let listbox = make_role_child(&mut scene, id, ROLE_LISTBOX);
    // option > TextNode("Deep")
    let opt = create_node_from_template(
        &mut scene,
        NodeKind::OptionItem,
        ResolvedStyle::default(),
        None,
    );
    let txt = create_node_from_template(
        &mut scene,
        NodeKind::TextNode,
        ResolvedStyle::default(),
        None,
    );
    scene.text_contents.insert(txt, "Deep".into());
    append_child(&mut scene, opt, txt).expect("text append");
    append_child(&mut scene, listbox, opt).expect("option append");
    sync_control_visuals(&mut scene, id, 0.0);
    assert_eq!(
        value_text(&scene, id),
        "Deep",
        "collects text from option subtree"
    );
}

/// popup 视口感知定位三档：下方 / 上翻 / 收缩。几何用 layout_rect 直喂（sync 读上帧
/// solve 的约定在测试里 = 直接写值）。
#[test]
fn dropdown_popup_stays_below_when_fits() {
    let mut scene = Scene::default();
    let sel = make_dropdown_with_options(&mut scene, &["A"], 0);
    let popup = find_child_by_role_recursive(&scene, sel, ROLE_LISTBOX).unwrap();
    // select y=100 h=30，popup h=80，视口 720：下方 130+80=210 放得下。
    scene.get_mut(sel).unwrap().layout_rect.h = 30.0;
    scene.get_mut(sel).unwrap().layout_rect.y = 100.0;
    scene.get_mut(popup).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 40.0,
        w: 100.0,
        h: 80.0,
    };
    if let Some(ControlState::Dropdown { open, .. }) = scene.controls.get_mut(sel) {
        *open = true;
    }
    sync_control_visuals(&mut scene, sel, 720.0);
    let t = scene.get(popup).unwrap().user_transform.translate;
    assert_eq!(
        t[1],
        100.0 + 30.0 - 40.0,
        "下方展开：ty = combo_y+sel_h-static_y"
    );
    // 非收缩档：无 max-height 覆写。
    assert!(
        !scene.get(popup).unwrap().inline_set.0 & crate::style::dynamic::INLINE_MAX_HEIGHT != 0
    );
}

#[test]
fn dropdown_popup_flips_up_near_viewport_bottom() {
    let mut scene = Scene::default();
    let sel = make_dropdown_with_options(&mut scene, &["A"], 0);
    let popup = find_child_by_role_recursive(&scene, sel, ROLE_LISTBOX).unwrap();
    // select y=650 h=30，popup h=100，视口 720：下方 680+100=780 放不下；上方 650-100=550 放得下。
    scene.get_mut(sel).unwrap().layout_rect.h = 30.0;
    scene.get_mut(sel).unwrap().layout_rect.y = 650.0;
    scene.get_mut(popup).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 40.0,
        w: 100.0,
        h: 100.0,
    };
    if let Some(ControlState::Dropdown { open, .. }) = scene.controls.get_mut(sel) {
        *open = true;
    }
    sync_control_visuals(&mut scene, sel, 720.0);
    let t = scene.get(popup).unwrap().user_transform.translate;
    assert_eq!(t[1], 650.0 - 100.0 - 40.0, "上翻：popup 底贴 select 顶");
}

#[test]
fn dropdown_popup_shrinks_when_neither_direction_fits() {
    let mut scene = Scene::default();
    let sel = make_dropdown_with_options(&mut scene, &["A"], 0);
    let popup = find_child_by_role_recursive(&scene, sel, ROLE_LISTBOX).unwrap();
    // select y=360 h=30，popup h=800，视口 720：下方/上方都放不下 → 收缩。
    scene.get_mut(sel).unwrap().layout_rect.h = 30.0;
    scene.get_mut(sel).unwrap().layout_rect.y = 360.0;
    scene.get_mut(popup).unwrap().layout_rect = Rect {
        x: 0.0,
        y: 40.0,
        w: 100.0,
        h: 800.0,
    };
    if let Some(ControlState::Dropdown { open, .. }) = scene.controls.get_mut(sel) {
        *open = true;
    }
    sync_control_visuals(&mut scene, sel, 720.0);
    let n = scene.get(popup).unwrap();
    assert_eq!(n.user_transform.translate[1], -40.0, "收缩：top 贴视口顶");
    let set = n.inline_set.0;
    assert!(
        set & crate::style::dynamic::INLINE_MAX_HEIGHT != 0,
        "max-height 覆写置位"
    );
    assert!(
        set & crate::style::dynamic::INLINE_OVERFLOW_Y != 0,
        "overflow-y 覆写置位"
    );
    // 收起：覆写回落（unset）。
    if let Some(ControlState::Dropdown { open, .. }) = scene.controls.get_mut(sel) {
        *open = false;
    }
    sync_control_visuals(&mut scene, sel, 720.0);
    let set = scene.get(popup).unwrap().inline_set.0;
    assert_eq!(
        set & crate::style::dynamic::INLINE_MAX_HEIGHT,
        0,
        "收起清 max-height"
    );
    assert_eq!(
        set & crate::style::dynamic::INLINE_OVERFLOW_Y,
        0,
        "收起清 overflow-y"
    );
}

#[test]
fn sync_dropdown_open_toggles_popup_display() {
    // open=true → popup display:block（标准弹出列表语义，option 垂直堆叠）；open=false → display:none。
    let mut scene = Scene::default();
    let sel = make_dropdown_with_options(&mut scene, &["A"], 0);
    // 默认 open=false
    sync_control_visuals(&mut scene, sel, 0.0);
    let popup = find_child_by_role_recursive(&scene, sel, ROLE_LISTBOX).expect("listbox present");
    assert_eq!(
        scene
            .get(popup)
            .unwrap()
            .inline_override
            .taffy_style
            .display,
        taffy::Display::None,
        "closed → display:none"
    );
    // 展开
    if let Some(ControlState::Dropdown { open, .. }) = scene.controls.get_mut(sel) {
        *open = true;
    }
    sync_control_visuals(&mut scene, sel, 0.0);
    assert_eq!(
        scene
            .get(popup)
            .unwrap()
            .inline_override
            .taffy_style
            .display,
        taffy::Display::Block,
        "open → display:block"
    );
}

// TabList 的 panel 跨树（非 tablist 子，靠 tab 的 aria-controls + panel 的 id 关联），
// 区别于 Dropdown 的 listbox（combobox 直接子）。非激活 panel 强制 display:none（剪枝
// 同 Dropdown listbox）；激活 panel **不写 display**——清 inline bit 回落作者 CSS
//（作者 flex/grid 布局不被覆写；未声明则 base_style 默认）。改 selected_index 再
// sync → 反转。

/// 建一个 role=tab 子节点（带 aria-controls 指向 panel id 串），挂到 parent（tablist）。
/// 复刻 instantiate 从模板填 RoleTable 的路径（作者写 `<div role=tab aria-controls=pa>`）。
fn make_tab_child(scene: &mut Scene, parent: NodeId, aria_controls: &str) -> NodeId {
    let id = create_node_from_template(scene, NodeKind::Tab, ResolvedStyle::default(), None);
    append_child(scene, parent, id).expect("fresh tab has no parent");
    scene.roles.insert(
        id,
        RoleInfo {
            role: Some(ROLE_TAB.to_string()),
            slots: Default::default(),
            attrs: vec![("aria-controls".to_string(), aria_controls.to_string())],
        },
    );
    id
}

/// 建一个游离 Container 节点并设 id_attr（模拟作者在 tablist 同层或别处写的 panel），
/// find_by_id_attr 靠 id_attr 扫全树解析（与树位置无关）。
fn make_panel(scene: &mut Scene, id_str: &str) -> NodeId {
    let id = create_node_from_template(scene, NodeKind::Container, ResolvedStyle::default(), None);
    scene.get_mut(id).unwrap().id_attr = Some(id_str.to_string());
    id
}

#[test]
fn tablist_panel_display_follows_selected_index() {
    // selected_index=0 → pa 激活（display 覆写权交还作者）、pb 非激活 display:none。
    // 改 selected_index=1 再 sync → 反转。panel 跨树（非 tablist 子，靠 aria-controls + id 关联）。
    let mut scene = Scene::default();
    let tl = create_node_from_template(
        &mut scene,
        NodeKind::TabList,
        ResolvedStyle::default(),
        Some(ControlInit::TabList {
            selected_index: 0,
            manual: false,
        }),
    );
    let _t0 = make_tab_child(&mut scene, tl, "pa");
    let _t1 = make_tab_child(&mut scene, tl, "pb");
    let pa = make_panel(&mut scene, "pa");
    let pb = make_panel(&mut scene, "pb");

    sync_control_visuals(&mut scene, tl, 0.0);
    assert_eq!(
        scene.get(pa).unwrap().inline_set.0 & crate::style::dynamic::INLINE_DISPLAY,
        0,
        "selected_index=0 → pa 激活：不覆写 display（bit 清，回落作者 CSS）"
    );
    assert_eq!(
        scene.get(pb).unwrap().inline_override.taffy_style.display,
        taffy::Display::None,
        "selected_index=0 → pb 非激活 hidden"
    );

    // 切到第 2 个 tab，再 sync：显隐反转（pa 被 none 剪枝，pb 的覆写 bit 交还）。
    if let Some(ControlState::TabList { selected_index, .. }) = scene.controls.get_mut(tl) {
        *selected_index = 1;
    }
    sync_control_visuals(&mut scene, tl, 0.0);
    assert_eq!(
        scene.get(pa).unwrap().inline_override.taffy_style.display,
        taffy::Display::None,
        "selected_index=1 → pa 非激活 hidden"
    );
    assert_eq!(
        scene.get(pb).unwrap().inline_set.0 & crate::style::dynamic::INLINE_DISPLAY,
        0,
        "selected_index=1 → pb 激活：不覆写 display（bit 清，回落作者 CSS）"
    );
}

#[test]
fn tablist_r1_missing_aria_controls_and_missing_panel_skip_cleanly() {
    // 容错：tab 未写 aria-controls（role 在但 aria_controls=None）→ 跳；tab 有 aria-controls
    // 但 panel id 解析不到（panel 未建）→ 跳。两者均不 panic，且不影响合法 panel 的显隐切换。
    let mut scene = Scene::default();
    let tl = create_node_from_template(
        &mut scene,
        NodeKind::TabList,
        ResolvedStyle::default(),
        Some(ControlInit::TabList {
            selected_index: 0,
            manual: false,
        }),
    );
    // tab0：role=tab 但无 aria-controls（make_role_child 只设 role）→ 跳过。
    let _t_no_controls = make_role_child(&mut scene, tl, ROLE_TAB);
    // tab1：aria-controls="ghost"，但 panel "ghost" 从未建 → find_by_id_attr 返 None → 跳过。
    let _t_ghost = make_tab_child(&mut scene, tl, "ghost");
    // tab2：合法 aria-controls="ok"，panel 存在；selected_index=0 → 非激活 → display:none。
    let _t_ok = make_tab_child(&mut scene, tl, "ok");
    let ok = make_panel(&mut scene, "ok");

    sync_control_visuals(&mut scene, tl, 0.0); // 不 panic
    assert_eq!(
        scene.get(ok).unwrap().inline_override.taffy_style.display,
        taffy::Display::None,
        "合法非激活 panel 仍切 none（前两个 tab 的 R1 跳过不影响它）"
    );
}

// find_control_at 从命中节点向上找最近有 ControlState 的节点：Tab 无 ControlState、
// TabList 有 → 点 tab 命中的 id 是父 TabList。on_pointer_down 的 TabList 臂收
// (id=TabList, pos)，须判定 pos 落在哪个 role=tab 子的 layout_rect 内，设其序号为
// selected_index。镜像 Dropdown 的 dropdown_option_at_pos 命中模式（rect-contains）。

/// 建 TabList + N 个 role=tab 子节点，每个 tab 占 80×30 矩形横向铺开：
/// tab_i @(i*80, 0, 80, 30)。复刻 layout_rect 由上一帧 solve 写入的标准（点击命中靠
/// layout_rect）。返回 (scene, tablist_id, [tab_id,...])。无 root——同现有 TabList 测
/// 模式（layout_rect 手设，点击测不需 solve/world_matrix）。
fn tablist_click_scene(num_tabs: usize, selected_index: usize) -> (Scene, NodeId, Vec<NodeId>) {
    let mut s = Scene::default();
    let tl = create_node_from_template(
        &mut s,
        NodeKind::TabList,
        ResolvedStyle::default(),
        Some(ControlInit::TabList {
            manual: false,
            selected_index: selected_index as u32,
        }),
    );
    let mut tab_ids = Vec::new();
    for i in 0..num_tabs {
        // make_tab_child 建 role=tab 子（带 aria_controls 占位，本测不关心 panel）。
        let tab = make_tab_child(&mut s, tl, &format!("p{i}"));
        s.get_mut(tab).unwrap().layout_rect = Rect {
            x: (i as f32) * 80.0,
            y: 0.0,
            w: 80.0,
            h: 30.0,
        };
        tab_ids.push(tab);
    }
    (s, tl, tab_ids)
}

#[test]
fn click_tab_sets_selected_index_and_emits() {
    // TabList(selected_index=0) + 2 tabs。点 tab1 区（x=100 在 [80,160)）→ selected_index=1
    // + 发 EVT_SELECTION_CHANGED@tablist，payload touch_id=1。on_pointer_down 收的 id 是
    // TabList（find_control_at 向上找到 ControlState::TabList）。
    let (mut s, tl, _tabs) = tablist_click_scene(2, 0);
    let events = on_pointer_down(&mut s, tl, [100.0, 15.0]);
    assert!(
        matches!(
            s.controls.get(tl),
            Some(ControlState::TabList {
                selected_index: 1,
                ..
            })
        ),
        "点 tab1 → selected_index=1"
    );
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EVT_SELECTION_CHANGED && e.node_id == tl.0 && e.touch_id == 1),
        "发 SelectionChanged@tablist，payload touch_id=新 index 1"
    );
}

#[test]
fn click_active_tab_emits_no_event() {
    // selected_index=0，点 tab0（已激活）→ 不改 selected_index、不发 SelectionChanged
    // （changed-guard，镜像 commit_dropdown_selection 的「仅净变才发」）。
    let (mut s, tl, _tabs) = tablist_click_scene(2, 0);
    let events = on_pointer_down(&mut s, tl, [40.0, 15.0]);
    assert!(
        matches!(
            s.controls.get(tl),
            Some(ControlState::TabList {
                selected_index: 0,
                ..
            })
        ),
        "点已激活 tab0 → selected_index 不变（仍 0）"
    );
    assert!(
        !events.iter().any(|e| e.event_type == EVT_SELECTION_CHANGED),
        "点已激活 tab → 不发 SelectionChanged（changed-guard）"
    );
}

#[test]
fn click_tablist_padding_noop() {
    // 点在 TabList 自身矩形内但不在任一 tab 子矩形内（tablist padding）→ no-op，不改
    // selected_index、不发事件。
    let (mut s, tl, _tabs) = tablist_click_scene(2, 0);
    let events = on_pointer_down(&mut s, tl, [300.0, 40.0]);
    assert!(
        matches!(
            s.controls.get(tl),
            Some(ControlState::TabList {
                selected_index: 0,
                ..
            })
        ),
        "点 tablist padding → selected_index 不变"
    );
    assert!(
        !events.iter().any(|e| e.event_type == EVT_SELECTION_CHANGED),
        "点 padding → 不发事件"
    );
}

// 生产路径：Stage::instantiate 建完子树后对每个 Dropdown 调 reparent。作者正确写法是
// option 已在 listbox 内（本函数 no-op）；这里测「option 直接写在 combobox 下」的兜底移动。
// direct/popup option children helper + 原语本身 + 顺序保序 + 幂等 + nth_option_text 扫 listbox。

/// 返回 select 的 OptionItem 直接子节点列表（旧结构：option 是 select 的直接子）。
fn direct_option_children(scene: &Scene, select: NodeId) -> Vec<NodeId> {
    scene
        .get(select)
        .map(|n| n.children.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|&cid| {
            scene
                .get(cid)
                .is_some_and(|c| c.kind == NodeKind::OptionItem)
        })
        .collect()
}

/// 返回 listbox（role=listbox，递归定位）的 OptionItem 直接子节点列表。
fn popup_option_children(scene: &Scene, select: NodeId) -> Vec<NodeId> {
    let popup = find_child_by_role_recursive(scene, select, ROLE_LISTBOX).expect("listbox present");
    scene
        .get(popup)
        .map(|n| n.children.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|&cid| {
            scene
                .get(cid)
                .is_some_and(|c| c.kind == NodeKind::OptionItem)
        })
        .collect()
}

#[test]
fn reparent_moves_options_from_combobox_into_listbox() {
    // 作者错误结构兜底：option 直接写在 combobox 下（应在 listbox 内）。reparent 把它们
    // 挪进 listbox role 子节点（递归定位）。
    let mut scene = Scene::default();
    let sel = create_node_from_template(
        &mut scene,
        NodeKind::Dropdown,
        ResolvedStyle::default(),
        Some(ControlInit::Dropdown {
            selected_index: 0,
            option_values: Vec::new(),
        }),
    );
    // listbox role 子（空，待 reparent 填充）。
    let listbox = make_role_child(&mut scene, sel, ROLE_LISTBOX);
    // 3 个 option 直接挂 combobox（错误结构）。
    let mut opts = vec![];
    for t in ["A", "B", "C"] {
        let opt = create_node_from_template(
            &mut scene,
            NodeKind::OptionItem,
            ResolvedStyle::default(),
            None,
        );
        scene.text_contents.insert(opt, t.into());
        append_child(&mut scene, sel, opt).unwrap();
        opts.push(opt);
    }
    // reparent 前：option 是 combobox 直接子、listbox 为空。
    assert_eq!(direct_option_children(&scene, sel), opts);
    assert!(scene.get(listbox).unwrap().children.is_empty());
    reparent_options_into_popup(&mut scene, sel);
    // reparent 后：combobox 不再含 option 直接子；listbox 含全部 3 个 option、保声明顺序。
    assert!(
        direct_option_children(&scene, sel).is_empty(),
        "combobox 不再含 OptionItem 直接子"
    );
    assert_eq!(
        popup_option_children(&scene, sel),
        opts,
        "option 移进 listbox 且保序"
    );
    // parent 指针指向 listbox（不是 combobox）。
    for &opt in &opts {
        assert_eq!(scene.get(opt).unwrap().parent, Some(listbox));
    }
}

#[test]
fn reparent_preserves_option_order() {
    // 5 个 option reparent 后顺序与声明一致（顺序决定 nth_option_text 取值 + listbox 渲染序）。
    let mut scene = Scene::default();
    let sel = create_node_from_template(
        &mut scene,
        NodeKind::Dropdown,
        ResolvedStyle::default(),
        Some(ControlInit::Dropdown {
            selected_index: 0,
            option_values: Vec::new(),
        }),
    );
    make_role_child(&mut scene, sel, ROLE_LISTBOX); // 空 listbox（待填充）
    let texts = ["alpha", "beta", "gamma", "delta", "epsilon"];
    for t in texts {
        let opt = create_node_from_template(
            &mut scene,
            NodeKind::OptionItem,
            ResolvedStyle::default(),
            None,
        );
        scene.text_contents.insert(opt, t.into());
        append_child(&mut scene, sel, opt).unwrap();
    }
    reparent_options_into_popup(&mut scene, sel);
    let popup_kids = popup_option_children(&scene, sel);
    assert_eq!(popup_kids.len(), texts.len());
    for (i, &opt) in popup_kids.iter().enumerate() {
        assert_eq!(
            scene.text_contents.get(&opt).map(|s| s.as_str()),
            Some(texts[i]),
            "第 {i} 个 option 须是 `{}`（声明顺序），顺序乱了",
            texts[i]
        );
    }
}

#[test]
fn reparent_is_idempotent() {
    // 重复调用不重复移动 / 不丢 option / 不 panic。option 已在 popup 里时再调为 no-op。
    let mut scene = Scene::default();
    let sel = make_dropdown_with_options(&mut scene, &["A", "B"], 0);
    let after_first = popup_option_children(&scene, sel);
    reparent_options_into_popup(&mut scene, sel); // 已 reparent 过（helper 调过）
    reparent_options_into_popup(&mut scene, sel); // 再调一次
    assert_eq!(
        popup_option_children(&scene, sel),
        after_first,
        "幂等：重复 reparent 不改 popup 内容"
    );
}

#[test]
fn reparent_no_listbox_is_noop() {
    // combobox 无 listbox 子节点（作者漏写）→ 无可 reparent 目标，不 panic、不误移 option。
    // 打包期结构契约会报 error，但运行时仍须 no-op（不杀进程）。
    let mut scene = Scene::default();
    let sel = create_node_from_template(
        &mut scene,
        NodeKind::Dropdown,
        ResolvedStyle::default(),
        None,
    );
    let opt = create_node_from_template(
        &mut scene,
        NodeKind::OptionItem,
        ResolvedStyle::default(),
        None,
    );
    append_child(&mut scene, sel, opt).unwrap();
    reparent_options_into_popup(&mut scene, sel); // 无 listbox → no-op
    assert_eq!(
        direct_option_children(&scene, sel),
        vec![opt],
        "无 listbox → option 留在 combobox（不误移）"
    );
}

#[test]
fn nth_option_text_reads_options_from_popup() {
    // nth_option_text 须扫 popup（不是 select）拿 option 文本——reparent 后 select 无 option 直接子。
    // 这里走 helper（已 reparent），验 selected_index=2 取到第 3 个 option 文本。
    let mut scene = Scene::default();
    let sel = make_dropdown_with_options(&mut scene, &["A", "B", "C"], 2);
    assert_eq!(nth_option_text(&scene, sel, 0).as_deref(), Some("A"));
    assert_eq!(nth_option_text(&scene, sel, 1).as_deref(), Some("B"));
    assert_eq!(nth_option_text(&scene, sel, 2).as_deref(), Some("C"));
    // 越界 → None。
    assert!(nth_option_text(&scene, sel, 3).is_none());
}

#[test]
fn nth_option_text_returns_none_when_options_are_select_direct_children() {
    // 证明 nth_option_text 现严格扫 popup：未 reparent（option 仍在 select 直接子）时返 None，
    // 防止误以为还能从 select 拿 option（旧行为）。反向验证新扫 popup 的正确性。
    let mut scene = Scene::default();
    let sel = create_node_from_template(
        &mut scene,
        NodeKind::Dropdown,
        ResolvedStyle::default(),
        Some(ControlInit::Dropdown {
            selected_index: 0,
            option_values: Vec::new(),
        }),
    );
    let opt = create_node_from_template(
        &mut scene,
        NodeKind::OptionItem,
        ResolvedStyle::default(),
        None,
    );
    scene.text_contents.insert(opt, "A".into());
    append_child(&mut scene, sel, opt).unwrap(); // 未 reparent：option 是 select 直接子
    assert!(
        nth_option_text(&scene, sel, 0).is_none(),
        "option 不在 popup 里 → nth_option_text 返 None（严格扫 popup）"
    );
}

#[test]
fn option_value_and_selected_derive_from_parent_state() {
    // value 语义：打包期静态配置优先（opts[0]="en"、opts[2]="ja"），缺席回落该项文本
    // （opts[1]="中文"，HTML 无 value 的 option 提交其文本）；selected 是父
    // selected_index + 序号的合成值（selected=2 → opts[2] 选中）。
    let mut scene = Scene::default();
    let sel = make_dropdown_with_options(&mut scene, &["English", "中文", "日本語"], 2);
    if let Some(ControlState::Dropdown { option_values, .. }) = scene.controls.get_mut(sel) {
        *option_values = vec![Some("en".into()), None, Some("ja".into())];
    }
    let opts: Vec<NodeId> = find_child_by_role_recursive(&scene, sel, ROLE_LISTBOX)
        .and_then(|lb| scene.get(lb).map(|n| n.children.clone()))
        .unwrap_or_default()
        .into_iter()
        .filter(|&c| scene.get(c).is_some_and(|n| n.kind == NodeKind::OptionItem))
        .collect();
    assert_eq!(opts.len(), 3);

    assert_eq!(dropdown_selected_value(&scene, sel).as_deref(), Some("ja"));
    assert_eq!(option_value(&scene, opts[0]).as_deref(), Some("en"));
    assert_eq!(
        option_value(&scene, opts[1]).as_deref(),
        Some("中文"),
        "absent value → falls back to option text"
    );
    assert_eq!(option_selected(&scene, opts[0]), Some(false));
    assert_eq!(option_selected(&scene, opts[2]), Some(true));

    // 非 Dropdown / 非 option 节点 → None（不 panic）。
    assert!(dropdown_selected_value(&scene, opts[0]).is_none());
    assert!(option_value(&scene, sel).is_none());
    assert!(option_selected(&scene, sel).is_none());
}

#[test]
fn dropdown_selected_value_falls_back_to_text_without_values() {
    // option_values 全缺席（未写 value 属性的包）→ 整体回落文本语义。
    let mut scene = Scene::default();
    let sel = make_dropdown_with_options(&mut scene, &["A", "B"], 1);
    assert_eq!(dropdown_selected_value(&scene, sel).as_deref(), Some("B"));
}

#[test]
fn tab_selected_derives_from_parent_tablist() {
    let mut scene = Scene::default();
    let tl = create_node_from_template(
        &mut scene,
        NodeKind::TabList,
        ResolvedStyle::default(),
        Some(ControlInit::TabList {
            selected_index: 1,
            manual: false,
        }),
    );
    let t0 = make_role_child(&mut scene, tl, ROLE_TAB);
    let t1 = make_role_child(&mut scene, tl, ROLE_TAB);
    assert_eq!(tab_selected(&scene, t0), Some(false));
    assert_eq!(tab_selected(&scene, t1), Some(true));
    // 切换父 selected_index → 合成值跟随（非字面存储）。
    if let Some(ControlState::TabList { selected_index, .. }) = scene.controls.get_mut(tl) {
        *selected_index = 0;
    }
    assert_eq!(tab_selected(&scene, t0), Some(true));
    assert_eq!(tab_selected(&scene, t1), Some(false));
    // 非 tab / 无 TabList 祖先 → None。
    assert!(tab_selected(&scene, tl).is_none());
}

// 直接调交互函数验逻辑（隔离 PointerState 仲裁）：Toggle 翻转、Radio 同名组互斥、
// Slider 拖拽改 value + step 量化。slider 几何手动设（解耦 solve：测试不把 slider 入 roots，
// solve 不触达，故手动写 layout_rect，同 slider_thumb_positioned_by_transform 模式）。

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

/// 手动设 slider 自身的 layout_rect（解耦 solve：新结构无 track 中间层，slider_pos_to_value
/// 与 sync 都读 slider 自身 layout_rect；测试不把 slider 入 roots，solve 不触达）。
fn set_slider_rect(scene: &mut Scene, slider: NodeId, x: f32, y: f32, w: f32, h: f32) {
    scene.get_mut(slider).unwrap().layout_rect = Rect { x, y, w, h };
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
    set_slider_rect(&mut scene, id, 0.0, 0.0, 200.0, 20.0);
    // 按下在 track 中间（pos.x=100 → value=50），拖到 75%（pos.x=150 → value=75）
    on_pointer_down(&mut scene, id, [100.0, 10.0]);
    on_pointer_move(&mut scene, id, [150.0, 10.0], true);
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
    set_slider_rect(&mut scene, id, 0.0, 0.0, 100.0, 20.0);
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
    set_slider_rect(&mut scene, id, 0.0, 0.0, 200.0, 20.0);
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
    set_slider_rect(&mut scene, id, 0.0, 0.0, 200.0, 20.0);
    on_pointer_move(&mut scene, id, [150.0, 10.0], true);
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
    set_slider_rect(&mut scene, id, 0.0, 0.0, 100.0, 20.0);
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
    set_slider_rect(&mut scene, id, 0.0, 0.0, 100.0, 20.0);
    // 不 panic 即过；dragging 仍置（交互被处理）。
    let _ = on_pointer_down(&mut scene, id, [50.0, 10.0]);
    let _ = on_pointer_move(&mut scene, id, [80.0, 10.0], true);
    let _ = on_pointer_up(&mut scene, id);
    // sanitize 后 min≤max：min 被 clamp 到 max（0≤0）。
    assert!(
        matches!(scene.controls.get(id), Some(ControlState::Slider { min, max, .. }) if min <= max),
        "sanitize 保证 min<=max"
    );
}

#[test]
fn malformed_progress_negative_max_sanitized() {
    // <progress min="-10" max="-5">：instantiate sanitize 到 max≥min（-5 ≥ -10 成立，
    // 不再强制 max≥0——min≠0 域合法），value clamp 进 [min,max]=[-10,-5]。
    // 下游 sync_control_visuals 的 (value-min)/(max-min) 不 panic（max>min 守卫）。
    let mut scene = Scene::default();
    let id = create_node_from_template(
        &mut scene,
        NodeKind::ProgressBar,
        ResolvedStyle::default(),
        Some(ControlInit::Progress {
            value: 30.0,
            min: -10.0,
            max: -5.0,
            indeterminate: false,
        }),
    );
    sync_control_visuals(&mut scene, id, 0.0);
    match scene.controls.get(id) {
        Some(ControlState::Progress {
            value, min, max, ..
        }) => {
            assert!(*max >= *min, "max sanitized to >=min, got {min}..{max}");
            assert!((value - -5.0).abs() < 1e-6, "value clamped into [-10,-5]");
        }
        _ => panic!("progress state exists"),
    }
}

#[test]
fn find_control_at_walks_to_ancestor() {
    // 命中控件的 thumb slot 子节点 → 向上找到 Slider 控件本身。
    let mut scene = Scene::default();
    let id = make_slider(&mut scene, 50.0, 0.0, 100.0);
    let thumb = find_child_by_slot(&scene, id, SLOT_THUMB).expect("slider has thumb");
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
    assert!(occupies_gesture(&scene, slider, true));
    assert!(!occupies_gesture(&scene, toggle, true));
    assert!(!occupies_gesture(&scene, radio, true));
    assert!(!occupies_gesture(&scene, progress, true));
}

#[test]
fn occupies_gesture_false_for_disabled_slider() {
    // disabled Slider 不占据手势 → 不抑制祖先 scroll（照 HTML：disabled input 不接受交互）。
    // 坑：旧实现对所有 Slider 返 true，按下后 scroll 仲裁被清却无人处理 → 用户滚不动。
    let mut scene = Scene::default();
    let slider = make_slider(&mut scene, 0.0, 0.0, 100.0);
    assert!(
        occupies_gesture(&scene, slider, true),
        "enabled slider 占据手势"
    );
    scene
        .get_mut(slider)
        .unwrap()
        .interaction
        .flags
        .insert(NodeFlags::DISABLED);
    assert!(
        !occupies_gesture(&scene, slider, true),
        "disabled slider 不占据手势（不抑制 scroll）"
    );
}

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
    set_slider_rect(&mut scene, id, 0.0, 0.0, 200.0, 20.0);
    let _ = on_pointer_down(&mut scene, id, [100.0, 10.0]); // value=50，无变化→不发
    let events = on_pointer_move(&mut scene, id, [150.0, 10.0], true); // value→75
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
    set_slider_rect(&mut scene, id, 0.0, 0.0, 200.0, 20.0);
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
    set_slider_rect(&mut scene, id, 0.0, 0.0, 200.0, 20.0);
    let _ = on_pointer_down(&mut scene, id, [100.0, 10.0]);
    let _ = on_pointer_move(&mut scene, id, [160.0, 10.0], true); // value→80
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

/// 建带 TextLayout 缓存的 TextField（解耦 solve：手动测文本 + 设 layout_rect）。
fn make_scene_with_textfield(text: &str) -> (Scene, NodeId) {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let font_data = std::fs::read(font_path).unwrap();
    let mut fonts = crate::text::layout::FontTable::new();
    fonts.register("DejaVu", font_data, true).unwrap();

    let mut scene = Scene::default();
    let id = create_node_from_template(
        &mut scene,
        NodeKind::TextField,
        ResolvedStyle::default(),
        Some(crate::asset::ControlInit::TextField(
            crate::asset::EditInit {
                value: text.to_string(),
                placeholder: String::new(),
                max_length: 0,
                readonly: false,
            },
        )),
    );
    // 设 layout_rect：click 坐标转换用 layout_rect.xy + border/padding。
    scene.get_mut(id).unwrap().layout_rect = Rect {
        x: 10.0,
        y: 10.0,
        w: 200.0,
        h: 30.0,
    };

    // 单行语义：生产 TextField 的 nowrap 由打包期烙印，手搓场景在此补齐
    //（不补则 measure 按宽度折行，单行视口/命中测试的前提不成立）。
    scene.get_mut(id).unwrap().style.white_space = crate::style::resolved::WhiteSpace::Nowrap;
    // 手动测文本 + 缓存 TextLayout（on_text_pointer_down 需要已缓存）。
    let style = scene.get(id).unwrap().style.clone();
    let stack = fonts.stack_for(style.font_family.as_deref());
    let off_left = crate::render::resolve_lp(style.taffy_style.border.left)
        + crate::render::resolve_lp(style.taffy_style.padding.left);
    let off_right = crate::render::resolve_lp(style.taffy_style.border.right)
        + crate::render::resolve_lp(style.taffy_style.padding.right);
    let lr = scene.get(id).unwrap().layout_rect;
    let content_w = (lr.w - off_left - off_right).max(0.0);
    let layout = crate::text::layout::measure_text(
        text,
        style.font_size,
        style.effective_line_height(),
        style.letter_spacing,
        style.text_align,
        crate::style::resolved::control_wrap_control(&style),
        Some(content_w),
        &stack,
        style.color,
        crate::text::rich::weight_from_font_weight(style.font_weight),
    );
    scene.text_layouts[id.index()] = Some(layout);
    (scene, id)
}

/// 建带 TextLayout 缓存的 TextArea（多行：measure 按 `\n` 断行，不 nowrap）。
/// 同 make_scene_with_textfield 的解耦手法：手动测文本 + 设 layout_rect。
fn make_scene_with_textarea(text: &str) -> (Scene, NodeId) {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let font_data = std::fs::read(font_path).unwrap();
    let mut fonts = crate::text::layout::FontTable::new();
    fonts.register("DejaVu", font_data, true).unwrap();

    let mut scene = Scene::default();
    let id = create_node_from_template(
        &mut scene,
        NodeKind::TextArea,
        ResolvedStyle::default(),
        Some(crate::asset::ControlInit::TextArea(
            crate::asset::EditInit {
                value: text.to_string(),
                placeholder: String::new(),
                max_length: 0,
                readonly: false,
            },
        )),
    );
    scene.get_mut(id).unwrap().layout_rect = Rect {
        x: 10.0,
        y: 10.0,
        w: 200.0,
        h: 90.0,
    };
    let style = scene.get(id).unwrap().style.clone();
    let stack = fonts.stack_for(style.font_family.as_deref());
    let layout = crate::text::layout::measure_text(
        text,
        style.font_size,
        style.effective_line_height(),
        style.letter_spacing,
        style.text_align,
        crate::style::resolved::control_wrap_control(&style),
        Some(200.0),
        &stack,
        style.color,
        crate::text::rich::weight_from_font_weight(style.font_weight),
    );
    scene.text_layouts[id.index()] = Some(layout);
    (scene, id)
}

#[test]
fn move_word_browser_semantics() {
    // forward 跳词尾后、backward 跳词首前（浏览器惯例）；标点是词间分隔。
    let mut e = EditState::from_init("hello world, again".into(), String::new(), 0, false);
    e.cursor = 0; // from_init 光标在串尾，词移动从首起测
    move_word(&mut e, true, false);
    assert_eq!(e.cursor, 5, "词尾后");
    move_word(&mut e, true, false);
    assert_eq!(e.cursor, 11, "跳过空格到下一词尾");
    move_word(&mut e, true, false);
    assert_eq!(e.cursor, 18, "跳过逗号+空格到句尾");
    move_word(&mut e, false, false);
    assert_eq!(e.cursor, 13, "backward 落词首");
    move_word(&mut e, false, false);
    assert_eq!(e.cursor, 6, "上一词首");
    move_word(&mut e, false, false);
    assert_eq!(e.cursor, 0, "到串首");
    move_word(&mut e, false, false);
    assert_eq!(e.cursor, 0, "越首 no-op");

    // select 扩展：anchor 不动。
    e.cursor = 0;
    e.anchor = 0;
    move_word(&mut e, true, true);
    assert_eq!((e.anchor, e.cursor), (0, 5));

    // CJK 连续段 = 一词（Chrome 同款：跳整段）。
    let mut c = EditState::from_init("你好 abc".into(), String::new(), 0, false);
    c.cursor = 0;
    move_word(&mut c, true, false);
    assert_eq!(c.cursor, 6, "「你好」整段 6 字节");
    move_word(&mut c, true, false);
    assert_eq!(c.cursor, 10, "跳空格到 abc 尾");
    move_word(&mut c, false, false);
    assert_eq!(c.cursor, 7, "abc 词首");
    move_word(&mut c, false, false);
    assert_eq!(c.cursor, 0, "回到「你好」段首");
}

#[test]
fn delete_word_forward_and_backward() {
    // ctrl+Backspace 删到前词首（先跳过词间分隔，Chrome 同款）、ctrl+Delete 删到后词尾；
    // 选区优先；边界外返 false。
    let mut e = EditState::from_init("hello world, again".into(), String::new(), 0, false);
    e.cursor = 18; // 串尾
    assert!(delete_word(&mut e, true), "删 again");
    assert_eq!(&e.value, "hello world, ");
    assert_eq!((e.cursor, e.anchor), (13, 13));
    assert!(delete_word(&mut e, true), "连分隔带词删 world");
    assert_eq!(&e.value, "hello ");
    assert_eq!(e.cursor, 6);

    let mut e2 = EditState::from_init("hello world".into(), String::new(), 0, false);
    e2.cursor = 0;
    e2.anchor = 0;
    assert!(delete_word(&mut e2, false), "删 hello");
    assert_eq!(&e2.value, " world");
    assert_eq!(e2.cursor, 0);
    assert!(!delete_word(&mut e2, true), "串首无前词");
    // 选区优先：非零选区时删选区而非词。
    e2.cursor = 0;
    e2.anchor = e2.value.len();
    assert!(delete_word(&mut e2, true));
    assert_eq!(&e2.value, "");
}

#[test]
fn textarea_vertical_nav_visual_lines_with_sticky_x() {
    // 3 行：aaaaa / be / aaa。宽行→短行→窄行：sticky x 必须保持宽行的列位，
    // 无 sticky 时第二次 Down 会从短行行尾的小 x 起跳（阶梯漂移）。
    let (scene, id) = make_scene_with_textarea("aaaaa\nbe\naaa");
    let ctx = text_nav_context(&scene, id).expect("layout cached");
    let mut e = EditState::from_init("aaaaa\nbe\naaa".into(), String::new(), 0, false);
    e.cursor = 5; // 行 0 尾（aaaaa 后、\n 前）

    move_vertical(&mut e, &ctx, true, false);
    assert_eq!(e.cursor, 8, "行 1 尾（be 后）；ideal(5×a宽) 超行宽钳到行尾");
    let ideal_after_first = e.ideal_cursor_x;
    assert!(ideal_after_first > 0.0);

    move_vertical(&mut e, &ctx, true, false);
    assert_eq!(e.cursor, 12, "行 2 尾（aaa 后）；sticky x 仍是宽行列位");
    assert_eq!(
        e.ideal_cursor_x, ideal_after_first,
        "连续 Down 复用同一 ideal（阶梯防漂移）"
    );

    move_vertical(&mut e, &ctx, true, false);
    assert_eq!(e.cursor, 12, "越底行 no-op");

    move_vertical(&mut e, &ctx, false, false);
    assert_eq!(e.cursor, 8, "回行 1 尾（sticky 仍宽行）");

    // 字符移动后 ideal 失效：下次垂直导航从当前光标像素重算。Home 到行 1 首
    //（x=0）→ Down 重算 ideal=0 → 确定性落行 2 首（不依赖字形 advance）。
    move_cursor(&mut e, NodeKind::TextArea, false, false);
    assert!(!e.ideal_cursor_valid);
    line_home_end(&mut e, &ctx, true, false);
    move_vertical(&mut e, &ctx, true, false);
    assert_eq!(e.cursor, 9, "ideal 重算为 0 → 行 2 首");
}

#[test]
fn textarea_home_end_line_level() {
    // 行级 Home/End：裸键=当前视觉行首/尾；行尾退到 \n 前（视觉行尾）。
    let (scene, id) = make_scene_with_textarea("aaaaa\nbe\naaa");
    let ctx = text_nav_context(&scene, id).expect("layout cached");
    let mut e = EditState::from_init("aaaaa\nbe\naaa".into(), String::new(), 0, false);
    e.cursor = 7; // 行 1 中段

    line_home_end(&mut e, &ctx, true, false);
    assert_eq!((e.cursor, e.anchor), (6, 6), "行 1 首");
    line_home_end(&mut e, &ctx, false, false);
    assert_eq!(e.cursor, 8, "行 1 尾（\\n 前，非 9）");

    // select：anchor 保持。
    e.cursor = 7;
    e.anchor = 7;
    line_home_end(&mut e, &ctx, true, true);
    assert_eq!((e.anchor, e.cursor), (7, 6));
}

#[test]
fn textarea_nav_maps_cursor_through_masked_display() {
    // 回归（review 抓出）：move_vertical/line_home_end 的 value→display 偏移换算曾把
    // (value, display) 两参传反。display==value 或同为 ASCII 时两向数值恒等抓不出；
    // 掩码 display（● 3B/字符）与 ASCII value（1B/字符）字节布局不同才有判别力。
    // value "ab\ncd"，display "●●\n●●"：正确方向 value 光标 4（'d' 前）→ display
    // 字符 4（行 1 第 2 个 ●）→ 行 1；交换方向 display[..3] 回退成 1 字符 → value
    // 字节 1 被当 display 偏移 → 行 0。
    let (mut scene, id) = make_scene_with_textarea("●●\n●●"); // layout 按 display 形状测
    scene.get_mut(id).unwrap().style.text_security =
        Some(crate::style::resolved::TextSecurity::Disc);
    if let Some(ControlState::TextArea(e)) = scene.controls.get_mut(id) {
        e.value = "ab\ncd".into();
        e.cursor = 4; // value 'd' 前（行 1 末段）
    }
    let ctx = text_nav_context(&scene, id).expect("layout cached");
    assert_eq!(ctx.display, "●●\n●●", "display = 掩码串");

    // Home：正确映射 → 行 1 首（display 字节 7）→ value 'c' 前 = 3；
    // 交换参数 → 行 0 首 → cursor 0（错）。
    let mut e = EditState::from_init("ab\ncd".into(), String::new(), 0, false);
    e.cursor = 4;
    line_home_end(&mut e, &ctx, true, false);
    assert_eq!(
        e.cursor, 3,
        "掩码下 Home 落行 1 首（value↔display 按字符数映射）"
    );

    // Down：正确映射 → 已在末行 no-op 停 4；交换参数 → 起点误判行 0 → 跳行 1 首 → 3（错）。
    let mut e2 = EditState::from_init("ab\ncd".into(), String::new(), 0, false);
    e2.cursor = 4;
    move_vertical(&mut e2, &ctx, true, false);
    assert_eq!(
        e2.cursor, 4,
        "掩码下末行 Down no-op（行判定用 value 侧位置）"
    );
}

#[test]
fn on_pointer_move_text_drag_gated_by_disabled() {
    // review 回归：disabled 文本框不接受拖选 Move——on_pointer_down 与 occupies_gesture
    // 都门控了 disabled，Move 臂曾漏（拖过 disabled 框仍推 cursor/anchor/cursor_visible）。
    let (mut scene, id) = make_scene_with_textfield("hello world");
    scene
        .get_mut(id)
        .unwrap()
        .interaction
        .flags
        .insert(NodeFlags::DISABLED);
    if let Some(ControlState::TextField(e)) = scene.controls.get_mut(id) {
        e.cursor = 0;
        e.anchor = 0;
    }
    let events = on_pointer_move(&mut scene, id, [11.0 + 190.0, 10.0 + 1.0], true);
    assert!(events.is_empty(), "disabled 文本臂不产事件");
    if let Some(ControlState::TextField(e)) = scene.controls.get(id) {
        assert_eq!(
            (e.cursor, e.anchor),
            (0, 0),
            "disabled 拖选不推 cursor/anchor"
        );
    } else {
        panic!("not TextField");
    }
}

#[test]
fn text_drag_extends_selection_anchor_kept() {
    // Down 落行首附近（anchor=cursor=0），拖到远右 → cursor 到串尾、anchor 不动。
    let (mut scene, id) = make_scene_with_textfield("hello world");
    on_text_pointer_down(&mut scene, id, 11.0 + 1.0, 10.0 + 1.0);
    let (a0, c0) = match scene.controls.get(id) {
        Some(ControlState::TextField(e)) => (e.anchor, e.cursor),
        _ => panic!("not TextField"),
    };
    assert_eq!(a0, c0, "down 折叠");
    on_text_pointer_drag(&mut scene, id, 11.0 + 190.0, 10.0 + 1.0);
    if let Some(ControlState::TextField(e)) = scene.controls.get(id) {
        assert_eq!(e.anchor, a0, "拖拽不动 anchor");
        assert_eq!(e.cursor, "hello world".len(), "cursor 拖到串尾");
    } else {
        panic!("not TextField");
    }
}

#[test]
fn occupies_gesture_text_mouse_only() {
    // 文本控件仅鼠标占据（拖=选区）；触摸不占（拖让位视口 pan）；disabled 不占。
    let mut scene = Scene::default();
    let id = create_node_from_template(
        &mut scene,
        NodeKind::TextArea,
        ResolvedStyle::default(),
        Some(crate::asset::ControlInit::TextArea(
            crate::asset::EditInit {
                value: String::new(),
                placeholder: String::new(),
                max_length: 0,
                readonly: false,
            },
        )),
    );
    assert!(occupies_gesture(&scene, id, true), "鼠标拖=选区，占手势");
    assert!(!occupies_gesture(&scene, id, false), "触摸拖让位 pan，不占");
    scene
        .get_mut(id)
        .unwrap()
        .interaction
        .flags
        .insert(NodeFlags::DISABLED);
    assert!(!occupies_gesture(&scene, id, true), "disabled 不拦截指针");
}

#[test]
fn on_pointer_move_text_drag_gated_by_mouse() {
    // 鼠标：Move 推进选区 cursor；触摸（is_mouse=false）：no-op。
    let (mut scene, id) = make_scene_with_textfield("hello world");
    on_pointer_down(&mut scene, id, [11.0, 11.0]);
    on_pointer_move(&mut scene, id, [200.0, 11.0], false);
    let after_touch = match scene.controls.get(id) {
        Some(ControlState::TextField(e)) => e.cursor,
        _ => panic!("not TextField"),
    };
    on_pointer_move(&mut scene, id, [200.0, 11.0], true);
    if let Some(ControlState::TextField(e)) = scene.controls.get(id) {
        assert!(
            e.cursor >= after_touch,
            "鼠标拖推进 cursor（{after_touch} → {}）",
            e.cursor
        );
        assert_eq!(e.cursor, "hello world".len());
    } else {
        panic!("not TextField");
    }
}

#[test]
fn sync_edit_view_follows_cursor_beyond_content() {
    // 长文本（超 200px 宽）+ 光标在末尾 → 视口右滚，光标 x 落入可视窗
    // [view_x, view_x + content_w]（留 margin）。
    let long = "hello world, this is a long line of text";
    let (mut scene, id) = make_scene_with_textfield(long);
    sync_edit_view(&mut scene);
    if let Some(ControlState::TextField(e)) = scene.controls.get(id) {
        let layout = scene.text_layouts[id.index()].as_ref().unwrap();
        let ranges = line_byte_ranges(layout, long);
        let (cx, _) = cursor_pixel_x(layout, &ranges, e.cursor);
        let content_w = scene.get(id).unwrap().layout_rect.w;
        let text_w = layout.lines[0].width;
        assert!(text_w > content_w, "fixture must overflow: text_w={text_w}");
        assert!(
            e.view_x > 0.0,
            "cursor at end must scroll, view_x={}",
            e.view_x
        );
        assert!(
            cx >= e.view_x && cx <= e.view_x + content_w,
            "caret x {cx} must stay in view [{}, {}]",
            e.view_x,
            e.view_x + content_w
        );
        assert!(
            e.view_x <= text_w - content_w + 0.5,
            "view_x must clamp to max scroll"
        );
    } else {
        panic!("not TextField");
    }
}

#[test]
fn sync_edit_view_zero_when_content_fits() {
    // 短文本不溢出 → 视口恒 0。
    let (mut scene, id) = make_scene_with_textfield("hi");
    sync_edit_view(&mut scene);
    if let Some(ControlState::TextField(e)) = scene.controls.get(id) {
        assert_eq!(e.view_x, 0.0);
    } else {
        panic!("not TextField");
    }
}

#[test]
fn sync_edit_view_scrolls_back_to_start() {
    // 末尾滚过去后光标回行首 → 视口归零（Home/点行首的跟随）。
    let long = "hello world, this is a long line of text";
    let (mut scene, id) = make_scene_with_textfield(long);
    sync_edit_view(&mut scene);
    let vx_before = match scene.controls.get(id) {
        Some(ControlState::TextField(e)) => e.view_x,
        _ => panic!("not TextField"),
    };
    assert!(vx_before > 0.0);
    if let Some(ControlState::TextField(e)) = scene.controls.get_mut(id) {
        e.cursor = 0;
    }
    sync_edit_view(&mut scene);
    let vx_after = match scene.controls.get(id) {
        Some(ControlState::TextField(e)) => e.view_x,
        _ => panic!("not TextField"),
    };
    assert_eq!(vx_after, 0.0, "cursor at start must scroll back to 0");
}

#[test]
fn textfield_click_sets_cursor() {
    // 点击 "hello" 在 local_x=20 附近（第 2 个字符 "e" 区域），光标应落在合理范围。
    let (mut scene, id) = make_scene_with_textfield("hello");
    on_text_pointer_down(&mut scene, id, 20.0, 5.0);
    if let Some(ControlState::TextField(e)) = scene.controls.get(id) {
        assert!(
            e.cursor >= 1 && e.cursor <= 3,
            "cursor near char 2 (byte range 1..=3), got {}",
            e.cursor
        );
        assert_eq!(e.anchor, e.cursor, "anchor equals cursor (no selection)");
        assert!(e.cursor_visible, "cursor_visible true after click");
    } else {
        panic!("not TextField");
    }
}

#[test]
fn textfield_click_noop_without_layout_cache() {
    // 无 TextLayout 缓存（首帧尚无 measure）→ no-op，不 panic。
    let mut scene = Scene::default();
    let id = create_node_from_template(
        &mut scene,
        NodeKind::TextField,
        ResolvedStyle::default(),
        Some(crate::asset::ControlInit::TextField(
            crate::asset::EditInit {
                value: "hello".into(),
                placeholder: String::new(),
                max_length: 0,
                readonly: false,
            },
        )),
    );
    on_text_pointer_down(&mut scene, id, 20.0, 5.0);
    // 未设 layout_rect 也无 TextLayout → no-op，cursor 维持初始值（末尾）。
    if let Some(ControlState::TextField(e)) = scene.controls.get(id) {
        assert_eq!(e.cursor, 5, "cursor stays at end (initial value)");
    } else {
        panic!("not TextField");
    }
}

#[test]
fn advance_cursor_blink_flips_visibility() {
    let (mut scene, id) = make_scene_with_textfield("hi");
    scene.focused_node = Some(id);
    // 初始 cursor_visible = true（from_init 设）
    assert!(get_cursor_visible(&scene, id));
    // 推进 < 0.7s：不应翻转
    advance_cursor_blink(&mut scene, 0.3);
    assert!(get_cursor_visible(&scene, id));
    // 推进够 0.7s（累计 1.0s）：应翻转一次
    advance_cursor_blink(&mut scene, 0.5);
    assert!(!get_cursor_visible(&scene, id));
    // 再 0.7s：再次翻转
    advance_cursor_blink(&mut scene, 0.7);
    assert!(get_cursor_visible(&scene, id));
}

#[test]
fn advance_cursor_blink_hides_when_not_focused() {
    let (mut scene, id) = make_scene_with_textfield("hi");
    scene.focused_node = None;
    advance_cursor_blink(&mut scene, 1.0);
    // 未聚焦 → cursor_visible 强制 false
    assert!(!get_cursor_visible(&scene, id));
}

/// 读 TextField EditState.cursor_visible。panic 若非 TextField。
fn get_cursor_visible(scene: &Scene, id: NodeId) -> bool {
    match scene.controls.get(id) {
        Some(ControlState::TextField(e)) => e.cursor_visible,
        _ => panic!("not TextField"),
    }
}

/// 读 TextField EditState.cursor（字节偏移）。panic 若非 TextField。
fn get_cursor(scene: &Scene, id: NodeId) -> usize {
    match scene.controls.get(id) {
        Some(ControlState::TextField(e)) => e.cursor,
        _ => panic!("not TextField"),
    }
}

// on_text_pointer_down 接收的坐标已是 content-area-local（减过 layout_rect.xy +
// border + padding）。on_pointer_down（公共协调器）负责这层减法。既有 4 个光标测试都
// 直调 on_text_pointer_down，跳过了减法——此测试锁住 on_pointer_down 的转换链：
//   world_x − lr.x − border_left − padding_left → content-local x → hit_byte_offset
// 用非零 border/padding（content offset = 6）+ 非零 layout_rect.xy，使减法非平凡，
// 并选点击点跨 glyph 中点，保证减法错误会翻转 byte offset（非退化）。

/// 建带非零 border/padding + 已缓存 TextLayout 的 TextField（解耦 solve）。
///
/// content offset = border_left(2) + padding_left(4) = 6（左），border_top(1) +
/// padding_top(3) = 4（上）。layout_rect = {x:10, y:20, w:200, h:30}。测文本时 content_w
/// 用同一 border/padding 算（与 measure_text_controls 一致），保证 TextLayout 坐标系
/// 与 on_pointer_down 减法后的 content-local 对齐。
fn make_scene_with_textfield_inset(text: &str) -> (Scene, NodeId) {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
    let font_data = std::fs::read(font_path).unwrap();
    let mut fonts = crate::text::layout::FontTable::new();
    fonts.register("DejaVu", font_data, true).unwrap();

    let mut scene = Scene::default();
    let id = create_node_from_template(
        &mut scene,
        NodeKind::TextField,
        ResolvedStyle::default(),
        Some(crate::asset::ControlInit::TextField(
            crate::asset::EditInit {
                value: text.to_string(),
                placeholder: String::new(),
                max_length: 0,
                readonly: false,
            },
        )),
    );
    // 非零 border/padding：使 on_pointer_down 的减法非平凡（content offset 左=6, 上=4）。
    scene.get_mut(id).unwrap().style.taffy_style.border = taffy::geometry::Rect {
        left: taffy::style::LengthPercentage::length(2.0),
        right: taffy::style::LengthPercentage::length(0.0),
        top: taffy::style::LengthPercentage::length(1.0),
        bottom: taffy::style::LengthPercentage::length(0.0),
    };
    scene.get_mut(id).unwrap().style.taffy_style.padding = taffy::geometry::Rect {
        left: taffy::style::LengthPercentage::length(4.0),
        right: taffy::style::LengthPercentage::length(0.0),
        top: taffy::style::LengthPercentage::length(3.0),
        bottom: taffy::style::LengthPercentage::length(0.0),
    };
    scene.get_mut(id).unwrap().layout_rect = Rect {
        x: 10.0,
        y: 20.0,
        w: 200.0,
        h: 30.0,
    };

    // 手动测文本 + 缓存 TextLayout（content_w 用同一 border/padding，对齐坐标系）。
    let style = scene.get(id).unwrap().style.clone();
    let stack = fonts.stack_for(style.font_family.as_deref());
    let off_left = crate::render::resolve_lp(style.taffy_style.border.left)
        + crate::render::resolve_lp(style.taffy_style.padding.left);
    let off_right = crate::render::resolve_lp(style.taffy_style.border.right)
        + crate::render::resolve_lp(style.taffy_style.padding.right);
    let lr = scene.get(id).unwrap().layout_rect;
    let content_w = (lr.w - off_left - off_right).max(0.0);
    let layout = crate::text::layout::measure_text(
        text,
        style.font_size,
        style.effective_line_height(),
        style.letter_spacing,
        style.text_align,
        crate::style::resolved::control_wrap_control(&style),
        Some(content_w),
        &stack,
        style.color,
        crate::text::rich::weight_from_font_weight(style.font_weight),
    );
    scene.text_layouts[id.index()] = Some(layout);
    (scene, id)
}

#[test]
fn on_pointer_down_converts_world_to_content_local() {
    // 锁住 on_pointer_down（公共协调器）的世界→内容区坐标转换。
    // content offset 左=6（border 2 + padding 4），上=4（border 1 + padding 3），
    // layout_rect.xy=(10,20)。点击点选在跨某 glyph 中点处，使减法错误会翻转 byte offset。
    let (mut scene, id) = make_scene_with_textfield_inset("hello");

    // 扫首行 glyph，取第一个中点 >= 6 的（保证 target = mid - 3 > 0）。
    let layout = scene.text_layouts[id.index()]
        .as_ref()
        .expect("layout cached")
        .clone();
    assert!(!layout.lines.is_empty(), "hello 至少一行");
    let first_line = &layout.lines[0];
    let mut pen = 0.0f32;
    let mut mid = None;
    'scan: for run in &first_line.runs {
        for g in &run.glyphs {
            let m = pen + g.advance / 2.0;
            if m >= 6.0 {
                mid = Some(m);
                break 'scan;
            }
            pen += g.advance;
        }
    }
    let mid = mid.expect("hello 有中点 >= 6 的 glyph");

    // content-local 目标 = mid - 3（中点左侧 → cursor 落在该 glyph 起始字节）。
    let target_x = mid - 3.0;
    let target_y = 5.0; // 单行，任意 content-local y（hit 选行 0）

    // 参考：直接用 content-local 调 on_text_pointer_down 取预期 offset。
    let expected = {
        let (mut ref_scene, ref_id) = make_scene_with_textfield_inset("hello");
        on_text_pointer_down(&mut ref_scene, ref_id, target_x, target_y);
        get_cursor(&ref_scene, ref_id)
    };

    // 经 on_pointer_down（公共协调器）点击对应世界坐标：
    //   world_x = lr.x(10) + border_left(2) + padding_left(4) + target_x
    //   world_y = lr.y(20) + border_top(1) + padding_top(3) + target_y
    let world_x = 10.0 + 2.0 + 4.0 + target_x;
    let world_y = 20.0 + 1.0 + 3.0 + target_y;
    on_pointer_down(&mut scene, id, [world_x, world_y]);

    assert_eq!(
        get_cursor(&scene, id),
        expected,
        "on_pointer_down 减 layout_rect.xy + border + padding 后命中 content-local x"
    );

    // 灵敏度保证：若减法被跳过/错误（如 resolve_lp 返 0），content-local 会偏 +6 到
    // mid+3（中点右侧 → cursor +1），与 expected 不同。这证明点击点对减法敏感（非退化）。
    let insensitive = {
        let (mut ref2, rid2) = make_scene_with_textfield_inset("hello");
        on_text_pointer_down(&mut ref2, rid2, target_x + 6.0, target_y);
        get_cursor(&ref2, rid2)
    };
    assert_ne!(
        insensitive, expected,
        "[target, target+6] 跨 glyph 中点：减法错误会翻转 offset（测试非退化）"
    );
}

#[test]
fn on_text_pointer_down_clamps_cursor_to_value_len_when_layout_exceeds_value() {
    // 回归：layout solve 对空 value 控件用 placeholder measure 并缓存到 text_layouts
    //（layout/mod.rs value 空时 display 退到 placeholder）。on_text_pointer_down 用
    // raw value（空）+ 该 layout 算 cursor → hit_byte_offset 返 placeholder 字节数 > 0，
    // 但 value.len()=0 → cursor 越界 → insert_str panic（is_char_boundary 断言失败，
    // showcase TextField 拼音输入崩溃根因）。cursor 须钳到 value.len() + char 边界。
    let (mut scene, id) = make_scene_with_textfield_inset("");
    // 覆盖 text_layouts：模拟 layout solve 缓存 placeholder layout（多 glyph；value 空时
    // layout 实际由 placeholder measure 产生）。make_scene_with_textfield_inset("") 对
    // 空串 measure 产零 glyph layout，这里手动换成有 glyph 的，复现 layout/value 不一致。
    {
        let style = scene.get(id).unwrap().style.clone();
        let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
        let font_data = std::fs::read(font_path).unwrap();
        let mut fonts = crate::text::layout::FontTable::new();
        fonts.register("DejaVu", font_data, true).unwrap();
        let stack = fonts.stack_for(style.font_family.as_deref());
        let lr = scene.get(id).unwrap().layout_rect;
        let off_left = crate::render::resolve_lp(style.taffy_style.border.left)
            + crate::render::resolve_lp(style.taffy_style.padding.left);
        let off_right = crate::render::resolve_lp(style.taffy_style.border.right)
            + crate::render::resolve_lp(style.taffy_style.padding.right);
        let content_w = (lr.w - off_left - off_right).max(0.0);
        let layout = crate::text::layout::measure_text(
            "abcd",
            style.font_size,
            style.effective_line_height(),
            style.letter_spacing,
            style.text_align,
            crate::style::resolved::control_wrap_control(&style),
            Some(content_w),
            &stack,
            style.color,
            crate::text::rich::weight_from_font_weight(style.font_weight),
        );
        scene.text_layouts[id.index()] = Some(layout);
    }
    // 点击 placeholder 中部（content-local）：hit_byte_offset 返 placeholder 字节偏移（>0），
    // 但 value 空 → 修复前 cursor 越界，修复后须钳到 0。
    // 先验前置条件：raw offset 须 > value.len()=0，否则测试平凡通过没真正测到 clamp 路径
    // （依赖 DejaVuSans 首字形宽度，x=50 须落在首字之后）。
    {
        let layout = scene.text_layouts[id.index()].as_ref().unwrap();
        let value = "";
        let ranges = line_byte_ranges(layout, value);
        let raw = hit_byte_offset(layout, &ranges, 50.0, 5.0);
        assert!(
            raw > 0,
            "前置条件：raw offset={raw} 须 > value.len()=0，否则测试未真正触发 clamp"
        );
    }
    on_text_pointer_down(&mut scene, id, 50.0, 5.0);
    let cursor = get_cursor(&scene, id);
    assert_eq!(
        cursor, 0,
        "value 空 → cursor 须钳到 value.len()=0（不越界），实际 cursor={cursor}"
    );
}

// 纯函数 over EditState（无 Scene 改动）。insert_text/delete_char/move_cursor 是
// textinput channel + control-key 路由的编辑内核。UTF-8 边界
// 保证 cursor/anchor 永远落在 char 起始字节（CJK 3 字节字符不能停在中间字节）。

#[test]
fn insert_at_cursor() {
    let mut e = EditState::from_init("ac".into(), "".into(), 0, false);
    e.cursor = 1;
    e.anchor = 1;
    insert_text(&mut e, NodeKind::TextField, "b");
    assert_eq!(e.value, "abc");
    assert_eq!(e.cursor, 2);
}

#[test]
fn insert_replaces_selection() {
    let mut e = EditState::from_init("hello".into(), "".into(), 0, false);
    e.anchor = 1;
    e.cursor = 4;
    insert_text(&mut e, NodeKind::TextField, "X");
    assert_eq!(e.value, "hXo");
    assert_eq!(e.cursor, 2);
}

#[test]
fn backspace_deletes_left() {
    let mut e = EditState::from_init("abc".into(), "".into(), 0, false);
    e.cursor = 2;
    e.anchor = 2;
    delete_char(&mut e, NodeKind::TextField, true);
    assert_eq!(e.value, "ac");
    assert_eq!(e.cursor, 1);
}

#[test]
fn sanitize_strips_newline_single_line() {
    let mut e = EditState::from_init("a\nb".into(), "".into(), 0, false);
    sanitize_value(&mut e, NodeKind::TextField);
    assert_eq!(e.value, "ab");
    let mut e2 = EditState::from_init("a\nb".into(), "".into(), 0, false);
    sanitize_value(&mut e2, NodeKind::TextArea);
    assert_eq!(e2.value, "a\nb");
}

#[test]
fn utf8_boundary_clamp() {
    // 你好 = 6 字节（每字 3 字节）。cursor=3 落在第一字末尾（非法边界）→ move right
    // 应跳到下一 char 边界 6，不停在 3（中途字节）。
    let mut e = EditState::from_init("你好".into(), "".into(), 0, false);
    e.cursor = 3;
    move_cursor(&mut e, NodeKind::TextField, true, false);
    assert_eq!(e.cursor, 6);
}

#[test]
fn max_length_truncates() {
    // max_length 按 UTF-8 字符数计（非字节）。已有 2 字符 "ab"，上限 2 → 插 "c" 拒绝。
    let mut e = EditState::from_init("ab".into(), "".into(), 2, false);
    e.cursor = 2;
    e.anchor = 2;
    insert_text(&mut e, NodeKind::TextField, "c");
    assert_eq!(e.value, "ab");
}

#[test]
fn insert_over_max_after_selection_rejects_cleanly() {
    // value="hello"(5 chars), 选区 [1,4)="ell"(3), max_length=2。插 "XYZ"(3) 会超 2 →
    // 必须干净拒绝：不删选区、不改 value、selection 完好。
    // 回归契约：max_length 校验须在 delete_selection 之前，否则被拒插入会静默丢掉选区。
    let mut e = EditState::from_init("hello".into(), "".into(), 2, false);
    e.anchor = 1;
    e.cursor = 4;
    assert!(!insert_text(&mut e, NodeKind::TextField, "XYZ"));
    assert_eq!(e.value, "hello"); // value 未变
    assert_eq!(e.anchor, 1); // 选区完好
    assert_eq!(e.cursor, 4);
}

#[test]
fn display_value_range_normal_field_matches_raw_comp_pos() {
    // display_value 返回的区间 = raw comp.pos..+len（value 原样拼接，无字节布局变换）。
    // 回归锁：确保 char 对齐改造未坏掉普通文本框的常见路径。
    let mut e = EditState::from_init("ab".into(), "".into(), 0, false);
    set_composition(&mut e, "ni", 1);
    let (display, range) = display_value(&e);
    assert_eq!(display, "anib");
    let (start, end) = range.expect("composition range present");
    assert_eq!(&display[start..end], "ni");
}

#[test]
fn display_value_no_composition_returns_none() {
    // 无 composition → range 为 None（render 不画下划线，cursor_rect 退回原始光标）。
    let e = EditState::from_init("ab".into(), "".into(), 0, false);
    let (display, range) = display_value(&e);
    assert_eq!(display, "ab");
    assert!(range.is_none());
}

#[test]
fn display_value_masked_replaces_chars_keeps_count() {
    // 掩码 1 char : 1 char（字符数不变、字节宽变）；换行保留（TextArea 多行）。
    let e = EditState::from_init("ab密码".into(), "".into(), 0, false);
    let (masked, range) = display_value_masked(&e, Some('●'));
    assert_eq!(masked, "●●●●");
    assert_eq!(masked.chars().count(), "ab密码".chars().count());
    assert!(range.is_none());
}

#[test]
fn display_value_masked_none_passthrough() {
    let e = EditState::from_init("ab".into(), "".into(), 0, false);
    let (display, _) = display_value_masked(&e, None);
    assert_eq!(display, "ab");
}

#[test]
fn display_value_masked_composition_range_remap() {
    // comp 区间按字符数换算到掩码串字节空间：掩码点即预提交文本位置。
    let mut e = EditState::from_init("ab".into(), "".into(), 0, false);
    set_composition(&mut e, "ni", 1);
    let (masked, range) = display_value_masked(&e, Some('●'));
    assert_eq!(masked, "●●●●");
    let (start, end) = range.expect("composition range present");
    assert_eq!(&masked[start..end], "●●");
}

#[test]
fn mask_byte_conversions_roundtrip() {
    // value↔display 字节换算：掩码下字节宽不同（ASCII 1B vs ● 3B），按字符数映射。
    let value = "ab密码";
    let (display, _) = display_value_masked(
        &EditState::from_init(value.into(), "".into(), 0, false),
        Some('●'),
    );
    for off in [0, 1, 2, 5, 8, value.len()] {
        let d = value_to_display_byte(value, &display, off);
        let back = display_to_value_byte(&display, value, d);
        assert_eq!(back, off, "off {off} roundtrip");
    }
    // '密' 的 value 字节 2..5 → display 字节 6..9（前 2 个掩码字符）。
    assert_eq!(value_to_display_byte(value, &display, 2), 6);
    assert_eq!(value_to_display_byte(value, &display, 5), 9);
}

#[test]
fn set_composition_empty_clears_composition() {
    // 空串 = 取消 composition：set_composition("") 应清掉 composition（设 None），
    // 而不是存一个零宽空 composition（FFI 文档约定「传空串 = 取消」）。
    let mut e = EditState::from_init("ab".into(), "".into(), 0, false);
    set_composition(&mut e, "ni", 1);
    assert!(e.composition.is_some());
    set_composition(&mut e, "", 1);
    assert!(e.composition.is_none(), "empty text clears composition");
    // display_value 随之返 None 区间（无下划线 / 候选窗）。
    let (_display, range) = display_value(&e);
    assert!(range.is_none());
}

// core 是 cdylib，不能 extern 调宿主剪贴板（Unity GUIUtility.systemCopyBuffer），故走
// host callback 注册：测试注册一对 Rust fn（匹配 ClipboardSetFn/GetFn 签名）做内存中
// round-trip，不依赖真实系统剪贴板。剪贴板测试共享全局 callback 槽 + 全局测试 buffer，
// 须串行（cargo test 默认多线程并行）——用 CLIP_TEST_LOCK 把所有剪贴板测试串成独占段，
// 防并发注册/读写互踩。锁取 poison-tolerant 访问（前测 panic 不连坐后测）。

use std::sync::atomic::Ordering;
use std::sync::Mutex;

/// 串行所有剪贴板测试（共享全局 callback + 测试 buffer，必须独占）。
static CLIP_TEST_LOCK: Mutex<()> = Mutex::new(());

/// 测试用剪贴板内容（test_set 写 / test_get 读）。
static TEST_CLIP: Mutex<String> = Mutex::new(String::new());

/// test_get 把剪贴板内容 leak 成 'static 切片返回稳定指针——host 须持有缓冲区至下次 get
/// （见 ClipboardGetFn 契约）；测试小量 leak 可接受，避免 dangling / static_mut_refs lint。
static TEST_GET_BYTES: Mutex<&'static [u8]> = Mutex::new(&[]);

/// test_get 写回泄漏字节长度（'static 切片 len 在 leak 时固定，存一份供 read 校验）。
static TEST_GET_LEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// host 「写剪贴板」回调：拷贝 (ptr,len) 进 TEST_CLIP。返 0。
unsafe extern "C" fn test_set(ptr: *const u8, len: usize) -> i32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    *TEST_CLIP.lock().unwrap() = String::from_utf8_lossy(bytes).into_owned();
    0
}

/// host 「读剪贴板」回调：把 TEST_CLIP 内容 leak 一份返稳定指针 + len。返 0。
/// leak 进 TEST_GET_BYTES 持有 'static 引用防回收；长度另存 TEST_GET_LEN。
unsafe extern "C" fn test_get(out: *mut *mut u8, out_len: *mut usize) -> i32 {
    let s = TEST_CLIP.lock().unwrap().clone();
    let leaked: &'static [u8] = s.into_bytes().leak();
    TEST_GET_LEN.store(leaked.len(), Ordering::SeqCst);
    *TEST_GET_BYTES.lock().unwrap() = leaked;
    unsafe {
        *out = leaked.as_ptr() as *mut u8;
        *out_len = leaked.len();
    }
    0
}

/// 注册测试 callback 并取串行锁。返回锁 guard（测试体内持有）。结束时 register(None)
/// 清回调槽（下个剪贴板测试从干净态开始）。
fn clip_test_setup() -> std::sync::MutexGuard<'static, ()> {
    let g = CLIP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    *TEST_CLIP.lock().unwrap() = String::new();
    register_clipboard(Some(test_set), Some(test_get));
    g
}

#[test]
fn selected_text_returns_selection() {
    let _g = clip_test_setup();
    // value "hello", 选区 [0,3)="hel"。
    let mut e = EditState::from_init("hello".into(), "".into(), 0, false);
    e.anchor = 0;
    e.cursor = 3;
    assert_eq!(selected_text(&e), "hel");
}

#[test]
fn selected_text_empty_when_no_selection() {
    let _g = clip_test_setup();
    // 无选区（anchor==cursor）→ 空串。
    let e = EditState::from_init("hello".into(), "".into(), 0, false);
    assert_eq!(selected_text(&e), "");
}

#[test]
fn copy_selection_fills_clipboard() {
    let _g = clip_test_setup();
    let mut e = EditState::from_init("hello".into(), "".into(), 0, false);
    e.anchor = 0;
    e.cursor = 3;
    let s = copy_selection(&e);
    assert_eq!(s, "hel");
    // host callback 把 "hel" 写进 TEST_CLIP。
    assert_eq!(*TEST_CLIP.lock().unwrap(), "hel");
    assert_eq!(e.value, "hello", "copy does not mutate value");
}

#[test]
fn cut_selection_copies_and_deletes() {
    let _g = clip_test_setup();
    let mut e = EditState::from_init("hello".into(), "".into(), 0, false);
    e.anchor = 1;
    e.cursor = 4; // 选区 [1,4)="ell"
    assert!(cut_selection(&mut e, NodeKind::TextField));
    assert_eq!(e.value, "ho", "selection removed");
    assert_eq!(
        *TEST_CLIP.lock().unwrap(),
        "ell",
        "clipboard filled with cut text"
    );
    assert_eq!(e.cursor, 1, "cursor at selection start after cut");
}

#[test]
fn cut_selection_noop_without_selection() {
    let _g = clip_test_setup();
    let mut e = EditState::from_init("abc".into(), "".into(), 0, false);
    // 无选区 → delete_selection 返 false（cut 返 false），value 不变。
    assert!(!cut_selection(&mut e, NodeKind::TextField));
    assert_eq!(e.value, "abc");
}

#[test]
fn cut_selection_readonly_copies_but_does_not_delete() {
    // 照 HTML：readonly 允许 copy、禁止修改。Ctrl+X 在 readonly 字段上应复制选区
    // 到剪贴板，但不删 value、不发 ValueChanged（cut 返 false），选区保持不变。
    let _g = clip_test_setup();
    let mut e = EditState::from_init("hello".into(), "".into(), 0, true); // readonly
    e.anchor = 1;
    e.cursor = 4; // 选区 [1,4)="ell"
    assert!(
        !cut_selection(&mut e, NodeKind::TextField),
        "readonly cut returns false (no mutation)"
    );
    assert_eq!(e.value, "hello", "readonly value untouched");
    assert_eq!(e.anchor, 1, "selection anchor intact");
    assert_eq!(e.cursor, 4, "selection cursor intact");
    assert_eq!(*TEST_CLIP.lock().unwrap(), "ell", "copy still happened");
}

#[test]
fn paste_inserts_clipboard_at_cursor() {
    let _g = clip_test_setup();
    *TEST_CLIP.lock().unwrap() = "hi".into();
    let mut e = EditState::from_init("XY".into(), "".into(), 0, false);
    // 光标在末尾（from_init 默认）→ 插 "hi" → "XYhi"。
    assert!(paste(&mut e, NodeKind::TextField));
    assert_eq!(e.value, "XYhi");
}

#[test]
fn paste_replaces_selection() {
    let _g = clip_test_setup();
    *TEST_CLIP.lock().unwrap() = "QQ".into();
    let mut e = EditState::from_init("hello".into(), "".into(), 0, false);
    e.anchor = 1;
    e.cursor = 4; // 选区 "ell"
    assert!(paste(&mut e, NodeKind::TextField));
    assert_eq!(e.value, "hQQo", "selection replaced with clipboard");
}

#[test]
fn cut_then_paste_roundtrip() {
    let _g = clip_test_setup();
    // 完整 round-trip：cut 把 "ell" 进剪贴板 + 删（value "hello"→"ho"，cursor=1），
    // paste 在 cursor=1 插 "ell" → "h"+"ell"+"o" = "hello"（原地 cut/paste 还原原文，
    // insert_str(idx,...) 在 idx 前插入，把原本的 'o' 推到末尾）。这是 std insert_str 语义，
    // 非逻辑错误——cut 后 paste 在同一位置插回选区文本，等价于撤销删除。
    let mut e = EditState::from_init("hello".into(), "".into(), 0, false);
    e.anchor = 1;
    e.cursor = 4;
    assert!(cut_selection(&mut e, NodeKind::TextField));
    assert_eq!(e.value, "ho");
    assert!(paste(&mut e, NodeKind::TextField));
    assert_eq!(
        e.value, "hello",
        "paste at the cut gap reinserts text in place"
    );
    assert_eq!(e.cursor, 4, "cursor advanced past pasted text");
}

#[test]
fn paste_filters_non_numeric_for_number_field() {
    // NumberField 的 keydown-paste 渠道须与 textinput/IME-commit 共享输入 guard
    // （filter_number_field_text，三渠同语义防漂移）：粘贴 "1a2" → 滤掉 'a' → "12"。
    let _g = clip_test_setup();
    *TEST_CLIP.lock().unwrap() = "1a2".into();
    let mut e = EditState::from_init("".into(), "".into(), 0, false);
    assert!(paste(&mut e, NodeKind::NumberField));
    assert_eq!(e.value, "12", "paste 滤掉 'a' 仅留数字语法字符");
}

#[test]
fn read_clipboard_empty_when_unregistered() {
    // 注销 callback 后 read_clipboard 返空串（no-op，不 panic）。
    let _g = CLIP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    register_clipboard(None, None);
    assert_eq!(read_clipboard(), "");
    // 复原（注册回测试 callback，防污染后续测试）。
    register_clipboard(Some(test_set), Some(test_get));
}

#[test]
fn write_clipboard_noop_when_unregistered() {
    // 注销 set 后 write_clipboard 是 no-op（不 panic），TEST_CLIP 不被写。
    let _g = CLIP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    *TEST_CLIP.lock().unwrap() = "sentinel".into();
    register_clipboard(None, None);
    write_clipboard("ignored");
    assert_eq!(
        *TEST_CLIP.lock().unwrap(),
        "sentinel",
        "unregistered write is no-op"
    );
    register_clipboard(Some(test_set), Some(test_get));
}
