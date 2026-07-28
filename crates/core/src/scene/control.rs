//! 控件视觉子节点注入：instantiate 时给控件节点追加框架内部 `.loom-*` 子节点。
//!
//! 控件即容器模型——ProgressBar 注入 `.loom-fill`，Slider 注入 `track > fill` + thumb
//! （结构：slider → [track, thumb]，track → [fill]），Toggle/RadioButton 注入 `.loom-check`。
//! 这些子节点只携带保留 class（无 id_attr），绝不污染用户 id 命名空间（`Get<T>` 作用域查找
//! 不会误命中框架内部节点）。子节点是普通 Container（div），display 默认按 schema 铺底，
//! 与用户手写 `<div class="loom-fill">` 实例化结果一致。

use crate::input::{
    EventRecord, EVT_CHANGE_COMMITTED, EVT_CHECKED_CHANGED, EVT_SUBMITTED, EVT_VALUE_CHANGED,
};
use crate::scene::dynamic::{append_child, create_node, set_inline_override, set_user_transform};
use crate::scene::node::{
    Composition, ControlState, EditState, NodeFlags, NodeId, NodeKind, Scene,
};
use crate::scene::text_cursor::{hit_byte_offset, line_byte_ranges};
use crate::transform::NodeTransform;

const FILL: &str = "loom-fill";
const TRACK: &str = "loom-track";
const THUMB: &str = "loom-thumb";
const CHECK: &str = "loom-check";
const VALUE: &str = "loom-value";
/// Dropdown 浮层容器的保留 class。pub 供 render 层识别 open popup 子树（Task 11
/// 浮层渲染：open Dropdown 的 .loom-popup 子树跳出正常 DFS，末尾追加，mask=0）。
pub const POPUP: &str = "loom-popup";

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
/// composition 期间 value 仍掩码，但预提交 composition 文本不掩码（见 [`display_value`]）。
pub fn transform_display_value(kind: NodeKind, value: &str) -> String {
    match kind {
        NodeKind::PasswordField => value.chars().map(|_| '•').collect(),
        _ => value.to_string(),
    }
}

/// 返回 measure/render 应用的「显示文本」及其 composition 字节区间：value 经
/// [`transform_display_value`] 掩码后，把 composition 预提交文本拼到 composition.pos 处
/// （composition 本身不掩码——用户输拼音须可见）。
///
/// 返回 `(String, Option<(usize, usize)>)`：第一个是显示文本，第二个是 composition 在该
/// 显示文本里的字节区间 `[start, end)`（无 composition 或空 composition 时为 `None`）。
/// 这个区间是 render 下划线 / 光标几何 / IME 候选窗定位的统一真相源——对 PasswordField
/// 尤其关键：掩码后字节布局改变（每字符→1 个 '•'），原始 `comp.pos`（value 字节偏移）
/// 不再落在正确字符上，必须用此返回区间。
///
/// 标记子串模型（RmlUi/FairyGUI 共识）：composition 不是独立 buffer，而是拼进显示文本的
/// 一个段落，作为一个 text run 参与 measure/换行/光标几何。该段落在 render 时由 Task 12
/// 的 composition 分支按下划线区间绘制。无 composition 时等价于 `transform_display_value`。
///
/// PasswordField 的 char 对齐：掩码后 value 的字节长度改变（每个字符→1 个 '•'），故
/// composition.pos（基于原始 value 的字节偏移）不能直接索引掩码串。用字符计数对齐——
/// composition.pos 落在原始 value 的某字符边界，掩码串同位字符位置 = 该边界前的字符数。
/// composition 占据掩码串 `[pos_char, pos_char + composition_chars)` 字符位，再换算成字节区间。
pub fn display_value(e: &EditState, kind: NodeKind) -> (String, Option<(usize, usize)>) {
    let base = transform_display_value(kind, &e.value);
    let Some(c) = e.composition.as_ref() else {
        return (base, None);
    };
    let mut chars: Vec<char> = base.chars().collect();
    // composition.pos 钳到原始 value 的合法 UTF-8 字符边界（防后端传越界/中间字节 pos）。
    // value[..aligned] 仅在 aligned 是字符边界时合法；回退到最近 char 起始字节避免切片 panic。
    let mut aligned = c.pos.min(e.value.len());
    while aligned > 0 && !e.value.is_char_boundary(aligned) {
        aligned -= 1;
    }
    // 掩码改变字节长度（PasswordField 每字符→1 个 '•'），故按字符计数对齐：
    // value 边界前的字符数 = 掩码串里的对应字符位置。钳到掩码串当前长度内（防 pos 越界）。
    let pos_char = e.value[..aligned].chars().count();
    let insert_start_char = pos_char.min(chars.len());
    let comp_chars: Vec<char> = c.text.chars().collect();
    for (i, ch) in comp_chars.iter().enumerate() {
        // 插入点越界（composition.pos 在掩码串末尾之外）时追加到末尾，不丢字符。
        let at = (insert_start_char + i).min(chars.len());
        chars.insert(at, *ch);
    }
    let display: String = chars.iter().collect();
    if comp_chars.is_empty() {
        return (display, None);
    }
    // composition 在 display 串里的真实字节区间 [start, end)。对非 PasswordField 与 raw
    // comp.pos 等价；对 PasswordField 因掩码改变字节布局而不同——此区间让下划线 / 光标几何
    // 对齐预提交文本本身（而非误指某个圆点）。
    let comp_end_char = insert_start_char + comp_chars.len();
    let comp_start_byte = char_index_to_byte(&display, insert_start_char);
    let comp_end_byte = char_index_to_byte(&display, comp_end_char);
    (display, Some((comp_start_byte, comp_end_byte)))
}

/// 字符索引 → 字节偏移：返回 s 里第 `char_idx` 个字符的起始字节。`char_idx` 超出字符数
/// 时返回 `s.len()`（串末尾）。用于 [`display_value`] 把 composition 的字符区间换算成
/// 字节区间（multi-byte 字符下字符数 ≠ 字节数）。
fn char_index_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// 把 `e.value` 的字节偏移换算成 [`display_value`] 返回的显示串里的字节偏移。
///
/// 掩码（PasswordField 每字符→'•'）与 composition 拼接都按字符改变字节长度，
/// 故原始 value 字节偏移不能直接索引显示串——PasswordField value="ab"（2 字节）
/// 掩码成 "••"（6 字节），末尾光标（value byte 2）必须映射到显示串 byte 6，
/// 否则光标会落在第一个圆点之后而非第二个圆点之后。
///
/// 对齐单位是字符（掩码与拼接都在字符边界操作，绝不拆分多字节字符）：先取 value 的
/// 字符序号，再按 composition 拼接点平移到显示串字符序号，最后经 [`char_index_to_byte']
/// 落成字节偏移。非 PasswordField 显示串的 value 段与原值字节相同（identity），
/// 直接返回原偏移（composition 的下划线几何由 `comp_range` 单独驱动，不经此函数）。
///
/// `display` 须为 `display_value(e, kind)` 的第一个返回值（与 measure/render 同源）。
pub fn value_byte_to_display_byte(
    e: &EditState,
    kind: NodeKind,
    vbyte: usize,
    display: &str,
) -> usize {
    if kind != NodeKind::PasswordField {
        return vbyte;
    }
    let vc = e.value[..vbyte.min(e.value.len())].chars().count();
    // composition 拼在显示串的 comp.pos 字符位；value 字符序号越过拼接点后须加上
    // composition 字符数才得到显示串里的对应字符序号（与 display_value 的插入逻辑对齐）。
    let display_vc = match e.composition.as_ref() {
        Some(c) => {
            let mut p = c.pos.min(e.value.len());
            while p > 0 && !e.value.is_char_boundary(p) {
                p -= 1;
            }
            let splice_char = e.value[..p].chars().count();
            let comp_chars = c.text.chars().count();
            if vc <= splice_char {
                vc
            } else {
                vc + comp_chars
            }
        }
        None => vc,
    };
    char_index_to_byte(display, display_vc)
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
        NodeKind::Dropdown => {
            // select 设 position:relative 作 absolute containing block（同 Slider 模式），
            // 锚定 .loom-popup（position:absolute）相对 select 定位。
            let _ = set_inline_override(scene, id, "position:relative");
            let value = make_child(scene, VALUE);
            append_child(scene, id, value).expect("fresh child has no parent");
            // .loom-value 是 Container（div），文本渲染只对 TextNode 生效（render 按
            // NodeKind::TextNode 读 text_contents）。故注入一个 TextNode 子节点承载选中项
            // 文本——与用户手写 `<div class="loom-value"><span>...</span></div>` 同构。
            // sync_control_visuals 每帧把选中 option 文本写进这个 TextNode 的 text_contents。
            let value_text =
                create_node(scene, "span", "").expect("\"span\" is in the fence whitelist");
            append_child(scene, value, value_text).expect("fresh child has no parent");
            let popup = make_child(scene, POPUP);
            append_child(scene, id, popup).expect("fresh child has no parent");
            // popup 默认收起（display:none），展开由 sync_control_visuals（open=true）移除覆盖。
            let _ = set_inline_override(scene, popup, "display:none;position:absolute");
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

/// 取 select（Dropdown）的第 `n` 个 option 子节点的文本内容。
///
/// option 是 select 的 DOM 子节点（`NodeKind::OptionItem`），与框架注入的 `.loom-value` /
/// `.loom-popup`（Container）平级——按 NodeKind 过滤掉非 option 子节点，保证只数用户写的
/// `<option>`。文本可能在 option 自身的 `text_contents`（`<option>B</option>` 打包期把 content
/// 存进 side table），也可能在后代 TextNode（`<option><span>B</span></option>`），故递归收集
/// option 子树所有 TextNode 的文本，与 render 的文本采集口径一致。
///
/// 越界（n 超过 option 数）/ select 无 option → None。调用方据此显空（.loom-value 清空）。
pub fn nth_option_text(scene: &Scene, select: NodeId, n: usize) -> Option<String> {
    let children = scene.get(select)?.children.clone();
    let opt = children
        .into_iter()
        .filter(|&cid| {
            scene
                .get(cid)
                .is_some_and(|c| c.kind == NodeKind::OptionItem)
        })
        .nth(n)?;
    let mut buf = String::new();
    collect_subtree_text(scene, opt, &mut buf);
    Some(buf)
}

/// 递归收集 `id` 子树的全部文本：先取节点自身的 text_contents（option 自带 content 的常见路径），
/// 再 DFS 所有子节点。与 render 只渲染 TextNode 的口径一致——非 TextNode 的 text_contents
/// 不参与渲染，但 option 节点自身的 content 是打包期为非 TextNode 叶子存的源文本，这里一并
/// 收（option 几乎不含非 TextNode 子树，叠加不会重复）。
fn collect_subtree_text(scene: &Scene, id: NodeId, buf: &mut String) {
    if let Some(t) = scene.text_contents.get(&id) {
        buf.push_str(t);
    }
    if let Some(n) = scene.get(id) {
        for &c in n.children.clone().iter() {
            collect_subtree_text(scene, c, buf);
        }
    }
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
        // display_value 同时返回 composition 的 display 字节区间；measure 只需显示文本本身
        // （区间由 render underline / cursor_rect 消费）。
        let (display, _comp_range) = display_value(e, n.kind);
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
        // TextField/TextArea: 光标闪烁 timer 已实现（advance_cursor_blink，stage tick 驱动）；
        // cursor/selection/composition 的渲染同步（几何计算 + 绘制）仍 pending（Task 12）。
        ControlState::TextField(_) | ControlState::TextArea(_) => {}
        ControlState::Dropdown {
            selected_index,
            open,
            ..
        } => {
            // popup display 切换：open → display:flex，收起 → display:none。
            // 与 Toggle/Radio 的 check 切换同机制（inline 最高优先级）。
            if let Some(popup) = find_child_by_class(scene, id, POPUP) {
                let decl = if open { "display:flex" } else { "display:none" };
                let _ = set_inline_override(scene, popup, decl);
            }
            // value 显示选中 option 的文本：读第 selected_index 个 option 子节点的文本，
            // 写进 .loom-value 内的 TextNode（inject 时注入）。越界 / 无 option → 清空。
            if let Some(value) = find_child_by_class(scene, id, VALUE) {
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
        // NumberField: visuals injected via inject_control_children;
        // dynamic value sync (number constraint clamp) pending.
        ControlState::NumberField { .. } => {}
    }
}

/// 光标闪烁周期（秒）。stage tick 每帧累计，周期到翻转 cursor_visible。
/// 0.7s 对齐常见平台光标闪烁频率（~1.4Hz 全周期，0.7s 半周期 ON/OFF）。
const CURSOR_BLINK_PERIOD: f32 = 0.7;

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
        // TextField/TextArea: convert world pos to content-area-local coords
        // (subtract layout_rect offset + border+padding inset), then set cursor/anchor
        // via hit_byte_offset. TextLayout glyphs are in content-area-local space.
        ControlState::TextField(_) | ControlState::TextArea(_) => {
            if let Some(n) = scene.get(id) {
                let lr = n.layout_rect;
                let border_left = crate::render::resolve_lp(n.style.taffy_style.border.left);
                let padding_left = crate::render::resolve_lp(n.style.taffy_style.padding.left);
                let border_top = crate::render::resolve_lp(n.style.taffy_style.border.top);
                let padding_top = crate::render::resolve_lp(n.style.taffy_style.padding.top);
                let local_x = pos[0] - lr.x - border_left - padding_left;
                let local_y = pos[1] - lr.y - border_top - padding_top;
                on_text_pointer_down(scene, id, local_x, local_y);
            }
        }
        ControlState::Progress { .. } => {}
        // Dropdown/NumberField: pointer interaction pending Task 2.
        ControlState::Dropdown { .. } | ControlState::NumberField { .. } => {}
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

/// 文本控件 pointer-down：世界坐标已转为 content-area-local（减 layout_rect.xy + border+padding），
/// 用 hit_byte_offset 计算字节偏移，设 cursor=anchor=offset，重置闪烁 timer。
///
/// 无缓存 TextLayout（首帧尚无 measure）→ no-op。非 TextField/TextArea → no-op。
pub fn on_text_pointer_down(scene: &mut Scene, id: NodeId, local_x: f32, local_y: f32) {
    let value = match scene.controls.get(id) {
        Some(ControlState::TextField(e) | ControlState::TextArea(e)) => e.value.clone(),
        _ => return,
    };
    // 克隆 TextLayout 解借用冲突：text_layouts 不可变借 + controls 可变写。
    let Some(layout) = scene.text_layouts[id.index()].as_ref().cloned() else {
        return;
    };
    let ranges = line_byte_ranges(&layout, &value);
    let offset = hit_byte_offset(&layout, &ranges, local_x, local_y);
    if let Some(ControlState::TextField(e) | ControlState::TextArea(e)) = scene.controls.get_mut(id)
    {
        e.cursor = offset;
        e.anchor = offset;
        e.cursor_visible = true;
        e.cursor_timer = 0.0;
    }
}

/// 推进光标闪烁 timer（每帧由 Stage tick 调用，单一动画时钟不变量）。
///
/// 仅处理 TextField/TextArea 的 EditState：
/// - 有焦点：累计 cursor_timer += dt，每 CURSOR_BLINK_PERIOD (0.7s) 翻转 cursor_visible。
/// - 无焦点：cursor_visible = false（隐藏光标）。
///
/// 先取 `scene.focused_node` 副本再可变迭代 controls，避免对 scene 的双借冲突。
pub fn advance_cursor_blink(scene: &mut Scene, dt: f32) {
    let focused = scene.focused_node;
    for (&id, state) in scene.controls.0.iter_mut() {
        if let ControlState::TextField(e) | ControlState::TextArea(e) = state {
            if Some(id) == focused {
                e.cursor_timer += dt;
                if e.cursor_timer >= CURSOR_BLINK_PERIOD {
                    e.cursor_timer = 0.0;
                    e.cursor_visible = !e.cursor_visible;
                }
            } else {
                e.cursor_visible = false;
            }
        }
    }
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

// ── 文本编辑原语（pure functions over EditState） ──────────────────────────────
//
// Task 8：编辑内核。insert_text/delete_char/move_cursor/sanitize_value 是 Task 9
// （textinput channel）与 Task 10（control-key 路由）的底层原语。它们是纯函数——
// 仅读写 EditState（无 Scene 访问），故可独立单测。读写光标/锚点后由调用方决定是否
// 同步渲染（Task 12）。
//
// 不变量：cursor/anchor 必须永远落在合法 UTF-8 字符边界上（char 起始字节）。CJK 字符
// 占 3 字节，若停在中间字节则后续 str slice panic。下面三个边界助手保证所有偏移合法。
//
// max_length 按 UTF-8 字符数计（value.chars().count()），非字节——用户感知「字数」
// 而非内存占用，与 HTML maxlength 语义一致。0 = 无限。
//
// readonly 守卫：insert/delete 在 readonly=true 时 no-op 返 false（照 HTML disabled/readonly）。
//
// 单行 vs 多行：sanitize 按 NodeKind 分派——TextArea 保留换行（删 \r/\t），其余
// （TextField/PasswordField/SearchField/...）删 \n/\r/\t。paste/输入到单行框时换行被滤。

/// 向左找前一个 UTF-8 字符的起始字节（即 idx 左侧那个 char 的开头）。
///
/// backspace 删除左侧字符 / move-cursor 左移时用：cursor 在某 char 之后（落在该 char 的
/// 起始字节上），prev 边界 = 左侧那个 char 的起始字节。idx=0（无前驱）时返回 0。
///
/// 与 [`next_char_boundary`] 对称：后者从 idx+1 向右扫，本函数从 idx-1 向左扫——
/// 若直接从 idx 起扫则 idx 落在边界时会原地返回（delete/move 会 no-op，ASCII 场景全坏）。
fn prev_char_boundary(value: &str, idx: usize) -> usize {
    if idx == 0 {
        return 0;
    }
    let mut i = idx - 1;
    while i > 0 && !value.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// 向右找最近的 UTF-8 字符边界（下一个 char 起始字节，或末尾 len）。
///
/// delete（向前删）用：cursor 在某 char 之前，next 边界 = 该 char 结束字节 = 下一 char 起始。
/// 从 idx+1 起扫（idx 自身可能是边界，但删右侧需跨过当前 char）。
fn next_char_boundary(value: &str, idx: usize) -> usize {
    let mut i = idx + 1;
    while i < value.len() && !value.is_char_boundary(i) {
        i += 1;
    }
    i.min(value.len())
}

/// 把任意字节偏移 clamp 到合法 UTF-8 边界（向左回退到最近 char 起始字节）。
///
/// sanitize_value 在重写 value 后重对齐 cursor/anchor 用——旧偏移可能因 value 变短越界
/// 或落在 char 中间。先 clamp 到 [0, len]，再回退到 char 边界。
fn clamp_boundary(value: &str, idx: usize) -> usize {
    let mut i = idx.min(value.len());
    while i > 0 && !value.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// 按 NodeKind 净化字符串：TextArea 保留 `\n`（多行换行），其余控件删 `\n`/`\r`/`\t`。
///
/// 单行输入框（TextField/PasswordField/SearchField/...）不应含换行/制表符——
/// paste 带换行的多行文本进单行框时滤成单行（照 HTML 单行 input 行为）。
/// TextArea 保留 `\n`（用户可手动换行）但仍删 `\r`/`\t`（CR 与 TAB 在文本域内无意义）。
fn sanitize_str(kind: NodeKind, s: &str) -> String {
    match kind {
        NodeKind::TextArea => s.chars().filter(|&c| c != '\r' && c != '\t').collect(),
        _ => s
            .chars()
            .filter(|&c| !matches!(c, '\n' | '\r' | '\t'))
            .collect(),
    }
}

/// 在 cursor 处插入文本（若有选区则先删选区再插）。成功改动返回 true，否则 false。
///
/// 步骤：readonly → no-op；sanitize 输入（单行框滤换行）；空串插入 no-op；
/// max_length 校验须在 delete_selection 之前——按「删后长度 = 当前 - 选区 + 新增」算，
/// 超额则干净拒绝（不删选区、不改 value），否则被拒的插入会静默丢掉用户选区；
/// 有选区则 delete_selection；insert_str 后 cursor/anchor 同步到新末尾，
/// 重置光标闪烁 timer（显示光标）。返回 true 表示 value 已变（调用方据此产 change 事件）。
pub fn insert_text(e: &mut EditState, kind: NodeKind, text: &str) -> bool {
    if e.readonly {
        return false;
    }
    let text = sanitize_str(kind, text);
    if text.is_empty() {
        return false;
    }
    // max_length 校验在任何 mutation 之前：post-delete 长度 = 当前字符数 - 选区字符数 + 新增字符数。
    // selection_range 返回的字节区间必落在 char 边界上，可安全切片计字符数。
    if e.max_length > 0 {
        let (sel_b, sel_e) = e.selection_range();
        let sel_chars = e.value[sel_b..sel_e].chars().count();
        let cur = e.value.chars().count();
        let add = text.chars().count();
        if cur - sel_chars + add > e.max_length {
            return false;
        }
    }
    delete_selection(e);
    e.value.insert_str(e.cursor, &text);
    e.cursor += text.len();
    e.anchor = e.cursor;
    e.cursor_visible = true;
    e.cursor_timer = 0.0;
    true
}

/// 删除选区 [min(anchor,cursor), max]。无选区（anchor==cursor）时 no-op 返 false。
///
/// replace_range 删区间后 cursor=anchor=选区起点。供 insert_text（先删后插）与
/// delete_char（有选区时退化为删选区）复用。
pub fn delete_selection(e: &mut EditState) -> bool {
    let (b, end) = e.selection_range();
    if b == end {
        return false;
    }
    e.value.replace_range(b..end, "");
    e.cursor = b;
    e.anchor = b;
    true
}

/// 删一个字符。backspace=true 删左（cursor 前），false 删右（cursor 后）。有选区时删选区。
///
/// readonly → no-op。无选区时按方向用 prev/next 边界确定删除区间（保证跨多字节字符不 panic）。
/// 边界 case（cursor 在头/尾且方向越界）no-op 返 false。
pub fn delete_char(e: &mut EditState, _kind: NodeKind, backspace: bool) -> bool {
    if e.readonly {
        return false;
    }
    if e.anchor != e.cursor {
        return delete_selection(e);
    }
    if backspace && e.cursor > 0 {
        let nc = prev_char_boundary(&e.value, e.cursor);
        e.value.replace_range(nc..e.cursor, "");
        e.cursor = nc;
        e.anchor = nc;
        true
    } else if !backspace && e.cursor < e.value.len() {
        let end = next_char_boundary(&e.value, e.cursor);
        e.value.replace_range(e.cursor..end, "");
        e.anchor = e.cursor;
        true
    } else {
        false
    }
}

/// 移动光标一个字符。right=true 右移，false 左移。select=true 扩展选区（anchor 不动），
/// 否则折叠（cursor=anchor=新位）。跨越按 UTF-8 字符（非字节），保证停在 char 边界。
///
/// 重置光标闪烁 timer（移动后立显光标）。无返回值（光标移动必生效，无失败语义）。
pub fn move_cursor(e: &mut EditState, _kind: NodeKind, right: bool, select: bool) {
    let nc = if right {
        next_char_boundary(&e.value, e.cursor)
    } else {
        prev_char_boundary(&e.value, e.cursor)
    };
    e.cursor = nc;
    if !select {
        e.anchor = nc;
    }
    e.cursor_visible = true;
    e.cursor_timer = 0.0;
}

/// 按 NodeKind 净化 EditState.value（重写 value + 重对齐 cursor/anchor 到合法边界）。
///
/// 供 paste/FFI 设值后调用：外部注入的 value 可能含单行框不该有的换行，或 cursor 落在
/// char 中间。sanitize_str 重写 value 后用 clamp_boundary 把 cursor/anchor 回退到
/// 合法 char 边界（value 变短后旧偏移可能越界，clamp 到 [0,len] + char 边界）。
pub fn sanitize_value(e: &mut EditState, kind: NodeKind) {
    e.value = sanitize_str(kind, &e.value);
    e.cursor = clamp_boundary(&e.value, e.cursor);
    e.anchor = clamp_boundary(&e.value, e.anchor);
}

/// 推一条 EVT_VALUE_CHANGED@node（文本框值变更后调用）。payload 无额外字段——
/// 文本值变更不报新值进 EventRecord（文本框的 value 走 Get<T> 直读 ControlState，
/// 与 Slider 的 x=新值 不同）。对照 EVT_VALUE_CHANGED 现有约定：Slider 用 x 装新 float，
/// 文本框语义是「值已变」，业务通过 API 读当前值。
pub fn emit_value_changed(out: &mut Vec<EventRecord>, node: NodeId) {
    out.push(EventRecord {
        node_id: node.0,
        event_type: EVT_VALUE_CHANGED,
        click_count: 0,
        pad: [0, 0],
        touch_id: 0,
        x: 0.0,
        y: 0.0,
    });
}

/// 回车键处理（单行/多行分派）。
///
/// 单行框（TextField/PasswordField/SearchField/...）→ 不改 value，推一条 EVT_SUBMITTED@node
/// （照 HTML 单行 input Enter=表单提交语义）。TextArea → 插入 `\n`（insert_text）+
/// ValueChanged；不发 Submitted（多行框 Enter 是换行，非提交）。
///
/// readonly 单行框仍发 Submitted（提交是意图表达，不受只读限制）；readonly TextArea 的
/// insert_text 自身 no-op 返 false（不发 ValueChanged）。
pub fn line_break(e: &mut EditState, kind: NodeKind, out: &mut Vec<EventRecord>, node: NodeId) {
    match kind {
        NodeKind::TextArea => {
            if insert_text(e, kind, "\n") {
                emit_value_changed(out, node);
            }
        }
        _ => {
            out.push(EventRecord {
                node_id: node.0,
                event_type: EVT_SUBMITTED,
                click_count: 0,
                pad: [0, 0],
                touch_id: 0,
                x: 0.0,
                y: 0.0,
            });
        }
    }
}

// ── IME composition 原语（set/commit） ─────────────────────────────
//
// Task 13：IME 渠道。后端读平台 IME 的 compositionString 回灌 core——set_composition 存进
// EditState.composition，commit_composition 落定进 value。显示侧由 [`display_value`] 把
// composition 拼进显示文本（measure/render 同源），下划线由 Task 12 的 composition 分支画。
//
// composition.pos 是基于原始 value 的字节偏移（光标在 value 中的位置）。提交时把光标定位到
// composition.pos，再 insert_text 落定（insert_text 自带选区删除 + sanitize + max_length
// 校验，复用它保持与普通字符输入一致的落定语义）。

/// 设置 composition（后端读 IME compositionString 回灌）。pos 钳到 value 合法字节边界。
///
/// **空串 = 取消 composition**：`text` 为空时清掉 `e.composition`（设为 None），与 FFI 文档
/// 约定（传空串取消正在进行的 composition）一致，不存空 composition（否则 commit/render 会
/// 拿到一个零宽 composition，下游边界判断 `comp_end > comp_start` 退化）。
///
/// 非空 text 时重置光标闪烁 timer（显示光标）——与编辑原语（insert_text/move_cursor）一致，
/// 让用户在输入过程中能看到光标位置。连续 set_composition（每帧更新 composition string）是常态。
pub fn set_composition(e: &mut EditState, text: &str, pos: usize) {
    if text.is_empty() {
        e.composition = None;
        return;
    }
    let mut p = pos.min(e.value.len());
    // 钳到 UTF-8 字符边界：后端传的 pos 可能落在多字节字符中间，直接存会让下游
    // value[..pos] 切片 panic。回退到最近的 char 起始字节。
    while p > 0 && !e.value.is_char_boundary(p) {
        p -= 1;
    }
    e.composition = Some(Composition {
        text: text.to_string(),
        pos: p,
    });
    e.cursor_visible = true;
    e.cursor_timer = 0.0;
}

/// 提交 composition：把 composition.text 落定进 value（在 composition.pos 插入）。
///
/// 光标先定位到 composition.pos（并折叠选区），再调 insert_text 插 composition.text——
/// 复用 insert_text 的选区删除/sanitize/max_length 校验逻辑，保持与普通字符输入一致的
/// 落定语义。有 composition 且 value 改变时返 true，无 composition 返 false。
pub fn commit_composition(e: &mut EditState, kind: NodeKind) -> bool {
    let Some(comp) = e.composition.take() else {
        return false;
    };
    e.cursor = comp.pos;
    e.anchor = comp.pos;
    insert_text(e, kind, &comp.text)
}

// ── 剪贴板（host callback 注册模式） ──────────────────────────────
//
// core 是 cdylib，不能 extern 调宿主剪贴板（Unity GUIUtility.systemCopyBuffer / Win32
// clipboard）——宿主符号在 core 链接期不可解析，且 C# 宿主无法提供 linkable C 符号。
// 故由后端在启动时经 FFI `loomgui_register_clipboard` 注册一对 set/get 函数指针，core
// 经这两指针间接调。未注册时 write_clipboard no-op、read_clipboard 返空串（防宿主未接线
// 时 panic）。
//
// 内存契约：get 回调返的缓冲区由宿主持有（至少活到下次 get 调用）；core 立即拷成 String，
// 不释放（避免跨分配器 free）。set 回调收 (ptr,len)，宿主在调用期间拷走，ptr 不需宿主释放。

use std::sync::Mutex;

/// 宿主「写剪贴板」回调签名。收 (ptr,len) 指向合法 UTF-8 字节，宿主拷走；返 0=成功。
pub type ClipboardSetFn = unsafe extern "C" fn(*const u8, usize) -> i32;
/// 宿主「读剪贴板」回调签名。宿主写 (out_ptr,out_len)，缓冲区宿主持有（活到下次 get）；
/// 返 0=成功。非 0 / null ptr 视作空。
pub type ClipboardGetFn = unsafe extern "C" fn(*mut *mut u8, *mut usize) -> i32;

/// 注册的回调槽。Option：None = 未注册（no-op）。Mutex 包串行注册/读写的并发安全。
static CLIPBOARD_SET: Mutex<Option<ClipboardSetFn>> = Mutex::new(None);
static CLIPBOARD_GET: Mutex<Option<ClipboardGetFn>> = Mutex::new(None);

/// 注册宿主剪贴板回调（FFI 层 `loomgui_register_clipboard` 调）。传 None 可注销。
/// 重复注册覆盖旧值（测试需重注册）。后端应在 Stage 启动后尽早注册一次。
pub fn register_clipboard(set_fn: Option<ClipboardSetFn>, get_fn: Option<ClipboardGetFn>) {
    *CLIPBOARD_SET.lock().unwrap() = set_fn;
    *CLIPBOARD_GET.lock().unwrap() = get_fn;
}

/// 读剪贴板。未注册 get 回调 / 回调返非 0 / null ptr / 非 UTF-8 → 返空串（no-op 不 panic）。
/// 宿主缓冲区立即拷成 String（缓冲区宿主持有，见 [`ClipboardGetFn`] 契约）。
pub fn read_clipboard() -> String {
    // 拷出 fn 指针再解锁，回调在锁外调（防回调内再 lock 造成重入死锁）。
    let Some(get) = *CLIPBOARD_GET.lock().unwrap() else {
        return String::new();
    };
    let mut ptr: *mut u8 = std::ptr::null_mut();
    let mut len: usize = 0;
    // SAFETY: 宿主保证 rc=0 且 ptr 非空时 [ptr, ptr+len) 是合法字节切片。
    let rc = unsafe { get(&mut ptr, &mut len) };
    if rc != 0 || ptr.is_null() || len == 0 {
        return String::new();
    }
    // SAFETY: len 由宿主给出，已在上面非零校验；ptr 非空。
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// 写剪贴板。未注册 set 回调 → no-op（不 panic）。
pub fn write_clipboard(s: &str) {
    // 拷出 fn 指针再解锁，回调在锁外调。
    if let Some(set) = *CLIPBOARD_SET.lock().unwrap() {
        // SAFETY: 传 (ptr,len) 指向 s 的合法 UTF-8 字节，宿主在调用期间拷走。
        unsafe { set(s.as_ptr(), s.len()) };
    }
}

/// 当前选区文本（无选区 → 空串）。选区字节区间必落在 UTF-8 char 边界（selection_range
/// 返回的是 cursor/anchor 的 min/max，二者均由编辑原语维护在 char 边界）。
pub fn selected_text(e: &EditState) -> String {
    let (b, end) = e.selection_range();
    e.value[b..end].to_string()
}

/// 复制选区到剪贴板，返回选区文本。无选区 → 直接返空串，不碰剪贴板
///（照 HTML/浏览器：Ctrl+C 无选区时 no-op，不清空系统剪贴板）。
/// 有选区 → 写剪贴板并返回选区文本。不改 value（copy 是非破坏性）。
pub fn copy_selection(e: &EditState) -> String {
    if e.anchor == e.cursor {
        return String::new();
    }
    let s = selected_text(e);
    write_clipboard(&s);
    s
}

/// 剪切选区：先复制到剪贴板，再在非 readonly 时 [`delete_selection`]。返回 value 是否改变。
/// 照 HTML：readonly 不阻止 copy，但禁止修改——故复制永远发生，删除受 readonly 守卫
///（readonly 时 copy 后直接返 false，不动 value、不发 ValueChanged）。无选区时
/// copy 也是 no-op（见 [`copy_selection`]）。
/// `kind` 未使用（delete_selection 只动选区），保留参数为与 [`paste`] API 对称。
pub fn cut_selection(e: &mut EditState, _kind: NodeKind) -> bool {
    let s = selected_text(e);
    write_clipboard(&s); // 复制永远发生（readonly 允许 copy）
    if e.readonly {
        return false; // readonly 禁止删除
    }
    delete_selection(e)
}

/// 粘贴：读剪贴板后 [`insert_text`]（自带选区替换 + sanitize + max_length 校验）。
/// 返回 value 是否改变。readonly / 剪贴板空 / 超 max_length → no-op 返 false。
pub fn paste(e: &mut EditState, kind: NodeKind) -> bool {
    insert_text(e, kind, &read_clipboard())
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
    fn dropdown_injects_value_and_popup() {
        // select → [.loom-value, .loom-popup]：value 显示选中项文本，popup 是展开列表容器。
        // 模拟 instantiate：先把 select 加入 controls 表为 Dropdown 状态，再注入视觉子节点。
        let mut scene = Scene::default();
        let id = make_control(&mut scene, NodeKind::Dropdown);
        scene.controls.ensure(
            id,
            ControlState::Dropdown {
                selected_index: 0,
                open: false,
                value_lock: false,
            },
        );
        inject_control_children(&mut scene, id, NodeKind::Dropdown);
        let value = find_child_by_class(&scene, id, "loom-value").expect("loom-value injected");
        let popup = find_child_by_class(&scene, id, "loom-popup").expect("loom-popup injected");
        assert!(scene
            .get(value)
            .unwrap()
            .classes
            .iter()
            .any(|c| c == "loom-value"));
        assert_eq!(scene.get(value).unwrap().kind, NodeKind::Container);
        assert!(scene
            .get(popup)
            .unwrap()
            .classes
            .iter()
            .any(|c| c == "loom-popup"));
        assert_eq!(scene.get(popup).unwrap().kind, NodeKind::Container);
        // 两个子节点都不带 id（不污染用户命名空间）。
        assert!(scene.get(value).unwrap().id_attr.is_none());
        assert!(scene.get(popup).unwrap().id_attr.is_none());
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

    // ── sync_control_visuals：Dropdown（value 文本 + popup display 切换） ──
    //
    // select 的 selected_index → .loom-value 显示对应 option 文本；open → .loom-popup 的
    // display:flex/none 切换。option 文本取自 option 子树（自身 text_contents 或后代 TextNode）。
    // .loom-value 是 Container（div），文本落在其内部 TextNode 子节点（inject 时注入）。

    /// 建一个带 ControlInit 的 Dropdown，并挂 N 个 OptionItem 子节点（每个带文本）。
    /// 模拟 `<select><option>A</option><option>B</option>...</select>`。
    fn make_dropdown_with_options(
        scene: &mut Scene,
        option_texts: &[&str],
        selected: u32,
    ) -> NodeId {
        let id = create_node_from_template(
            scene,
            NodeKind::Dropdown,
            ResolvedStyle::default(),
            Some(ControlInit::Dropdown {
                selected_index: selected,
            }),
        );
        // 挂用户 option 子节点（instantiate 流程里 option 作为 select 的 DOM 子节点挂入）。
        for &t in option_texts {
            let opt = create_node_from_template(
                scene,
                NodeKind::OptionItem,
                ResolvedStyle::default(),
                None,
            );
            scene.text_contents.insert(opt, t.to_string());
            append_child(scene, id, opt).expect("option append");
        }
        id
    }

    /// 取 .loom-value 内的 TextNode 子节点的文本内容（inject 注入的 span）。
    fn value_text(scene: &Scene, select: NodeId) -> String {
        let value = find_child_by_class(scene, select, VALUE).expect("loom-value present");
        let text_node = scene
            .get(value)
            .unwrap()
            .children
            .iter()
            .find(|&&c| scene.get(c).is_some_and(|n| n.kind == NodeKind::TextNode))
            .copied()
            .expect("loom-value has a TextNode child");
        scene
            .text_contents
            .get(&text_node)
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn sync_dropdown_shows_selected_option_text_in_value() {
        // selected_index=1 → .loom-value 文本应是第 2 个 option 的文本（"B"）。
        let mut scene = Scene::default();
        let sel = make_dropdown_with_options(&mut scene, &["A", "B", "C"], 1);
        sync_control_visuals(&mut scene, sel);
        assert_eq!(
            value_text(&scene, sel),
            "B",
            "value shows selected option text"
        );
    }

    #[test]
    fn sync_dropdown_value_text_tracks_selected_index_change() {
        // 改 selected_index 后再 sync，.loom-value 文本随之更新。
        let mut scene = Scene::default();
        let sel = make_dropdown_with_options(&mut scene, &["A", "B", "C"], 0);
        sync_control_visuals(&mut scene, sel);
        assert_eq!(value_text(&scene, sel), "A");
        if let Some(ControlState::Dropdown { selected_index, .. }) = scene.controls.get_mut(sel) {
            *selected_index = 2;
        }
        sync_control_visuals(&mut scene, sel);
        assert_eq!(value_text(&scene, sel), "C", "re-sync after index change");
    }

    #[test]
    fn sync_dropdown_selected_index_out_of_range_yields_empty() {
        // selected_index 越界（无对应 option）→ .loom-value 文本为空（不 panic、不残留旧值语义由
        // 调用方保证；此处只验不 panic 且文本被写成空串）。
        let mut scene = Scene::default();
        let sel = make_dropdown_with_options(&mut scene, &["A", "B"], 0);
        if let Some(ControlState::Dropdown { selected_index, .. }) = scene.controls.get_mut(sel) {
            *selected_index = 99;
        }
        sync_control_visuals(&mut scene, sel);
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
            Some(ControlInit::Dropdown { selected_index: 0 }),
        );
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
        append_child(&mut scene, id, opt).expect("option append");
        sync_control_visuals(&mut scene, id);
        assert_eq!(
            value_text(&scene, id),
            "Deep",
            "collects text from option subtree"
        );
    }

    #[test]
    fn sync_dropdown_open_toggles_popup_display() {
        // open=true → popup display:flex；open=false → display:none。
        let mut scene = Scene::default();
        let sel = make_dropdown_with_options(&mut scene, &["A"], 0);
        // 默认 open=false
        sync_control_visuals(&mut scene, sel);
        let popup = find_child_by_class(&scene, sel, POPUP).expect("loom-popup present");
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
        sync_control_visuals(&mut scene, sel);
        assert_eq!(
            scene
                .get(popup)
                .unwrap()
                .inline_override
                .taffy_style
                .display,
            taffy::Display::Flex,
            "open → display:flex"
        );
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

    // ── 文本控件 pointer-down 光标命中 ──

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
            style.line_height,
            style.letter_spacing,
            style.text_align,
            style.white_space_nowrap,
            Some(content_w),
            &stack,
            style.color,
            crate::text::rich::weight_from_font_weight(style.font_weight),
        );
        scene.text_layouts[id.index()] = Some(layout);
        (scene, id)
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
        // 聚焦该节点
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
        // 不聚焦（focused_node = None）
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

    // ── on_pointer_down 世界→内容区坐标转换 ──
    //
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
            style.line_height,
            style.letter_spacing,
            style.text_align,
            style.white_space_nowrap,
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

    // ── 文本编辑原语（insert/delete/move + UTF-8 边界 + sanitize） ──
    //
    // Task 8：纯函数 over EditState（无 Scene 改动）。insert_text/delete_char/move_cursor
    // 是 Task 9（textinput channel）+ Task 10（control-key 路由）的编辑内核。UTF-8 边界
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

    // ── composition / display_value（Fix 1/3/4） ──

    #[test]
    fn password_field_composition_display_range_points_at_composition() {
        // PasswordField value "ab" → 掩码 "••"。composition "中" 插在 pos=1（ab 中间）→
        // 显示文本 "•中•"（composition 不掩码，用户输入拼音须可见）。composition 的 display
        // 字节区间必须指向 "中"，而不是某个圆点——这是 Fix 1 的核心：掩码改变字节布局后，
        // raw comp.pos(=1) 会落在第二个圆点上，下划线 / 光标会误指。
        let mut e = EditState::from_init("ab".into(), "".into(), 0, false);
        set_composition(&mut e, "中", 1);
        assert_eq!(e.composition.as_ref().unwrap().pos, 1);
        let (display, range) = display_value(&e, NodeKind::PasswordField);
        assert_eq!(display, "•中•", "masked value + visible composition");
        let (start, end) = range.expect("composition range present for PasswordField");
        assert_eq!(
            &display[start..end],
            "中",
            "range points at composition char, not a bullet"
        );
    }

    #[test]
    fn display_value_range_normal_field_matches_raw_comp_pos() {
        // 非 PasswordField：掩码不改变字节布局，display_value 返回的区间 = raw comp.pos..+len。
        // 回归锁：确保 char 对齐改造未坏掉普通文本框的常见路径。
        let mut e = EditState::from_init("ab".into(), "".into(), 0, false);
        set_composition(&mut e, "ni", 1);
        let (display, range) = display_value(&e, NodeKind::TextField);
        assert_eq!(display, "anib");
        let (start, end) = range.expect("composition range present");
        assert_eq!(&display[start..end], "ni");
    }

    #[test]
    fn display_value_no_composition_returns_none() {
        // 无 composition → range 为 None（render 不画下划线，cursor_rect 退回原始光标）。
        let e = EditState::from_init("ab".into(), "".into(), 0, false);
        let (display, range) = display_value(&e, NodeKind::TextField);
        assert_eq!(display, "ab");
        assert!(range.is_none());
    }

    #[test]
    fn set_composition_empty_clears_composition() {
        // Fix 3：空串 = 取消 composition。set_composition("") 应清掉 composition（设 None），
        // 而不是存一个零宽空 composition（FFI 文档约定「传空串 = 取消」）。
        let mut e = EditState::from_init("ab".into(), "".into(), 0, false);
        set_composition(&mut e, "ni", 1);
        assert!(e.composition.is_some());
        set_composition(&mut e, "", 1);
        assert!(e.composition.is_none(), "empty text clears composition");
        // display_value 随之返 None 区间（无下划线 / 候选窗）。
        let (_display, range) = display_value(&e, NodeKind::TextField);
        assert!(range.is_none());
    }

    // ── 剪贴板原语（copy/cut/paste + host callback 注册） ──
    //
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
}
