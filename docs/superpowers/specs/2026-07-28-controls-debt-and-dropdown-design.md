# 控件债收口 + Dropdown 全栈 设计

> **状态**：设计待 review
> **范围**：roadmap §4 控件束 P3。一个 spec 一批做掉——A 控件债收口（5 项小修）+ B Dropdown/OptionItem 全栈（含 scrollbar 模式浮层基建）。
> **不在范围**：TabList/Tree + WAI-ARIA 复合控件（roadmap §4 P4 独立线，牵出 Node attrs 存储 + role dispatch + `[aria-selected]` 运行时匹配，见 roadmap tech-debt `[aria-selected]` 条）。

---

## 1. 背景与动机

控件束 P1（ProgressBar/Toggle/Slider/RadioButton）+ P2（TextField/Password/Search/TextArea + IME）已完成。剩两类缺口：

- **散落的小债**：NumberField 全 NE、文本控件 3 个 getter 缺口、3 个控件 C# 投影类 fallback Container、UIStyleException 未定义、攒批回写 + set_transform 接通（摸黑期 deferred 的债，Slider/TextField 已是高频改值控件，债已欠）。这些修法明确、互相独立、纯编码机可验。
- **Dropdown 全栈**：`<select>`/`<option>` 当前只有 NodeKind enum 标签（`scene/node.rs:104`）+ tag 映射，零行为逻辑（`ControlInit`/`ControlState` 无 Dropdown 变体，`inject_control_children` 走空分支，C# 全 NE）。它的前置是**浮层渲染基建**——这是当前最大的渲染管线空白，也是以后所有浮层控件（tooltip/context-menu/dialog）的公共基建。

两件事技术线单一（渲染管线 + 控件行为，不碰 cascade/attr 主路径），合并一个 spec 交付。

## 2. 设计原则（对照项目既有不变量）

本 spec **不发明新机制**，全部沿用项目既有模式：

| 问题 | 既有先例 | 本 spec 套用点 |
|---|---|---|
| 浮层（popup 脱离文档流画最上） | **scrollbar thumb 模式**（`render/mod.rs:1019` 末尾追加 + `max_sort+1`；`hit.rs:48` 前置 check；`scroll.rs:60` 合成 NodeId flag）| Dropdown open popup 子树 |
| 控件注入视觉子节点 | **`inject_control_children`**（ProgressBar→`.loom-fill`、Slider→`.loom-track/.loom-thumb`、Toggle/Radio→`.loom-check`）| Dropdown→`.loom-value` + `.loom-popup` |
| 控件运行时状态 | **`ControlState` side table**（`HashMap<NodeId, ControlState>`，不进 Node struct）| `ControlState::Dropdown` |
| 控件初始值进 pkg | **`ControlInit`**（打包期 bridge 提取 HTML 属性 → pkg.bin → instantiate 填 side table）| `ControlInit::Dropdown` |
| 攒批 flush seam | projection-layer §2「StyleMirror 稀疏镜像 + FlushInline seam」（`StyleMirror.cs:60` setter 立即过桥，注释已写升级路径）| setter 改标脏 + 帧末 flush |
| set_transform 数值 FFI | `loomgui_stage_set_transform`（`ffi/src/lib.rs:2282`，已实现；C# 未接通）| C# NodeTransform.Store 接通 |

## 3. A 部分：控件债收口

五项独立小修，每项根因与修法都已锁定。

### 3.1 NumberField（`<input type="number">`）

**根因**：`NodeKind::NumberField` 有 enum 标签 + tag 映射，无 number 专属逻辑（min/max/step/值解析/clamp/量化）。C# 全 NE。

**修法**：NumberField 本质是 TextField + 数值约束层。
- `ControlState::TextField(EditState)` 复用——number 的 value 仍是字符串（`"3.14"`），EditState 存原始文本。数值约束在**读写门**做：`get_control_value`（FFI）解析为 f32 + clamp `[min,max]` + step 量化；`set_control_value`（FFI）把 f32 格式化回字符串写 EditState。
- min/max/step 走 EditState 之外的字段。**决策**：不复用 EditState（它是纯文本编辑态），给 TextField 变体扩 numeric 元数据。最小改动：`ControlState` 加 `NumberField { edit: EditState, min, max, step }` 变体（ControlInit 同步加 `NumberField(EditInit, min, max, step)`）。
- 围栏：`FenceControlWithoutCss` 的 `CONTROL_KINDS` 已含 TextField？——查现状不含（NumberField 未实装前没加）。本 spec 把 NumberField 加进 `CONTROL_KINDS`（走文本控件分支教学文案：background/border + caret-color）。
- 键盘：NumberField 只接受数字键、`-`、`.`、`e`（科学记数法）。非数字字符输入被 filter（core 字符输入通道 `set_input` 路由处加 NumberField guard）。

### 3.2 文本控件 getter 缺口（ReadOnly/Disabled getter + Blur）

**根因**：core 数据齐全（`NodeFlags::DISABLED`（`node.rs` bitflags）+ `EditState.readonly`），setter FFI 齐全（`set_node_disabled`、`set_control_readonly`），**只缺导读侧 FFI**。

**修法**（新增 FFI 导出，照 `get_control_text` 模式）：
- `loomgui_stage_get_node_disabled(h, node_id, *out_bool) -> i32`：读 `NodeFlags::DISABLED`。
- `loomgui_stage_get_control_readonly(h, node_id, *out_bool) -> i32`：读 `EditState.readonly`（仅 TextField/TextArea/NumberField，其他 kind 返错码）。
- `loomgui_stage_blur(h) -> i32`：清除当前 focus 节点的 FOCUSED flag（现有 `request_focus` 的反向操作；focus 管理在 core，查 `stage.rs` focus 字段）。
- C# getter 改读 FFI（TextField/TextArea/Slider/Toggle/RadioButton/NumberField 的 `Disabled` getter；TextField/TextArea/NumberField 的 `ReadOnly` getter；TextField/TextArea 的 `Blur()`）。

### 3.3 控件 C# 投影类补齐（OptionItem / Slot / CustomElement）

**根因**：`NodeFactory.cs:66` 这三个 arm 各 `new Container(ctx, id)`（fallback）。Rust 有对应 NodeKind 变体（`scene/node.rs` OptionItem=12、Slot=16、CustomElement=17），C# 无专用 class。

**修法**：
- OptionItem：作为 Dropdown 的子项，加 `public class OptionItem : Container`（带 `Value`/`Selected`/`Disabled`/`Index` 只读属性，从 core 读）。本 spec 与 Dropdown 同批做（3.3 与 §4 联动）。
- Slot：加 `public class Slot : Container`（复合束 slot 内容投影用，本 spec 只加壳占位，投影机制留复合束）。
- CustomElement：加 `public class CustomElement : Container`（复合束 custom element 用，本 spec 只加壳占位）。
- NodeFactory 三个 arm 改 dispatch 到新 class。

### 3.4 UIStyleException

**根因**：`public-api.md §1.4` 声明 4 种运行时异常，`Types.cs` 只有 `UIContractException` + `UIPackageException`。缺 `UIStyleException`（运行时 CSS 解析失败用，如 `StyleSheet.Add` 解析失败）。

**修法**：`Types.cs` 加 `public class UIStyleException : Exception`（照 `UIContractException` 模板：构造器 + message）。`StyleSheet.Add`（`LoomGUI.Nodes.cs:2170` 当前 `throw NE()`）将来接通时换抛此异常。本 spec 只补类定义，不接 StyleSheet.Add（那是运行时 CSS 解析的活，留后）。

### 3.5 攒批回写 flush + set_transform 接通

**根因 A（攒批）**：`StyleMirror.Set`（`Projection/StyleMirror.cs:60`）每次 setter 立即 `FlushInline()` 过桥——高频改值（Slider 拖拽、TextField 输入）每帧多次 FFI。projection-layer §2 已写升级路径：setter 改标脏、帧末批量 flush。

**修法 A**：
- `StyleMirror` 加 `_dirty: bool`（或 NodeRegistry 持 dirty StyleMirror 集）。`Set`/`Unset` 只写 `_set[prop]` + 标脏，删立即 `FlushInline()`。
- `LoomHost.Step`（`Host/LoomHost.cs`，frame 序的 tick 前或后）插帧末 flush：扫所有脏 StyleMirror 调一次 `FlushInline()`。
- getter 契约不变（稀疏镜像仍存在，FlushInline seam 保留）。

**根因 B（set_transform）**：FFI `loomgui_stage_set_transform(h, node_id, tx, ty, sx, sy, rot)` 已实现（`ffi/src/lib.rs:2282`，走 `set_user_transform` 只写 user_transform 不触发 solve）。C# DllImport 已声明（`LoomGUIBindings.cs:735`）。但 C# `NodeTransform.Store`（`LoomGUI.Nodes.cs:680`）只写镜像字段 + 标 `_dirty`，**从不调 FFI**。

**修法 B**：
- `NodeTransform.Store` 改为：写镜像 + 标脏（同 StyleMirror 模式），帧末由 LoomHost flush 调 `loomgui_stage_set_transform`。
- **Origin 缺口**：FFI 签名只有 6 个 f32（tx,ty,sx,sy,rot），无 origin；C# `NodeTransform.Origin` 是公共签名一部分。修法：FFI 加 `ox: f32, oy: f32` 两个参数（origin 在 translate/scale/rotate 前先减），core `set_user_transform` 扩展接收 origin。**这是唯一需要改 FFI 签名 + 重编 dll 的点**。

## 4. B 部分：Dropdown 全栈

### 4.1 子树结构（套 `inject_control_children` 模式）

```
<select>  (用户节点，position:relative 作 absolute containing block)
  ├─ .loom-value   (core 注入：收起态显示当前选中 option 的文本)
  ├─ .loom-popup   (core 注入：展开浮层容器；收起态 display:none)
  │    ├─ <option> (用户的真 DOM 子节点，渲染时摘进 popup)
  │    ├─ <option>
  │    └─ ...
  └─ .loom-arrow   ← 不注入（见 §4.6）
```

- option 是用户写的真 DOM 子节点（围栏 `<select>` ContentModel = `Only([option])` 已校验，`fence/schema/tag.rs`）。
- `.loom-value` / `.loom-popup` 由 `inject_control_children` 注入（同 ProgressBar 注入 `.loom-fill`）：`make_child(scene, CLASS)` + `append_child` + `set_inline_override`。按 class 定位（`find_child_by_class`），不带 id，不污染用户命名空间。
- `.loom-popup` 默认 `position:absolute`（脱离 flex 流，锚定 select）。收起态 `.loom-popup` 设 `display:none`；展开态移除该 override。

### 4.2 浮层渲染（套 scrollbar thumb 模式）

scrollbar thumb 是项目"脱离文档流的顶层对象"的唯一先例（`render/mod.rs:1011-1029`）。Dropdown popup 是它的自然扩展（scrollbar 追加单个 quad，popup 追加一段子树 DFS）。

**`build_render_nodes` 改动**（`render/mod.rs`）：
1. 正常 DFS（`assign_sort_keys`）**跳过 open popup 子树**：open 的 Dropdown 的 `.loom-popup` 子树在 DFS 时不 assign sort_key、不进 RenderNode vec（像 pruned `display:none` 子树一样处理）。
2. 整树 DFS + `reorder_for_batching` + `merge_meshes` 跑完后，算 `max_sort`。
3. **末尾追加循环**：对每个 open Dropdown，对它的 `.loom-popup` 子树跑一遍独立 DFS（复用 `assign_sort_keys` 的逻辑，counter 从 `max_sort+1` 续），产出的 RenderNode push 进 vec。popup 子树的 `mask_context = MaskContext(0)`（**跳出所有祖先 overflow:hidden clip**，同 scrollbar thumb）。

**为什么 popup 跳出 clip**：dropdown 常出现在 scroll 容器/固定高度面板里，展开的列表要溢出父边界显示。scrollbar thumb 也是这个语义（画在容器边缘外）。一致处理。

**Unity 后端镜像**：MirrorPool 按 node_id 去重建 GameObject。popup 子树的 RenderNode node_id 是真节点 id（option、`.loom-popup` 都是真 DOM 节点，有真 NodeId），后端镜像逻辑零改——只是这些节点在某些帧出现/消失（open/close），走现有 change_level diff 路径。

### 4.3 命中层（套 scrollbar thumb 前置 check）

`hit.rs:hit_test` 当前前置 `hit_scrollbar_grip`。加一个**前置 popup 命中 check**：
- open 时，先测 popup 区域：命中 popup 内 option → 选中该 option + 关闭；命中 popup 容器空白 → 不关闭（点在 popup 里）；popup 外 → 触发 outside-click 关闭。
- popup 命中走正常 `hit_subtree`（option 是真节点，有 layout_rect）——不需要合成 flag id（scrollbar thumb 要 flag 是因为 thumb 不是真节点；popup 的 option 是真节点）。

### 4.4 选中态（套 RmlUi 双存储 + ControlState side table）

**`ControlState::Dropdown`**（`scene/node.rs`）：
```rust
Dropdown {
    selected_index: usize,      // 当前选中项（option 在 select 子节点里的索引）
    open: bool,                 // popup 是否展开
    value_lock: bool,           // 防 SetSelection→SetValue 反馈环（RmlUi lock_selection 模式）
}
```
**`ControlInit::Dropdown`**（`asset/mod.rs`，打包期载荷）：
```rust
Dropdown { selected_index: usize }  // 从 <option selected> bake
```

- 打包期 bridge：扫 select 的 option 子节点，`selected` 属性的 option 索引 → `selected_index`（无 selected 则 0，RmlUi 语义：默认选第一项）。
- 双存储（RmlUi 模式）：`selected_index` 为权威源；option 的 `selected` 属性是初始值。改选时设 `value_lock` 防 OnValueChange 回写再触发 SetSelection。
- `sync_control_visuals`：`selected_index` → `.loom-value` 显示对应 option 的文本内容（读 option 子节点的 text content）+ 给选中 option 加 `.loom-selected` class（或设 inline override）。

### 4.5 交互

| 触发 | 行为 |
|---|---|
| 点击 select（收起态） | open = true，注入 `.loom-popup` 显示 |
| 点击 select（展开态） | open = false |
| 点击 option（popup 内） | selected_index = 该 option 索引 + value_lock + 发 SelectionChanged + open = false |
| 点击 popup 外 | outside-click → open = false |
| Esc | open = false（回滚到打开时的 selected_index，RmlUi `CancelSelectBox` 语义）|
| Up/Down（open 时） | seek 选中超前一/后一个非 disabled option（RmlUi `SeekSelection`），高亮但不提交 |
| Enter（open 时） | 确认当前高亮 option + 关闭 |

事件：core 新增 `EVT_SELECTION_CHANGED`（携带 node_id + new_index），C# `Dropdown.SelectionChanged` typed event（照 Slider ValueChanged backing-dict 模式）。

### 4.6 不注入 `.loom-arrow`（设计决策）

**决策**：Dropdown 只注入 `.loom-value` + `.loom-popup`，**不注入箭头**。理由：
- 游戏 UI 的 dropdown 形态多样：图标选择器、卡片列表、轮盘式选择器——很多没有标准 HTML form 的下拉箭头。注入 `.loom-arrow` 是把标准 HTML form 的形态偏见强加给所有 dropdown。
- 一致性：ProgressBar/Slider 不预设颜色、TextField 不预设光标样式——控件不预设装饰外观是项目一贯原则。箭头是装饰，不是功能必需（不注入 select 仍能工作）。
- **AI 先验对齐靠围栏教学文案**（见 §5），不靠注入结构槽。

### 4.7 pkg 格式影响

- `ControlInit` 加 `Dropdown` 变体 → bincode 形状变 → **pkg 版本 bump**（当前 v25 → v26，`asset/mod.rs` `PKG_FORMAT_VERSION`，`MIN=MAX=v26` 一刀切）。
- option 的 `selected`/`value`/`disabled` 初始属性：option 是 select 的真 DOM 子节点，正常序列化为 TemplateNode（content 字段存文本）。option 的 `selected` 属性由 bridge 在打包期读取算 `selected_index`，不单独进 pkg（runtime 以 selected_index 为权威）。
- NumberField 的 `ControlInit::NumberField` 变体同此 bump 一并进。

## 5. 围栏拦截（`FenceControlWithoutCss` 扩展）

**现状**：`fence/src/control_css_check.rs` 的 `CONTROL_KINDS` 含 ProgressBar/Slider/Toggle/RadioButton/TextField/PasswordField/SearchField/TextArea，**不含 Dropdown/NumberField/OptionItem**。

**改动**：
1. `CONTROL_KINDS` 加 `SemanticKind::Dropdown` + `SemanticKind::NumberField`。
2. `has_injected_children` 加 `Dropdown`（注入子节点型：`.loom-value` + `.loom-popup`）。NumberField 走文本控件分支（无注入子节点）。
3. `loom_children_hint` 加 Dropdown 分支：
   ```
   SemanticKind::Dropdown => "`.loom-value` (shows selected text) and `.loom-popup` (the popup list container); `<option>` children also need CSS"
   ```
4. 教学文案**显式说明无原生箭头**（关键：对齐 AI 先验，防"我的 select 没箭头"困惑）。Dropdown 的 `fix_hint` 单独分支：
   ```
   "Provide CSS for <select> (background/border so the box is visible) and for its
    internal `.loom-value` and `.loom-popup` child elements. LoomGUI dropdowns have NO
    built-in arrow indicator — if you want one, draw it yourself via CSS (e.g. a
    background-image on `.loom-value`, or an extra child element). `<option>` children
    also need CSS (they are normal DOM children of <select>)."
   ```
5. **OptionItem 不加进 CONTROL_KINDS**：option 是普通 DOM 子节点（像 div），不配 CSS 也能渲染（继承样式 + 默认 block）。强制配 CSS 会和普通 div 不一致。靠 Dropdown 报错时的教学文案提示"option 也要配 CSS"即可。

**为什么围栏只拦 select 不拦 `.loom-*` 子节点**：`.loom-value`/`.loom-popup` 是 core 运行时注入的，打包期 fence 解析时它们不在 HTML 里（围栏只看用户写的 HTML）。所以围栏只能保证 select 本身有 CSS 命中，`.loom-*` 配没配靠教学文案——这和 ProgressBar/Slider 完全一致（现有不变量）。

## 6. 数据流（Dropdown 生命周期）

```
打包期：
  HTML <select><option selected>A</option><option>B</option></select>
    → fence 校验（ContentModel Only([option]) + FenceControlWithoutCss select 有 CSS）
    → bridge：扫 option[selected] 算 selected_index → ControlInit::Dropdown{ selected_index: 0 }
    → pkg.bin v26（TemplateNode kind=Dropdown + ControlInit + option 子节点）

运行时实例化：
  instantiate(pkg) → ControlInit::Dropdown → ControlState::Dropdown{ selected_index, open:false, value_lock:false }
    → inject_control_children：注入 .loom-value + .loom-popup
    → sync_control_visuals：selected_index=0 → .loom-value 文本="A"

tick（收起态）：
  正常 DFS（.loom-popup display:none 子树被 pruned）→ 渲染 select + .loom-value="A"

用户点击 select → open=true：
  - set .loom-popup 移除 display:none override
  - 下个 build_render_nodes：正常 DFS 跳过 popup 子树，末尾追加 popup 子树 DFS（max_sort+1, mask=0）
  - 渲染：select + .loom-value="A" + 浮层 popup（option A 高亮 + option B）

用户点击 option B：
  - selected_index=1 + value_lock=true + 发 EVT_SELECTION_CHANGED
  - sync_control_visuals：.loom-value 文本="B"
  - open=false → .loom-popup display:none
  - 下帧正常 DFS（popup pruned）
```

## 7. 测试策略

### 7.1 core 单测（`crates/core/src/`）

- Dropdown ControlState 读写：selected_index / open / value_lock。
- inject_control_children：Dropdown 注入 `.loom-value` + `.loom-popup`，结构正确（class 定位）。
- build_render_nodes 浮层：open popup 子树 sort_key > 所有正常节点；mask_context=0；收起态 popup 不渲染。
- hit_test：open 时 popup 区域命中 option；popup 外 outside-click 关闭。
- sync_control_visuals：selected_index → `.loom-value` 文本同步。
- NumberField：value 解析/clamp/step 量化；非数字输入被 filter。
- 新 FFI 导出：get_node_disabled / get_control_readonly / blur。

### 7.2 fence 单测（`crates/fence/`）

- Dropdown 无 CSS → `FenceControlWithoutCss` error。
- Dropdown 有 CSS（tag/class/id/后代命中）→ pass。
- NumberField 无 CSS → error。
- 教学文案含"NO built-in arrow"措辞（防回归）。

### 7.3 C# Headless 测试（`tests/dotnet/LoomGUI.HeadlessTests/`）

- NumberField Value get/set + clamp。
- TextField/TextArea ReadOnly/Disabled getter + Blur。
- OptionItem/Slot/CustomElement 投影类实例化 + NodeFactory dispatch。
- 攒批 flush：连续多次 StyleMirror.Set 只触发一次帧末 FFI。
- set_transform：NodeTransform.Store 后 world_matrix 更新。

### 7.4 Unity 真机验收（家里机）

- Dropdown 完整交互：点击展开 → 选 option → 收起 + `.loom-value` 更新 + SelectionChanged 事件。
- popup 浮层视觉：溢出父 overflow:hidden 容器正确显示在最上。
- 键盘 Up/Down/Enter/Esc。
- NumberField 输入 + clamp。

### 7.5 showcase 整包

- `cargo run -p loomgui_pkg -- build showcase` exit 0（showcase 的 2 个 select + 5 个 number 不再是空行为）。

## 8. pkg 版本与 ABI

- pkg v25 → **v26**：`ControlInit` 加 `Dropdown` + `NumberField` 变体（bincode 形状变）。`PKG_FORMAT_VERSION` MIN=MAX=26，一刀切，弃 v25（个人项目不留迁移器，加 bincode 稳定性测试）。
- FFI：新增 `get_node_disabled` / `get_control_readonly` / `blur`；`loomgui_stage_set_transform` 签名扩展（加 ox/oy）。重编 dll + `xtask sync-bindings`。
- NodeKind enum 不变（Dropdown/NumberField 变体早存在）。

## 9. 实现顺序（建议）

1. **A 部分（控件债收口）先做**：都是独立小修，先落地能把"文本/数值输入 + 投影债"收口。NumberField 的 ControlInit 变体和 Dropdown 一起 bump v26。
2. **B 部分 Dropdown**：
   - 4.1 注入子树结构（inject_control_children）
   - 4.2 浮层渲染（build_render_nodes 末尾追加）
   - 4.3 命中前置 check
   - 4.4 ControlState + sync_control_visuals
   - 4.5 交互 + 事件
   - 4.6 围栏拦截
3. **pkg v26 bump + dll 重编**（A/B 合并一次 bump）。
4. C# 投影 + Headless 测试。
5. Unity 真机验收（家里机）。

## 10. 风险与缓解

| 风险 | 缓解 |
|---|---|
| 浮层"末尾追加子树 DFS"破坏现有 batch/merge 假设（batch 依赖 sort_key 单调连续） | scrollbar thumb 已是末尾追加模式（`render/mod.rs:1019`），证明 append-after-merge 可行；popup 复用同路径。先写 core 单测验 rect + sort_key，再接 Unity |
| popup 子树 DFS 复用 `assign_sort_keys` 时 counter 续号 + clip 状态污染 | popup 子树强制 `MaskContext(0)` + 独立 accumulated=None，不继承父 clip 链 |
| set_transform 加 origin 改 FFI 签名，binding 要重新生成 | xtask sync-bindings 流程已成熟（Spec-4a 起）；origin 是追加参数，旧 caller 传 0 等价 |
| Dropdown 交互复杂（键盘/outside-click/反馈环）状态多 | 照 RmlUi `WidgetDropDown` 逐条迁移，单测覆盖每个触发；value_lock 防反馈环 |
| NumberField 数值约束与 IME/字符输入冲突 | NumberField guard 加在字符输入通道（`set_input` 路由），IME composition 期间不 filter（commit 时才校验） |

## 11. 不做的事（显式 defer）

- **TabList/Tree + WAI-ARIA 复合控件**：roadmap §4 P4 独立线（Node attrs 存储 + role dispatch + `[aria-selected]` 匹配）。
- **`.loom-arrow` 注入**：见 §4.6，纯靠围栏教学文案。
- **OptionItem 强制 CSS 校验**：option 是普通 DOM 子节点，不强制（见 §5）。
- **StyleSheet.Add 接通 UIStyleException**：运行时 CSS 解析的活，本 spec 只补异常类定义。
- **Slot/CustomElement 的投影机制**：本 spec 只加 C# 壳，投影/注册机制留复合束。
- **popup 定位启发式（above/below/shrink）**：RmlUi 按 viewport 边距选展开方向。本 spec 先做"向下方展开"单一方向，定位启发式留后续（true 上线后看是否需要）。
