# 组件系统：Custom Element + `<slot>` 投影 + scope 三件套（复合束）

- 日期：2026-08-14
- 状态：**实施中**（feat/component-system 分支，T0–T7 分 task 推进）
- 契约依据：main-design §4.3（Get 不穿透组件边界）、§5.4（Shadow DOM 样式边界）、§7.4（Package 注册表承担 `customElements.define()` 角色；未注册元素、无效 slot 打包期报错）；public-api §2（Slot/CustomElement 是 Container 子类；IsScopeRoot 是运行时标记非类型）；fence.md §2.4 hyphen 标签 + §6 `slot` 表。
- 参考对照：RmlUi 模板 = parse 期注入 + `content` 属性单 slot 投影（`Template.cpp:83-110`）；FairyGUI 无投影、组件实例 ID 查找按实例内 scan（`GComponent.GetChildById`）——两者合并成「打包期展开 + 实例内作用域查找」。

## 1. 动机与验收载体

- 里程碑 2 门点名「scope 三件套完成（组件封装真正成立）」；当前 `components/` 目录被刻意排除出打包扫描，hyphen 标签平掉当普通容器，`<slot>` 渲染 fallback，`LOOKUP_SCOPE` 位挂了没人消费。
- showcase 重复源：7 页顶部导航逐页复制粘贴、character 页 stat 行 ×3 手抄、lab 页 item-card 是"期望式"摆设。
- 验收载体：nav-bar 组件化铺 7 页 + lab item-card 真投影 + character stat-bar×3；rect-diff 对齐浏览器；cargo/dotnet 全绿。

## 2. 设计决策（定案）

### 2.1 打包期展开（非运行时实例化）

packer 见 hyphen 标签 → 查组件注册表 → 把组件模板子树**内联**进宿主页面的节点树。运行时零新树构建机制（展开产物就是普通 TemplateNode 树），Unity 后端 / MirrorPool / render blob 全部零改动。代价：每处使用内联一份节点（游戏 UI 页量级 ~百节点，可忽略）。FairyGUI 式运行时 CreateObject 克隆被否：需要 pkg 跨组件引用 + 运行时建树机制，复杂度不成比例。

### 2.2 注册 = Package 注册表（`customElements.define` 等价物）

- 组件 = 包内 HTML 资产：每个 package dir 下 `components/` 子目录自动扫描（`showcase/showcase/components/` 现成），文件名 = 标签名，**必须含连字符**。
- 组件文件走与页面完全相同的 fence 管线（parse_template → diagnostics），单根校验由 bridge 既有逻辑强制。
- 同名冲突（含跨 package dir 扫描）→ 打包错误。
- 页面/组件里出现**未注册** hyphen 标签 → 打包错误，发射 fence 预留的 `UnregisteredCustomElement` 诊断码（packer 侧构造）。

### 2.3 展开形状

```html
<!-- 页面写法 -->
<game-item-card id="sword" class="rare">
    <button id="equip" slot="action">装备</button>
</game-item-card>

<!-- components/game-item-card.html（组件模板，含自己的 <style>）-->
<div class="gic">
    <slot name="title"><span class="gic-title">默认标题</span></slot>
    <slot name="action"></slot>
</div>
```

产物节点树：

```
CustomElement host（id="sword" class="rare" custom_tag="game-item-card" component_scope=1）
└── div.gic（组件模板根，host 第一个子）
    ├── span.gic-title        ← fallback 内容（该 slot 无分配子时保留）
    └── Button（id="equip"）  ← light 子拼接进 slot 位
```

- **host 在页面作用域**：页面对 host 的 id / class / inline style / tag 选择器（`game-item-card { … }`）照常生效。
- **组件模板根是 host 第一个子**：组件自身的 class/style 原样带过来。
- host 的 `component_scope` 位 → core instantiate 时打 `SCOPE_ROOT | LOOKUP_SCOPE`（与页面实例根同机制）。

### 2.4 slot 投影（打包期拼接，slot 是编译期糖）

- 组件模板里的 `<slot name="x">`（无 `name` = 默认 slot）在拼接位被**移除**，替换为 host 里 `slot="x"` 的 light 子（文档序保持）；无分配子时保留 slot 的 fallback 子原位拼接；有分配子时 fallback 丢弃（DOM 语义）。
- light 子无 `slot` 属性 / 裸文本节点 → 默认 slot；无默认 slot → **打包错误**。
- light 子的 `slot` 属性指向不存在的 slot 名 → **打包错误**（§7.4「无效 slot 打包期报错」）。
- **页面级（非展开上下文）出现 `<slot>` → 打包错误**（lab 现存"期望式"写法随 T7 改造为注册组件用法）。
- 嵌套组件（组件文件里再用 hyphen 标签）递归展开，展开栈环检测 → 打包错误。
- 产物中不再有 NodeKind::Slot 节点；Slot kind 仅为 fence 期语义保留。

### 2.5 硬墙作用域（本设计最重要的取舍）

投影进组件的 light 子**归组件实例作用域**：

- **CSS**：动态规则按 scope 匹配——组件 `<style>` 规则以「锚定规则表」随展开实例包装（scope_root=展开域根），能样式化投影内容（普通选择器即可，比 Web `::slotted()` 更直接）；页面规则止步于 host 边界，**不能**样式化投影内容。可继承属性与 `--*` 经现有非 scope-aware 继续传播自然跨界（符合 §5.4）。
- **查找**：投影子 id 进入组件实例查找域。driver 经两跳访问：`page.Get<CustomElement>("sword").Get<Button>("equip")`。
- **与 Web Components 的有意分歧**：真实 DOM 的 light 子留在 light tree、由外层 sheet 样式化、`querySelectorAll` 可达（flat-tree 双域模型）。拼接式实现撑不起双域（要么给每个节点挂双 scope + 查找走 flat tree——core 大改；要么接受单域）。选单域：规则一条、心智一堵墙，「进组件的就是组件的」。
- 新增打包检查：同一展开域内，light 子 id 不得与组件模板 id 撞车（per-scope 唯一性的补全；跨实例重复合法）。

### 2.6 scope 三件套落点

| 件 | 落点 |
|---|---|
| IsScopeRoot 边界 | `find_node_by_id_in_subtree`（core）DFS 遇 `LOOKUP_SCOPE` 子节点：查其 id 后不再下钻；`Query`（C# DfsPreOrder）经新 FFI `loomgui_node_is_lookup_scope` 同样剪枝。list slot 现有 `LOOKUP_SCOPE` 位随之生效——L3 完成 |
| per-scope ID 去重 | fence per-file 校验 = per-scope 校验（页面、组件各自唯一）+ 2.5 的展开域撞车检查；跨实例重复合法 |
| Shadow DOM 样式隔离 | 现有 `ScopedRule` 机制复用：展开域根 `SCOPE_ROOT` + 组件规则锚定包装 |

### 2.7 aria-controls 串实例雷（探查发现，必修）

TabList panel 解析用全局 `find_by_id_attr`（`scene/control.rs` sync arm）——同组件展开多实例后，全部 tab 会解析到首个实例的 panel。改法：从 tab 节点向上找最近 `LOOKUP_SCOPE` 根，在其子树内（含嵌套边界剪枝）查找。

### 2.8 C# v1 面（仅 typed 投影）

- `CustomElement`（已有空壳）接 `Tag` 属性（FFI `get_custom_tag` 双调法读 `custom_tag`）。
- `RegisterComponent<T>` 类绑定 / OnConnected 生命周期回调 **defer**（判据：dogfood 逼出；契约未承诺；fgui extensionCreator 同演化路径）。生命周期沿用节点契约（Remove/Dispose 可重挂）。
- `Container.GetTemplate`（内联 `<template id>` 取 UITemplate）与本 spec 无关，维持既有 defer。

## 3. pkg v35（34 → 35，MIN=MAX，全部 fixture 重打）

- `TemplateNode` += `custom_tag: Option<String>`（string idx 序列化）+ `component_scope: bool`（u8）。
- `ComponentTemplate` += `component_scopes: Vec<(usize 锚节点 idx, DynamicRuleTable)>`——每展开实例一条，序列化于每组件 DynamicRules blob 之后（count u32 + 每条 anchor_idx u32 + blob len u32 + blob）。
- `Node` += `custom_tag: Option<String>`（instantiate 拷入；rematch tag 选择器匹配 + dump 发射用）。
- blob 版本不变（纯 scene/style 侧，无渲染变更）。
- 组件 `<style>` 规则不进宿主规则表，进锚定表；组件 `@keyframes` **合并入宿主** keyframes（同名宿主胜 + PackWarning；运行时 keyframes 表本就全局按名）。

## 4. 打包错误 / 警告全集

| 场景 | 级别 |
|---|---|
| hyphen 标签无注册组件 | error（`UnregisteredCustomElement`） |
| light 子 `slot` 属性无匹配 slot | error |
| light 子无 slot 属性且组件无默认 slot | error |
| 页面级（非展开上下文）`<slot>` | error |
| 展开环（a 用 b 用 a） | error |
| 组件文件名无连字符 | error |
| 组件名同名冲突 | error |
| 组件模板多根 | error（bridge 既有） |
| 同展开域 light 子 id 与组件模板 id 撞车 | error |
| 组件 keyframes 与宿主同名 | warning（宿主胜） |

## 5. 浏览器预览 / rect-diff 对齐

- `browser-rect.mjs`：Node 侧扫 components/ 目录，`addInitScript` 注入 `window.__LOOM_COMPONENTS__ = { name: html }`。
- `loom-preview.js` 新增 `expandComponents()`（跑在 fillListViews/wireControls 之前）：按注册表 DOMParser 展开 + slot 拼接（同一套语义）；组件 `<style>` 注入 document 但每条选择器加 `[data-loom-comp="name"]` 前缀（导入根打同名 data 属性）防泄漏到页面。
- 手工 file:// 双击预览：无注入源、组件不展开（未知元素渲染 light 子）——文档注明的已接受降级；rect-diff（PlayMode 注入路径）是验收门。
- `normalize-dump-scene.mjs` KIND_TAG：CustomElement → `custom_tag` 字面量；browser 侧 semanticTag 同口径（hyphen 原样），两侧配对。

## 6. Task 序列（feat/component-system）

T0 本 spec → T1 core 地基（pkg v35 读写 + instantiate + FFI）→ T2 lookup 边界（L3 + aria-controls 雷排）→ T3 打包器展开（最大块）→ T4 选择器匹配 + fence/AGENTS 文档 → T5 C# Tag + headless → T6 工具链 → T7 showcase 落地 + 全门禁 + roadmap。每 task commit + 自查 + 测试绿。

## 7. 验收判据

- cargo workspace / fmt / clippy 严门 / dotnet 三套 / PublicApi 编译门全绿。
- showcase 变更页 rect-diff 0 unmatched（浏览器注入展开 ↔ core dump 字面量 tag 配对）。
- 里程碑 2「scope 三件套完成」判据可勾；roadmap 延期表组件系统两项清除（剩余 RegisterComponent 等登记为 dogfood 触发 defer）。
