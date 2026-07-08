# Task 6 报告：display:block desugar → RichText 叶 + dom 保序 + fence

## 状态

DONE

## Commit

`f7fb625` — `feat(pkg): display:block desugar -> RichText leaf + fence (v1.7)`

## 测试摘要

`cargo test`（workspace，含 default features）：701 passed / 0 failed。
- `loomgui_core` lib: 574 passed
- `fence_contract`: 30 passed（含 3 新 block-div 围栏测试）
- `loomgui_pkg` pack 集成测: 17 passed（含 7 新 desugar 测试）
- 其余集成测（snapshot/node_sort_keys/stage_getters/v1e_dirty）全绿

`cargo fmt --all -- --check` clean；`cargo clippy --all-targets -- -D warnings` clean。

## 改动摘要

### 1. `loomgui_core/src/parse/dom.rs` — ElementData 加字段 + block div 捕获

- `ElementData` 加 `raw_rich: Option<String>`（parse 期捕获 inline display:block div 的 inner HTML 原文）+ `rich_runs: Option<Vec<RichRun>>`（desugar 期填）。
- `build_element`：tag=div 且 inline style attr 含 `display:block` → `el_node.inner_html()` 捕获原文到 `raw_rich`，early-return 不递归子元素（绕 FENCE_TAGS 让 b/i/span/a 进 raw，不被围栏挡）。
- 新增 `is_inline_display_block(attrs)` helper：去所有空白后查子串 `display:block`，容忍 `display: block` / `display :block` 等变体。
- flex div（默认）行内混排仍报错（铁律不变）；错误信息更新指向 `<div style="display:block">` 作富文本入口。
- 3 新 dom 单测：block div 捕获 raw_rich 不递归 / 空白变体容忍 / class-based display:block 不触发（MVP 限定）。

### 2. `loomgui_pkg/src/lib.rs` — desugar_block_divs 函数 + 调用点

- `pack()` 内 `resolve_styles` 后、`build_scene` 前插 `desugar_block_divs(tree, styles)?`。
- `pub fn desugar_block_divs(tree, styles) -> Result<(ElementTree, Vec<ResolvedStyle>), String>`：
  - 遍历 `tree.nodes`，对 `raw_rich` 非空元素：
    1. 守护栏（spec §4.2）：block div 拒 flex 属性（justify-content/align-items/gap 非默认 → Err）。
    2. base 样式从 ResolvedStyle 转 RichBaseStyle（color/font_size；weight/deco MVP 用默认——bold 走 `<b>`、underline 走 `<u>`，避免 base 已粗时 `<b>` 重复加粗语义混乱）。
    3. `parse_rich_markup(raw, base, 0)` → runs，存进 `ElementData.rich_runs`。
  - `(tree, styles)` 同长同序不变量保持：只填 `rich_runs`，不增删节点、不改 styles 顺序。
- `check_no_flex_props(s)`：justify_content.is_some() / align_items.is_some() / gap != default → Err。
- desugar_block_divs 设 `pub` 供 `loomgui_pkg/tests/pack.rs` 集成测直接调（免走完整 pack 的文件目录）。

### 3. `loomgui_core/src/scene/node.rs` — build_scene 产 RichText

- `gather_rec`：`kind_from_tag` 后，若 `el.rich_runs` Some → 覆盖 kind 为 `NodeKind::RichText { runs }`。
- block div 原 kind_from_tag("div")=Container 被覆盖成 RichText 叶。raw_rich 的 div 无子元素（parse 期 early-return），故 Container 的 children/text 逻辑不触发。

### 4. `loomgui_core/src/parse/selector.rs` — 测试 helper 补字段

- `el()` test helper 补 `raw_rich: None, rich_runs: None`。

### 5. `loomgui_core/tests/fence_contract.rs` — 围栏契约 E 节

- 3 新测试（parse 级）：
  - `block_div_captures_raw_rich_bypassing_fence_tags`：block div 接受 b/i（raw_rich 捕获，不递归）。
  - `flex_div_inline_mix_still_rejected`：默认 flex div 文本+元素混排仍报错（铁律不变）。
  - `class_based_display_block_still_blocked_at_parse`：MVP 限定——class 规则的 display:block 不触发 rich（parse 期未 cascade class）。

### 6. `loomgui_pkg/tests/pack.rs` — desugar 集成测

- 7 新测试（desugar 级，调 `desugar_block_divs` 直接验）：
  - `desugar_block_div_produces_rich_runs`：raw_rich → runs 含 bold + link。
  - `desugar_block_div_then_build_scene_emits_richtext_kind`：端到端 desugar + build_scene → 根节点 kind = RichText。
  - `desugar_block_div_rejects_justify_content` / `_align_items` / `_gap`：block div 拒 flex 属性。
  - `desugar_block_div_accepts_non_flex_props`：color/font-size/width 等非 flex 属性不报错。
  - `desugar_flex_div_unaffected`：普通 flex div raw_rich/rich_runs 均 None。

## 设计决策

1. **MVP 限定：仅 inline `display:block` 触发**——parse 期未 cascade class 规则，无法知 class→display:block。class-based 须两遍 parse 或延迟子解析，留 follow-up。`is_inline_display_block` 只查 inline style attr 字符串。

2. **base weight/deco 用默认**——避免 base 已粗时 `<b>` 重复加粗语义混乱。bold/underline 全由 `<b>`/`<u>` 标签驱动，base 只贡献 color/font_size。caller 在 block div 上写 `font-weight:bold` 会被静默忽略（base weight 硬编码 Normal）。

3. **desugar 不改 styles 顺序/长度**——`(tree, styles)` 同长同序不变量保持。block div 的 taffy_style 仍 Flex（layout 照常），desugar 只填 ElementData.rich_runs。build_scene 据 rich_runs 覆盖 kind。

4. **check_no_flex_props 用 ResolvedStyle::default() 比对**——taffy 不是 loomgui_pkg 直接依赖，不能在 fn 签名里写 taffy 类型。改为函数内取 `ResolvedStyle::default().taffy_style` 比对 gap。justify_content/align_items 是 Option，is_some() 判（默认 None）。

## Concerns

- **pre-existing feature-gate check failure**：`cargo build -p loomgui_core --no-default-features --all-targets` 在 baseline（commit ccd3748，本 task 改动前）即失败（asset/tests.rs 引用 parse-feature 门控的 extract_component_css）。非本 task 引入，未修复。CI 的 feature-gate 检查可能用不同口径（如只 build lib 不 build tests）。
- **class-based display:block 是 follow-up**：MVP 只支持 inline `display:block`。AI 若写 `<div class="rich">...</div>` + `.rich{display:block}` 不会触发 rich 转换（b 等围栏外 tag 会被挡）。follow-up 需两遍 parse 或 parse 延迟子解析。
- **base font-weight 静默忽略**：block div 上写 `font-weight:bold` 不会让 base 粗（desugar 的 RichBaseStyle.weight 硬编码 Normal）。caller 须用 `<b>` 标签驱动粗体。fence.md 应补一条说明。
