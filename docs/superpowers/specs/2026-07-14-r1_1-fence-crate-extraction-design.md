# R1.1：fence crate 独立 + 旧代码清除

> **状态**：设计完成，待写实施计划
> **前置**：R1（fence schema + parser）已完成，commit `bc80411`
> **后续**：R2（类型化对象树）依赖 R1.1 产出的干净 core

## 1. 问题

R1 的围栏验证流水线（schema + 6 阶段 pipeline）全部住在 `crates/core` 里，靠 `#[cfg(feature = "parse")]` 门控。但 main-design §14.1 明确写道"运行时只认二进制；HTML 解析只在打包器"。core 同时承载运行时逻辑和构建期工具，职责混杂。

更深层的问题：core 的旧 `parse/` 模块（FENCE_TAGS 四标签围栏、旧 CSS cascade、build_scene、RichText desugar）仍然存活，packer 的 `pack()` 仍在直接调用。新旧两套 HTML 处理路径并存于 core，代码语义冲突。

## 2. 目标

1. fence 代码独立为 `crates/fence/`（crate 名 `loomgui_fence`），依赖 core 共享类型，但不被 core 反向依赖。
2. 从 core 中彻底删除旧 parse 路径（FENCE_TAGS、cascade、build_scene、parse_rich_markup、RichText desugar），以及 `parse` feature gate 和 `scraper` 依赖。
3. packer 的 HTML→pkg.bin 编译路径暂时 break（R3 重建），保留不依赖 parse 的部分（atlas、workspace、fonts、runtime manifest）。
4. 重写 fence 文档和 packer 模板，反映新 30 标签 schema。
5. 实现 R1 deferred validation（ARIA 关系、template 根、label for）。

## 3. 设计

### 3.1 crate 结构

```
crates/
  core/         纯运行时（无 HTML/CSS 解析，无 parse feature）
  fence/        围栏（build-time HTML 验证）
  packer/pkg/   打包流水线（依赖 fence + core）
  packer/gui/   Tauri GUI
  ffi/          C FFI（只依赖 core）
  xtask/        构建辅助
```

依赖方向（无环）：

```
  fence ──→ core ←── ffi
    │
  packer ──→ core
    │         ──→ fence   (R3 才接入，R1.1 暂不连)
   gui
```

fence 依赖 core 的原因：`css_resolve.rs` 输出 `core::style::resolved::ResolvedStyle`。构建期依赖运行时共享类型是自然方向。

workspace `Cargo.toml` 新增 `crates/fence` 成员。

### 3.2 fence crate 组成

**从 core 搬入的模块（`core/src/fence/` → `crates/fence/src/`）：**

| 模块 | 职责 |
|---|---|
| `schema/tag.rs` | 23 runtime + 7 shell 标签注册表，SemanticKind（24 variants），Category，ContentModel，DisplayDefault，resolve_semantic() |
| `schema/attr.rs` | AttrSpec，AttrValueDomain，INPUT_STRUCTURAL，LABEL_STRUCTURAL，A_STRUCTURAL，is_global_attr() |
| `schema/css.rs` | 56 CSS properties，11 shorthands，CssValueParser（22 variants），ShorthandKind |
| `schema/mod.rs` | schema re-exports |
| `ir.rs` | IrNode，IrTree，IrElement，IrAttribute，IrNodeKind，Span，IrNodeId |
| `diagnostic.rs` | Diagnostic，Severity，DiagnosticCode（13 variants），LineMap，SourceLocation |
| `tree_builder.rs` | html5gum 0.8 WHATWG tokenizer → IrTree |
| `fence_gate.rs` | per-element tag/attr/CSS property name validation |
| `css_resolve.rs` | schema-driven inline style validation + apply_decl application |
| `structural.rs` | content model + ID uniqueness |
| `annotate.rs` | SemanticKind 填充 |
| `pipeline.rs` | 6 阶段编排 → ParsedTemplate |
| `mod.rs` | crate root |

**依赖迁移：**

| 依赖 | 从 | 到 |
|---|---|---|
| `html5gum 0.8` | core (optional, parse feature) | fence (required) |
| `cssparser 0.34` | core (optional, parse feature) | fence (required) |
| `loomgui_core` | — | fence (path dep，用 style::resolved::ResolvedStyle) |

**测试迁移 + 改名：**

| 旧路径 | 新路径 | 测什么 |
|---|---|---|
| `core/tests/r1_schema_contract.rs` | `fence/tests/schema_contract.rs` | schema 注册表契约：tag/attr/css 白名单 + 类型映射的正反例 |
| `core/tests/r1_pipeline.rs` | `fence/tests/pipeline_integration.rs` | 端到端流水线：HTML → 6 阶段 → diagnostics 全链路 |

命名约定：`<模块>_contract.rs` 测单一模块契约，`<流程>_integration.rs` 测跨模块端到端。不带 roadmap 阶段前缀。

fence crate 的 `lib.rs`：

```rust
pub mod schema;
pub mod ir;
pub mod diagnostic;
pub mod tree_builder;
pub mod fence_gate;
pub mod css_resolve;
pub mod structural;
pub mod annotate;
pub mod pipeline;

pub use pipeline::{parse_template, ParsedTemplate};
pub use diagnostic::Diagnostic;
pub use ir::{IrTree, IrElement, IrNode, IrNodeKind};
pub use schema::{TagSpec, SemanticKind, Category, ContentModel};
```

### 3.3 core 清理

#### 3.3.1 整体删除的文件

| 文件 | 理由 |
|---|---|
| `src/parse/mod.rs` | 模块壳，旧 HTML/CSS 解析入口 |
| `src/parse/dom.rs` | FENCE_TAGS + parse_html，被 fence tree_builder 替代 |
| `src/parse/css.rs` | parse_css，被 fence css_resolve 替代 |
| `src/parse/selector.rs` | 旧 CSS 选择器引擎，R3 CSS cascade 时重建 |
| `src/style/cascade.rs` | resolve_styles，旧 build-time CSS cascade |
| `src/scene/node/parse_tests.rs` | 全部测试依赖 parse_html + build_scene |

#### 3.3.2 手术切除（保留 runtime 类型，删除 build-time 逻辑）

**`scene/node.rs`：**

删除：
- `build_scene()` 函数（L458+）——从 ElementTree 构建 Scene 的 build-time 入口。
- `rich_runs` / `raw_rich` 相关分支（L515-518）——RichText desugar 的 build-time 钩子。

保留：
- `Scene` struct + `Scene::build()`（从 Vec 手搓构造，runtime 实例化用）。
- `Node` struct + 所有字段。
- `NodeKind` enum（Container/Image/Text/RichText/...）——runtime 节点类型，`read_package` 也用它。
- `NodeId`——运行时句柄（R2 会重做，R1.1 不动）。

**`text/rich.rs`：**

删除：
- `parse_rich_markup()` 函数（L162+）及其全部测试（L584-811）——build-time 富文本标记解析。

保留：
- `RichRun`, `RichKind`, `RichVAlign`, `RichWeight`, `RichStyle`, `RichDeco`, `RichBaseStyle`——runtime 渲染消费的富文本数据类型。这些类型被 `text/layout.rs`、`render/`、`asset/mod.rs`（read_package 反序列化）使用。

**`asset/mod.rs`：**

删除：
- `extract_component_css()`（L211+）——build-time CSS 抽取。
- `extract_dynamic_rules()`（L138+）——build-time 动态规则抽取（依赖 `parse::css::StyleSheet`）。

保留：
- `TemplateNode`——模板节点序列化结构。
- `PackageInput`——打包器写入时的输入。
- `ControllerEntry`——v1 Controller 条目（R5 退役前仍用）。
- `write_package()`——pkg.bin 序列化。
- `read_package()`——pkg.bin 反序列化（runtime 用）。

**`style/resolved.rs`：**

删除：
- `DisplayMode` 的 desugar 相关注释（L67, L153）——旧 `display:block` desugar 钩子说明。

保留：
- `ResolvedStyle` struct——runtime 样式表示。
- `DisplayMode` enum——layout 仍用（Block/Flex/None 切换 Strategy）。

#### 3.3.3 受影响测试的处理

**内联测试（`#[cfg(test)] mod tests`）：**

| 文件 | 处理 |
|---|---|
| `scene/node/tests.rs` | 保留（用 Scene::build 手搓，不依赖 parse） |
| `style/mapping/tests.rs` | 删除 parse 测试段（L782+，L963+ 的 `parse_css` + `parse_html` 引用），保留纯映射断言 |
| `style/dynamic.rs` tests | 删除 `parse::css::Declaration` / `parse::selector::parse_selector` 引用的测试段，保留纯逻辑测试 |
| `asset/tests.rs` | 删除 `parse::css` / `parse::selector` 引用的测试段 |
| `layout/mod.rs` tests | 删除 parse_html 测试段（L423-481），保留 Scene::build 手搓测试 |
| `stage/tests.rs` | 删除 parse_html + resolve_styles + build_scene 引用的测试段 |
| `stage/instantiate_tests.rs` | 删除 `parse::css` / `parse::selector` 引用 |
| `stage/dynamic_tests.rs` | 检查是否引用 parse 路径，有则清理 |

**core 内联模块清理：**

| 文件 | 处理 |
|---|---|
| `stage.rs` `load_inline_for_test()` (L802+) | 删除（test helper，用旧 parse 路径） |
| `style/mapping.rs` L660 注释 | 清理 desugar 相关注释 |
| `lib.rs` | 删除 `#[cfg(feature = "parse")] pub mod fence;` 和 `#[cfg(feature = "parse")] pub mod parse;` |

**core/tests/ 集成测试：**

| 文件 | 处理 |
|---|---|
| `fence_contract.rs` | 删除（被 fence crate 的 schema_contract + pipeline_integration 替代） |
| `snapshot.rs` | 删除（依赖旧 parse 路径） |
| `stage_getters.rs` | 删除（依赖旧 parse 路径） |
| `v1e_dirty.rs` | 删除（依赖旧 parse 路径） |
| `node_sort_keys.rs` | 删除（依赖旧 parse 路径） |
| `sdf_shader_contract.rs` | 保留（不依赖 parse） |
| `r1_schema_contract.rs` | 搬到 fence crate（改名 schema_contract.rs） |
| `r1_pipeline.rs` | 搬到 fence crate（改名 pipeline_integration.rs） |

**dump examples：**

| 文件 | 处理 |
|---|---|
| `examples/dump_showcase_text.rs` | 删除（依赖旧 parse 路径，R3+ 用 pkg.bin 加载重建） |
| `examples/dump_render.rs` | 删除 |
| `examples/dump_interact.rs` | 删除 |

#### 3.3.4 Cargo.toml 清理

删除：
- `[features]` 段落（`default = ["parse"]` 和 `parse = [...]`）——core 不再有 feature gate。
- `scraper` 依赖。
- `html5gum` 依赖。
- `cssparser` 依赖。
- 所有 `required-features = ["parse"]` 声明（bench + test + example 各项）。

core 最终依赖只剩 runtime 必需：`taffy`, `ttf-parser`, `ab_glyph_rasterizer`, `etagere`, `unicode-linebreak`, `serde`, `serde_json`, `bincode`, `slotmap`。dev-deps 只剩 `insta`, `criterion`。

### 3.4 packer 清理

packer 的 HTML→pkg.bin 编译路径（`pack()` → `parse_html` → `resolve_styles` → `build_scene` → `scene_to_template`）全部依赖 core 旧 parse，删除后 break。

#### 3.4.1 pkg crate 清理

| 文件 | 处理 |
|---|---|
| `lib.rs` | 删除 `pack()`, `scene_to_template`, `desugar_block_divs`, `collect_controller_pages`, `strip_style_and_link`, `serialize_children`, `escape_text_into`, `escape_attr_into` |
| `build.rs` | 删除 packages 循环（调 pack 的部分）。保留 atlas + fonts + runtime manifest 产出 |
| `resolve.rs` | 删除（img src → sprite_key 解析，旧编译路径专用） |
| `workspace.rs` | 保留 |
| `runtime.rs` | 保留 |
| `atlas/` 全部 | 保留（不依赖 parse） |
| `main.rs` | 简化：build 暂时只产出 atlas + fonts。packages 部分打印 "HTML packing not yet reimplemented (R3)" 或直接跳过 |

packer 暂时失去 HTML→pkg.bin 能力。atlas 打包、workspace 管理、fonts 拷贝、runtime.json 产出仍然可用。R3 重建新编译路径时接 fence pipeline。

#### 3.4.2 packer Cargo.toml

- `loomgui_core` 依赖去掉 `features = ["parse"]`（core 不再有 parse feature）。
- 暂不添加 `loomgui_fence` 依赖（R3 才接入）。

### 3.5 文档重写

#### 3.5.1 `docs/design/fence.md` — 完全重写

旧内容是四标签围栏（div/span/img/button + display:block desugar + flex-only）。用新设计完全替代。

新内容大纲：

1. **设计哲学**——标准 HTML 语义 + AI 强先验。标签决定稳定对象类型，CSS 赋予行为能力不改变类型。
2. **围栏元素**——7 文档壳标签 + 23 运行时标签的完整表格（tag → SemanticKind → Category → ContentModel → DisplayDefault）。
3. **稳定语义签名**——tag + 不可变结构属性（type, role）决定类型。
4. **CSS 三正交维度**——CssPropSpec（属性白名单）、CssValueParser（值校验）、ShorthandSpec（展开规则）。
5. **6 阶段流水线**——tree_builder → fence_gate → css_resolve → structural → annotate → pipeline。
6. **失败策略**——一次性收集所有 diagnostic，不 fail-fast。diagnostic 含文件名/行列号/原值/建议。
7. **权威真相源**——指向 fence crate 的 `schema/` 注册表（machine-readable Rust const tables）。

#### 3.5.2 `docs/design/main-design.md` — 一致性检查

§3 围栏章节的"权威清单 = fence.md"指向仍然准确。§14.1 "HTML 解析只在打包器"现在与代码一致。确认无旧设计残留（`data-widget`、`display:block` desugar 等已不在）。预期改动极小。

#### 3.5.3 packer templates — 重写

**`workspace-CLAUDE.md`：**
- 围栏章节从旧四标签 + flex-only 替换为新 30 标签规则。
- 指向新 fence.md 作为权威。

**`skill/SKILL.md`：**
- 从旧"div/span/img/button + 每个div是flex容器 + inline mix报错"替换为新设计。
- 标准 HTML block/inline 语义。
- `display:flex` 默认 `flex-direction:row`。
- `overflow:auto/scroll` 控制滚动。
- 围栏外输入打包期报错（不静默忽略）。

#### 3.5.4 roadmap 更新

R1.1 状态更新为完成。确认 R1.1 的 11 项：完成项标记完成，defer 项标记 defer 到具体后续阶段。

### 3.6 Deferred validation（Stage 5 补全）

在 fence crate 的 `structural.rs` 中新增 3 项验证（第 4 项 defer 到 R3）：

| 验证 | 检查内容 | 错误诊断 |
|---|---|---|
| ARIA 关系 | `aria-controls` / `aria-labelledby` 的 IdRef 目标在当前模板作用域内存在 | E_ARIA_REF_NOT_FOUND，含被引用的 id 值 + 当前元素位置 |
| template 根 | ListView（`ul/ol`）内的 `<template>` 根元素必须是 `<li>` | E_TEMPLATE_ROOT_MUST_BE_LI，含当前根标签 + ul/ol 位置 |
| label[for] | `label[for]` 指向的 ID 在当前组件作用域内存在 | E_LABEL_FOR_NOT_FOUND，含被引用的 id 值 + label 位置 |
| Custom Element 注册 | 名称含 `-`（已有 hyphen 检测），注册验证 defer 到 R3 | defer |

Custom Element 注册机制需要 package 级别的注册表（哪些自定义元素被 `customElements.define()` 注册）。R1.1 确认 hyphen 检测已就位；注册验证 defer 到 R3（package 格式承载注册表后）。

## 4. 执行顺序

考虑并行化，分五条线：

**线 A（前置，其他线依赖）：fence crate 创建 + 搬迁**
1. 新建 `crates/fence/Cargo.toml`（依赖 loomgui_core + html5gum + cssparser）。
2. 搬入 `core/src/fence/` → `crates/fence/src/`。
3. 搬入 R1 测试，改名（schema_contract.rs + pipeline_integration.rs）。
4. workspace Cargo.toml 新增成员。
5. 验证 fence crate 独立编译 + 855 测试全绿。
6. 从 core 删除 `fence/` 模块 + `lib.rs` 的 fence 声明。

**线 B（依赖 A）：core 清理**
1. 删除 `src/parse/` 整个模块。
2. 删除 `src/style/cascade.rs`。
3. 删除 `build_scene()` + `rich_runs` 分支 from `scene/node.rs`。
4. 删除 `parse_rich_markup()` + 测试 from `text/rich.rs`。
5. 删除 `extract_component_css()` + `extract_dynamic_rules()` from `asset/mod.rs`。
6. 清理受影响测试。
7. 删除 `parse` feature gate + scraper 依赖 + Cargo.toml 清理。
8. 验证 core 独立编译。

**线 C（与 A/B 并行）：文档重写**
1. fence.md 完全重写。
2. main-design.md 一致性检查。
3. packer templates 重写。
4. roadmap R1.1 状态更新。

**线 D（A 完成后）：packer 清理**
1. 删除旧编译路径。
2. 保留 atlas/workspace/fonts。
3. Cargo.toml 去掉 parse feature。

**线 E（A 完成后，可与 B/C/D 并行）：deferred validation**
1. ARIA 关系验证。
2. template 根验证。
3. label[for] 验证。

## 5. 验证标准

- `cargo build -p loomgui_fence` 成功。
- `cargo test -p loomgui_fence` 855+ 测试全绿（含 deferred validation 新增测试）。
- `cargo build -p loomgui_core` 成功（无 parse feature，无 scraper）。
- `cargo test -p loomgui_core` 成功（剩余 runtime 测试全绿）。
- `cargo build -p loomgui_pkg` 成功（不含 HTML 编译路径）。
- `cargo build` (workspace) 成功。
- `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` 通过。
- fence.md / main-design.md / packer templates 无旧围栏残留。