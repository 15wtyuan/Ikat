# M3 · TabList 复合控件 设计

> 控件束 P4 的第一棒。把 WAI-ARIA `role=tablist/tab/tabpanel` 做成框架自带复合控件，
> 镜像已有 Dropdown（role=combobox）的全套机制（NodeKind + ControlState + role 分派 +
> aria 合成 + click/键盘 + 显隐 + 事件），解 settings 页 tab 高亮 + 面板切换。

## 1. 背景与动机

roadmap §4 控件束 P4 = WAI-ARIA 复合控件（TabList/Tree）。milestones.md M3 把它切成
「Node attrs 存储 + attr_matches_node 扩展 + role dispatch + `[aria-selected]` selector +
TabList 投影类」。但本轮 grill 核实代码后发现 **M3 spec 的前三条 exit criteria 已被控件束
P1-P3 做掉**，M3 的真实缺口远比文档说的窄——只剩「TabList 这个复合控件本身 + aria-selected
合成 + 非激活 panel 显隐」。

settings 页是直接驱动力：`showcase/showcase/settings.html` 已用标准 `role=tablist` +
`role=tab`（带 `aria-controls`/`aria-selected`）+ `role=tabpanel` 标记，但 CSS 规则
`.tab[aria-selected="true"]{...}` 被注释（roadmap tech-debt 标 defer），因为 `[aria-selected]`
选择器运行时根本匹配不到任何节点（aria-selected 没有合成源）。本 spec 解掉这条，让 settings
tab 高亮 + 面板切换在写标准 HTML 的前提下「写了就自动切」。

## 2. 现状核实（事实，非推断）

### 2.1 文档过期：M3 前三条 exit criteria 已完成

| milestones M3 / roadmap tech-debt 原文 | 实际状态 | 证据 |
|---|---|---|
| `attr_matches_node` 硬编码只认 `[type=...]`，要扩展 | **已做** | `crates/core/src/style/dynamic.rs:325` `attr_matches_node` 已分派 `[aria-*]`(synth)/`[role]`/`[data-slot]`/`[type]` 四类。commit `0a7373d` |
| role 不参与 `resolve_semantic` 分派 | **已做**（控件类） | `crates/fence/src/schema/tag.rs:122` `resolve_semantic` 先查 role；`ROLE_TO_SEMANTIC:101` 已含 combobox/option/listbox/slider/spinbutton/switch/radio/progressbar/list/listitem |
| Node 不存任意 HTML 属性 → `[aria-*]` 运行时无法匹配 | **部分已做** | aria-checked/expanded/valuenow/multiline 走 `synth_aria_value`（从 ControlState 合成，非字面存储）；role/data-slot 走 RoleTable。仅「非控件状态属性」（如 aria-selected）无合成源 |

### 2.2 M3 真实缺口（仅这三条）

1. **`ROLE_TO_SEMANTIC` 无 `tablist`/`tab`** → `<div role=tab>` 塌成普通 Container，role 纯透传。
2. **`synth_aria_value` 不合成 `aria-selected`** → `[aria-selected="true"]` 选择器对所有节点返回 None。
3. **panel 显隐无机制** → tab 切换后面板不会跟着显隐（panel 非 tablist 子节点，CSS 跨子树够不着）。

### 2.3 对标（temp/ 只读源码核实）

- **FairyGUI**：无 Tab 控件，tab = button + Controller 切页（DIY，偏 β 路线）。无标准 HTML，标记不可直接借鉴。
- **RmlUi**：`ElementTabSet` 框架自带，但用**自创标签 `<tabset>`**（违反本项目「不自创标签、保 AI 先验」原则），且 tab/panel 是 tabset 子节点按序关联。LoomGUI 只用标准 `<div role=tablist>`，标记不能照搬；关联模型也不同（见 §4.3）。
- **结论**：两个对标一偏 DIY、一偏框架控件，且标记都不能直接抄。LoomGUI 走「标准 HTML role + 框架自带行为」的 α 路线（见 §3）。

## 3. 设计决策（grill 锁定）

### 3.1 α：框架自带 TabList（不做通用 attrs 仓库 β）

曾考虑 (β) 给 Node 加通用 attrs 存储 + 业务代码驱动选中。否决，理由：
- α **复用已有机制**（ControlState + synth_aria_value + role 分派，控件束 P1-P3 全建好），只加一个变体；β 要发明新存储（Node 加字段 + 克隆 + pkg 序列化 + 新 FFI），blast radius 更大。
- α 让标准 HTML **连行为也描述全**（写了就自动切），最大化本项目核心论点「AI 写 DSL 即懂界面、HTML 作设计期 DSL」。
- β 的「可复用」优势在 showcase 里无第二个用例（控件类 aria 全 synth 了，唯一非控件状态属性就 aria-selected）= YAGNI。等 Tree/Accordion 真出现再补通用 attrs，不返工。
- **用户决策理由**：「其他组件也是这么干的，同一套机制比较好」——一致性。

### 3.2 K1：键盘导航纳入（方向键移动选中）

镜像 Dropdown 的 Up/Down/Enter/Esc。TabList 做左/右（水平）/上/下（垂直）方向键移动
`selected_index`，自动激活模型（焦点即选中，WAI-ARIA 最简模型）。**不做 roving tabindex
焦点管理**（重 a11y，游戏 UI 不需要，defer）。

### 3.3 P1：panel 显隐复用 display:none 剪枝

非激活 panel 的生效 display 置 none，激活的置回自然值——复用 layout 现成的 display:none
子树剪枝，**不加 hidden 标志字段、不动作者 CSS、不新增 render 层分支**。
作者写的 `style="display:none"` 正好是「默认隐藏」语义，框架把激活那个翻回来。

## 4. 架构：镜像 Dropdown

TabList 的每个维度都对齐已有 Dropdown（`role=combobox`）的机制，唯一结构差异是 panel 跨树关联。

| 维度 | TabList 决定 | 镜像的 Dropdown |
|---|---|---|
| **NodeKind** | 新增 `SemanticKind::TabList/Tab`（fence 枚举）+ `NodeKind::TabList/Tab`（core 枚举），packer bridge `map_semantic` 1:1 映射；`tabpanel` 保持 Container（role 透传） | Dropdown + OptionItem；listbox=Container |
| **ControlState** | `TabList { selected_index }` 挂 tablist 节点（仅选中序号，命名镜像 Dropdown；panel 不在此缓存）；**Tab 无状态**（选中态从父 selected_index 派生） | Dropdown{selected_index,open,...}；OptionItem 无状态 |
| **role 分派** | `ROLE_TO_SEMANTIC` 加 `tablist→TabList`、`tab→Tab`（tabpanel 不加），`resolve_semantic` 先查 role | combobox/option 已在表 |
| **aria 合成** | `synth_aria_value` 加分支：节点是 Tab → 查父 TabList.selected_index + 自身序号 → 相等 "true" | checked/expanded/valuenow 查自身 |
| **click** | pointer-down 命中 Tab → selected_index = 该 tab 序号 | pointer-down → open=true |
| **键盘** | 方向键移动 selected_index（K1） | Up/Down seek + Enter/Esc |
| **显隐** | 非激活 panel 生效 display=none（P1） | open 时 listbox overlay；closed 跳过 |
| **事件** | selected_index 变 → `SelectionChanged`（复用 `EVT_SELECTION_CHANGED`，payload=新 index） | Dropdown SelectionChanged |

### 4.1 唯一结构差异：panel 跨树关联

Dropdown 的 listbox 是 **combobox 子节点**，按 role 定位 + 整棵子树 overlay/跳过。
TabList 的 panel **不是 tablist 子节点**（settings.html 里 panel 在 `.main` 容器，与 tablist
分置两处），靠 `aria-controls="panel-x"` ↔ `id="panel-x"` 跨树关联——这是标准 WAI-ARIA 模式。

后果：panel 显隐不能照搬「跳过子树」，得靠 `selected_index` + 各 tab 的 `RoleInfo.aria_controls`
（panel id 字符串）在 control-update pass（`sync_control_visuals`）里每帧 `find_by_id_attr`
解析 panel 再翻其生效 display（§5.7）。panel id 不缓存在 ControlState——按需每帧解析（与
Dropdown listbox 直接子定位的区别）。

### 4.2 Tab 无状态，aria-selected 跨节点合成

镜像 OptionItem（无 ControlState，选中态从父 Dropdown.selected_index 派生）。Tab 同样无
ControlState，其 `[aria-selected]` 由 `synth_aria_value` 跨节点合成：

```
synth_aria_value(id, "selected"):
  if node[id].kind == Tab:
    parent = find_parent_with_kind(id, TabList)?
    active = ControlState[parent].selected_index
    my_index = index of id among parent's role=tab children
    return Some("true") if my_index == active else Some("false")
  ...existing arms
```

这是 synth 第一处「跨节点」合成（现有 arm 全查自身 ControlState）。新增，但局部、可单测。

## 5. 各改动点

### 5.1 语义枚举链 + role 分派（fence + core，两枚举）

> 实现细化：新增 TabList/Tab 需动**两个枚举**——fence 侧 `SemanticKind`（role 驱动的语义分类）
> 和 core 侧 `NodeKind`（运行时节点类型），由 packer bridge 的 `map_semantic` 1:1 映射。
> spec 原文只提 `NodeKind`，此处补全链条。

- `crates/fence/src/schema/tag.rs`：`SemanticKind` 枚举加 `TabList`、`Tab` 两个变体；`ROLE_TO_SEMANTIC`
  表加 `("tablist", TabList)`、`("tab", Tab)`；`resolve_semantic` 先查 role（既有 role 优先逻辑）
  天然分派。`tabpanel` **不加**（走 div→Container 透传，像 listbox）。
- `crates/packer/pkg/src/bridge.rs:136` `map_semantic` 加 `SemanticKind::TabList => NodeKind::TabList`、
  `SemanticKind::Tab => NodeKind::Tab` 两个 1:1 arm（与现有 22 可映射语义同一模式）。
- `crates/core/src/scene/node.rs` `NodeKind` 加 `TabList`、`Tab` 两个 unit 变体（derives Copy，
  照 Spec-2 既有模式）。`NodeKind::from_u8` / 逆映射 `kind_tag` 同步。
- fence `control_structure_check.rs`：加 tablist 结构契约（含 tab 子节点；tabpanel 可选、可跨树）。
  对照现有 combobox 契约写。

### 5.2 ControlState::TabList

`crates/core/src/scene/node.rs` `ControlState` 加：

```rust
TabList {
    selected_index: usize, // 命名镜像 Dropdown.selected_index（一致性决策）
}
```

> 实现细化：spec 原文草拟过 `panel_ids: Vec<NodeId>` 字段缓存 panel，**被推翻**——panel 不在
> ControlState 缓存，改由 `RoleInfo.aria_controls` 存原始 panel id 字符串，`sync_control_visuals`
> 每帧 `find_by_id_attr` 解析（§5.4）。ControlState::TabList 只有 `selected_index`。

`ControlInit`（pkg 载入侧）加 `TabList { selected_index }`（与 pkg 字段对齐）。

### 5.3 aria-selected 跨节点合成

`crates/core/src/style/dynamic.rs:382` `synth_aria_value` 加 `("selected", ...)` 分支按 §4.2。
注意 Exists op：`[aria-selected]`（无值）对 Tab 节点应判 Some（有语义）。

### 5.4 aria-controls 关联（需 pkg v28→v29 bump）

**问题**：`aria-controls` 是任意 HTML 属性，fence `structural.rs:140` 已识别它做 idref 校验，
但**没存进 TemplateNode/RoleInfo**，parse 后丢弃。要支持 tab↔panel 关联，必须把 aria-controls
字符串保存到 pkg。

**方案**（α 精神：特定属性特定存储，不做通用 attrs 仓库）：
- `crates/core/src/asset/mod.rs` `TemplateNode` 加 `pub aria_controls: Option<String>`。
- fence extract 阶段把 tab 节点的 `aria-controls` 抽进 `ParsedTemplate` → packer bridge 写入
  `TemplateNode.aria_controls`。
- **初始 `selected_index`** 在 packer bridge 派生（不进 pkg 存 aria-selected）：bridge 扫 tablist
  的 role=tab 子，找到 `aria-selected="true"` 的那个 → `ControlInit::TabList.selected_index = 其序号`；
  无声明或多于一个 true → 默认 0（取首个）。故 aria-selected 本身**不存进 pkg**——它是运行时从
  selected_index 经 §5.3 synth 派生的值，初始值反由它在 HTML 的声明决定。
- **panel 关联的运行时存储 = `RoleInfo.aria_controls`**（实现细化，推翻 spec 原草拟的
  `ControlState::TabList.panel_ids` 字段）：`instantiate` 时把 tab 子节点的
  `TemplateNode.aria_controls` 字符串拷进对应 `RoleInfo.aria_controls`（随模板迁移的纯数据，
  同 role/data-slot 模式）。**不**在 ControlState 缓存解析后的 panel NodeId——panel 解析推迟到
  `sync_control_visuals`（§5.7）每帧按需 `find_by_id_attr(aria_controls)` 动态查（面板可被
  add/remove，缓存会 stale）。

**pkg 版本**：v28 → v29（TemplateNode 布局变）。加 bincode 稳定性测试（序列化形状变就红，
勿撞运行时 BadKind）。`MIN=MAX=29`，一刀切不向后兼容（个人项目惯例）。

**替代方案（否决）**：index-based 关联（panel 须是 tablist 子节点按序对齐）——会强制重写
settings.html 结构 + 违反 WAI-ARIA 标准（panel 可任意位置）。

### 5.5 click 激活

`crates/core/src/input.rs`：pointer-down 命中 `NodeKind::Tab` → 找父 TabList → 设
`selected_index = 该 tab 序号`。镜像 Dropdown 的「pointer-down → open=true」落点。
（Tab 是独立 NodeKind，非 Button 变体——命中层按 `NodeKind::Tab` 分派，不误走 Button 逻辑；
详见 §10 R2。）

### 5.6 键盘导航（K1）

`crates/core/src/input.rs:563` 附近（Dropdown 键盘路由同处）加 TabList 键盘路由：
当焦点在 TabList 子树内，方向键（水平 tablist 用 Left/Right、垂直用 Up/Down——按 tablist
的 flex-direction 判）移动 selected_index（clamp 到 tab 数），自动激活。
**不做** Tab/Shift+Tab 焦点链、不做 Home/End、不做 roving tabindex（defer §9）。

方向判定：tablist 的 `flex-direction:row/column`（默认 row）。垂直 tablist（column）用 Up/Down。

### 5.7 panel 显隐（P1）

**不变量**：复用 display:none layout 剪枝；不加 hidden 字段；不动作者 CSS。

**机制**：control-update pass（`sync_control_visuals`，Dropdown 管 open/selected_index 的同阶段）里，
TabList 按 `selected_index` 遍历自己的 role=tab 子节点，对每个 tab 读其 `RoleInfo.aria_controls`
（panel id 字符串）→ `find_by_id_attr` 解析 panel NodeId（每帧动态查，不缓存）→ 切该 panel 的 display：
- tab 序号 == selected_index → 该 panel 的 resolved display 置回自然值（清掉 control 强制的 none）。
- tab 序号 != selected_index → 该 panel 的 resolved display = none。

（「resolved display」= ResolvedStyle 里 layout 实际读的 display 值，区别于 CSS 声明的 display；
control 只写 resolved 层，不进 base_style，与 rematch 每帧从 base_style 重起不冲突。）

**落点选择**（实现时按现有 pipeline 顺序 `process→rematch→solve→...` 定，spec 锁不变量不锁
落点）：control 驱动的 display 写入 resolved style，让 solve 的 display:none 剪枝天然吃掉。
两种候选：
- (a) control-update pass 直接写 panel 的 resolved `display`（新方向：control 影响 style）。
- (b) rematch 阶段 panel resolve display 时查 controller back-ref 的 selected_index。

推荐 (a)（control-update 已是 Dropdown 改 open 的地方，集中；back-ref 需 panel 知道自己的
controller，额外维护）。实现采用 (a)，通过 `set_inline_override(scene, panel, "display:block/none")`。

### 5.8 事件

selected_index 变 → 发 `EVT_SELECTION_CHANGED`（复用 Dropdown 的，`input.rs:111`），
payload `touch_id = 新 selected_index`（usize→i32，tab 数远小于 i32 范围）。
C# 侧 `TabList.SelectionChanged`（typed 事件层 demux，见 §5.9）。

### 5.9 C# 投影

- 新增 `LoomGUI.Nodes.cs`（或对应文件）`TabList` / `Tab` public class（projection-layer 模式）。
  `TabList.SelectedIndex`（get/set，FFI 读写 selected_index）+ `SelectionChanged` typed 事件。
  `Tab` 暂无额外 API（无状态）。
- `NodeFactory` dispatch 加 TabList/Tab 分支（照 Dropdown/OptionItem 模式）。
- FFI：加 `loomgui_stage_get/set_tablist_selected_index`（或复用通用 control-state getter/setter，
  核实现有 Dropdown selected_index 的 FFI 路径是否可复用）。

## 6. 文档对齐（本 spec 同步修，不另开 task）

- `docs/roadmap/roadmap.md` §4 tech-debt `[aria-selected] state-attr selector + Node attrs 存储
  + role dispatch` 条：改写为「控件束 P1-P3 已做掉 attr_matches_node role/aria 支持 + 控件 role
  分派；M3 真实缺口 = TabList 复合控件 + aria-selected 跨节点合成 + panel 显隐」。
- `docs/roadmap/milestones.md` M3 exit criteria：删掉已完成的「attr_matches_node 扩展」「role
  dispatch」两条；保留并细化「TabList/Tab 投影类」「`[aria-selected]` 命中」；加「pkg v29 bump
  （aria-controls）」「键盘导航 K1」「panel 显隐 P1」。
- `showcase/showcase/settings.html:17` 解注释 `.tab[aria-selected="true"]` 规则（实现完跑通后）。

## 7. pkg 版本与 ABI

- v28 → **v29**（TemplateNode 加 `aria_controls: Option<String>`）。bincode 布局变。
- `PKG_FORMAT_VERSION` (`crates/core/src/asset/mod.rs:25`) 改 29；`MIN=MAX=29`。
- 加/改 bincode 序列化形状测试（`asset/tests.rs:139` 附近）。
- NodeKind 新增两 unit 变体：`#[repr(u8)]` + `from_u8` 同步；bincode serialize 仍 4B（FixintEncoding u32 判别值），不影响 ABI 兼容性断言（Spec-2 已验）。
- FFI 若新增 command，CS binding 走 `cargo run -p xtask -- sync-bindings`。
- 重打所有 fixture pkg.bin（v29）；重编 .dll 入库。

## 8. 测试策略

### 8.1 core 单测（`crates/core/src/`）

- `resolve_semantic`：`div+role=tablist→TabList`、`button/div+role=tab→Tab`、`div+role=tabpanel→Container`。
- `synth_aria_value`：Tab 的 aria-selected 随父 selected_index 翻（selected=true、其余=false、Exists op）。
- `attr_matches_node`：`[aria-selected="true"]` 命中当前激活 tab，不命中非激活 tab 与普通 div。
- control-update：切 selected_index 后，非激活 panel 生效 display=none、激活 panel 自然值（§5.7）。
- click：pointer-down 命中 Tab → selected_index 更新 + SelectionChanged 发出。
- 键盘：方向键移动 selected_index（clamp、水平/垂直分派）。
- sync_control_visuals：aria-controls 字符串 → find_by_id_attr 解析 panel（含找不到 panel 的容错，不缓存 NodeId）。

### 8.2 fence 单测（`crates/fence/`）

- `ROLE_TO_SEMANTIC` 含 tablist/tab。
- 结构契约：tablist 须含 tab 子；tabpanel 可跨树（不强制是 tablist 子）。
- aria-controls idref 校验仍工作（structural.rs:140 现有逻辑不回归）。
- 文档↔schema 交叉校验门（`doc_schema_sync.rs`）同步 fence.md。

### 8.3 C# Headless 测试（`tests/dotnet/LoomGUI.HeadlessTests/`）

- 实例化含 tablist 的 pkg → `Get<TabList>("...")` 命中。
- `SelectedIndex` get/set 往返。
- `SelectionChanged` typed 事件在 set SelectedIndex 后触发。
- `[aria-selected]` computed style 随 SelectedIndex 变（用 `get_node_computed_style` 出口断言）。

### 8.4 Unity 真机验收（家里机，defer 到 M0 验收窗口）

- settings 页 PlayMode：点 tab → 高亮 + 对应 panel 显示、其他 panel 隐藏。
- 方向键切 tab。
- `[aria-selected="true"]` CSS 命中（settings.html:17 解注释后）。

### 8.5 showcase 整包

重打 showcase.pkg.bin（v29）；settings 页跑通。

## 9. 实现顺序（建议）

1. **pkg v29 + aria_controls 字段**：TemplateNode 加字段 + fence extract + packer bridge + bincode 测试 + 重打 fixture。先把数据通路铺好。
2. **语义枚举链 + role 分派**：SemanticKind 变体 + ROLE_TO_SEMANTIC + map_semantic + NodeKind 变体 + kind_tag/from_u8 + fence 结构契约。
3. **ControlState::TabList{selected_index} + RoleInfo.aria_controls（instantiate 拷贝）**。
4. **aria-selected 跨节点合成** + attr_matches_node 集成（headless 断言 `[aria-selected]` 命中）。
5. **panel 显隐 P1**（control-update pass 翻 display）。
6. **click 激活 + 事件**。
7. **键盘 K1**。
8. **C# 投影 + FFI + Headless 测试**。
9. **文档对齐 + settings.html 解注释**。
10. Unity 真机验收（家里机）。

每步可独立 headless 验证（core dump / HeadlessTests），符合两台机约束。

## 10. 风险与缓解

- **R1 · aria-controls 解析失败**：tab 写了 aria-controls 但找不到对应 id 的 panel。
  容错：`sync_control_visuals` 中 `find_by_id_attr` 返 None 时 `continue` 跳过该 tab（该 tab 选中时无 panel 可显隐，不 panic）；fence 期已校验 idref 存在，运行时动态缺则静默跳。
- **R2 · Tab 的 NodeKind 与 button tag**：settings.html 用 `<button role=tab>`。resolve_semantic
  现在 role 优先于 tag，故 button+role=tab → Tab（非 Button）。核实命中层按 NodeKind::Tab 分派
  click（不误走 Button 逻辑）。tab 也可写在 `<div role=tab>` 上——两种 tag 统一映射 Tab。
- **R3 · panel 显隐落点（§5.7 a/b）**：control 写 resolved display 是新方向，核实不破坏
  rematch「每帧从 base_style 重起」契约（display 写在 resolved 层、不进 base_style，每帧重算，
  与 base_style 重起不冲突）。
- **R4 · 垂直/水平 tablist 方向误判**：flex-direction 缺省/异常时键盘方向退化（默认 row）。
  低风险，单测覆盖 row/column 两态。
- **R5 · clone_node_recursive**（List item 模板内含 tablist 时）：RoleTable 已克隆（commit
  `582ba8c`），但 ControlState::TabList{selected_index} 克隆/re-init 需核实（dynamic.rs:311 注释警告
  「RoleTable 复制只解锁 role/slot 定位；完整视觉正确性需补 ControlState 克隆」）。tablist
  在 list item 模板里出现的概率低，若 showcase 不触发则 defer。

## 11. 不做的事（显式 defer）

- **Tree 复合控件**（P4 第二棒）：按需，character 技能树/背包分类树真需要再提。
- **roving tabindex / Home/End / Tab 焦点链**：重 a11y，游戏 UI 不需要。
- **通用 Node attrs 仓库（β）**：等 Tree/Accordion 等第二个非控件状态属性用例出现再补；本 spec
  用特定 aria_controls 字段（α 精神），不预通通用机制。
- **`aria-labelledby`**（tab↔panel 反向关联）：fence 已识别（structural.rs:140），运行时未用；
  与 aria-controls 同机制，将来需要时一并补。
- **手动激活模型（manual activation：方向键只移焦点、Enter 才选中）**：K1 只做自动激活。
