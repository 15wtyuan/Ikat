# `loomgui_core/src/input.rs` 代码审查报告

**审查日期**：2026-07-09
**文件大小**：1028 行（含模块声明，test 在 `src/input/tests.rs`，约 2300 行）

---

## 严重问题计数

| 严重级别 | 数量 |
|---------|------|
| 🔴 高危（设计缺陷，可能引发运行时 bug） | 4 |
| 🟡 中危（维护性/可读性/扩展性） | 6 |
| 🟢 低危（代码风格/注释/微小隐患） | 4 |

---

## 🔴 高危

### 1. `process()` 函数过长，421 行，单片巨石

**行号**：459–880

**代码**：整个 `process` 方法。

**问题**：421 行的 `process` 函数包含了所有事件处理逻辑——长按、静止 hover 刷新、4 种 PointerKind 分支、scroll 仲裁、drag 检测、click-to-focus、事件产出。`PointerKind::Move` 一个 arm 就占了 161 行（517–678），内含 5 个独立关注点：
1. Move 取消 click/longpress（518–526）
2. Scroll 手势阈值赛跑 + 轴锁 + 让出提升（528–587）
3. Drag 检测（589–623）
4. Scroll 跟手驱动（628–663）
5. Hover diff + Move 事件派发（665–677）

任意改动一个子逻辑都可能误伤另一个。新增手势（如 pinch）只能继续往这个函数里塞。

**建议**：每个关注点抽独立方法。Move arm 拆为 `handle_move_cancel`、`handle_scroll_testing`、`handle_drag_testing`、`handle_scroll_drag_follow`，每个方法收 `&mut TouchSlot` + `&Scene` + 事件参数，返 `Vec<EventRecord>` 或 direct push into `out`。

---

### 2. Sentinel NodeId：scrollbar grip 用合成 NodeId 欺骗类型系统

**相关代码**：
- `hit.rs:52`：`NodeId(container.0 | flag)` — 合成 sentinel
- `input.rs:681`：`grip.0` 被当作 scroll pane id 使用
- `input.rs:810-811`：注释明确说 "grip_dragging 时 hit 为 sentinel（scene.nodes 越界），跳过 EVT_UP/EVT_CLICK"

**问题**：scrollbar thumb 命中后产出一个不存在于 `Scene.nodes` 的 NodeId——它的高位 bit 被上了 flag（`0x40000000` 或 `0x20000000`，`scroll.rs:60-61`）。这个 NodeId 作为 `scrolling_pane` 的 container_id 使用时碰巧"能用"（因为 container.0 的低位才是真实 id），但在 Up arm 里需要特殊 case `!slot.grip_dragging` 跳过 EVT_UP/EVT_CLICK——否则 `scene.get(hit)` 会返回 None，导致 Up 和 Click 丢失。

如果将来 `NodeId.0` 的分配靠近 flag bit 位（u32 的高位），sentinel 可能与真实 NodeId 碰撞。这是一个靠注释而非类型系统的脆弱设计。

**建议**：为 grip 命中定义独立类型，比如：
```rust
enum HitResult {
    Node(NodeId),
    ScrollbarGrip { container: NodeId, axis: u8 },
}
```
让 `hit_test` 返 `HitResult`，`process` 不再需要靠 `grip_dragging` bool 来做"这个 hit 不发 Up/Click"的特殊分支。

---

### 3. `touch_id: -1` 承载双重语义

**行号**：多处，核心在 `find_or_alloc_slot:438-456`

**代码**：
```rust
if ev.touch_id == -1 {
    return Some(0); // 鼠标主指
}
// ...
if self.slots[i].touch_id == -1 {
    self.slots[i].touch_id = ev.touch_id;
    return Some(i);
}
```

**问题**：`-1` 同时表示"鼠标主指"和"空闲触摸槽"。这在 `find_or_alloc_slot` 中表现：鼠标恒进 slot0，空闲槽也靠 `touch_id == -1` 判定。语义依赖 if-else 分支顺序——若先检查空闲槽再检查鼠标，逻辑就崩。其他函数（`add_touch_monitor`、`cancel_click`）也有同样的 `if touch_id == -1 { 0 }` 分支。

**建议**：用 `Option<i32>` 或一个显式 enum：
```rust
enum SlotOwner {
    Mouse,
    Touch(u32), // fingerId
    Free,
}
```
或至少抽一个 `fn is_mouse(touch_id: i32) -> bool` 和 `fn is_free_slot(touch_id: i32) -> bool` 集中语义，避免散落各处的 `== -1` 判断。

---

### 4. 重复的槽查找逻辑（3 处完全相同的模式）

**行号**：
- `add_touch_monitor`：399–405
- `cancel_click`：424–431
- `find_or_alloc_slot`：部分重复（438–456）

**代码**（三处完全相同的模式）：
```rust
let slot_idx = if touch_id == -1 {
    0
} else {
    match (1..self.slots.len()).find(|&i| self.slots[i].touch_id == touch_id) {
        Some(i) => i,
        None => return,
    }
};
```

**问题**：完全相同的"按 touch_id 找槽"逻辑复制粘贴了 3 次。如果槽查找策略变化（比如支持更多槽），需要改 3 个地方。`find_or_alloc_slot` 是这个逻辑的增强版（找不到时分配而非返回），但没有复用基逻辑。

**建议**：抽一个 `fn find_slot(&self, touch_id: i32) -> Option<usize>` 私有方法，`add_touch_monitor`、`cancel_click` 直接调，`find_or_alloc_slot` 先调 `find_slot` 再走分配路径。

---

## 🟡 中危

### 5. `TouchSlot` god struct：17 个字段耦合不同状态机

**行号**：139–167

**代码**：`TouchSlot` 结构体定义，17 个字段涵盖输入坐标、click 状态（5 字段）、drag 状态（4 字段）、scroll 仲裁（6 字段）、longpress（3 字段）。

**问题**：单槽同时持有 click、drag、scroll、longpress 四种状态机的全部字段。每次 `process` 调用都要重置/检查/更新所有字段。新工程师读 `Down` arm（679–778）需要同时理解 click 初始化、drag 候选初始化、scroll 候选初始化、focus 目标查找、Down 事件产出——因为所有逻辑混在一个结构体上操作。新增交互手势（如 pinch）只能继续膨胀这个结构体。

**建议**：将 TouchSlot 按关注点拆为独立子状态：
```rust
struct ClickState { click_cancelled, last_click_time, last_click_pos, last_click_button, click_count }
struct DragState { drag_testing, dragging, drag_target }
struct ScrollGestureState { scroll_candidate, scroll_testing, scrolling_pane, scroll_gesture, grip_dragging, scroll_down_pos }
struct LongpressState { down_time, longpress_fired, longpress_cancelled }
struct TouchSlot { /* 公共字段 */ click: ClickState, drag: DragState, scroll: ScrollGestureState, longpress: LongpressState }
```
每个状态机专注自己的 reset/update/transition，`process` 变为调用各状态机方法。

---

### 6. `MOVE_CANCEL_PX = 50.0` 硬编码不合理

**行号**：82、522–523

**代码**：
```rust
const MOVE_CANCEL_PX: f32 = 50.0; // Move 硬编码取消阈值（per-axis，mouse+touch 通用）
// ...
if dx.abs() > MOVE_CANCEL_PX || dy.abs() > MOVE_CANCEL_PX {
    slot.click_cancelled = true;
    slot.longpress_cancelled = true;
}
```

**问题**：
1. 注释自己承认"硬编码"。50px 是 arbitrary 的，没有物理或用户体验依据。
2. 用 50px 取消 click，但 click 阈值是 10px（鼠标）/ 50px（触摸）。也就是说鼠标移动 11~49px 时 click 被取消但长按仍可能触发——行为不一致。
3. 鼠标和触摸共用同一个 50px 阈值：手指在触摸屏上 50px 漂移非常正常（触摸容差本就是 50px），但 mouse 50px 漂移几乎肯定是拖拽而非点击。用同一个值抹平了两者差异。

**建议**：拆为 `MOVE_CANCEL_MOUSE` 和 `MOVE_CANCEL_TOUCH`，参考 click/drag/scroll 阈值已有的 mouse/touch 分离模式。建议 mouse 用 20–30px，touch 用 80–100px。

---

### 7. `DRAG_FOLLOW_ASSUMED_DT` 是已知 bug

**行号**：86–87、662

**代码**：
```rust
/// drag_follow 占位 dt（process 未收真实 dt，假定 60fps；非 60fps 速度计算有偏差）。
const DRAG_FOLLOW_ASSUMED_DT: f32 = 0.016;
// ...
s.drag_follow(scroll_delta, DRAG_FOLLOW_ASSUMED_DT);
```

**问题**：注释写明这是个占位。30fps 设备上速度计算会偏差 2x，120fps 设备上偏差 0.5x。`drag_follow` 内部用 dt 做指数平滑速度计算（`VELOCITY_SMOOTH = 10.0`，`scroll.rs:38`），惯性质量直接受影响。

但 `process` 方法签名是 `fn process(&mut self, scene: &mut Scene, events: &[PointerEvent]) -> Vec<EventRecord>`——没有 dt 参数。加 dt 要改 FFI 接口和所有调用方，这是为什么一直没修。

**建议**：在 `process` 签名加 `dt: f32` 参数，或让 `PointerState` 持有 dt（类似 `time_s` 由外部 advance），用真实 dt 替换 `DRAG_FOLLOW_ASSUMED_DT`。如果暂时改不了接口，至少在常量注释里写清楚影响面：惯性距离、bounce 回弹速度在非 60fps 下都会缩放。

---

### 8. 事件类型枚举跳号：10、11 缺失

**行号**：69–77

**代码**：
```rust
pub const EVT_LONG_PRESS: u8 = 9;
pub const EVT_KEY_DOWN: u8 = 12;
pub const EVT_KEY_UP: u8 = 13;
```

**问题**：9 之后直接跳到 12，没有 10 和 11。如果 10/11 是已删除的事件类型（被 FFI 侧依赖过），跳号保留是正确的；但如果只是疏忽，留空 slot 是隐患——新事件类型可能被 assign 到 10/11，与某处硬编码冲突。

**建议**：补一行注释说明 10/11 的 slot 是否曾有定义、是否被 C# 侧占用、是否保留给未来类型。若无历史原因，建议连续编码。

---

### 9. `scroll_gesture: u8` bitfield 无命名常量

**行号**：164、553–556、645、650、688

**代码**：
```rust
pub scroll_gesture: u8, // bit0=垂直手势（Y 位移） bit1=水平手势（X 位移）
// ...
if eff_y && dy > 0.0 { slot.scroll_gesture |= 1; }
if eff_x && dx > 0.0 { slot.scroll_gesture |= 2; }
// ...
if slot.scroll_gesture & 1 != 0 { /* vertical */ }
if slot.scroll_gesture & 2 != 0 { /* horizontal */ }
```

**问题**：bit0/V 和 bit1/H 的判断散落各处，全是魔法数字 `1` 和 `2`。注释写着 bit0/bit1 的含义，但读码者需要来回对照注释才能看懂 `& 1` 和 `& 2`。

**建议**：
```rust
const SCROLL_GESTURE_V: u8 = 1 << 0;
const SCROLL_GESTURE_H: u8 = 1 << 1;
```
将所有 `|= 1` → `|= SCROLL_GESTURE_V`，`& 1` → `& SCROLL_GESTURE_V`。

---

### 10. `compute_world_transforms` 耦合在测试中

**行号**：tests.rs 中每个测试 helper 函数末尾都有 `compute_world_transforms(&mut s);`

**代码**：
```rust
fn one_button_scene() -> Scene {
    // ... build scene ...
    compute_world_transforms(&mut s);
    s
}
```

**问题**：`process` 不依赖 `world_matrix`（只用 `layout_rect` 做 hit test，`hit.rs:65` 用 `layout_rect` 而非 `world_rect`），但所有测试都手动调了 `compute_world_transforms`。如果未来 hit_test 改为依赖 `world_rect`（transform 动画后必然），这些测试会因为 helper 函数做了 init 而漏报缺失 transform 的状态。同时这也暴露了 `Scene` 与 transform 的隐式耦合——创建 Scene 后需要额外调用 compute 才完整。

**建议**：如果 hit_test 当前只用 layout_rect，测试应从 helper 中去掉 `compute_world_transforms`，在真正需要的测试中显式调用。或者给 `Scene` 一个 `new_complete` 构造器自动算 transform。

---

## 🟢 低危

### 11. `next_focus` 中的 `unwrap()`

**行号**：332

**代码**：
```rust
None => {
    if backward { *chain.last().unwrap() } else { chain[0] }
}
```

**问题**：`chain.is_empty()` 在 315 行已查过，这里的 `unwrap` 实际上永远安全。但 clippy 会告，且如果后续有人重构把 empty check 移到调用侧，`unwrap` 会在 FFI 路径 panic——根据 CLAUDE.md 的规定"FFI 入口绝不 panic"，虽然这里不在 FFI 直接入口（在 `process_keys` 里），但调用链是 Stage → process_keys → next_focus。

**建议**：用 `chain.last().copied().unwrap_or_else(|| chain[0])` 或直接 `chain[chain.len() - 1]`，把不变式通过代码结构而非运行时检查表达。

---

### 12. 部分冗余的双重注释

**行号**：416–417

**代码**：
```rust
// touch_monitors 是 Vec<NodeId>，用 retain 移除（Vec 无 sentinel 需求，retain 更简且无遍历期偏移）
slot.touch_monitors.retain(|n| *n != node);
```

**问题**：行内代码前的注释和上行注释内容完全相同，属于复制粘贴残留。

**建议**：删除重复注释行。

---

### 13. `process` 中 `used_touch_ids` 使用 `Vec::contains`（O(n²)）

**行号**：464、467

**代码**：
```rust
let used_touch_ids: Vec<i32> = events.iter().map(|e| e.touch_id).collect();
// ...
if active && !used_touch_ids.contains(&self.slots[i].touch_id) { ... }
```

**问题**：外层最多 5 个 slot，内层 events 本帧最多几十个，O(25-250) 不是性能热点。但这不是"懒得优化"——用 `Vec::contains` 是 O(n) 查找，语义上就是"本帧是否有该 touch_id 的事件"。几乎不会成为瓶颈，但写法不表达意图。

**建议**：等真有性能数据再改。如果真的需要，用 `FxHashSet` 或对 5 槽用位掩码就够。

---

### 14. `process_keys` 每帧重建 Tab 链

**行号**：348

**代码**：
```rust
let chain = build_tab_chain(scene);
```

**问题**：`build_tab_chain` 遍历整棵 Scene 树做 DFS + 排序。Tab 导航只在按下 Tab 键时调用（每帧顶多一次），且树节点数通常 < 1000，不算性能问题。但如果将来有大量 Tab 可聚焦节点或每帧多次调 Tab（不现实），此处是瓶颈。

**建议**：当前无需优化。如果未来树节点上万，再考虑缓存 + dirty flag。

---

## 附：整体评价

**做对的地方**：注释质量总体高，解释了 WHY 而非 WHAT；hover/active 的全局合并（`recompute_hovered`/`recompute_active`）设计正确；scroll-vs-drag 互斥 + 轴锁 + 嵌套让出的三轮仲裁逻辑虽复杂但经测试覆盖。

**核心问题**：`process` 函数单体会拖慢所有后续输入系统扩展。建议在下一轮特性开发（如 pinch、右键菜单、键盘 shortcut）之前先做方法抽取拆分。
