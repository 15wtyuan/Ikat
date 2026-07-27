//! 指针输入事件 + 多指针状态机。固定 5 槽（slot0=鼠标，slot1-4=触摸）。
//! 消费 PointerEvent[] + 命中 → 产 EventRecord[]。click 阈值 ~10px（鼠标）。
//! disabled 节点产 RollOver/Out 但不产 Down/Up/Click。

use crate::hit::hit_test;
use crate::scene::node::{ControlState, NodeFlags, NodeId, NodeKind, Scene};
use crate::scroll::{effective, SCROLL_THRESHOLD_MOUSE, SCROLL_THRESHOLD_TOUCH};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PointerEvent {
    pub kind: PointerKind,
    pub button: u8,
    pub pad: [u8; 2],
    pub touch_id: i32, // -1=鼠标主指 slots[0]；>=0=触摸 fingerId
    pub x: f32,
    pub y: f32,
}

/// 指针事件种类。repr(u8)：FFI 1 字节判别（PointerEvent 16B 紧凑布局，C# 对齐 byte）。
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerKind {
    Down = 0,
    Up = 1,
    Move = 2,
    Canceled = 3, // 触摸 TouchPhase.Canceled（鼠标无）
}

/// 键盘输入事件（FFI POD）。C# set_key_input 推一组；core process_keys 产 keydown/up EventRecord。
/// 8B：key_code@0(4) + modifiers@4(1) + is_down@5(1) + pad@6(2)。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KeyEvent {
    pub key_code: u32, // KeyCode 枚举值（Unity KeyCode 转 u32；core 不解释语义，只透传 + Tab 判定）
    pub modifiers: u8, // bit0=shift / bit1=ctrl / bit2=alt
    pub is_down: bool, // true=按下→keydown；false=松开→keyup
    pub pad: [u8; 2],
}

/// modifiers 位掩码（KeyEvent.modifiers）。
pub const MOD_SHIFT: u8 = 0x01;
pub const MOD_CTRL: u8 = 0x02;
pub const MOD_ALT: u8 = 0x04;

/// Tab 的 KeyCode 值（Unity KeyCode.Tab = 9）。core 内判定 Tab 导航用。
pub const KEY_TAB: u32 = 9;

/// 控制键 KeyCode 值。LoomInputCollector 用 `(uint)UnityEngine.KeyCode` 直传——core
/// 须匹配 Unity KeyCode 枚举的原值。数值源：`unity/package/Runtime/Public/LoomGUI.Types.cs`
/// 的 `KeyCode` enum（项目冻结的公共 API 镜像，注释明确「Values match Unity KeyCode enum」，
/// 且其 Tab=9 / Backspace=8 / Left=276 等与本仓既有 KEY_TAB 及 Unity 公开文档一致，互为佐证）。
/// 字母键 A-Z = 97-122（ASCII 小写区间，Unity KeyCode 同此）。Home/End 不在该 enum 内，
/// 取 Unity 公开文档的编辑键块值（UpArrow=273…LeftArrow=276…Home=278,End=279）。
pub const KEY_BACKSPACE: u32 = 8;
pub const KEY_RETURN: u32 = 13; // Unity KeyCode.Return（主回车；KeypadEnter=271 另算）
pub const KEY_ESCAPE: u32 = 27;
pub const KEY_DELETE: u32 = 127; // 前向删（Unity KeyCode.Delete）
pub const KEY_LEFT: u32 = 276; // LeftArrow
pub const KEY_RIGHT: u32 = 275; // RightArrow
pub const KEY_UP: u32 = 273; // UpArrow
pub const KEY_DOWN: u32 = 274; // DownArrow
pub const KEY_HOME: u32 = 278;
pub const KEY_END: u32 = 279;
pub const KEY_A: u32 = 97;
pub const KEY_C: u32 = 99;
pub const KEY_V: u32 = 118;
pub const KEY_X: u32 = 120;
pub const KEY_Z: u32 = 122;

/// 事件输出（FFI 扁平 POD）。event_type: 0=Down,1=Up,2=Move,3=Click,4=RollOver,5=RollOut。
/// +touch_id:i32 @8。pad[0]→click_count（20B 不变）。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EventRecord {
    pub node_id: u32,
    pub event_type: u8,
    pub click_count: u8, // 1 或 2（仅 Click 有意义，其余=0）
    pub pad: [u8; 2],
    pub touch_id: i32,
    pub x: f32,
    pub y: f32,
}

pub const EVT_DOWN: u8 = 0;
pub const EVT_UP: u8 = 1;
pub const EVT_MOVE: u8 = 2;
pub const EVT_CLICK: u8 = 3;
pub const EVT_ROLL_OVER: u8 = 4;
pub const EVT_ROLL_OUT: u8 = 5;
pub const EVT_DRAG_START: u8 = 6;
pub const EVT_DRAG_MOVE: u8 = 7;
pub const EVT_DRAG_END: u8 = 8;
pub const EVT_LONG_PRESS: u8 = 9;
pub const EVT_KEY_DOWN: u8 = 12;
pub const EVT_KEY_UP: u8 = 13;
pub const EVT_FOCUS_IN: u8 = 14;
pub const EVT_FOCUS_OUT: u8 = 15;
pub const EVT_TWEEN_COMPLETE: u8 = 16;

// 控件交互事件（22+）。payload 复用 EventRecord 现有字段（不扩 struct，ABI 安全）：
// - VALUE_CHANGED：x 装新 float 值（Slider 拖拽中逐值）。
// - CHECKED_CHANGED：pad[0] 装布尔（0=false/1=true；Toggle 翻转 / Radio 新选中）。
// - CHANGE_COMMITTED：x 装最终 float 值（Slider 松手，提交本次拖拽的终值）。
pub const EVT_VALUE_CHANGED: u8 = 22;
pub const EVT_CHECKED_CHANGED: u8 = 23;
pub const EVT_CHANGE_COMMITTED: u8 = 24;
/// TextField/Password/Search 单行框按 Enter 提交（TextArea 不发——Enter 插换行）。
/// payload 复用 EventRecord 现有字段（node_id 指向提交控件；x/y=0 无载荷）。
pub const EVT_SUBMITTED: u8 = 25;

const CLICK_THRESHOLD_MOUSE: f32 = 10.0; // per-axis click 容忍（鼠标）
const CLICK_THRESHOLD_TOUCH: f32 = 50.0; // per-axis click 容忍（触摸）
const DOUBLE_CLICK_TIME: f32 = 0.35; // 双击窗口秒
const MOVE_CANCEL_PX: f32 = 50.0; // Move 硬编码取消阈值（per-axis，mouse+touch 通用）
const DRAG_THRESHOLD_MOUSE: f32 = 2.0; // drag 启动阈值（鼠标）
const DRAG_THRESHOLD_TOUCH: f32 = 10.0; // drag 启动阈值（触摸）
const LONGPRESS_TRIGGER: f32 = 1.5; // 长按触发秒
/// drag_follow 占位 dt（process 未收真实 dt，假定 60fps；非 60fps 速度计算有偏差）。
const DRAG_FOLLOW_ASSUMED_DT: f32 = 0.016;

fn click_threshold(touch_id: i32) -> f32 {
    if touch_id == -1 {
        CLICK_THRESHOLD_MOUSE
    } else {
        CLICK_THRESHOLD_TOUCH
    }
}

fn drag_threshold(touch_id: i32) -> f32 {
    if touch_id == -1 {
        DRAG_THRESHOLD_MOUSE
    } else {
        DRAG_THRESHOLD_TOUCH
    }
}

/// scroll 手势触发阈值（mouse 8 / touch 20）。
/// 大于 drag 阈值（mouse 2 / touch 10）→ 两候选并存时 drag 通常先达。
fn scroll_threshold(touch_id: i32) -> f32 {
    if touch_id == -1 {
        SCROLL_THRESHOLD_MOUSE
    } else {
        SCROLL_THRESHOLD_TOUCH
    }
}

/// 候选让出后沿 parent 找下一个 effective 滚动祖先。
/// 从 `pane` 的 parent 起向上查（不含 pane 自身），首个 eff_x||eff_y 节点返。
/// 用于 V-only 容器遇水平手势时提升到外层可滚容器（嵌套让出）。
fn next_effective_ancestor(scene: &Scene, pane: NodeId) -> Option<NodeId> {
    let mut cur = scene.get(pane).and_then(|n| n.parent);
    while let Some(id) = cur {
        let n = match scene.get(id) {
            Some(n) => n,
            None => break,
        };
        if let Some(st) = scene.scroll.get(id) {
            let eff_x = effective(n.style.overflow_x, st.content_size.0, st.viewport_size.0);
            let eff_y = effective(n.style.overflow_y, st.content_size.1, st.viewport_size.1);
            if eff_x || eff_y {
                return Some(id);
            }
        }
        cur = n.parent;
    }
    None
}

/// 单触摸槽状态。slots[0]=鼠标主指（touch_id=-1 常驻），slots[1..4]=触摸。
#[derive(Debug, Clone)]
pub struct TouchSlot {
    pub touch_id: i32, // -1=鼠标主指/空闲触摸槽；>=0=触摸 fingerId
    pub last_pos: (f32, f32),
    pub is_down: bool,
    pub down_node: Option<NodeId>,
    pub down_pos: (f32, f32),
    pub last_hit: Option<NodeId>, // 本帧命中（hover_diff + is_pointer_on_ui 用）
    pub last_hovered_chain: Vec<NodeId>,
    pub touch_monitors: Vec<NodeId>, // capture 的节点（Move/Up 派发用）
    pub down_targets: Vec<NodeId>,   // Down 时填 [leaf, …祖先]
    pub control_target: Option<NodeId>, // Down 命中控件 → 记控件节点（Slider Move/Up 续推；Toggle/Radio 瞬时完成）
    pub click_cancelled: bool,          // Move>50 / CancelClick / Canceled 置
    pub last_click_time: f32,           // time_s（双击窗口）
    pub last_click_pos: (f32, f32),     // 上次 Click 位置
    pub last_click_button: u8,          // 上次 Click 键
    pub click_count: u8,                // 1→2→1 循环
    pub drag_testing: bool,             // Down 在 draggable 链上置 true
    pub dragging: bool,                 // DragStart 后置 true
    pub drag_target: Option<NodeId>, // down_targets 中最近 draggable（含 down_node）；None 无 drag
    pub down_time: f32,              // Down 时=time_s（longpress 用）
    pub longpress_fired: bool,       // 触发后置 true（本 press 不再发）
    pub longpress_cancelled: bool,   // 位移>50px 置 true（本 press 不再发）
    // scroll 手势仲裁（per-slot）。scroll-vs-drag 阈值赛跑 + 轴锁 + 嵌套让出提升。
    pub scroll_candidate: Option<NodeId>, // Down 沿 down_targets 找的最近 effective 滚动容器（待阈值判定）
    pub scroll_testing: bool,             // Down 时候选存在 → true，达阈值/让出/Up 后清
    pub scrolling_pane: Option<NodeId>,   // 已判定：本槽正滚动该容器
    pub scroll_gesture: u8,               // bit0=垂直手势（Y 位移） bit1=水平手势（X 位移）
    pub grip_dragging: bool,              // scrollbar grip 拖拽中（grip 不启 inertia）
    pub grip_grab_offset: (f32, f32),     // Down 时刻指针相对 thumb 中心的偏移（跟手拖拽不跳）
    pub scroll_down_pos: (f32, f32),      // Down 时刻 pos（scroll 阈值/跟手基准）
}

impl TouchSlot {
    fn new_slot() -> Self {
        Self {
            touch_id: -1,
            last_pos: (0.0, 0.0),
            is_down: false,
            down_node: None,
            down_pos: (0.0, 0.0),
            last_hit: None,
            last_hovered_chain: Vec::new(),
            touch_monitors: Vec::new(),
            down_targets: Vec::new(),
            control_target: None,
            click_cancelled: false,
            last_click_time: 0.0,
            last_click_pos: (0.0, 0.0),
            last_click_button: 0,
            click_count: 1,
            drag_testing: false,
            dragging: false,
            drag_target: None,
            down_time: 0.0,
            longpress_fired: false,
            longpress_cancelled: false,
            scroll_candidate: None,
            scroll_testing: false,
            scrolling_pane: None,
            scroll_gesture: 0,
            grip_dragging: false,
            grip_grab_offset: (0.0, 0.0),
            scroll_down_pos: (0.0, 0.0),
        }
    }
}

/// 多指针状态机（固定 5 槽）。slots[0]=鼠标，slots[1..4]=触摸。
pub struct PointerState {
    pub slots: Vec<TouchSlot>,
    pub time_s: f32, // 累积时间（Stage::advance_time 累加；双击窗口用）
}

impl Default for PointerState {
    fn default() -> Self {
        let mut slots = Vec::with_capacity(5);
        slots.push(TouchSlot::new_slot()); // slot 0 = 鼠标主指
        for _ in 0..4 {
            slots.push(TouchSlot::new_slot()); // slot 1..4 = 触摸
        }
        Self { slots, time_s: 0.0 }
    }
}

/// target 起沿 Node.parent 至 root 收集 NodeId 链（含 target）；target=None → 空链。
fn ancestor_chain(scene: &Scene, target: Option<NodeId>) -> Vec<NodeId> {
    let mut chain = Vec::new();
    let mut cur = target;
    while let Some(id) = cur {
        let parent = match scene.get(id) {
            Some(n) => {
                chain.push(id);
                n.parent
            }
            None => break, // 防御（脏 scene）
        };
        cur = parent;
    }
    chain
}

/// 设焦点为 new（None=清除焦点）。发 FocusOut@旧焦点 + FocusIn@新焦点。
/// 模块级 pub(crate) 自由函数——process（click-to-focus）+ process_keys（Tab）+ Stage（pending_focus_request）共用。
/// 写 scene.focused_node + node.focused 标志 + 推 FocusOut/FocusIn 进 out。old==new → no-op。
pub(crate) fn focus_node(scene: &mut Scene, new: Option<NodeId>, out: &mut Vec<EventRecord>) {
    let old = scene.focused_node;
    if old == new {
        return; // 无变化不发
    }
    if let Some(o) = old {
        if let Some(n) = scene.get_mut(o) {
            n.interaction.flags.remove(NodeFlags::FOCUSED);
        }
        out.push(EventRecord {
            node_id: o.0,
            event_type: EVT_FOCUS_OUT,
            click_count: 0,
            pad: [0, 0],
            touch_id: 0,
            x: 0.0,
            y: 0.0,
        });
    }
    if let Some(n) = new {
        if let Some(node) = scene.get_mut(n) {
            node.interaction.flags.insert(NodeFlags::FOCUSED);
        }
        out.push(EventRecord {
            node_id: n.0,
            event_type: EVT_FOCUS_IN,
            click_count: 0,
            pad: [0, 0],
            touch_id: 0,
            x: 0.0,
            y: 0.0,
        });
    }
    scene.focused_node = new;
}

/// DFS 先序收集 tabindex>=0 且非 disabled 节点，分桶 positive(>0)/zero(==0)。
fn dfs_collect(
    scene: &Scene,
    id: NodeId,
    positive: &mut Vec<(i32, NodeId)>,
    zero: &mut Vec<NodeId>,
) {
    let n = match scene.get(id) {
        Some(n) => n,
        None => return,
    };
    if !n.interaction.flags.contains(NodeFlags::DISABLED) {
        match n.interaction.tabindex {
            Some(t) if t > 0 => positive.push((t, id)),
            Some(0) => zero.push(id),
            _ => {} // Some(-1)/None 不进链
        }
    }
    // 先序：先本节点入桶，再递归 children（DOM 序）
    let children: Vec<NodeId> = n.children.clone();
    for c in children {
        dfs_collect(scene, c, positive, zero);
    }
}

/// 构造 Tab 链——正整数按 tabindex 升序（stable，同值保 DFS 序），后接 0 组（DFS 序）。
/// 照 DOM：正整数显式序先于 0 组。tabindex=-1/None/disabled 不进。
pub(crate) fn build_tab_chain(scene: &Scene) -> Vec<NodeId> {
    let mut positive: Vec<(i32, NodeId)> = Vec::new();
    let mut zero: Vec<NodeId> = Vec::new();
    for root in &scene.roots {
        dfs_collect(scene, *root, &mut positive, &mut zero);
    }
    positive.sort_by_key(|(t, _)| *t); // stable：同 tabindex 保 DFS 序
    positive.into_iter().map(|(_, n)| n).chain(zero).collect()
}

/// 从 current 焦点算 Tab/Shift+Tab 下一个焦点。空链 → None。
/// current 在链中 → 取前/后；不在（或 None）→ 链首(forward)/链尾(backward)；边界 wrap。
fn next_focus(chain: &[NodeId], current: Option<NodeId>, backward: bool) -> Option<NodeId> {
    if chain.is_empty() {
        return None;
    }
    let idx = current.and_then(|c| chain.iter().position(|n| *n == c));
    let next = match idx {
        Some(i) => {
            let len = chain.len();
            let ni = if backward {
                (i + len - 1) % len
            } else {
                (i + 1) % len
            };
            chain[ni]
        }
        None => {
            // current 不在链 → forward 取链首，backward 取链尾
            if backward {
                *chain.last().unwrap()
            } else {
                chain[0]
            }
        }
    };
    Some(next)
}

/// 处理键盘事件——keydown/up（有焦点才发）+ Tab/Shift+Tab 导航（focus_node）+
/// 控制键路由到 focused TextField/TextArea 编辑内核。
/// Stage tick 在 pointer process 后调。Tab 被导航消费（不发 keydown，照 DOM Tab 默认动作=移焦）。
/// 控制键（Backspace/Delete/方向/Home/End/Enter/Escape/ctrl+A）被编辑内核消费（不发 keydown，
/// 照现有 Tab 消费模式）；非控制键（如无 ctrl 的字母）透传 keydown（字符输入走 textinput 通道）。
pub(crate) fn process_keys(scene: &mut Scene, keys: &[KeyEvent], out: &mut Vec<EventRecord>) {
    for ke in keys {
        let focused = scene.focused_node;
        if ke.is_down && ke.key_code == KEY_TAB {
            // Tab 导航
            let chain = build_tab_chain(scene);
            if chain.is_empty() {
                continue; // 无可聚焦节点 → Tab 无操作（不发 keydown）
            }
            let backward = (ke.modifiers & MOD_SHIFT) != 0;
            let next = next_focus(&chain, focused, backward);
            focus_node(scene, next, out); // 发 FocusOut(旧)+FocusIn(新)
            continue; // Tab 被消费，不发 keydown
        }
        // 控制键路由：keydown 且 focused 是 TextField/TextArea → 路由到编辑内核。
        // 路由键 consume（continue，不发 keydown）。非路由键（含 keyup）透传到下面的普通分支。
        // 借用模式（同 stage.rs textinput / on_text_pointer_down）：先不可变读 kind，再 controls.get_mut。
        if ke.is_down {
            if let Some(fid) = focused {
                let kind = scene.get(fid).map(|n| n.kind);
                let is_text = matches!(kind, Some(NodeKind::TextField) | Some(NodeKind::TextArea));
                if is_text {
                    let ctrl = ke.modifiers & MOD_CTRL != 0;
                    let shift = ke.modifiers & MOD_SHIFT != 0;
                    let mut routed = false;
                    let mut changed = false;
                    // 单次 controls 可变借跑除 Escape 外的全部路由（这些只动 EditState / 推 out，
                    // 不碰 scene）。Escape 单独处理——它调 focus_node(scene,None) 要 &mut scene，
                    // 与此处 controls 借冲突。
                    if let Some(ControlState::TextField(e) | ControlState::TextArea(e)) =
                        scene.controls.get_mut(fid)
                    {
                        match ke.key_code {
                            KEY_BACKSPACE => {
                                if crate::scene::control::delete_char(e, kind.unwrap(), true) {
                                    changed = true;
                                }
                                routed = true;
                            }
                            KEY_DELETE => {
                                if crate::scene::control::delete_char(e, kind.unwrap(), false) {
                                    changed = true;
                                }
                                routed = true;
                            }
                            KEY_LEFT => {
                                crate::scene::control::move_cursor(e, kind.unwrap(), false, shift);
                                routed = true;
                            }
                            KEY_RIGHT => {
                                crate::scene::control::move_cursor(e, kind.unwrap(), true, shift);
                                routed = true;
                            }
                            KEY_HOME => {
                                e.cursor = 0;
                                if !shift {
                                    e.anchor = 0;
                                }
                                routed = true;
                            }
                            KEY_END => {
                                e.cursor = e.value.len();
                                if !shift {
                                    e.anchor = e.cursor;
                                }
                                routed = true;
                            }
                            // ctrl+A 全选（单选一条，避免与「无 ctrl 的 A」冲突——后者透传 keydown）。
                            KEY_A if ctrl => {
                                e.anchor = 0;
                                e.cursor = e.value.len();
                                routed = true;
                            }
                            // Enter：line_break 只用 e/kind/out/fid（不碰 scene），
                            // 可在此 controls 借内调用。
                            KEY_RETURN => {
                                crate::scene::control::line_break(e, kind.unwrap(), out, fid);
                                routed = true;
                            }
                            // TODO(Task 14): ctrl+C/X/V 需 clipboard FFI。暂不路由——
                            // ctrl+C/X/V 会透传 keydown（业务可自行读 selection 做剪贴板）。
                            _ => {}
                        }
                    }
                    // Escape 要改 scene.focused_node（focus_node 借 &mut scene），故放在 controls 借释放后。
                    if !routed && ke.key_code == KEY_ESCAPE {
                        focus_node(scene, None, out); // blur：发 FocusOut
                        routed = true;
                    }
                    if changed {
                        crate::scene::control::emit_value_changed(out, fid);
                    }
                    if routed {
                        continue; // 控制键被消费，不发 keydown
                    }
                }
            }
        }
        // 普通 keydown/up：有焦点才发（无焦点丢弃）
        if let Some(n) = focused {
            let event_type = if ke.is_down { EVT_KEY_DOWN } else { EVT_KEY_UP };
            out.push(EventRecord {
                node_id: n.0,
                event_type,
                click_count: 0,
                pad: [ke.modifiers, 0],       // pad[0]=modifiers
                touch_id: ke.key_code as i32, // touch_id 复用装 key_code（u32 bit pattern → i32）
                x: 0.0,
                y: 0.0,
            });
        }
    }
}

impl PointerState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 鼠标主指 last_pos（stage.rs tick_and_render 的 hit_test 用）。
    pub fn last_pos(&self) -> (f32, f32) {
        self.slots[0].last_pos
    }

    /// 任一活跃槽命中非根节点 → UI 挡住。
    pub fn is_pointer_on_ui(&self, scene: &Scene) -> bool {
        let root_id = scene.roots.first().copied();
        for slot in &self.slots {
            if let Some(hit) = slot.last_hit {
                if Some(hit) != root_id {
                    return true;
                }
            }
        }
        false
    }

    /// 加 touch monitor（去重）。touch_id 找槽（鼠标=-1→slot0）；找不到槽→no-op（Down 前调无效）。
    /// 仅加指定槽，不做 -1 广播。
    pub fn add_touch_monitor(&mut self, touch_id: i32, node: NodeId) {
        let slot_idx = if touch_id == -1 {
            0
        } else {
            match (1..self.slots.len()).find(|&i| self.slots[i].touch_id == touch_id) {
                Some(i) => i,
                None => return,
            }
        };
        let slot = &mut self.slots[slot_idx];
        if !slot.touch_monitors.contains(&node) {
            slot.touch_monitors.push(node);
        }
    }

    /// 移除 touch monitor（从所有槽）。用 retain 移除（Vec 无 sentinel 需求，retain 更简且无遍历期偏移）。
    pub fn remove_touch_monitor(&mut self, node: NodeId) {
        for slot in &mut self.slots {
            // touch_monitors 是 Vec<NodeId>，用 retain 移除（Vec 无 sentinel 需求，retain 更简且无遍历期偏移）
            slot.touch_monitors.retain(|n| *n != node);
        }
    }

    /// 外部取消待 click：置对应槽 click_cancelled。
    /// 触摸槽满 / 未找到 → no-op。下个 Up 的 click_test 见 cancelled → 不发 Click + reset。
    pub fn cancel_click(&mut self, touch_id: i32) {
        let slot_idx = if touch_id == -1 {
            0
        } else {
            match (1..self.slots.len()).find(|&i| self.slots[i].touch_id == touch_id) {
                Some(i) => i,
                None => return,
            }
        };
        self.slots[slot_idx].click_cancelled = true;
    }

    /// 找/分配槽。鼠标(touch_id=-1)恒 slots[0]；触摸按 touch_id 找，找不到→分配首个空闲。
    /// 返回 slot index；找不到（触摸槽满）→ None。
    /// 触摸槽在任意事件（Move/Down/Up）分配（触摸可 Move 先于 Down 合成），Up 后释放（slot_idx>0 置 touch_id=-1）。
    fn find_or_alloc_slot(&mut self, ev: &PointerEvent) -> Option<usize> {
        if ev.touch_id == -1 {
            return Some(0); // 鼠标主指
        }
        // 找已占触摸槽
        for i in 1..self.slots.len() {
            if self.slots[i].touch_id == ev.touch_id {
                return Some(i);
            }
        }
        // 分配首个空闲触摸槽
        for i in 1..self.slots.len() {
            if self.slots[i].touch_id == -1 {
                self.slots[i].touch_id = ev.touch_id;
                return Some(i);
            }
        }
        None // 触摸槽满 → 丢弃
    }

    /// 消费本帧输入 → 产 EventRecord 序列。
    pub fn process(&mut self, scene: &mut Scene, events: &[PointerEvent]) -> Vec<EventRecord> {
        let mut out: Vec<EventRecord> = Vec::new();
        let time_s = self.time_s; // 本地化避免 &mut self 与 &mut slot 借用冲突
                                  // stationary hover follow：本帧无事件的活跃槽刷新命中 + hover diff
                                  // （静止光标下元素移动 → :hover 刷新）。
        let used_touch_ids: Vec<i32> = events.iter().map(|e| e.touch_id).collect();
        for i in 0..self.slots.len() {
            let active = i == 0 || self.slots[i].touch_id >= 0;
            if active && !used_touch_ids.contains(&self.slots[i].touch_id) {
                self.slots[i].last_hit = hit_test(scene, self.slots[i].last_pos);
                Self::hover_diff_slot(&mut self.slots[i], scene, &mut out);
            }
        }
        // longpress tick：每帧跑（含空事件 tick，此处先于 is_empty early-return）。
        // is_down 槽按住 ≥1.5s 且未取消 → 发一次 EVT_LONG_PRESS（与 Click 独立）。
        // longpress 取消靠 Move 臂：Move 超过 MOVE_CANCEL_PX(50) 时置 longpress_cancelled。
        for i in 0..self.slots.len() {
            let active = i == 0 || self.slots[i].touch_id >= 0;
            if !active {
                continue;
            }
            let slot = &mut self.slots[i];
            if slot.is_down && !slot.longpress_fired && !slot.longpress_cancelled {
                if let Some(n) = slot.down_node {
                    if scene
                        .get(n)
                        .is_some_and(|node| !node.interaction.flags.contains(NodeFlags::DISABLED))
                        && time_s - slot.down_time >= LONGPRESS_TRIGGER
                    {
                        slot.longpress_fired = true;
                        out.push(EventRecord {
                            node_id: n.0,
                            event_type: EVT_LONG_PRESS,
                            click_count: 0,
                            pad: [0, 0],
                            touch_id: slot.touch_id,
                            x: slot.last_pos.0,
                            y: slot.last_pos.1,
                        });
                    }
                }
            }
        }
        if events.is_empty() {
            self.recompute_hovered(scene);
            self.recompute_active(scene);
            return out;
        }
        for ev in events {
            let slot_idx = match self.find_or_alloc_slot(ev) {
                Some(i) => i,
                None => continue,
            };
            let slot = &mut self.slots[slot_idx];
            let prev_pos = slot.last_pos; // scroll 跟手 delta = new - prev
            slot.last_pos = (ev.x, ev.y);
            let hit = hit_test(scene, slot.last_pos);
            slot.last_hit = hit;
            let touch_id = ev.touch_id;
            match ev.kind {
                PointerKind::Move => {
                    // 按住中位移>50（per-axis，硬编码，mouse+touch 通用）→ 取消 click + longpress。
                    if slot.is_down {
                        let dx = slot.last_pos.0 - slot.down_pos.0;
                        let dy = slot.last_pos.1 - slot.down_pos.1;
                        if dx.abs() > MOVE_CANCEL_PX || dy.abs() > MOVE_CANCEL_PX {
                            slot.click_cancelled = true;
                            slot.longpress_cancelled = true;
                        }
                    }
                    // 控件 Move：Slider 拖拽中 → 跟随指针更新 value（scroll/drag 已被占据手势抑制）。
                    if slot.is_down {
                        if let Some(cid) = slot.control_target {
                            out.extend(crate::scene::control::on_pointer_move(
                                scene,
                                cid,
                                [ev.x, ev.y],
                            ));
                        }
                    }
                    // scroll 阈值赛跑（drag/scroll 都未判定时）。scene 此处只读（查 effective）。
                    if slot.is_down
                        && slot.scroll_testing
                        && slot.scrolling_pane.is_none()
                        && !slot.dragging
                    {
                        if let Some(pane_id) = slot.scroll_candidate {
                            if let Some(n) = scene.get(pane_id) {
                                let (eff_x, eff_y) = match scene.scroll.get(pane_id) {
                                    Some(st) => (
                                        effective(
                                            n.style.overflow_x,
                                            st.content_size.0,
                                            st.viewport_size.0,
                                        ),
                                        effective(
                                            n.style.overflow_y,
                                            st.content_size.1,
                                            st.viewport_size.1,
                                        ),
                                    ),
                                    None => (false, false),
                                };
                                let dx = (slot.last_pos.0 - slot.scroll_down_pos.0).abs();
                                let dy = (slot.last_pos.1 - slot.scroll_down_pos.1).abs();
                                if eff_y && dy > 0.0 {
                                    slot.scroll_gesture |= 1;
                                }
                                if eff_x && dx > 0.0 {
                                    slot.scroll_gesture |= 2;
                                }
                                let thr = scroll_threshold(touch_id);
                                if dx >= thr || dy >= thr {
                                    // 轴锁判定：V-only 容器遇水平更大手势让出；H-only 对称；Both 都跟。
                                    let lock_ok = if eff_x && eff_y {
                                        true
                                    } else if eff_y {
                                        !(dx > dy) // V-only：水平位移更大则让出
                                    } else if eff_x {
                                        !(dy > dx) // H-only：垂直位移更大则让出
                                    } else {
                                        false
                                    };
                                    if lock_ok {
                                        slot.scrolling_pane = Some(pane_id);
                                        slot.click_cancelled = true; // scroll-start 取消 click（滚动非点击）
                                        slot.scroll_testing = false;
                                        slot.drag_target = None; // 抑制 drag（互斥：scroll 赢）
                                        slot.drag_testing = false;
                                    } else {
                                        // 让出 → 提升到下一可滚祖先；无祖先可提升 → 停 scroll_testing
                                        slot.scroll_candidate =
                                            next_effective_ancestor(scene, pane_id);
                                        if slot.scroll_candidate.is_none() {
                                            slot.scroll_testing = false;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // drag 检测（仅 draggable 链）。scroll 赢时 drag_target 已清 → 不启动；drag 先达时清 scroll 候选（互斥）。
                    if slot.is_down && slot.drag_testing && !slot.dragging {
                        if let Some(tgt) = slot.drag_target {
                            let dx = slot.last_pos.0 - slot.down_pos.0;
                            let dy = slot.last_pos.1 - slot.down_pos.1;
                            let t = drag_threshold(touch_id);
                            if dx.abs() > t || dy.abs() > t {
                                slot.dragging = true;
                                slot.click_cancelled = true; // drag 必取消 click
                                slot.scroll_testing = false; // drag 赢 → 清 scroll（互斥）
                                slot.scroll_candidate = None;
                                out.push(EventRecord {
                                    node_id: tgt.0,
                                    event_type: EVT_DRAG_START,
                                    click_count: 0,
                                    pad: [0, 0],
                                    touch_id,
                                    x: ev.x,
                                    y: ev.y,
                                });
                            }
                        }
                    }
                    if slot.dragging {
                        if let Some(tgt) = slot.drag_target {
                            out.push(EventRecord {
                                node_id: tgt.0,
                                event_type: EVT_DRAG_MOVE,
                                click_count: 0,
                                pad: [0, 0],
                                touch_id,
                                x: ev.x,
                                y: ev.y,
                            });
                        }
                    }
                    // scrolling_pane 已判定 → 跟手 drag_follow（写 scene.scroll）。
                    // grip_dragging → grip 位置驱动 scroll_pos（非 delta 跟手）。
                    // 复制 scrolling_pane 出 slot 解借用冲突（slot &mut 与 scene.scroll.get_mut &mut）。
                    // delta = 本帧 pos - 上帧 pos（prev_pos 在 last_pos 覆盖前捕获）。
                    let scrolling_pane = slot.scrolling_pane;
                    if let Some(pane) = scrolling_pane {
                        if slot.grip_dragging {
                            // grip 拖拽：指针在 track 内的比例 → scroll_pos
                            // 容器可能在拖拽中被 remove_node 删（动态树）：pane 不 live 时
                            // 中断本次 grip 处理 + 清 scrolling_pane（防下帧再进此臂 panic）。
                            let lr = match scene.get(pane) {
                                Some(n) => n.layout_rect,
                                None => {
                                    slot.scrolling_pane = None;
                                    slot.grip_dragging = false;
                                    continue;
                                }
                            };
                            if let Some(s) = scene.scroll.get_mut(pane) {
                                let pe = slot.last_pos;
                                let grab = slot.grip_grab_offset;
                                // 跟手拖拽：thumb 中心 = 指针 - 抓取偏移（保持按下点在 thumb 上的相对位置）。
                                // thumb_top = 中心 - 半尺寸；perc = thumb_top / 可移动范围（track - thumb）；scroll_pos = perc * overlap。
                                // 旧实现把指针当 thumb 参考点且分母用 min_thumb（错），点击后第一次 Move 列表瞬移。
                                if slot.scroll_gesture & 1 != 0 {
                                    // 垂直 thumb
                                    let track_h = lr.h;
                                    let thumb_h = (s.viewport_size.1
                                        * (s.viewport_size.1 / s.content_size.1))
                                        .max(crate::scroll::MIN_THUMB_SIZE)
                                        .min(track_h);
                                    let thumb_top = (pe.1 - grab.1) - lr.y - thumb_h * 0.5;
                                    let range = (track_h - thumb_h).max(1.0);
                                    let perc = (thumb_top / range).clamp(0.0, 1.0);
                                    s.scroll_pos.1 = (perc * s.overlap.1).clamp(0.0, s.overlap.1);
                                }
                                if slot.scroll_gesture & 2 != 0 {
                                    // 水平 thumb
                                    let track_w = lr.w;
                                    let thumb_w = (s.viewport_size.0
                                        * (s.viewport_size.0 / s.content_size.0))
                                        .max(crate::scroll::MIN_THUMB_SIZE)
                                        .min(track_w);
                                    let thumb_left = (pe.0 - grab.0) - lr.x - thumb_w * 0.5;
                                    let range = (track_w - thumb_w).max(1.0);
                                    let perc = (thumb_left / range).clamp(0.0, 1.0);
                                    s.scroll_pos.0 = (perc * s.overlap.0).clamp(0.0, s.overlap.0);
                                }
                            }
                        } else if let Some(s) = scene.scroll.get_mut(pane) {
                            // 触屏拖动：手指位移 → scroll_pos 反向（下拖看上方 = scroll_pos 减），
                            // 与 apply_wheel 一致（wheel delta.y>0 → scroll_pos 减）。
                            // design y 向下（ScreenToDesign）→ 下拖 design delta.y>0 → scroll_pos 应减（反之方向反 + 越界回弹）。
                            let scroll_delta =
                                (prev_pos.0 - slot.last_pos.0, prev_pos.1 - slot.last_pos.1);
                            s.drag_follow(scroll_delta, DRAG_FOLLOW_ASSUMED_DT);
                        }
                    }
                    Self::hover_diff_slot(slot, scene, &mut out);
                    // Move 派发：有 monitor 产 Move@monitor，无 monitor 不产
                    for m in &slot.touch_monitors {
                        out.push(EventRecord {
                            node_id: m.0,
                            event_type: EVT_MOVE,
                            click_count: 0,
                            pad: [0, 0],
                            touch_id,
                            x: ev.x,
                            y: ev.y,
                        });
                    }
                }
                PointerKind::Down => {
                    // grip 命中优先于 scroll 候选（scrollbar 最上层）
                    if let Some(grip) = crate::hit::hit_scrollbar_grip(scene, (ev.x, ev.y)) {
                        slot.grip_dragging = true;
                        slot.scrolling_pane = Some(grip.0);
                        slot.click_cancelled = true;
                        slot.scroll_down_pos = (ev.x, ev.y);
                        slot.is_down = true;
                        slot.down_pos = (ev.x, ev.y);
                        slot.scroll_gesture = if grip.1 == 0 { 1 } else { 2 };
                        // 抓取偏移 = 指针相对 thumb 中心（跟手拖拽：保持按下点在 thumb 上的相对位置，
                        // 不把 thumb 顶端/中心跳到指针处——否则点击后第一次 Move 列表瞬移）。
                        let thumb = if grip.1 == 0 {
                            crate::scroll::v_thumb_rect(scene, grip.0)
                        } else {
                            crate::scroll::h_thumb_rect(scene, grip.0)
                        };
                        slot.grip_grab_offset = match thumb {
                            Some(r) => ((ev.x - (r.x + r.w * 0.5)), (ev.y - (r.y + r.h * 0.5))),
                            None => (0.0, 0.0),
                        };
                        Self::hover_diff_slot(slot, scene, &mut out);
                        continue; // grip 不走 drag/scroll/click 候选
                    }
                    slot.is_down = true;
                    slot.down_pos = (ev.x, ev.y);
                    slot.down_node = hit;
                    slot.down_targets = ancestor_chain(scene, hit); // [leaf,…祖先]
                    slot.click_cancelled = false; // 新按下重置
                                                  // drag/longpress 初始化
                    slot.down_time = time_s;
                    slot.longpress_fired = false;
                    slot.longpress_cancelled = false;
                    // drag_target = down_targets 中最近 draggable（叶子优先，含 down_node）；disabled 跳过。
                    slot.drag_target = slot
                        .down_targets
                        .iter()
                        .find(|&&n| {
                            scene.get(n).is_some_and(|node| {
                                node.interaction.draggable
                                    && !node.interaction.flags.contains(NodeFlags::DISABLED)
                            })
                        })
                        .copied();
                    slot.drag_testing = slot.drag_target.is_some();
                    slot.dragging = false;
                    // scroll 候选——沿 down_targets（leaf 优先）找最近 effective 滚动容器。
                    slot.scroll_candidate = None;
                    slot.scroll_testing = false;
                    slot.scrolling_pane = None;
                    slot.scroll_gesture = 0;
                    slot.scroll_down_pos = (ev.x, ev.y);
                    {
                        let mut cur = hit;
                        while let Some(id) = cur {
                            let n = match scene.get(id) {
                                Some(n) => n,
                                None => break,
                            };
                            let (eff_x, eff_y) = match scene.scroll.get(id) {
                                Some(st) => (
                                    effective(
                                        n.style.overflow_x,
                                        st.content_size.0,
                                        st.viewport_size.0,
                                    ),
                                    effective(
                                        n.style.overflow_y,
                                        st.content_size.1,
                                        st.viewport_size.1,
                                    ),
                                ),
                                None => (false, false),
                            };
                            if eff_x || eff_y {
                                slot.scroll_candidate = Some(id);
                                break;
                            }
                            cur = n.parent;
                        }
                        slot.scroll_testing = slot.scroll_candidate.is_some();
                    }
                    // 控件交互：命中（含 .loom-* 子）向上找控件 → on_pointer_down。
                    // Slider 占据手势：抑制 scroll（拖 thumb 不让祖先滚动）+ 记 control_target。
                    if let Some(cid) = crate::scene::control::find_control_at(scene, hit) {
                        slot.control_target = Some(cid);
                        if crate::scene::control::occupies_gesture(scene, cid) {
                            slot.scroll_testing = false;
                            slot.scroll_candidate = None;
                        }
                        out.extend(crate::scene::control::on_pointer_down(
                            scene,
                            cid,
                            [ev.x, ev.y],
                        ));
                    }
                    // click-to-focus：pointer-down 命中 tabindex>=0 节点 → 聚焦（照 DOM）。
                    // 沿 down_targets（leaf 优先，同 drag_target 模式）找最近可聚焦非 disabled 节点。
                    // 不可聚焦/`-1` → 不夺焦（照 DOM：点空白不 blur）。
                    let focus_target = slot
                        .down_targets
                        .iter()
                        .find(|&&n| {
                            scene.get(n).is_some_and(|node| {
                                !node.interaction.flags.contains(NodeFlags::DISABLED)
                                    && matches!(node.interaction.tabindex, Some(t) if t >= 0)
                            })
                        })
                        .copied();
                    if let Some(t) = focus_target {
                        focus_node(scene, Some(t), &mut out);
                    }
                    if let Some(n) = hit {
                        if scene.get(n).is_some_and(|node| {
                            !node.interaction.flags.contains(NodeFlags::DISABLED)
                        }) {
                            out.push(EventRecord {
                                node_id: n.0,
                                event_type: EVT_DOWN,
                                click_count: 0,
                                pad: [0, 0],
                                touch_id,
                                x: ev.x,
                                y: ev.y,
                            });
                        }
                    }
                    Self::hover_diff_slot(slot, scene, &mut out);
                }
                PointerKind::Up | PointerKind::Canceled => {
                    if ev.kind == PointerKind::Canceled {
                        slot.click_cancelled = true; // Canceled 隐式 CancelClick（不发 Click + reset）
                    }
                    // 控件 Up：Slider 松手（含 Canceled）→ 清 dragging + 提交 ChangeCommitted。
                    if let Some(cid) = slot.control_target {
                        out.extend(crate::scene::control::on_pointer_up(scene, cid));
                    }
                    // drag 中 Up/Canceled → DragEnd
                    if slot.dragging {
                        if let Some(tgt) = slot.drag_target {
                            out.push(EventRecord {
                                node_id: tgt.0,
                                event_type: EVT_DRAG_END,
                                click_count: 0,
                                pad: [0, 0],
                                touch_id,
                                x: ev.x,
                                y: ev.y,
                            });
                        }
                    }
                    // scrolling_pane 中 Up（非 Canceled）→ begin_inertia；Canceled 不启惯性。grip 拖拽不启惯性。
                    // 复制 scrolling_pane 出 slot 解借用冲突。
                    let scrolling_pane_up = slot.scrolling_pane;
                    if ev.kind == PointerKind::Up {
                        if let Some(pane) = scrolling_pane_up {
                            if !slot.grip_dragging {
                                if let Some(s) = scene.scroll.get_mut(pane) {
                                    s.begin_inertia(touch_id >= 0); // is_touch
                                }
                            }
                        }
                    }
                    slot.is_down = false;
                    // grip_dragging 时 hit 为 sentinel（scene.nodes 越界），跳过 EVT_UP/EVT_CLICK（grip Up 不产这些事件）。
                    if !slot.grip_dragging {
                        if let Some(n) = hit {
                            if scene.get(n).is_some_and(|node| {
                                !node.interaction.flags.contains(NodeFlags::DISABLED)
                            }) {
                                out.push(EventRecord {
                                    node_id: n.0,
                                    event_type: EVT_UP,
                                    click_count: 0,
                                    pad: [0, 0],
                                    touch_id,
                                    x: ev.x,
                                    y: ev.y,
                                });
                                if let Some(target) = Self::click_test(slot, scene, hit) {
                                    if scene.get(target).is_some_and(|node| {
                                        !node.interaction.flags.contains(NodeFlags::DISABLED)
                                    }) {
                                        let count = Self::bump_click_count(slot, ev.button, time_s);
                                        out.push(EventRecord {
                                            node_id: target.0,
                                            event_type: EVT_CLICK,
                                            click_count: count,
                                            pad: [0, 0],
                                            touch_id,
                                            x: ev.x,
                                            y: ev.y,
                                        });
                                    }
                                } else {
                                    // click_test 返 None（位移超阈值/cancelled）→ 重置双击窗口
                                    slot.last_click_time = 0.0;
                                    slot.click_count = 1;
                                }
                            }
                        }
                    }
                    // monitor 的 Up 直派（去重：monitor != hit）
                    for m in &slot.touch_monitors {
                        if Some(*m) != hit {
                            out.push(EventRecord {
                                node_id: m.0,
                                event_type: EVT_UP,
                                click_count: 0,
                                pad: [0, 0],
                                touch_id,
                                x: ev.x,
                                y: ev.y,
                            });
                        }
                    }
                    slot.touch_monitors.clear();
                    slot.down_targets.clear();
                    slot.control_target = None;
                    slot.down_node = None;
                    slot.drag_testing = false;
                    slot.dragging = false;
                    slot.drag_target = None;
                    // 清 scroll 仲裁字段
                    slot.scroll_testing = false;
                    slot.scrolling_pane = None;
                    slot.scroll_candidate = None;
                    slot.scroll_gesture = 0;
                    slot.grip_dragging = false; // grip Up 清（不惯性）
                    Self::hover_diff_slot(slot, scene, &mut out);
                    if slot_idx > 0 {
                        slot.touch_id = -1; // 释放触摸槽（鼠标不释放）
                    }
                }
            }
        }
        self.recompute_hovered(scene);
        self.recompute_active(scene);
        out
    }

    /// click 目标判定。返 Click 应派发的节点；None=不产 Click。
    /// cancelled（Move>50/CancelClick/Canceled）→ None。位移 per-axis 超阈值 → None。
    /// 否则优先 down_targets[0]（按下叶，"still on stage"≈索引有效）；叶失效则沿当前 hit 祖先兜底。
    fn click_test(slot: &TouchSlot, scene: &Scene, current_hit: Option<NodeId>) -> Option<NodeId> {
        if slot.click_cancelled {
            return None;
        }
        let t = click_threshold(slot.touch_id);
        let dx = slot.last_pos.0 - slot.down_pos.0;
        let dy = slot.last_pos.1 - slot.down_pos.1;
        if dx.abs() > t || dy.abs() > t {
            return None;
        }
        if let Some(&leaf) = slot.down_targets.first() {
            if scene.get(leaf).is_some() {
                return Some(leaf);
            }
        }
        let mut cur = current_hit;
        while let Some(id) = cur {
            let parent = match scene.get(id) {
                Some(n) => n.parent,
                None => break,
            };
            if slot.down_targets.contains(&id) {
                return Some(id);
            }
            cur = parent;
        }
        None
    }

    /// 双击 clickCount 累进：350ms + per-axis 位置 + 同键 → 1→2→1 循环。
    /// 返回本次 click_count 并更新 slot 的 last_click_* 状态。
    /// time_s 作参数传（非读 self.time_s），避免 &mut self 与 &mut slot 借用冲突。
    fn bump_click_count(slot: &mut TouchSlot, button: u8, time_s: f32) -> u8 {
        let t = click_threshold(slot.touch_id);
        let within_time = time_s - slot.last_click_time < DOUBLE_CLICK_TIME;
        let within_pos = (slot.last_pos.0 - slot.last_click_pos.0).abs() < t
            && (slot.last_pos.1 - slot.last_click_pos.1).abs() < t;
        let same_button = slot.last_click_button == button;
        let count = if within_time && within_pos && same_button {
            if slot.click_count == 2 {
                1
            } else {
                slot.click_count + 1
            } // 1→2→1 循环
        } else {
            1
        };
        slot.click_count = count;
        slot.last_click_time = time_s;
        slot.last_click_pos = slot.last_pos;
        slot.last_click_button = button;
        count
    }

    /// per-slot hover diff：产 RollOut(旧链独有)/RollOver(新链独有)。
    /// 不调 set_hovered_chain（全局 union 在 recompute_hovered）。
    fn hover_diff_slot(slot: &mut TouchSlot, scene: &mut Scene, out: &mut Vec<EventRecord>) {
        let new_chain = ancestor_chain(scene, slot.last_hit);
        if new_chain == slot.last_hovered_chain {
            return;
        }
        for n in &slot.last_hovered_chain {
            if !new_chain.contains(n) {
                out.push(EventRecord {
                    node_id: n.0,
                    event_type: EVT_ROLL_OUT,
                    click_count: 0,
                    pad: [0, 0],
                    touch_id: slot.touch_id,
                    x: slot.last_pos.0,
                    y: slot.last_pos.1,
                });
            }
        }
        for n in &new_chain {
            if !slot.last_hovered_chain.contains(n) {
                out.push(EventRecord {
                    node_id: n.0,
                    event_type: EVT_ROLL_OVER,
                    click_count: 0,
                    pad: [0, 0],
                    touch_id: slot.touch_id,
                    x: slot.last_pos.0,
                    y: slot.last_pos.1,
                });
            }
        }
        slot.last_hovered_chain = new_chain;
    }

    /// 全局 hovered 合并：清所有 → 所有活跃槽命中链 union（任一指命中元素或祖先 → :hover）。
    fn recompute_hovered(&self, scene: &mut Scene) {
        for n in scene.nodes.values_mut() {
            n.interaction.flags.remove(NodeFlags::HOVERED);
        }
        for i in 0..self.slots.len() {
            if i == 0 || self.slots[i].touch_id >= 0 {
                let mut cur = self.slots[i].last_hit;
                while let Some(id) = cur {
                    let parent = match scene.get_mut(id) {
                        Some(n) => {
                            n.interaction.flags.insert(NodeFlags::HOVERED);
                            n.parent
                        }
                        None => break,
                    };
                    cur = parent;
                }
            }
        }
    }

    /// 全局 active 合并：清所有 → 所有 is_down 槽的 down_node 命中链 union（基于 down_node，Down 时命中）。
    fn recompute_active(&self, scene: &mut Scene) {
        for n in scene.nodes.values_mut() {
            n.interaction.flags.remove(NodeFlags::ACTIVE);
        }
        for slot in &self.slots {
            if slot.is_down {
                let mut cur = slot.down_node;
                while let Some(id) = cur {
                    // disabled 节点截断 active 链——自身不设 active，其祖先也不（按下 disabled
                    // 子树不应让 disabled 节点或其上层变 active）。逐节点查（不只 down_node）：
                    // hit 落 disabled 节点的非 disabled 子（如 Text 子）时，链上遇到 disabled
                    // 祖先须截断，而非只查 down_node。
                    let parent = match scene.get_mut(id) {
                        Some(n) => {
                            if n.interaction.flags.contains(NodeFlags::DISABLED) {
                                break;
                            }
                            n.interaction.flags.insert(NodeFlags::ACTIVE);
                            n.parent
                        }
                        None => break,
                    };
                    cur = parent;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
