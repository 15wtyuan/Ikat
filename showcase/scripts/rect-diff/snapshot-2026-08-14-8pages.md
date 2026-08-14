# rect-diff 报告 — 8 页全量（2026-08-14）

- 命令：`./run-page.sh <page>`（tol-box=1, tol-text=3），页 ∈ settings/character/shop/form/lab/mail/inventory/home
- 路径：browser-rect.mjs（headless Chromium DOM rect）↔ `dump_page --json`（core DFS rect）→ diff.mjs
- 上一份报告：`snapshot-2026-08-12-settings.md`（settings 单页，12 残余）。本轮扩展到全部 8 页 + 工具链净化。
- 背景：里程碑 1 任务 4（逐页 Unity PlayMode + rect-diff）的编码机半场——core 侧布局 bug 在家里修掉，Unity 机只剩后端/驱动问题。

## 工具链修复（本轮 5 项，噪声 106 unpaired → 2）

1. **tag 词汇表归一**（browser-rect.mjs `semanticTag`）：core dump 按 `kind_to_html_tag`（crates/core/src/dump.rs）报语义 tag（role=listitem→li、progressbar→progress、spinbutton/slider/switch/radio/textbox→input、combobox→select、hyphen 标签→custom），browser 侧 DOM 字面 tag 全是 div——两侧桶永配不上。browser-rect 现按同一张表从 `role` 属性归一。
2. **preview JS 保留 + data-fill 撤销**：loom-preview.js 是 browser 侧的 core 行为模拟器（textbox placeholder 行高压 39px、progressbar fill 宽、slider thumb 定位）——**必须保留**，全拦会让空 textbox 从 39 掉到 20 制造假 diff（form 页实测 +13 diff）。唯一要撤销的是 `fillListViews`（role=list[data-fill] 模板克隆）：core dump 无 C# driver 跑不了 ItemCount，克隆项无 core 对应物。browser-rect 在测量前删除克隆项；driver 驱动的列表虚拟化归 Unity 机验收（任务 4 本体）。
3. **0×0 盒不进 idless 桶**（diff.mjs）：browser 枚举一切 DOM（含 display:none），core 只发 laid-out 节点——0×0 进 index-aligned 桶必错位（form 一个隐藏 option 平移整桶配对）。core 侧 0×0 span 例外保留（rich-text folded 要配上报 FOLDED）。
4. **FOLDED 类别**（diff.mjs）：rich-text inline span 在 core 折叠进父 block run（任务 1 文本模型），公共树留 id 但 rect 0×0 by design——报信息行不计失败。
5. **preview-base.css 补 workspace 字体**：JetBrainsMono / PressStart2P / DejaVuSans 三族 @font-face（lab 页等宽标本依赖；此前 browser 静默 fallback 系统字体）。

## 8 页结果

| 页 | rect diffs | unmatched | idless-unpaired | folded | 幅度分布（≤3 / 3-10 / 10-30 / >30 px） |
|---|---|---|---|---|---|
| settings | 12 | 0 | 0 | 0 | 5 / 2 / 2 / 3 |
| character | 27 | 0 | 0 | 0 | 23 / 4 / 0 / 0 |
| shop | 12 | 0 | 0 | 0 | 12 / 0 / 0 / 0 |
| form | 63 | 0 | 0 | 0 | 18 / 40 / 0 / 5 |
| lab | 245 | 0 | 2 | 0 | 8 / 5 / 19 / 213 |
| mail | 7 | 0 | 0 | 4 | 3 / 3 / 1 / 0 |
| inventory | 8 | 0 | 0 | 3 | 5 / 0 / 3 / 0 |
| home | 101 | 0 | 0 | 1 | 54 / 14 / 11 / 22 |

**结论：8 页 0 unmatched、无结构性分歧，没有疑似 core 布局 bug。** 全部残余归入下列四类。

## 残余分类（475 条全归类）

### A. 文本测量精度差（lab 245 主体 / form 40 / home 68 / character 23 / shop 12）

core（ttf-parser advance 累计）vs Chromium（harfbuzz shaping）对同一 LXGW/JetBrainsMono 字形 advance 的百分比级差 → flex 内容尺寸（shrink-to-fit 列宽）→ wrap 换行点漂移 → 整块 y 级联（lab 恒定 ~38px 块偏移即此）。lab 是文本标本页故最敏感。**core 的 #text 内容宽与 browser 一致（settings 实测 48=48）**——差在排版级联放大，非字符级错误。护城河判据是「布局 rect 对齐（容差内）」；lab/home 的容差定标归任务 4 Unity 机逐页过。

### B. TextElement inline 盒宽语义（settings 3 / form 少量）

`<span>` 无显式宽时：browser inline shrink-to-fit 文本宽（48px），core 把 span resolve 成 Flex+Row 后**拉伸到父 block 内容宽**（78px）。内部 #text 宽两侧一致。潜在视觉影响仅限带 background/border 的 span（core 会画更宽的底）——showcase 现无此用法。已知语义差，若 dogfood 逼出带底 inline 样式再修。

### C. slider thumb transform 发射缺口（settings 4 / form 5 的 >30px）

同 2026-08-12 报告 A 类：core 按「transform 是渲染/命中层不进布局」用 translate 定位 thumb，dump_page 只发 layout_rect（slider 左缘），browser（preview wireSliders）镜像运行时定位在 value%。非 core bug（渲染正确）；dump 侧补 transform 合成是可选增强。

### D. template 子树枚举差（lab 2 slot，信息性）

core dump 输出 `<template>`/组件模板内部节点（slot 0×0），browser `body *` 不枚举 template content。结构性预期，无信号。

## 留给任务 4（Unity 机）

- Unity rect 半（`DumpSceneJson` 路径）接 run-page.sh 第三数据源（Unity render rect vs core rect）。
- mail/inventory 的 driver 驱动列表虚拟化（ItemCount + slot 覆盖）PlayMode 验收。
- lab/home 容差定标（A 类在 Unity 渲染下的可视影响）。
- 任务 3 渐变视觉验收（lab section 12 + home 光晕 + 渐变字；GRADIENT shader 首次真编译）。
