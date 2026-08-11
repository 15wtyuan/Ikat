# 文本模型回归标准子树（inline flow）

- **日期**：2026-08-12
- **状态**：设计（待 review → writing-plans）
- **范围**：复合束级，roadmap 里程碑 1 任务 1
- **相关**：`docs/design/fence.md` §2.2/§6.5、`docs/roadmap/roadmap.md` 近期任务 1、退役的 `2026-07-07-v1.7-rich-text-design.md`（`display:block` RichText 暗号，勿照抄）

---

## 1. 背景与问题

### 1.1 现状（代码实况）

LoomGUI 运行时**不实现 CSS inline flow**（fence.md §2.2 自述）。当前文本管线（读码核实）：

**pack 期（`crates/packer/pkg/src/bridge.rs`）**：IrTree 每个 IrText 与 IrElement 各自成独立 `TemplateNode`。`<div>text <span>x</span> more</div>` 产 4 个兄弟节点——div(Container)、`"text "`(TextNode)、span(TextElement)、`" more"`(TextNode)。

**runtime（`crates/core/src/layout/mod.rs`）**：
- `TextNode` → `MeasureContext::Text`，**叶子**，单一 content/color/font（`new_leaf_with_context`，`layout/mod.rs:163`）。
- `TextElement`(span) → 落 `_ => None` → 当**容器**（`new_with_children`），装自己的 TextNode 子。
- `MeasureContext::RichText`（`layout/mod.rs:87`）标了 `#[allow(dead_code)]`——退役的 `display:block` RichText 暗号曾喂数，暗号退役后**无人构造**，空转。

**结果**：div 的 4 个子各自当 flex/block item，竖排或 flex 排，**互不流动**——"text x more" 变三行（showcase "文本经常被换行"主诉）。

### 1.2 已就绪、可复用

- **`measure_rich_text`**（`text/layout.rs:646`）：算法完整——token 扁平化（CJK 逐字 / Latin 逐词 / 空白折叠）、行宽换行、`RichImagePlacement`、对齐。吃 `&[RichRun]` 产 `TextLayout`（lines/runs/glyphs）。**算法不动。**
- **`build_text_mesh`**（`render/mod.rs:1216`）：已支持多 run（per-run color/weight/style），多页拆分（`is_text_sub_page`）。
- **`RichRun`/`RichKind`**（`text/rich.rs:89/102`）：Text/Image 两 kind，per-run color/font/size/weight/style/deco/link_id。
- render/mod.rs:84 注释明说 RichText 路径"为复合束文本模型保留"。

### 1.3 差距（一句话）

**缺"标准子树 → runs 编译器"**：把 block 容器里的 inline 子节点（TextNode/TextElement/Image）拍平成 `Vec<RichRun>` 喂 `measure_rich_text`，公共树保留这些节点的 ID/事件，内部当一坨测。算法 + 接线点都在，补的是编译器 + 公共树/内部树分离 + render 新 arm + 命中测试 FFI。

---

## 2. 设计决策（brainstorm 已定）

| # | 决策 | 选项 | 理由 |
|---|---|---|---|
| D1 | **触发** | block 容器（`display:block`）直接子**全是 inline 级** → 自动 rich-text block | 浏览器式隐式，贴 AI 对 HTML 先验 |
| D2 | **inline 级集合** | TextNode + TextElement(span) + Image(img)；button 排除；link 暂 dorm（无 `<a>` 标签）| 贴旧 M4 ImageRun 契约；button 是控件非 phrasing |
| D3 | **mixed 直接子** | fence 打包期报错 `FenceMixedInlineBlock`（block 容器不得既有 inline 又有 block 子）| 项目「fail-loud 不静默降级」；showcase 多用 flex 故 block-mixed 罕见 |
| D4 | **scope** | 完整文本模型（ID **+ 事件**），命中测试 FFI 一起做 | 兑现契约 |

**关键澄清**：rich-text-block 判定**要看 display**——`display:block` 容器 + 全 inline 直接子。`display:flex` 容器（showcase 的 `.field`/`.radio-opt`/`.slider-field`）**不触发**，其子是 flex item。这是 showcase 的 label+控件布局不报 mixed-error 的原因。

---

## 3. 架构（方案 1：pack flag + runtime 编译）

```
pack 期（fence + bridge）
  fence 阶段 6.4（新，先于 6.5）：分类 block 容器
    ├─ display:block + 全 inline 直接子 → 标 rich_text_block
    ├─ mixed（inline + block 子）→ FenceMixedInlineBlock error
    └─ else（全 block 子 / flex / 空）→ 不标
  6.5（img/button 裸放 block）：parent 是 rich_text_block → img 豁免（当 inline run），button 仍报错
  bridge：rich_text_block flag → TemplateNode.rich_text_block=true

runtime solve
  flagged 节点：编译 inline 子树 → Vec<RichRun>（当前 computed style）
    → MeasureContext::RichText → taffy 叶子；inline 子跳过 taffy（折进父）
  rich_text_fingerprint memo → measure_rich_text → TextLayout 存 scene.text_layouts[父]

runtime render
  Container + RICH_TEXT_BLOCK flag → build_text_mesh(父 TextLayout) 多 run mesh；跳过 inline 子

公共树
  inline 子仍是 Scene 节点 → Get<TextNode/TextElement>("id") 照常
  span 事件 → hit_test_rich 点→run→source NodeId（FFI）
```

**不变量保住**：
- inline 子节点**不消失**（公共树保留 ID/kind/style），只是 layout/render 被 fold——Get<>/事件不被破坏。
- runs 用**当前 computed style** 每帧重编译（便宜），measure 经指纹 memo——:hover 改 span 色 / StyleSheet.Add 改规则，下帧 runs 变 → 指纹变 → 重测，不 stale。

---

## 4. 数据模型改动

| 位置 | 改动 | 代码位置 |
|---|---|---|
| `TemplateNode` | +`rich_text_block: bool` | `crates/core/src/asset/mod.rs:119` |
| `NodeFlags`（u8 bitflags） | +`RICH_TEXT_BLOCK` bit（instantiate 从 TemplateNode 烘入） | `crates/core/src/scene/node.rs:16` |
| `RichRun` | +`source: NodeId`（命中测试 run→节点）；runs 改 runtime 编译、退出 pkg → 摘 serde | `crates/core/src/text/rich.rs:89` |
| `MeasureContext::RichText` | 摘 `#[allow(dead_code)]`，重新接通 | `crates/core/src/layout/mod.rs:87` |
| pkg format | v32 → **v33**，MIN_VERSION bump | `crates/core/src/asset/mod.rs:37` |

`TemplateNode` 现无暗号 `runs` 残留（已核：`asset/mod.rs:119` 干净），只 +bool。

---

## 5. fence 新阶段 6.4（rich-text 分类，先于 6.5）

**输入**：IrTree + ResolvedStyle（每节点 display 已 resolve）。

**规则**：对每个元素节点，若它是 **block-formatting 容器**（`display:block`，非 flex/none）且其**直接元素+文本子**满足：
- 全是 inline 级（IrText / `span`(TextElement) / `img`(Image)）且 ≥1 个 → 标 rich-text-block（记进 `ParsedTemplate` 一个 ir_idx 集合，bridge 读）
- 混了 block 级子（div/控件/template 等）+ inline 级子 → **`FenceMixedInlineBlock` error**，教学文案："inline 子裹进子 div" 或 "容器改 `display:flex`"
- 全 block 级子 / 空子 → 不标

**6.5 联动**（`crates/fence/src/` 阶段 6.5）：img/button 裸放 block 报错前，查 parent 是不是 rich-text-block → 是则 **img 豁免**（当 inline run），button 仍报错（不进 rich-text）。span 本就豁免 6.5，无影响。

**DiagnosticCode 新增**：`FenceMixedInlineBlock`（error）。

**注意 display 判定来源**：css_resolve 烘 inline style + tag 默认；`<style>` class 规则的 display 在 dynamic_rules（运行时 rematch）。6.4 **复用 6.5 的 parent-display 判定 helper**（inline + tag 默认 + 单 compound class 规则声明 display:block/flex 参与；多 compound 后代/子代规则保守——既不假阳性报 mixed、也不假标记 rich-text-block，与 6.5 同向保守）。

---

## 6. Run 编译器（runtime，solve 内）

新 `compile_rich_runs(scene, parent_id) -> Vec<RichRun>`，遍历 `scene.children(parent)`（**含空白 TextNode，不套 `is_whitespace_only_text`**）：

```
for child in scene.children(parent):
  match child.kind:
    TextNode:
      text = scene.text_contents[child]
      run(text, style=child.style, source=child.id)        # source=TextNode 自己
    TextElement (span):
      recurse_span(scene, child, runs)                      # span.style 作 context
    Image:
      run(Image{src,w,h,valign}, style=white, source=child.id)
    else: unreachable  # fence 保证全 inline
```

`recurse_span(span)`：span 自己的 computed style 作 context；遍历 span 子——TextNode → run(text, **style=span.style**, **source=span.id**)；嵌套 TextElement → 递归（推新 context）；Image → Image run。

**source 规则**（命中测试事件命谁）：run 的 `source` = 其**最近 inline 元素**——span 内 TextNode 的 source=span（事件挂 span）；rich-text-block 直接 TextNode 子的 source=TextNode 自己。

**style 取值**：run 的 color/font_size/font_weight/font_family/text-decoration 取自该 context 节点的**当前 computed `ResolvedStyle`**（cascade 后）。:hover 改 span.style.color → 下帧编译读到新值。

**空白折叠**：编译器保留原始 text（含空白），`measure_rich_text` 内部按 HTML 语义折叠（连续空白 → 单空格）。

**run 行 rect（`TextLayout` 新增输出）**：现 `TextLayout` 只存 lines/runs/glyphs，无 per-run-line rect。命中测试（§10）要 **`measure_rich_text` 新增输出**每 run 每行 bounding rect（从 glyph x/advance + `RichImagePlacement` 推），或独立 post-pass 从现有 lines/runs 推——实现时定。

---

## 7. solve 折叠（`layout/mod.rs`）

`build()` 遍历到 rich-text-block 节点（查 `NodeFlags::RICH_TEXT_BLOCK`）：
- 编译 runs（§6）
- 构造 `MeasureContext::RichText { runs, line_height, align, family, h_inset }`
- `tree.new_leaf_with_context(style, mctx)`——**叶子**
- **不递归子进 taffy**（children 空，inline 子被折进父测）

measure 闭包现有 RichText arm（`layout/mod.rs:459`）摘 `dead_code`：
- `mw = known.width.map(|w| (w - h_inset).max(0.0))`
- `rich_text_fingerprint` 查 memo → 未命中调 `measure_rich_text(runs, mw, line_height, align, stack)` → 存 `scene.text_layouts[parent_id]`

**whitespace 过滤不冲突**：`is_whitespace_only_text`（`scene/node.rs:855`）只在 taffy 子（`layout/mod.rs:257`）+ render 子（`render/mod.rs:618/719`）过滤。rich-text-block 的 inline 子**不进 taffy（父叶子）也不进 render 子遍历（被 fold）**，过滤器碰不到；编译器直接读 `Scene.children` 含空白，安全。

---

## 8. render 新 arm（`render/mod.rs`）

现有文本渲染按 `NodeKind::TextNode` 分派（`render/mod.rs:1886`）。rich-text-block 是 `NodeKind::Container`——**Container 分派内前置 flag 特判**：
- if `NodeFlags::RICH_TEXT_BLOCK`：
  - 读 `scene.text_layouts[parent_id]`
  - `build_text_mesh(layout)` → 多 run mesh（per-run 色 + image placement）
  - 复用 `is_text_sub_page` / synth_text_node_id 多页拆分
  - **跳过递归 inline 子**（已画进父 mesh）
- else：原 Container 逻辑（background/children）

`bake_content_offset` 同样适用（padding/border 偏移烤进 layout）。

---

## 9. 指纹 memo（`rich_text_fingerprint`）

新 `rich_text_fingerprint(runs, line_height, align, family, mw) -> u64`（仿 `text_fingerprint`，`text/layout.rs:372`）：
- 哈希全 runs：每 run 的（text/src + color bits + size + weight + style + deco + source）
- + line_height/align/family + mw 桶（量化 0.25px，同 text_fingerprint）
- 存进现有 `text_measure_cache`（`text/layout.rs:359`）

**每 solve 重编译 runs（便宜，O(inline 节点)）→ 算指纹 → 命中跳过贵的 `measure_rich_text`**。span 改色 → runs 变 → 指纹变 → 重测。**不依赖 dirty_text 传播**（dirty_text 现只标文本节点自身，无"span 改色标父"路径——经指纹 memo 闭环更干净）。

---

## 10. 命中测试 FFI（sub-node 事件）

命中测试在 backend（Unity C#），用 core 经 FFI 导出的 layout_rect。rich-text-block 整块一个 rect——点块内只命 block，不知哪个 span。

**core 新增 `hit_test_rich(scene, block_id, local_pt) -> Option<NodeId>`**：查 block 的 TextLayout 各 run 每行 rect（§6 存的）+ image placement → 命中 → 返该 run 的 `source` NodeId。

**FFI 新增** `loomgui_hit_test_rich(scene, node_id, x, y) -> u32`（`crates/ffi/src/lib.rs`）：backend 命中 rich-text-block 节点后调它细化到 source 节点 → 走正常事件路由（span click 触发 span 事件）。

**links**：`RichRun.link_id`/`RichFragment` 机制保留（`measure_rich_text` 已有跨行链接拆 rect），但 fence 无 `<a>` 标签 → 暂 dorm，不接 C# Link 事件。

---

## 11. pkg bump + 兼容

- pkg format version v32 → **v33**（`crates/core/src/asset/mod.rs:37`），`MIN_VERSION` bump。
- 旧 pkg 不兼容 → 重打 showcase.pkg.bin（`cargo run -p loomgui_pkg -- build ...`）。
- `NodeFlags` +`RICH_TEXT_BLOCK` bit。
- csbindgen：FFI 新增 `loomgui_hit_test_rich` → `cargo run -p xtask -- sync-bindings` 同步 C# 绑定。

---

## 12. 测试矩阵

| 层 | 测什么 |
|---|---|
| fence | `FenceMixedInlineBlock` 报错（mixed 子）/ rich-text-block 标记正确（全 inline）/ 不标（全 block / flex / 空）/ img §6.5 豁免 / 6.4 先于 6.5 |
| core 编译器 | runs 正确：纯文本 / text+span / 嵌套 span / span+img / 空白折叠保留 |
| core measure | `rich_text_fingerprint` memo 命中（同 runs）+ 失效（改色重测）|
| core solve | 宽度受限换行 + 多 run 行高对齐（`dump_text` example 复现）|
| core render | 多 run mesh（headless 顶点/snapshot 校验）|
| core hit-test | `hit_test_rich` 各点位返对 source NodeId（含 image run）|
| PublicApi | `Get<TextNode/TextElement>("id")` 命中（ID 保留）|
| doc-schema | fence.md 补 §6.4 + `FenceMixedInlineBlock`（`cargo test -p loomgui_fence` 含 doc_schema_sync）|
| showcase | form/mail 文本块绿 + rect-diff 对齐浏览器（里程碑 1 任务 1 门）|

---

## 13. Showcase 迁移影响

pack 期新 mixed-error 会逼改 block 容器 mixed 写法：
- **扫 8 页**：把 `display:block` 容器 mixed 子的地方改（inline 裹子 div 或容器改 flex）。form.html 的 label+控件多在 flex（`.field`/`.radio-opt`/`.slider-field`），预计不报；须实扫确认。
- **img §6.5 豁免**：原本 img 裸放 block 报错的，rich-text-block 语境解锁——部分写法可简化。
- **自动修好**：纯文本/span 块（`.page-desc` 等）自动 inline flow，换行修好。

---

## 14. 不在本 spec 范围（defer）

- **`<a>` 标签 / Link 事件**：fence 无 link 标签，`link_id`/`RichFragment` 暂 dorm。链接随围栏加 `<a>` 标签时再接。
- **完整 CSS IFC**（`white-space` 全集 / `overflow-wrap` / `word-break` / `text-wrap`）：先支持 `white-space:nowrap`（现已有），其余按需。
- **per-glyph 多色**：per-run 多色（相邻同 style 拆多 run）已够；per-glyph 渐变文字另有 `background-clip:text` 机制。
- **button / 控件进 inline flow**：button 是交互控件，排除在 rich-text 外（当 flex/block item）。

---

## 15. 验收判据

- [ ] fence：mixed 报错 + rich-text-block 分类 + img 豁免，全测绿（`cargo test -p loomgui_fence`）
- [ ] core：编译器 / solve 折叠 / render 多 run / 指纹 memo / hit_test_rich，全测绿（`cargo test -p loomgui_core`）
- [ ] PublicApi：`Get<TextNode/TextElement>` 命中（`tests/dotnet/LoomGUI.PublicApi`）
- [ ] pkg v33 + FFI `loomgui_hit_test_rich` + C# 绑定同步
- [ ] showcase form/mail 文本块 inline 流动 + 换行对齐浏览器（rect-diff 或真机）
- [ ] fence.md 补 §6.4 + `FenceMixedInlineBlock`，随包副本同步
