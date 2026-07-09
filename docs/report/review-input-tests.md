# LoomGUI `input/tests.rs` 代码审查报告

> 审查文件：`loomgui_core/src/input/tests.rs`（3650 行，85 个 `#[test]` 函数）
> 被测试源码：`loomgui_core/src/input.rs`（1028 行）
> 审查日期：2026-07-09

## 一、测试组织结构

### 1.1 扁平文件 + 注释分隔，无模块分组（中等）

**位置**：整体文件结构

tests.rs 使用行注释 `// ===== 段名 =====` 来划分功能区：
- `多槽测试`（line 613）—— 鼠标/触摸 slot 分配
- `touch_monitors capture 测`（line 1034）—— 触摸事件捕获/释放
- `click_test + per-axis 阈值 + down_targets`（line 1206）—— Click 判定
- `双击 + Move 取消`（line 1411）—— 双击窗口
- `Canceled + CancelClick`（line 1622）—— 取消机制
- `Stationary hover 跟随`（line 1768）—— 静止光标跟随
- `core drag 检测`（line 1819）—— 拖拽
- `core longpress 检测`（line 2253）—— 长按
- `焦点 + 键盘`（line 2434）—— 焦点管理 + Tab 导航
- `scroll 手势仲裁`（line 2867）—— 滚动 vs 拖拽仲裁
- `scrollbar grip 拖拽`（line 3429）—— 滚动条 thumb 拖拽

**问题分析**：85 个测试函数平铺在单个 `mod tests` 中，采用注释分节而非 `#[cfg(test)] mod multi_touch { ... }` 等子模块。注释分节比无组织好，但不如 Rust 模块结构清晰：开发者看不到明确的子模块包含关系，IDE 大纲只显示扁平的函数列表，新增测试不知道应放在哪个"段"。

**修复方向**：拆分为子模块（`mod multi_touch`、`mod drag`、`mod scroll` 等），每个子模块可独立 `use super::*`，更干净地限定 helper 作用域。

**严重级别**：建议（不影响正确性，影响可维护性）

### 1.2 测试命名风格一致、信息量高（良好）

所有测试函数名采用 `snake_case` 英文描述行为模式，如 `down_up_same_node_within_threshold_emits_click`、`hover_into_child_no_rollout_parent`。每个 `assert!` 带有中文自定义消息（如 `"btn（Text 的祖先）也 hovered——祖先链"`），调试时定位迅速。命名可自解释，平均长度在可接收范围。

---

## 二、测试覆盖分析

### 覆盖矩阵

| 功能域 | 覆盖情况 | 缺失场景 |
|--------|----------|----------|
| **pointer down/up/click** | Click 基本路径、阈值边界（mouse 10/touch 50）、disabled 抑制、节点销毁兜底、per-axis 判定 | 无 touch_id 非 -1 且非 ≥0 的越界值测试 |
| **hover/rollover/rollout** | 祖先链合并、兄弟切换、移出 UI、幂等性、静止光标跟随、Text 子命中传播 | hover 全局合并后移出一指其余仍 hover 的场景 |
| **active/hover 祖先链** | Text 子 down/up 传播 active、disabled 祖先链抑制 | 多层祖先 disabled 中间节点非 disabled 的链裁剪 |
| **多 touch/mouse slot** | 分配/满槽丢弃、touch_id 透传、hover/active 全局合并、RollOver per-touch | touch 槽分配后 Up 释放再分配的 cycle、同一 touch_id 重复 Down（恶意/延迟事件） |
| **touch_monitors (capture)** | add/remove/Up 清/capture==hit 去重 | `add_touch_monitor` 对未分配槽（Down 前调用）的行为 |
| **click_test / down_targets** | down_leaf 优先、per-axis 阈值、mouse vs touch 不同阈值、祖先兜底 | down_targets 全部悬空（祖先也销毁）的极端 case |
| **双击** | 窗口内 count=2、超时复位、1→2→1 循环、Cancel 复位 | 异键（button 0 vs 1）不叠加双击、异位置复位 |
| **Move 取消 (MOVE_CANCEL_PX)** | 位移 >50 取消 click+longpress | exactly 50.0 的边界行为（`>50` 还是 `>=50`）、逐轴各自超的 case |
| **Canceled (触摸中断)** | 发 Up 不发 Click、drag 中发 DragEnd | 多指中一指 Canceled 其余指不受影响 |
| **cancel_click API** | 取消 Click | 仅测 touch_id=-1（mouse），未测 touch_id≥0（触摸） |
| **drag** | 启动/移动/结束、阈值（mouse 2/touch 10）、draggable 祖先链、disabled 抑制、Canceled→DragEnd、低于阈值仍 Click | drag 中节点 disable（运行时禁拖拽）、drag 中节点销毁 |
| **longpress** | 1.5s 触发、未达阈值不发、Move>50 取消、单次 fired、与 Click 独立、disabled 抑制 | 触摸 vs 鼠标 longpress 无区分测试 |
| **焦点 / Tab 导航** | FocusIn/Out、Tab 前向循环、Shift+Tab 反向、tabindex 次序（正数升序→0→DFS）、空链 no-op、Tab 不产 keydown、click-to-focus、不可聚焦节点不夺焦、disabled 聚焦抑制 | `build_tab_chain` 在场景变更后不自动重建的一致性（测中只调一次）、Ctrl+Tab 等非纯导航组合键 |
| **keydown/keyup** | keydown 到焦点节点、无焦点丢弃 | **无 EVT_KEY_UP 测试**（keyup 路径未经任何测试验证） |
| **scroll 手势仲裁** | scroll vs drag 阈值赛跑、V-only 让出、嵌套内层优先、drag_follow 更新 scroll_pos、Up 惯性启动、无容器零保险 | touch 阈值 20 的测试全部缺（仅测 mouse=8）、scroll 惯性速度精确计算、水平滚动、overflow_x+overflow_y 同时 Scroll 的双轴仲裁 |
| **scrollbar grip** | thumb 命中→grip_dragging、click_cancelled、Move 驱动 scroll_pos、Up 清状态+无惯性、非 thumb 区不 grip | 水平 grip（overflow_x=Scroll）、同一容器同时有垂直 grip+水平 grip 的命中区分 |

### 2.1 严重缺失：无 EVT_KEY_UP 测试（高）

**位置**：`tests.rs` 全文搜索 `EVT_KEY_UP` 无匹配

**问题分析**：`input.rs` 的 `process_keys()`（line ~320-371）对 `is_down=false` 的 KeyEvent 产 `EVT_KEY_UP` 事件。但 tests.rs 中所有键盘测试仅覆盖 keydown（`is_down: true`），没有任何测试验证 keyup 事件的产生、`node_id` 正确性（指回焦点节点）、或 `focus_node` 变更后 keyup 仍发至旧焦点的边界行为。

**修复方向**：补 `keyup_emitted_to_focused_node` 和 `keyup_no_focus_dropped` 至少两个测试。

**严重级别**：高（未经测试的代码路径可能含 bug）

### 2.2 `cancel_click` 仅测 mouse 路径（中）

**位置**：line 1663 `cancel_click_api_skips_click`

**问题分析**：`cancel_click` 接受 `touch_id` 参数且有找槽逻辑（line 423-433）。测试仅用 `touch_id: -1`（mouse slot0），未测 `touch_id: 1`（触摸 slot1）路径，也未测无效 `touch_id`（槽未分配）的 no-op 行为。

**修复方向**：补触摸 `touch_id` 的 cancel_click 测试 + 无效 `touch_id` 的 no-op 测试。

**严重级别**：中（核心路径覆盖，触摸分支遗漏）

### 2.3 scroll 触摸阈值测试完全缺失（中）

**位置**：所有 scroll 测试（line 2950-3427）

**问题分析**：所有 7 个 scroll 手势仲裁测试均使用 `touch_id: -1`（mouse），对应 `SCROLL_THRESHOLD_MOUSE = 8`。`SCROLL_THRESHOLD_TOUCH = 20` 从未被测试覆盖。scroll 阈值通过 `scroll_threshold(touch_id)` 函数选择（line 107-113），触摸分支未经验证。

**修复方向**：补至少 1 个 touch 滚动阈值测试（如 `touch_id: 1` Move dy=25 达 20 阈值启动滚动）。

**严重级别**：中（触摸滚动是移动端核心场景）

---

## 三、测试质量

### 3.1 大量重复的场景构造代码（高）

**位置**：多处，典型如下

**代码片段**（line 466-489, line 652-677, line 813-838, line 868-894, line 941-966 等）：
```rust
let mut root = Node::default();
root.layout_rect = Rect { x: 0.0, y: 0.0, w: 200.0, h: 200.0 };
let mut a = Node::default();
a.layout_rect = Rect { x: 0.0, y: 0.0, w: 50.0, h: 50.0 };
let mut b = Node::default();
b.layout_rect = Rect { x: 100.0, y: 0.0, w: 50.0, h: 50.0 };
let mut s = Scene::from_nodes(vec![root, a, b], vec![(0, 1), (0, 2)]);
compute_world_transforms(&mut s);
```

**问题分析**：root + 两兄弟（A/B）的场景构造在 `hover_between_siblings_old_chain_rollout`（line 466）、`two_touches_independent_down_up`（line 651）、`hover_global_merge_two_fingers`（line 812）、`active_global_merge_two_fingers`（line 867）、`rollover_per_touch_independent`（line 940）、`two_focusable_scene`（line 2437）等至少 6 处重复，差异仅在于 A/B 是否设 `NodeKind::Button`、是否设 `tabindex`。

**修复方向**：泛化为如 `fn two_sibling_scene(a_is_button: bool, b_is_button: bool) -> Scene` 或 builder 模式。

**严重级别**：高（代码膨胀 ~150 行，改 rect 含义需同步改 ≥6 处）

### 3.2 scroll 场景构造模式复制粘贴（中）

**位置**：line 2871-2947 (`v_scroll_scene`)、line 3026-3120 (`nested_innermost_scroll_wins`)、line 3261-3342 (`scroll_start_suppresses_drag`)、line 3431-3507 (`grip_scroll_scene`)

**问题分析**：四个 scroll 相关 helper 均包含同样的 `use crate::style::resolved::{OverflowMode, ResolvedStyle}` + `entries: Vec<(Option<usize>, NodeKind, ResolvedStyle, ...)>` + `Scene::build(&entries)` + 手动设 `layout_rect`/`clip_rect` + `compute_world_transforms` + `refresh_content_sizes` 序列。仅样式参数（Scroll vs Visible）和尺寸不同。

大量注释里的 "模拟 layout solve 的 clip_rect 填充（见 v_scroll_scene 注释）" 说明这是已知的样板，但未抽象。

**修复方向**：提取 `fn build_scroll_scene(overflow_x, overflow_y, viewport_w, viewport_h, content_w, content_h) -> (Scene, NodeId, NodeId)` 等方式。

**严重级别**：中

### 3.3 `drag_threshold_mouse_2_touch_10_per_axis` 内联 4 个相同场景（低）

**位置**：line 1978-2093

**问题分析**：一个测试函数创建 4 个独立 `one_draggable_button_scene()` + 4 个独立 `PointerState::new()`，测 mouse=2/3 和 touch=10/11 四个边界。可拆为 4 个独立 `#[test]` 或用参数化方式。当前格式下断言失败仅显示行号，不明确是 mouse 还是 touch 子用例失败。

**修复方向**：拆为 `drag_threshold_mouse_2_rejects`、`drag_threshold_mouse_3_fires` 等独立测试。

**严重级别**：低

### 3.4 断言精度不足（低）

**位置**：line 3155 `scroll_drag_follow_advances_scroll_pos`、line 3544 `grip_move_drives_scroll_pos`

**代码片段**：
```rust
assert!(st.scroll_pos.1 < 0.0, "下拖...got {}", st.scroll_pos.1);  // line 3196-3198
assert!(st.scroll_pos.1 > 50.0, "grip move...got {}", st.scroll_pos.1);  // line 3574-3577
```

**问题分析**：滚动跟手和 grip 拖拽均只验证方向（`<0.0`、`>50.0`），不验证精确增量。`drag_follow` 位移 delta 和 grip 百分比到 scroll_pos 的映射有明确算术公式（drag_follow 用 `DRAG_FOLLOW_ASSUMED_DT`，grip 用 `perc * overlap`）。不验证精确值意味着计算公式变更可能导致滚动速度偏差而无测试报警。

**修复方向**：使用 `assert!((st.scroll_pos.1 - expected).abs() < 0.01)` 精确验证。

**严重级别**：低（方向正确性已覆盖，精确值需重构后才易测）

### 3.5 scroll_up_starts_inertia 未验证惯性启动（中）

**位置**：line 3203-3258 `scroll_up_starts_inertia_and_clears_state`

**代码片段**：
```rust
let _st = s.scroll.get(scroll_id).unwrap();
// 不硬断 tweening（速度可能因阈值不启），仅验字段清。
```

**问题分析**：测试名声称 "starts inertia"，但实际仅验证 `scrolling_pane` 和 `scroll_testing` 被清空，没有验证 `tweening` 字段变为非零（惯性动画启动）。注释坦承不硬断因为速度可能不达阈值——这说明测试本身不可靠，依赖速度累积的随机性。

**修复方向**：给定足够大的 Move 位移（如 `dy=100`）保证速度超阈值，然后断言 `st.tweening > 0`。

**严重级别**：中（测试名与实际断言不符，且可能误导开发者以为惯性已验）

---

## 四、硬编码阈值与源码一致性

### 4.1 测试中硬编码的阈值一览（高）

| 源码常量 | 源码值 | 测试中硬编码值 | 测试位置 | 一致性 |
|----------|--------|--------------|---------|--------|
| `CLICK_THRESHOLD_MOUSE` | 10.0 | 10（通过/不通过边界） | line 176-199, 1257-1285, 1289-1317 | 一致（用 `>10` 判，即 10 不超） |
| `CLICK_THRESHOLD_TOUCH` | 50.0 | 30（通过） | line 1342-1344 | 一致（30<50→通过） |
| `DRAG_THRESHOLD_MOUSE` | 2.0 | 2/3/5/1 | line 1978-2093 | 一致（per-axis `>2`，2 不超 3 超） |
| `DRAG_THRESHOLD_TOUCH` | 10.0 | 10/11 | line 2037-2093 | 一致 |
| `LONGPRESS_TRIGGER` | 1.5 | 1.5/1.0/2.0 | line 2256-2369 | 一致 |
| `DOUBLE_CLICK_TIME` | 0.35 | 0.2/0.4 | line 1415-1535 | 一致（但注释 '350ms' 应写 '350ms 内'，见 4.2） |
| `MOVE_CANCEL_PX` | 50.0 | 60/50（不确定） | line 1579-1619, 2307-2340 | `\|dx\|>50` 判超，60 超 50 一致；无 exactly-50 测试 |
| `SCROLL_THRESHOLD_MOUSE` | 8.0 | 15/20 | line 2951-2988, 3026-3152 | 一致（15>8→达阈值，20>8） |
| `SCROLL_THRESHOLD_TOUCH` | 20.0 | 无 touch 测试 | — | **未覆盖** |

**问题分析**：所有阈值均为硬编码字面量。若源码常量被修改（如 `CLICK_THRESHOLD_MOUSE` 从 10 改为 12），测试将继续通过而不报警（因为测试用的硬编码 10 与源码 12 不同步）。测试变成了对"某种行为"的验证而非对"源码行为"的验证。

**修复方向**：在 `input.rs` 中将所有阈值常量设为 `pub(crate)`（当前是 `const` 私有），测试中通过 `super::THRESHOLD_NAME` 引用。或至少测试文件顶部定义 `const CLICK_THRESHOLD_MOUSE: f32 = super::...`。

**严重级别**：高（一旦改常量遗漏更新测试，测试价值归零）

### 4.2 注释中 350ms vs 常量 0.35（低）

**位置**：line 1475

**代码片段**：
```rust
assert_eq!(count2, 2, "350ms 内同位同键 → count=2");
```

`DOUBLE_CLICK_TIME = 0.35`（秒），即 350ms。注释正确但测试用 `time_s = 0.2`（200ms）而非边界附近（如 0.34 vs 0.36）。边界附近的测试比内部值更有价值。

**修复方向**：补 `双击刚好在 350ms 边界` 的测试（0.34→count2, 0.35→count1）。

**严重级别**：低

---

## 五、辅助函数与 Setup 模式

### 5.1 已有的辅助函数（良好）

| 函数 | 行号 | 复用次数（估算） |
|------|------|----------------|
| `one_button_scene()` | 5 | ~15 次 |
| `button_with_text_child_scene()` | 29 | ~3 次 |
| `nested_scene()` | 396 | ~4 次 |
| `one_draggable_button_scene()` | 1822 | ~6 次 |
| `two_focusable_scene()` | 2437 | ~5 次 |
| `tab_chain_scene()` | 2530 | ~4 次 |
| `v_scroll_scene()` | 2871 | ~ 3 次 |
| `grip_scroll_scene()` | 3431 | ~3 次 |

**评价**：辅助函数覆盖了最常见场景（单按钮、嵌套父子、双可聚焦、Tab 链、垂直滚动），设计合理。但仍有大量 inline 场景构造（见 3.1）。

### 5.2 缺失的通用 helper（建议）

- `fn two_sibling_scene(a_rect, b_rect, ...)` — 覆盖 6+ 处重复的 root+A+B 构造
- `fn build_scroll_container(...)` — 覆盖 scroll 段的 4 处重复的 `entries` + `Scene::build` + `clip_rect` 填充模式
- `fn move_event(x, y, touch_id) -> PointerEvent` — 减少 `PointerEvent { kind: PointerKind::Move, x, y, button: 0, pad: [0, 0], touch_id }` 的 8-9 行重复

---

## 六、不可靠测试分析

### 6.1 时间依赖测试无真实时钟（良好）

所有依赖时间的测试（longpress、双击）均通过显式设置 `ps.time_s` 推进模拟时间，不依赖系统时钟。这是正确做法。

### 6.2 场景切换（`down_leaf_destroyed_fallback_to_ancestor`）跨 Scene 实例（中）

**位置**：line 1350-1409

**代码片段**：
```rust
ps.process(&mut s1, &[...]);  // Down on child in s1
// ... 构造 s2（child 不在 s2 中）
let out = ps.process(&mut s2, &[...]);  // Up in s2
```

**问题分析**：测试在同一个 `PointerState` 上先用 `&mut s1` 再用 `&mut s2` 调用 `process`。两个 Scene 是不同实例，NodeId 值取决于 `Scene::from_nodes` 的内部分配——依赖 s1 中 child 的 NodeId 在 s2 中变成悬空句柄。当前 `Scene::from_nodes` 每次分配 `NodeId(1), NodeId(2)...`，所以 s1 的 child 是 `NodeId(2)` 而 s2 只有 root=`NodeId(1)`——`NodeId(2)` 在 s2 中不存在。**这隐式依赖 Scene 内部实现**（`from_nodes` 从 1 开始逐增分配编号）。如果实现改为其他分配策略（如基于 slotmap key 的 hash），测试预期将失效。

**修复方向**：注释说明这种依赖并添加对 `slot.down_node` 的显式断言（验证 child NodeId 在 s2 中 `scene.get()` 返 None），使测试意图显式化。

**严重级别**：中（当前实现稳定，但实现细节泄漏进测试）

### 6.3 `grip_scroll_scene` 依赖 thumb 位置计算（中）

**位置**：line 3431-3507

**问题分析**：测试用硬编码坐标 `(96, 25)` 作为 "thumb 中心" 来测试 grip 命中。thumb 位置由 scroll 模块的内部公式（`viewport_h / content_h * track_h`）计算，计算依赖 `content_size` / `viewport_size`。如果 scroll 模块调整 thumb 布局公式，这些坐标将不再命中 thumb，测试将误报。

**修复方向**：从 `scroll_state` 读取 thumb rect 再构造输入坐标（`let thumb_y = st.thumb_rect.y + st.thumb_rect.h * 0.5`），而非硬编码。

**严重级别**：中（当前公式稳定，但耦合紧）

---

## 七、总结与优先修复建议

### 高优先级
1. **补 EVT_KEY_UP 测试**（line 2744 附近）—— 当前 keyup 路径零覆盖
2. **阈值硬编码改为引用源码常量** —— 将 `input.rs` 中的 `const` 改为 `pub(crate)` 并在测试中用 `super::CONST_NAME` 引用。当前硬编码与源码不同步的风险使测试价值减弱
3. **抽象重复场景构造** —— `root + A + B` 两兄弟场景至少 6 处内联，`entries + Scene::build` 的 scroll 样板 4 处内联

### 中优先级
4. **补 touch 路径测试** —— `cancel_click` 触摸版、scroll 触摸阈值（`SCROLL_THRESHOLD_TOUCH = 20`）
5. **验证 scroll 惯性启动** —— `scroll_up_starts_inertia_and_clears_state` 应给足够位移使速度超阈值，然后断言 `tweening > 0`
6. **`down_leaf_destroyed` 场景切换测试注释化依赖** —— 显式断言 `scene.get(down_node)` 返 None

### 低优先级
7. **`drag_threshold_mouse_2_touch_10_per_axis` 拆分为 4 个独立测试**
8. **补齐边界测试** —— 双击窗口边界（0.34→count2, 0.35→count1）、MOVE_CANCEL_PX 的 exactly-50 行为
9. **精确化滚动断言** —— 用公式算期望 scroll_pos 再对比
