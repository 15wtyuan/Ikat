use crate::scene::dynamic::{set_inline_override, set_user_transform, unset_inline_override};
use crate::scene::node::{ControlState, NodeId, NodeKind, Scene};
use crate::transform::NodeTransform;

use super::dropdown::nth_option_text;
use super::edit::{display_value_masked, mask_char};
use super::roles::{
    find_child_by_role_recursive, find_child_by_slot, ROLE_LISTBOX, ROLE_TAB, SLOT_FILL,
    SLOT_THUMB, SLOT_VALUE,
};

/// Dropdown popup 展开位置（视口感知）→ 返回 user_transform 的 ty。
///
/// 三档：下方放得下 → 正下方（默认）；下方放不下且上方放得下 → 上翻（popup 底贴
/// select 顶）；两向都放不下 → 收缩（inline `max-height` 钉视口高 + `overflow-y:auto`
/// 接既有滚动机制，top 贴视口顶）。`viewport_h <= 0` = 无视口约束（恒下方，历史行为）。
///
/// **坐标空间**：视口判定用 `combo_world_y`（combobox 顶的世界 y——祖先滚动时 layout y
/// 与世界 y 劈叉，拿 layout y 判会在滚动页上误翻/误收缩）；返回的 ty 是 layout 空间量
/// （transform 作用于静态位再随祖先滚动渲染），故目标世界 y 经 `shift = combo_y −
/// combo_world_y` 折回 layout 空间再减 static_y。popup_h 读上帧 solve 的 layout_rect——
/// open 首帧为陈旧值，次帧收敛（错位帧几何在视口外，不可见）。收缩覆写在非收缩档/
/// 收起时 unset 回落作者 CSS（core 在 open 期拥有 popup inline 覆写是既有模式：display:block 同款）。
fn place_dropdown_popup_ty(
    scene: &mut Scene,
    popup: NodeId,
    combo_y: f32,
    combo_world_y: f32,
    sel_h: f32,
    static_y: f32,
    viewport_h: f32,
) -> f32 {
    let popup_h = scene.get(popup).map(|n| n.layout_rect.h).unwrap_or(0.0);
    let shift = combo_y - combo_world_y; // world → layout 折回（祖先滚动/变换位移）
    let below_y = combo_world_y + sel_h;
    if viewport_h <= 0.0 || below_y + popup_h <= viewport_h {
        let _ = unset_inline_override(scene, popup, "max-height");
        let _ = unset_inline_override(scene, popup, "overflow-y");
        return below_y + shift - static_y;
    }
    let above_y = combo_world_y - popup_h;
    if above_y >= 0.0 {
        let _ = unset_inline_override(scene, popup, "max-height");
        let _ = unset_inline_override(scene, popup, "overflow-y");
        return above_y + shift - static_y;
    }
    // 两向都放不下：收缩。max-height 走 inline（solve 可见，次帧 popup_h 收敛到视口高）。
    let _ = set_inline_override(scene, popup, "overflow-y:auto");
    let _ = set_inline_override(
        scene,
        popup,
        &format!("max-height:{}px", viewport_h.max(0.0)),
    );
    shift - static_y
}

/// 在 layout 阶段提前 measure 文本控件的显示文本 TextLayout，写入 `scene.text_layouts`。
///
/// 正常文本节点的 TextLayout 在 render 阶段 lazily 计算（`unwrap_or_else(measure_text)`），
/// 但文本控件需要在 render 前就拿到 TextLayout：光标命中测试需 glyph 位置、
/// 光标几何需行高/基线，都依赖已算好的 TextLayout。
///
/// 在 solve 后调用——此时 `layout_rect.w` 已就位（content width = rect.w - border - padding），
/// `ControlState` 已在之前步骤同步（值/placeholder 在 sync_control_visuals 无关——TextField
/// 视觉同步委托给 TextField beam，此处直接用 `display_value` 取显示文本）。
///
/// 写入的 TextLayout 不含 border/padding 偏移——偏移由 render 阶段统一 `bake_content_offset`，
/// 与正常 TextNode 路径（solve 测原始，render 烤偏移）保持一致。placeholder 场景（value 为空）：
/// 跳过缓存（continue），render 阶段的 lazy fallback 会用 placeholder 重测。
/// 布局阶段 measure 只是预热缓存：光标命中/几何依赖 TextLayout 在 render 前就位。
pub fn measure_text_controls(scene: &mut Scene, fonts: &crate::text::layout::FontTable) {
    let ids: Vec<NodeId> = scene
        .controls
        .0
        .iter()
        .filter(|(_, s)| {
            matches!(
                s,
                ControlState::TextField(_)
                    | ControlState::TextArea(_)
                    | ControlState::NumberField { .. }
            )
        })
        .map(|(&id, _)| id)
        .collect();
    for id in ids {
        let Some(n) = scene.get(id) else {
            continue;
        };
        let Some(
            ControlState::TextField(e)
            | ControlState::TextArea(e)
            | ControlState::NumberField { edit: e, .. },
        ) = scene.controls.get(id)
        else {
            continue;
        };
        // display_value 同时返回 composition 的 display 字节区间；measure 只需显示文本本身
        // （区间由 render underline / cursor_rect 消费）。-webkit-text-security 在此掩码
        // （缓存 layout 与 render 显示同源，掩码下 caret/hit 走 display 字节空间 + 换算）。
        let mask = n.style.text_security.map(mask_char);
        let (display, _comp_range) = display_value_masked(e, mask);
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
            s.effective_line_height(),
            s.letter_spacing,
            s.text_align,
            crate::style::resolved::control_wrap_control(s),
            Some(content_w),
            &stack,
            s.color,
            crate::text::rich::weight_from_font_weight(s.font_weight),
        );
        scene.text_layouts[id.index()] = Some(layout);
        if id.index() < scene.text_layout_versions.len() {
            scene.text_layout_versions[id.index()] += 1;
        }
    }
}

/// 把控件状态同步到**作者写的**子节点的 inline style。
///
/// 这是状态→视觉的单向桥：上层逻辑改 `ControlState`（交互/Tween/C# API），core 据此按
/// role/data-slot 定位作者子节点并写 inline override。inline 是 HTML 语义最高优先级
/// （> 动态规则 > base_style），与手写 `<div style="width:70%">` 完全等价——故复用
/// `set_inline_override` 而非另建并行机制。
///
/// 各控件映射：
/// - ProgressBar：`(value-min) / (max-min)`（ARIA 语义，min 缺省 0）→ `data-slot="fill"` 子节点的
///   `width:%`；`indeterminate`
///   时让权——清 fill 的 inline width（几何归作者 `[aria-indeterminate]` 规则）。
/// - Slider：`value` → `data-slot="fill"` 的 `width:%`（fill 可选）+ `data-slot="thumb"` 的
///   `user_transform.translate` = `(slider_w - thumb_w) × pct`（水平，扣自身宽的可滑动距离）
///   + `(slider_h - thumb_h)/2`（垂直居中）。thumb 几何取 slider 自身的 layout_rect（新结构
///   无 track 中间层，fill/thumb 是 slider 的兄弟子节点）。渲染/命中层位移，不触发 solve。
/// - Toggle / Radio：无映射——作者用 `[aria-checked]` 属性选择器表达选中态（运行时 rematch 匹配）。
/// - Dropdown：`open` → `role="listbox"` 的 `display` + 位置 transform；`selected_index` →
///   `data-slot="value"` 内嵌 TextNode 的文本。
///
/// 无控件状态（非 control 节点）→ no-op。tick 每帧对所有控件节点调一次（控件稀疏，代价可接受）。
/// 对找不到子节点的控件（作者漏写某部件）静默跳过——打包期结构契约会拦下缺夹。
///
/// `viewport_h` = stage 视口高（popup 视口感知定位用）。<= 0 = 无视口约束
/// （headless 测试默认）——popup 恒下方展开（历史行为）。
pub fn sync_control_visuals(scene: &mut Scene, id: NodeId, viewport_h: f32) {
    let Some(state) = scene.controls.get(id).cloned() else {
        return;
    };
    match state {
        ControlState::Progress {
            value,
            min,
            max,
            indeterminate,
        } => {
            if let Some(fill) = find_child_by_slot(scene, id, SLOT_FILL) {
                if indeterminate {
                    // 让权：indeterminate 期间 fill 几何全归作者 CSS（[aria-indeterminate]
                    // 规则 + keyframes marquee）。清掉 value 时代写入的 inline width——
                    // inline 语义优先级最高，残留会压死作者规则（跳过不写不够，必须清 bit）。
                    let _ = unset_inline_override(scene, fill, "width");
                } else {
                    // ARIA 填充比例：(value-min)/(max-min)。min=0（缺省）时与 value/max 等价。
                    let pct = if max > min {
                        ((value - min) / (max - min)).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    // width:N% — 用百分比，随 progress 宽度自适应（尺寸由布局决定）。
                    let _ = set_inline_override(scene, fill, &format!("width:{}%", pct * 100.0));
                }
            }
        }
        // Toggle/Radio：作者用 [aria-checked="true"] 属性选择器表达选中态，
        // core 不再 sync check 子节点的 display。
        ControlState::Toggle { .. } | ControlState::Radio { .. } => {}
        ControlState::Slider {
            value, min, max, ..
        } => {
            let pct = if max > min {
                ((value - min) / (max - min)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if let Some(fill) = find_child_by_slot(scene, id, SLOT_FILL) {
                let _ = set_inline_override(scene, fill, &format!("width:{}%", pct * 100.0));
            }
            // thumb 沿 slider 滑动。新结构无 track 中间层，几何取 slider 自身的 layout_rect。
            // 水平位移走 user_transform（渲染/命中层，不触发 solve，供高频拖拽每帧写）。公式对齐
            // RmlUi PositionBar：可滑动距离 = slider_w - thumb_w（扣自身宽），位置 = 该距离 × pct。
            // 垂直方向把 thumb 居中到 slider 中心（thumb 绝对定位后 align-items 不生效）。
            if let Some(thumb) = find_child_by_slot(scene, id, SLOT_THUMB) {
                // thumb 定位权归控件：inset/margin 逐帧归零，位移全权走下方 user_transform。
                // 作者给 thumb 写定位（负 top 居中、left 百分比等）会与控件位移叠加成双偏移；
                // class 规则每帧经 rematch 重放，归零同频执行（本函数时序在 rematch 后、
                // solve 前）。尺寸与外观声明不受影响；定位声明由打包器出静态警告提示所有权。
                if let Some(tn) = scene.get_mut(thumb) {
                    let ts = &mut tn.style.taffy_style;
                    let zero = taffy::style::LengthPercentageAuto::length(0.0);
                    ts.inset.top = zero;
                    ts.inset.right = zero;
                    ts.inset.bottom = zero;
                    ts.inset.left = zero;
                    ts.margin.top = zero;
                    ts.margin.right = zero;
                    ts.margin.bottom = zero;
                    ts.margin.left = zero;
                }
                let (slider_w, slider_h) = scene
                    .get(id)
                    .map(|n| (n.layout_rect.w, n.layout_rect.h))
                    .unwrap_or((0.0, 0.0));
                let (thumb_w, thumb_h) = scene
                    .get(thumb)
                    .map(|n| (n.layout_rect.w, n.layout_rect.h))
                    .unwrap_or((0.0, 0.0));
                let traversable = (slider_w - thumb_w).max(0.0);
                let center_y = (slider_h - thumb_h) / 2.0;
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
        // TextField/TextArea: 光标闪烁 timer 已实现（advance_cursor_blink，stage tick 驱动）；
        // cursor/selection/composition 的渲染同步（几何计算 + 绘制）仍 pending。
        ControlState::TextField(_) | ControlState::TextArea(_) => {}
        ControlState::Dropdown {
            selected_index,
            open,
            ..
        } => {
            // listbox display 切换：open → display:block，收起 → display:none。
            // 用 block（标准 CSS 弹出列表语义）而非 flex——option 是 display:block 的列表项，
            // block 容器让它们垂直堆叠（AI/人类可预测的标准 HTML 语义）。若用 flex 默认 row，
            // option 会横向排列，违背弹出列表的预期。listbox 递归定位（作者可能裹 wrapper）。
            if let Some(popup) = find_child_by_role_recursive(scene, id, ROLE_LISTBOX) {
                let decl = if open {
                    "display:block"
                } else {
                    "display:none"
                };
                let _ = set_inline_override(scene, popup, decl);
                // listbox 位置：出现在 combobox 正下方。taffy 对 absolute 节点的百分比 inset
                // 解析不可靠（containing-block height measure 时序），改用 user_transform 把
                // listbox 偏移到 combobox 底边（同 Slider thumb 定位模式：渲染/命中层，进
                // world_matrix）。CSS `top:100%` 被围栏丢弃后 absolute 节点回落 taffy 静态
                // 位置（≈ value 行之后；layout_rect.y 已含该偏移，且是页面绝对坐标）——
                // ty = 目标（layout 空间）− 静态 y，同空间相减；把静态 y 当父相对值
                // 去减会把弹层甩到页面顶端。视口翻转判定用世界 y（祖先滚动下 layout≠世界，
                // 详见 place_dropdown_popup_ty 坐标空间注）。
                let (combo_y, sel_h) = scene
                    .get(id)
                    .map(|n| (n.layout_rect.y, n.layout_rect.h))
                    .unwrap_or((0.0, 0.0));
                let combo_world_y = scene
                    .world_transforms
                    .get(id.index())
                    .map(|wt| wt[5])
                    .unwrap_or(combo_y);
                let static_y = scene.get(popup).map(|n| n.layout_rect.y).unwrap_or(0.0);
                let ty = if open {
                    place_dropdown_popup_ty(
                        scene,
                        popup,
                        combo_y,
                        combo_world_y,
                        sel_h,
                        static_y,
                        viewport_h,
                    )
                } else {
                    // 收起：清收缩覆写（下轮 open 按当帧视口重判），回零偏移。
                    let _ = unset_inline_override(scene, popup, "max-height");
                    let _ = unset_inline_override(scene, popup, "overflow-y");
                    0.0
                };
                let _ = set_user_transform(
                    scene,
                    popup,
                    NodeTransform {
                        translate: [0.0, ty],
                        ..Default::default()
                    },
                );
            }
            // value 显示选中 option 的文本：读第 selected_index 个 option 子节点的文本，
            // 写进 value slot 内嵌的 TextNode（作者写 `<div data-slot=value><span/></div>`）。
            // 越界 / 无 option → 清空。
            if let Some(value) = find_child_by_slot(scene, id, SLOT_VALUE) {
                let text = nth_option_text(scene, id, selected_index).unwrap_or_default();
                if let Some(tn) = scene.get(value).and_then(|n| {
                    n.children
                        .iter()
                        .find(|&&c| scene.get(c).is_some_and(|x| x.kind == NodeKind::TextNode))
                        .copied()
                }) {
                    scene.text_contents.insert(tn, text);
                    scene.get_mut(tn).unwrap().dirty_text = true;
                }
            }
        }
        // NumberField: 纯数值输入控件，无视觉子节点 sync（数值约束 clamp + step 量化在 FFI 读写门执行）。
        ControlState::NumberField { .. } => {}
        // TabList: aria-selected 由 synth_aria_value 合成；panel 显隐据 selected_index
        // + 各 tab 的 RoleInfo.aria_controls（panel id 串）切换——本 arm 实现 panel display。
        // panel 跨树（非 tablist 子，靠 aria-controls + id 关联），区别于 Dropdown listbox
        // （combobox 直接子）。复用 display:none 剪枝：非激活 panel "display:none" 强制隐藏；
        // 激活 panel unset inline display 回落作者 CSS——显隐所有权归控件，但激活态的
        // 布局方式（flex/grid/…）归作者，core 不覆写（浏览器 tab 库同语义：JS 只管
        // ''/none 切换，激活布局由作者样式表决定）。
        ControlState::TabList { selected_index } => {
            // 按 DOM 序遍历 role=tab 子节点（selected_index 是 tab 的序号）。clone children
            // 释放不可变借，供循环内 set_inline_override 取 &mut scene。
            let tab_ids: Vec<NodeId> = scene
                .get(id)
                .map(|n| n.children.clone())
                .unwrap_or_default()
                .into_iter()
                .filter(|&c| scene.roles.role_of(c) == Some(ROLE_TAB))
                .collect();
            for (i, &tab) in tab_ids.iter().enumerate() {
                // aria_controls 是 String，clone 出来释放对 roles 的不可变借，再 scope 内解析。
                // 多实例安全：组件展开多份时各实例的 tab 只命中本实例的 panel
                //（nearest LOOKUP_SCOPE 根内查找，不串全局首匹配）。
                let Some(panel_id_str) = scene.roles.get(tab).and_then(|r| r.aria_controls.clone())
                else {
                    continue; // tab 未写 aria-controls：无 panel 可切
                };
                let Some(panel) = scene.find_node_by_id_in_own_scope(tab, &panel_id_str) else {
                    continue; // panel id 解析不到（fence 期已校验 idref 存在，运行时动态缺则跳）
                };
                if i == selected_index {
                    // 激活：清 inline display（此前帧写的 none），回落作者 CSS 的 display
                    //（未声明则默认 block）。作者 flex/grid 布局不再被覆写。
                    let _ = unset_inline_override(scene, panel, "display");
                } else {
                    let _ = set_inline_override(scene, panel, "display:none");
                }
            }
        }
    }
}
