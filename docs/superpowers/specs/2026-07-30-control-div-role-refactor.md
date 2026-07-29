# 控件重构：抛弃原生标签 + `.loom-*` 注入，改 `div + role` + 作者自写结构

> **状态**：设计探索（2026-07-30）。方向已定（抛弃原生控件标签），细节待决策。明天复盘。

## 起因：当前 `.loom-*` 注入模式违背核心赌注

现状：作者写 `<select>` / `<progress>` / `<input type=range>`，core 运行时**注入固定类名子节点**（`.loom-value`/`.loom-popup`/`.loom-fill`/`.loom-track`/`.loom-thumb`/`.loom-check`），fence 校验作者 CSS 命中这些注入节点。

三个问题：
1. **破坏 AI 先验**：`.loom-*` 是框架私有魔法类名，AI/作者无法从 HTML 预测运行时结构。
2. **浏览器预览 ≠ Unity 渲染**：`.loom-*` 是 runtime 注入的，浏览器 DOM 里没有 → 作者在浏览器看到的（原生 `<select>` 系统下拉）和 Unity 自绘的（`.loom-popup`）完全对不上，无法预览。
3. **违背游戏 UI 现实**：游戏 UI 多样化，根本没有"浏览器默认样式"这回事。`<select>`/`<progress>` 这种"写一个标签出控件"的浏览器便利，正是游戏 UI 要摒弃的。

根因：原生控件标签（`<select>`/`<progress>`/`<input>`）在浏览器里有**系统原生 UI，CSS 样式化能力有限**（select 下拉框完全不可样式化、progress/range 靠 webkit 专属伪元素、checkbox 几乎不能样式化）。框架想用这些语义标签又要自绘 → 只能注入魔法节点 → 破坏一致性。

## 方向：`div + role` + 作者自写结构

控件不用原生标签，用 `<div role="...">` + 作者自写内部结构 + CSS：

```html
<div role="dropdown" class="quality" tabindex="0">
  <div class="quality-current">中等</div>
  <div class="quality-list">
    <div role="option" class="quality-item">低</div>
    <div role="option" class="quality-item">中</div>
  </div>
</div>
```

- **浏览器 == Unity**：两边都渲染 div+CSS（作者写的结构就是最终结构，无 runtime 注入）✓
- **AI 先验**：div + role + class 是标准 HTML / WAI-ARIA，AI 强先验 ✓
- **类名自由**：作者起任何名字，框架靠 `role` 识别语义 ✓
- **行为框架管**：框架识别 `role=dropdown` 接管点击/键盘/选中，作者不写 JS ✓

## 工作量评估（分层）

| 层 | 当前规模 | 改动 | 量级 |
|---|---|---|---|
| core/control.rs | 3235 行 | 删 `inject_control_children`（注入 .loom-*）+ 重写 `sync_control_visuals`（按 role/结构识别作者子节点）| 🔴 ~1500 行重写 |
| core/render/mod.rs | TextField arm + Dropdown 特例 | 控件走普通 Container 渲染（删特例 arm）| 🟡 |
| core/input.rs | 1403 行（文本输入/光标/IME）| **仅当文本控件也纳入才改**（见决策 1）| 🔴 若纳入 |
| fence/schema/tag.rs | resolve_semantic（标签→语义）| 改 **role 属性→语义**，删 select/progress/input 标签映射 | 🟡 |
| fence/control_css_check.rs | 校验 .loom-* CSS | 改校验"role 控件有可见 CSS"（不限类名）| 🟢 |
| packer/bridge.rs | SemanticKind→NodeKind | 映射基本不变 | 🟢 |
| C# Nodes.cs | 控件类 1379 行 | 类保留，内部实现改（不再假设 .loom-* 子节点）| 🟡 |
| FFI | 64 行 + 263 测试 | getter/setter 大部分保留 | 🟢 |
| showcase HTML | 7 文件（form 17 / settings 18 / shop 5 / ...）| 全部重写 div+role | 🔴 机械但量大 |

**粗估**：只改视觉控件 = 40-60 commits（比 P3 的 29 大）；含文本控件再 +50%。

## 待决策（明天定）

### 决策 1：文本控件（text/password/search/textarea/number）纳不纳入？

- **视觉控件**（select/progress/slider/toggle/radio）：浏览器原生 UI 不可控，**必须**改。
- **文本控件**：当前**已不注入 .loom-***（作者只给 input 写 CSS），但用 `<input>` 原生标签。
  - 纳入（`<div role=textbox>`）：彻底一致，但 input.rs 1403 行 + IME/光标/选区全链路重写，**工作量翻倍**。
  - 不纳入：保留 `<input>`，文本框浏览器仍原生，但 LoomGUI 已自绘。**作者负担小、风险低**。
  - **倾向不纳入**：文本控件无 .loom-* 注入痛点，input.rs 是 P2 重资产，推倒风险高。

### 决策 2：框架怎么识别作者结构里的子节点角色？

当前靠 `.loom-*` 固定类名。改 div+role 后三个候选：
- **a) WAI-ARIA 标准 role**（推荐）：`role=option` 是列表项；slider 靠 `aria-valuenow` + 结构。最标准、AI 先验最强，但 ARIA 复合控件学习曲线陡。
- **b) 结构位置约定**：dropdown 第一个子节点=选中框。简单但脆弱。
- **c) `data-slot` 属性**：显式但私有约定。

**倾向 a**：roadmap 本就规划了 P4 WAI-ARIA 复合控件（TabList/Tree），这次正好对齐。

### 决策 3：围栏怎么强制作者写 CSS？

校验 `role=dropdown` 等控件节点有 CSS 命中（不限类名）。"关键子节点"靠决策 2 机制定义。

## 参考实现

- **RmlUi**：`temp/RmlUi/Source/Core/Elements/`（WidgetDropDown / WidgetSlider / InputCheckbox）——控件是 element + 作者样式，无魔法注入。
- **WAI-ARIA APG**：combobox/listbox/slider/tab 复合控件模式（标准 role 结构）。
- **FairyGUI**：组件是对象树 + 自定义结构，无"标签即控件"概念。

## 建议推进顺序

1. 定决策 1/2/3（写 spec）
2. 对照 RmlUi 源码 + WAI-ARIA APG 定 role 结构契约
3. 从最简单的 ProgressBar 开始迁移（验证新模型），再 Slider/Toggle/RadioButton，最后 Dropdown（popup 浮层最复杂）
4. showcase 随控件迁移改写
5. 文本控件留后续（若决策 1 选不纳入）

## 推翻的工作

控件束 P1/P2/P3（commit `1062d9d..009e0b7`，60+ commits）的"注入 .loom-*"模式作废。文本编辑算法（insert/delete/cursor/IME/光标）是可复用资产（与节点表示解耦），迁移时算法主体不动，只改"怎么挂在 div 上"。
