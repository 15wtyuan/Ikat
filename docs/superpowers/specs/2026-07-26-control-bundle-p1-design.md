# 控件束 P1：ProgressBar + Toggle + Slider

> 状态：设计稿（2026-07-26 brainstorming 共识）
> 范围：§4 控件束的第一棒，做三个控件（按复杂度梯度），建立控件地基
> 对齐契约：`docs/design/main-design.md` §6、`docs/design/public-api.md` §7、`docs/design/projection-layer.md` §2

---

## 1. 背景与目标

摸黑结束后进入 §4 三束加宽。本 spec 是**控件束第一棒**，理由：showcase 全 8 页都用控件、且控件交互是 AI 对 HTML 先验最强的部分之一。

**本次做 ProgressBar + Toggle + Slider**（按复杂度梯度）：
- ProgressBar（只读展示）→ Toggle（离散点击）→ Slider（连续拖拽）
- 覆盖"只读 / 离散交互 / 连续交互"三种核心交互模式
- Slider 是第一个高频改值控件 → 触发**还 set_transform 债**

**不在本次**：Dropdown（弹出列表）、TextField/TextArea（IME/光标，最大工程）、NumberField（Slider 的数值子集，可顺带）。

## 2. 控件模型决策（核心架构）

### 2.1 控件当容器（改"叶子"契约）

main-design 原写"控件是叶子"。本 spec 改为**控件是容器**——core 实例化时自动注入约定 class 的视觉子节点。依据：对照 RmlUi 源码（`ElementProgress` 构造时 `AppendChild(fill)`、`WidgetSlider` 注入 track/bar/progress/arrows），成熟框架都这么做。

**为什么当容器最合理**（三选一调研后定）：
- **AI 先验**（LoomGUI 核心赌注）：AI 对"div 子树 + CSS 定制"理解最强，比伪元素（`::-webkit-progress-value`）或约定属性（color 当 fill 色）都直观
- **CSS 定制力**：用户 `.my-slider .loom-thumb{}` 直接定制视觉子部分
- **复用现有机制**：视觉子部分走正常 layout/cascade/render，render 不加控件特殊分支
- **RmlUi 验证**：和 RmlUi 一致

### 2.2 子节点注入：core 运行时（非打包期）

注入时机选**运行时**（core 实例化 NodeKind::ProgressBar 时动态 append 子节点），非打包期 bridge 展开：
- pkg.bin **保留 NodeKind::ProgressBar 语义**（控件类型分派靠 NodeKind，干净）
- 运行时 tree 有视觉子节点，pkg.bin 没有（语义骨架 vs 运行时展开）
- 控件类型分派不靠 class 猜，靠 NodeKind

### 2.3 默认样式：不内置 UA，围栏校验

**core 不内置 UA 默认样式**（保持 core 纯净，不开"框架自带样式源"先例）。控件不写 CSS = 空白。

**围栏打包期校验**（B+ 方案）：用了控件标签 → 必须有 CSS 规则命中它，否则报错 + 教学：
> LoomGUI 控件不带默认样式。`<progress>` 需要 CSS 规则（建议为它和 `.loom-fill` 子元素提供 background）。

校验机制：复用围栏现有 cascade（控件节点未被任何规则匹配 → 报错）。不是新引擎，是 resolve 的副产品。tag/class/后代选择器都算命中，只有"完全没匹配规则"才报错。

**AI 先验对齐方式**：不是"模仿浏览器有 UA 样式"，而是"打包期把 LoomGUI 与浏览器的差异明确告诉 AI + 教学"。AI 不会遇到"写了代码却空白"的困惑——打包期就报错引导补 CSS。

## 3. 三个控件的具体结构

子节点命名用 `.loom-` 前缀（表框架内部，避免和用户 class 冲突）。结构固定（core 注入时写死，不可配置）。

### 3.1 ProgressBar

```
<progress value="70" max="100">
  └─ <div class="loom-fill">          ← fill，width = value/max %
```
- `progress` 节点本身 = track（用户给 `background` 当轨道色）
- value/max 变 → core 改 fill 的 inline `width`（走 layout）
- **value 语义优先**：core 强制 fill inline `width:value%`（inline 优先级最高），用户 CSS 改不了填充比例（只能改颜色）。符合 HTML（progress 填充比例由 value 定）。
- IsIndeterminate：keyframes runtime 缺失 → 本次**不渲染 indeterminate 动画**（留 TODO），正常 value 渲染

### 3.2 Toggle（checkbox / radio）

```
<input type="checkbox" checked>
  └─ <div class="loom-check">         ← 打勾/图标容器，IsChecked 决定可见性
```
- `input` 节点本身 = 框（用户给 background/border）
- IsChecked → `:checked` 伪类重 cascade（离散点击，低频）
- **check 子节点可见性由 core 控制**（和 fill width 同构——都是「core 根据控件状态写子节点 inline style」）：IsChecked 时 core 给 .loom-check 写 inline `display:block`，否则 `display:none`。用户 CSS **不用写隐藏规则**，只管给 .loom-check 配图标/颜色。
- **radio 同 name 自动互斥**（core 处理）：选中一个 radio → 同 name 其它置 false + 只对新选中项触发 CheckedChanged

### 3.3 Slider（range）

```
<input type="range" value="50" min="0" max="100" step="1">
  ├─ <div class="loom-track">         ← track 容器
  │    └─ <div class="loom-fill">     ← fill，width = (value-min)/(max-min) %
  └─ <div class="loom-thumb">         ← 滑块头，位置走 transform（拖拽高频）
```
- fill width 走 **layout**（value 变化时 core 写 inline width，触发 solve）
- thumb 位置走 **transform**（set_translate，绕开 solve——这是还的债）
- **为什么 thumb 走 transform 不走 layout**：solve 是全树重算（O(整页)），现有高频先例（scroll_pos/tween）都刻意绕开 solve 走旁路。Slider 拖拽每帧改 thumb 位置，走 transform 是 O(1)，和 scroll_pos 平移同构。
- step 量化：value = min + round((raw-min)/step)×step

### 3.4 共同规则
- 子节点**只 class 无 id**（不占用户 id 命名空间；Query/Get 默认仍能找到它们）
- 用户**不该手写**这些子节点；`<progress>文本</progress>` 围栏拦（progress 是 void，不允许子内容）
- 状态初始值从 **HTML 属性** bake（见 §4）
- 围栏校验：用了控件 → 必须有 CSS 命中（§2.3）
- **统一模型：core 根据控件状态，控制注入子节点的 inline style**（状态语义优先于 CSS）：
  - ProgressBar fill：inline `width = value/max%`
  - Toggle check：inline `display`（IsChecked 决定）
  - Slider fill：inline `width`；thumb：transform translate
  - 用户 CSS 只管子节点的**外观**（background/color/border/圆角），状态驱动的几何/可见性由 core 写 inline（inline 优先级最高，用户 CSS 改不了）。符合 HTML（控件填充比例/勾选态由语义状态决定，CSS 只管外观）。

## 4. 数据流：HTML 属性 → side table

### 4.1 现状链路（断在 bridge）

契约"HTML 属性提供初始值"（value/max/checked/min/step），但链路断在 bridge：

| 环节 | 现状 |
|---|---|
| fence 解析 | ✓ 全保留在 `IrElement.attributes` |
| fence 校验 | ✓ 认属性名（不校验值域） |
| **bridge** | ✗ 断点：只提取 src/id/class/tabindex，控件属性全丢 |
| **TemplateNode** | ✗ 无控件初始值字段 |
| **core** | ✗ 无控件 side table |

### 4.2 本次补的环节（三处缺一不可）

1. **bridge 提取**：`bridge.rs` Element 分支提取 `value/max/min/step/checked`，按 NodeKind 装进 `ControlInit`
2. **TemplateNode 加字段**：`control_init: Option<ControlInit>`（union，按 NodeKind 分派）→ **pkg 格式 bump v23→v24**
3. **core side table**：新增 `Scene.controls: HashMap<NodeId, ControlState>`（仿 scroll/anim 并行表），instantiate 时从 TemplateNode.control_init 填初始值

### 4.3 ControlState 设计（统一 side table）

```rust
// core 新增，按 NodeKind 分派的 union
pub enum ControlState {
    Progress { value: f32, max: f32, indeterminate: bool },
    Toggle { checked: bool, radio_name: Option<String> },  // radio 用 name 互斥
    Slider { value: f32, min: f32, max: f32, step: f32 },
}
```
- 统一表（不分 progress/slider/toggle 各一表）：控件稀疏，一个 union 表省内存 + 管理简单
- C# 写 `progress.Value = 70` → FFI `set_control_value` → 写 side table → core 同步更新 fill 子节点 inline width

## 5. FFI + 攒批还债

### 5.1 控件状态 FFI（读写 side table）

新增 FFI（return-code + out-param 模式，避 Container=0 哨兵）：
- `loomgui_set_control_value(stage, node_id, value: f32) -> rc` — 写 value（Progress/Slider/NumberField 通用）
- `loomgui_get_control_value(stage, node_id, *out: f32) -> rc` — 读 value
- `loomgui_set_control_checked(stage, node_id, checked: bool) -> rc` — 写 Toggle/Radio
- `loomgui_get_control_checked(stage, node_id, *out: bool) -> rc`
- max/min/step 同理（get/set，Slider/Progress 共用）

### 5.2 set_transform 通用化（还债）

本次实现**通用 `set_transform` FFI**（projection-layer §2.2 早标的"必需通路"，roadmap 标"第一个高频控件触发时还"）：
- `loomgui_set_transform(stage, node_id, *repr: TransformRepr)` — 纯 f32（pos/scale/rot/origin）
- 不触发 solve，只标脏 transform → `compute_world_transforms` 重算
- Slider thumb 是第一个用户（拖拽每帧 set_translate）
- **通用化**而非 Slider 专用：NodeTransform 是所有节点的公共 API（public-api 冻结），一次做干净不留半成品

### 5.3 C# 投影层
- 填 ProgressBar/Toggle/Slider/RadioButton 的 `throw NE()` 壳 → 转发 FFI
- 攒批：本次**先即时过桥**（仿 4a 的 StyleMirror 即时调用），Slider thumb 的 set_transform 也即时（拖拽虽高频，但单次 FFI 开销可接受；真正攒批优化标 ponytail，profile 出热点再做）。projection-layer §2.1 明示"升级攒批只改 setter 调用时机，不推翻镜像结构"。

## 6. 交互与事件

### 6.1 交互（core process 指针输入，tick §16.c）

- **Toggle 点击**：pointer down 命中 input → 切换 IsChecked（side table）→ 标 rematch（下帧 :checked 重 cascade）→ 产生 CheckedChanged 事件
- **Radio 点击**：切换 IsChecked + 找同 name 兄弟置 false（各自触发 CheckedChanged，只新选中项）
- **Slider 拖拽**：pointer down 命中 thumb（或 track 点）→ 拖拽 move → 按 track 几何映射算 raw value → step 量化 → 写 side table + 更新 fill width(layout) + thumb transform → 产生 ValueChanged；pointer up → ChangeCommitted

### 6.2 事件出口（复用 borrow_events）

core 产生控件事件进事件队列 → `borrow_events` 出 → C# EventBus demux：
- 新增事件类型：`ValueChanged(f32)` / `CheckedChanged(bool)` / `ChangeCommitted`
- C# `ValueChangedEvent<T>` / `CheckedChangedEvent` struct 已声明（壳），本次填实
- 复用现有 ClickEvent 的 demux 机制，加控件事件 enum 变体

## 7. showcase 覆盖

showcase 现有控件覆盖很全（text/number/password/search/checkbox/radio/range/progress/select/textarea 都有），缺的是**交互行为**。本次补：

- **ProgressBar**：character/inventory/shop 页的 progress 要能渲染出 fill（配 CSS）+ 值变化演示（如角色页经验条点击加经验）
- **Toggle/Checkbox/Radio**：settings/form 页的 checkbox/radio 要能点击切换 + :checked 视觉变化
- **Slider/Range**：settings/form 页的 range 要能拖拽 + 实时值反馈（如设置页音量滑块拖动改变一个显示值）
- 每个控件配**教学 CSS**（showcase 里给标准样式，用户可参考）

## 8. 不在本次（defer）

- **Dropdown**（弹出列表，复合束边界）：本次不做
- **TextField/TextArea 全家**（IME/光标/字符输入通道，最大工程）：独立 spec
- **NumberField**：Slider 的数值子集，可顺带但优先级低
- **IsIndeterminate 动画**：等 keyframes runtime（§4 视觉束）
- **fence 值域校验**（max>0、min≤max 等）：本次只认属性名，值域校验后置
- **控件状态攒批 flush**：即时过桥兜底，profile 出热点再攒批（ponytail）

## 9. 验收标准

1. **headless 单测**（core 层）：HTML 属性 → bridge 提取 → TemplateNode.control_init → instantiate → side table 初始值正确；value setter → fill 子节点 inline width 更新；Toggle 点击 → IsChecked 翻转 + :checked rematch；Slider 拖拽 → value 量化 + thumb transform 更新
2. **PublicApi 编译门**：ProgressBar/Toggle/Slider/RadioButton 壳填实，编译通过
3. **showcase Unity PlayMode**：progress 渲染 fill、checkbox/radio 点击切换、slider 拖拽改值，各页控件可交互
4. **围栏校验**：控件无 CSS 命中 → 打包期报错 + 教学文案

## 10. pkg 版本

bump **v23 → v24**（TemplateNode 加 control_init 字段，bincode 布局变）。一刀切升，不留后向兼容（个人项目惯例）。

---

## 关键决策记录

- **控件当容器（改叶子契约）**：对照 RmlUi 源码验证，AI 先验 + CSS 定制力最优
- **子节点运行时注入**：保留 NodeKind 语义分派
- **不内置 UA 样式 + 围栏校验**：core 干净，AI 先验靠打包期教学对齐
- **Slider thumb 走 transform**：solve 是全树重算，高频拖拽必须绕开（和 scroll_pos/tween 一致）
- **set_transform 通用化**：一次还干净
- **统一 ControlState side table**：控件稀疏，union 表省内存
- **value 语义优先 fill inline width**：符合 HTML
