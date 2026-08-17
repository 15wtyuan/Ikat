# 公共底座 NE 批：调度三件套 + UnloadPackage + option getter + GetTemplate

日期：2026-08-17。状态：DONE（同日实现 + 全门绿：cargo workspace / dotnet 三套 / fmt / clippy 严门；dll、bindings、GUI exe、13 fixtures、showcase pkg 已同步 v37）。

## 0. 背景与目标

里程碑 1 完工后、里程碑 2 dogfood（并行推进中）期间，接通公共 API 里「无触发依赖、任何游戏都受益」的 NE stub 批。本批五件：

1. **调度三件套**：`Node.OnUpdate(Action<float>)` / `UIContext.CallLater(float, Action)` / `UIContext.CallNextFrame(Action)`。
2. **`UIContext.UnloadPackage`**：包生命周期收口。
3. **option getter 批**：`Dropdown.SelectedValue`、`OptionItem.Value`、`OptionItem.Selected`、`Tab.Selected`（含 option `value` 属性全链存储，pkg v37）。
4. **`Container.GetTemplate`**：设计期 `<template id>` 多模板故事的最后一块（`ItemTemplate`/`TemplateSelector` 已在）。

**非目标**（明确不碰）：`ZIndex`（stacking 语义设计件）、`SetVar`/`RemoveVar`/`StyleSheet.Add/Clear`（运行时 CSS 大件）、`Focusable`（焦点域）、`ListView.ItemExitClass`（list 进出场动画）、`UnloadPackage` 的引擎侧资源卸载（见 §2 的事实结论——不存在包级资源）。

## 1. 调度三件套（纯 C# 投影层，零 core/FFI 改动）

### 1.1 决策依据

- 回调是 C# 闭包，core 的 C ABI 存不了；core 侧方案实质 = core 存计时状态 + C# 每帧轮询 due 列表，纯多一层 FFI 与双端维护。
- `LoomHost.Step` 已有两个 C# 侧泵点先例（`FlushPendingWrites` flush seam、`DrainPendingBinds` tick-drain）。
- Godot 后端复用整个 Projection/Public 层——C# 侧调度器自动跨引擎共享。

### 1.2 设计

`UIContext` 新增内部三容器：

- `_updateHooks`：`node_id → List<(Action<float>, 取消令牌)>`。`Node.OnUpdate` 注册，返回 `IDisposable` 订阅句柄（Dispose 撤销）。
- `_timers`：`List<(剩余秒, Action)>`。`CallLater(d, cb)` 追加；泵时全体按 dt 递减，到期即调即删（one-shot）。
- `_nextFrame`：`Queue<Action>`。`CallNextFrame(cb)` 入队；**下一次泵的开头**先排空（帧头 fire 语义）。

**泵**：`internal void PumpLogic(float dt)`，插入 `LoomHost.Step` 的 `CollectInput` 之后、`FlushPendingWrites` 之前。headless 测试在 tick 前手动调（同 `FlushPendingWrites` 模式）。

**帧头泵的语义后果**（契约要点）：回调内改 `Style`/数据 → 走既有 flush seam → **本帧 solve 生效**，零延迟。`CallNextFrame` = 下一帧帧头 fire（本 spec 废弃 stub 注释里的「帧末 fire，先于 render」——`Step` 序列里 solve 后、render 前对 C# 不存在，render 在 core tick 内已 build 完 blob）。

**计时**：与 Step 同一 dt 累积（同源不双钟；TweenManager 单一动画时钟不变量不受影响——它管动画，这里管逻辑调度）。秒、帧级粒度（同 DOM setTimeout 不精确语义）。

**异常隔离**：单回调抛异常 catch + `Debug.WriteLine` 诊断，不阻断其他回调与后续帧（`DrainPendingBinds` 对 BindItem 的先例）。

**清理语义**（契约 §3 已定）：订阅随 `Node.Dispose` 自动清理（`Dispose` 路径里调 `ctx.RemoveUpdateHooks(nodeId)`）；`RemoveFromParent` 不清理；订阅句柄 Dispose 撤销单个订阅。已销毁节点上调 `OnUpdate` 抛 `ObjectDisposedException`（`ThrowIfDisposed` 先例）。

**迭代安全**：泵遍历前对 hooks/timers 拷贝快照（回调内注册/退订不炸遍历；新注册的 hook 下次泵起效，新 timer 同理）。

## 2. UnloadPackage

### 2.1 事实结论（纠正立项时的两个错误前提）

- **atlas 不是包级资源**：`loom.runtime.json` 的 `packages` 与 `atlases` 是 workspace 级平行列表；SpriteResolver 按 `(atlasIdx, page)` 全局懒缓存，与包注册表完全解耦。重载同名包**不会**重载纹理（无泄漏可言）；活实例的 sprite key 走共享 resolver，天然不受卸载影响。→ **无包级资源可释放，「按包释放纹理」架构上不成立也无必要。**
- 字体是 driver 级 `RegisterFont` 注册，不隶属任何包，不动。

### 2.2 设计

- core：`Stage::unload_package(&mut self, pkg: &str) -> Result<(), String>`——`self.packages.remove(pkg)`，不存在返 Err。
- FFI：`loomgui_stage_unload_package(h, name_ptr, name_len) -> i32`。
- C# `UIContext.UnloadPackage(name)`：
  - `!_loadedPackages.Contains(name)` → 抛 `UIContractException`（与 LoadPackage 同名重复抛异常对称，不静默）。
  - FFI 成功后 `_loadedPackages.Remove(name)`。
- 语义（契约 §11.2 不变）：卸载模板注册；已实例化活节点是独立副本不受影响；持有旧 `UITemplate`（pkg+path 模式）再 `Instantiate` → `UIPackageException`（prefab 已删语义）。
- 方法 doc 注释写明「不触碰 atlas/字体」的理由（workspace 级共享、与包生命周期解耦），防后人再立「按包释放」的错命题。

## 3. option getter 批（含 value 存储，pkg v37）

### 3.1 事实

- `ControlInit::Dropdown` 只载 `selected_index`；`value` 属性在围栏/打包器/pkg/运行时**任何一层都不存在**（延期表「option value 存储」即此意）。
- 围栏按**字面 tag**（div）校验属性，`value` 不在 div 白名单 → 今天写了直接 fence error。
- `Tab`/`OptionItem` 的 selected 是**合成值**：父 `ControlState::{TabList,Dropdown}.selected_index` + 自身序号派生（契约明确非字面存储）。

### 3.2 value 存储（v37）

- **围栏**：新增 semantic-scoped 内容属性表（schema 单一真相源）：`OptionItem → ["value"]`。fence_gate 属性校验在 global/structural/content 三档后加第四档「按 resolve 后的 SemanticKind 查表」——`value` 只对 `role=option` 合法，普通 div 仍拒。fence.md 同步（防漂移门含文档↔schema 交叉校验）。
- **打包器**：`extract_control_init` 的 Dropdown arm 增加 `option_values: Vec<Option<String>>`——扫 combobox 下的 option 子，逐个取 `value` 属性（缺席 = None，逐项可混）。与 `dropdown_selected_index` 同一 traversal。
- **core**：`ControlInit::Dropdown { selected_index, option_values }`（serde/bincode，布局变化 → `PKG_FORMAT_VERSION` 36→**37**，单一版本硬门，v36→v37 同今晨 v35→v36 先例）；`ControlState::Dropdown` 增加 `option_values`（instantiate 拷入，运行时只读）。
- **v37 同步链**（机械，照 v36 commit 清单）：bump + 更新 history-roundtrip 断言 + 重打全部 headless fixtures + showcase pkg + 重编 dll + sync-bindings + 重出 GUI exe（静态链 core/fence）。

### 3.3 getter

语义（HTML fallback）：**value = `value` 属性值，缺席时回落 option 文本**（`nth_option_text` 已有）。

- `loomgui_stage_get_dropdown_selected_value(h, dropdown_id, buf, cap, &len) -> i32`：core = `option_values[sel].clone()`，None 回落 `nth_option_text(sel)`；下拉无选项 → rc 特殊值（C# 返 null？——契约 `SelectedValue` 空态：无选项返 `null`）。
- `loomgui_stage_get_option_value(h, option_id, buf, cap, &len) -> i32`：从 option 上溯 combobox（经 listbox），算自身序号 → `option_values[idx]`，None 回落自身文本。
- `loomgui_stage_is_option_selected(h, option_id) -> i32`：上溯 + 序号 == 父 selected_index。
- `loomgui_stage_is_tab_selected(h, tab_id) -> i32`：同构，父 TabList。
- C#：四属性接 FFI（`OptionItem.Value`/`Selected`、`Tab.Selected`、`Dropdown.SelectedValue`）。

## 4. Container.GetTemplate（子树模式，零 core/FFI 改动）

- 实现：作用域内查 `NodeKind::Template` 且 `id_attr == name` 的节点（复用 `Get<T>` 的作用域边界 DFS），取其**单个元素子**（ListItem 蓝图）→ 子树模式 `UITemplate`（`DoInstantiateSubtree` 既有路径，`clone_subtree` FFI）。
- 未找到 → `UIContractException`（对齐 `Get<T>` miss 抛法）。
- 这是契约 §8「item 模板来源 2」的 documented 配套：`view.GetTemplate("name")` → 塞进 `TemplateSelector` lambda。**非目标**：list 下多 template 无 selector 的自动规则（单 template 自动用 / 多个无 selector 抛）不在本批——那是 ListView 内部行为，单独立项。

## 5. 测试

- **cargo**：fence（role=option 带 value 过、普通 div 带 value 拒、文档同步门）；packer（option_values 提取，含混布缺席）；core（unload_package 三态：卸载/不存在/重载；option_values instantiate 拷入；4 个 getter 的 Stage 层测试，含 value 缺席回落文本、无选项空态）；format v37 门 + history-roundtrip。
- **dotnet headless**：调度（OnUpdate 每泵 fire 且 dt 透传；句柄 Dispose 停；节点 Dispose 自动清；CallLater 按 dt 累积到期；CallNextFrame 恰好下泵开头 fire；回调异常隔离；回调内改 Style 本帧 solve 生效——marquee 测试）；UnloadPackage（卸载→重复加载抛→重载成功→旧模板句柄 Instantiate 抛）；option getter（fixture 打 value + 不打两档）；GetTemplate（fixture 含 `<template id>` → 取出 → Instantiate 克隆重叠结构）。
- **PublicApi 编译门**：新公共成员编译过。

## 6. 执行序与验收

1. core + fence + packer（v37 bump + 全链重打包）→ 2. FFI + sync-bindings → 3. C# 投影层（调度器/Unload/GetTemplate/getter 接线）→ 4. 测试补齐 → 5. 验收门：`cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test`（workspace）+ dotnet 三套（Headless/PublicApi/managed）+ dll/bindings/GUI exe/showcase pkg 同步入库。

## 7. 风险

- v37 bump 与并行线（dogfood）的 pkg 冲突：本批 spec 期间并行线已 v36 重打包入库；本批合入时重打一次即收敛。若并行线期间又 bump，按先合先走、后者 rebase 重打处理。
- `CallNextFrame` 帧头语义与任何人旧印象（「帧末 fire」注释）冲突：契约 §9.4 同步改，stub 旧注释删除。
- 泵点遗漏：只有 `LoomHost.Step` 与 headless 手动泵两处入口，无第三 tick 路径（已核实）。
