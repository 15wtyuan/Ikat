# 文本模型回归标准子树（inline flow）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 block 容器里的 inline 子（text/span/img）像浏览器一样 inline 流动 + 换行，公共树保留节点 ID + 事件，兑现里程碑 1 任务 1。

**Architecture:** pack 期 fence 6.4 分类 rich-text-block（+mixed 报错 +img 豁免）→ flag 进 TemplateNode/pkg → runtime solve 编译 inline 子树成 `Vec<RichRun>` 喂现成的 `measure_rich_text`（叶子测，子节点跳过 taffy）→ render 新 arm 按 flag 渲父 TextLayout → hit_test_rich FFI 解 sub-node 事件。算法（measure_rich_text/build_text_mesh）复用，补的是编译器 + 公共树/内部树分离 + 接线。权威设计：`docs/superpowers/specs/2026-08-12-text-model-design.md`。

**Tech Stack:** Rust（core/fence/ffi/packer），Rust edition 2021，taffy 0.12，ttf-parser 0.20，csbindgen 1，bincode。pkg format v32→v33。

## Global Constraints

- **围栏真相源** = `crates/fence/src/schema/` Rust const 表；改围栏后 `cargo test -p loomgui_fence`（含 `doc_schema_sync`）必跑；fence.md 改后 cp 随包副本 `unity/package/Editor/Resources/LoomGUI/skill/references/fence.md`（坑 183）。
- **pkg format version** v32→**v33**，`MIN_VERSION` 同步 bump（`crates/core/src/asset/mod.rs:37`）。
- **FFI 边界** C-like enum `#[repr(uN)]`；Rust FFI 返字符串 ptr+len；新增 FFI 后 `cargo run -p xtask -- sync-bindings` 同步 C# 绑定。
- **NodeFlags 不碰**（`node.rs:16` 是交互态，solve/build skip）；rich_text_block 用独立 `Node` 字段。
- **改 parse-time 逻辑（fence/bridge）后必须重打 pkg**：`cargo run -p loomgui_pkg -- build <workspace>`；纯 runtime 改重编 .dll 即可。
- **两台机约束**：编码机 headless 锁核心范式；Unity 真机视觉验收在另一台机（showcase 验收 task 9 标注）。
- **代码注释写上线品质**：自包含、说 WHY、不引用内部编号。
- **禁 netease-codemaker 模型** dispatch subagent（公司账号罚钱）。

## File Structure

| 文件 | 责任 | 任务 |
|---|---|---|
| `crates/fence/src/pipeline.rs:16` | `ParsedTemplate` +rich_text_blocks 字段 | T1 |
| `crates/fence/src/rich_text_classify.rs`（新） | 阶段 6.4 分类 + mixed 报错 | T1 |
| `crates/fence/src/inline_context_check.rs` | 阶段 6.5 img 豁免（读 6.4 结果） | T1 |
| `crates/fence/src/diagnostic.rs:25` | +`FenceMixedInlineBlock` code | T1 |
| `crates/fence/tests/` | 6.4 分类/mixed/豁免测试 | T1 |
| `docs/design/fence.md` + 随包副本 | §6.4 + 新 code 文档 | T1 |
| `crates/core/src/asset/mod.rs:37,119` | pkg v33 + TemplateNode.rich_text_block | T2 |
| `crates/core/src/scene/node.rs:227` | `Node.rich_text_block: bool` | T2 |
| `crates/core/src/scene/dynamic.rs` | instantiate 烘 flag | T2 |
| `crates/packer/pkg/src/bridge.rs:18` | bridge 读 fence 输出设 flag | T2 |
| `crates/core/src/text/rich.rs:89` | `RichRun` +source、摘 serde | T3 |
| `crates/core/src/text/rich_compile.rs`（新） | `compile_rich_runs` 编译器 | T4 |
| `crates/core/src/text/layout.rs:646,95` | `measure_rich_text` +run rect 输出；`TextLayout` +rects | T5 |
| `crates/core/src/text/layout.rs:372` | `rich_text_fingerprint` | T6 |
| `crates/core/src/layout/mod.rs:143-260,459` | build 跳子 + RichText arm 接通 + memo | T6 |
| `crates/core/src/render/mod.rs:1886` | Container+flag render arm | T7 |
| `crates/core/src/text/hit_test.rs`（新）或 layout.rs | `hit_test_rich` | T8 |
| `crates/ffi/src/lib.rs` | `loomgui_hit_test_rich` FFI | T8 |
| `unity/package/Plugins/LoomGUI/Bindings/` | C# 绑定（sync-bindings 产） | T8 |
| `showcase/showcase/*.html` + `showcase.pkg.bin` | 迁移 mixed + 重打 + 验收 | T9 |

---

## Task 1: fence 阶段 6.4 — rich-text-block 分类 + mixed 报错 + img 豁免

**Files:**
- Create: `crates/fence/src/rich_text_classify.rs`
- Modify: `crates/fence/src/pipeline.rs:16`（`ParsedTemplate` +字段）、`crates/fence/src/diagnostic.rs:25`（+code）、`crates/fence/src/inline_context_check.rs`（6.5 img 豁免读 6.4 结果）、`crates/fence/src/lib.rs`（管线调 6.4）、`docs/design/fence.md`（§6.4 + code 表）+ 随包副本
- Test: `crates/fence/tests/rich_text_classify.rs`（新）

**Interfaces:**
- Consumes: `IrTree`、`Vec<ResolvedStyle>`（stage 4 产，含 display）、stage 6.5 的 parent-display helper（`inline_context_check.rs`）
- Produces:
  - `ParsedTemplate.rich_text_blocks: Vec<usize>`（rich-text-block 根的 ir_idx 集合）
  - `DiagnosticCode::FenceMixedInlineBlock`
  - `pub fn classify_rich_text(tree: &IrTree, styles: &[ResolvedStyle]) -> (Vec<usize>, Vec<Diagnostic>)`（返 rich-text-block ir_idx + mixed 诊断）

**inline 级判定**（复用 schema）：IrText / `SemanticKind::TextElement`(span) / `SemanticKind::Image`(img)。block 级 = `SemanticKind::Container` / 控件 / template 等。

- [ ] **Step 1: 写 mixed 报错测试**

```rust
// crates/fence/tests/rich_text_classify.rs
use loomgui_fence::run_pipeline;

fn diags(html: &str) -> Vec<String> {
    let out = run_pipeline(html).unwrap();
    out.diagnostics.iter()
        .filter(|d| d.code == loomgui_fence::DiagnosticCode::FenceMixedInlineBlock)
        .map(|d| d.message.clone()).collect()
}

#[test]
fn mixed_inline_block_in_block_container_errors() {
    // span(inline) + div(block) 混在 block div 里 → 报错
    let d = diags("<div><span>x</span><div>y</div></div>");
    assert_eq!(d.len(), 1, "mixed direct children must error: {d:?}");
}

#[test]
fn all_inline_classified_no_error() {
    let out = run_pipeline("<div>text <span>x</span> <img src=\"a.png\"></div>").unwrap();
    assert!(out.diagnostics.iter().all(|d| d.code != loomgui_fence::DiagnosticCode::FenceMixedInlineBlock));
    // 根 div 是 rich-text-block
    assert!(out.rich_text_blocks.contains(&out.tree.roots[0].0));
}

#[test]
fn all_block_children_not_classified() {
    let out = run_pipeline("<div><div>a</div><div>b</div></div>").unwrap();
    assert!(!out.rich_text_blocks.contains(&out.tree.roots[0].0));
}

#[test]
fn flex_container_not_classified() {
    // display:flex 容器（即便全 inline 子）不当 rich-text-block（子是 flex item）
    let out = run_pipeline("<div style=\"display:flex\"><span>a</span><span>b</span></div>").unwrap();
    assert!(!out.rich_text_blocks.contains(&out.tree.roots[0].0));
}
```

- [ ] **Step 2: 跑测试确认失败**（FenceMixedInlineBlock 未定义、rich_text_blocks 字段不存在）

Run: `cargo test -p loomgui_fence --test rich_text_classify`
Expected: 编译失败（code/字段缺）

- [ ] **Step 3: 加 DiagnosticCode + ParsedTemplate 字段**

`crates/fence/src/diagnostic.rs` enum 加 `FenceMixedInlineBlock`。`pipeline.rs:16` `ParsedTemplate` 加 `pub rich_text_blocks: Vec<usize>`，pipeline 产出时填充。

- [ ] **Step 4: 写 `classify_rich_text`（新文件 rich_text_classify.rs）**

```rust
//! Stage 6.4：rich-text-block 分类 + mixed 报错。须在 6.5（inline_context_check）前跑。
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::ir::{IrNodeKind, IrTree, NodeId as IrId};
use crate::semantic::SemanticKind;
use crate::style::ResolvedStyleDisplay; // 按 ResolvedStyle 实际 display 字段类型调

/// 直接子是否 inline 级（IrText / span(TextElement) / img(Image)）。
fn is_inline_level(kind: &IrNodeKind, semantic: Option<SemanticKind>) -> bool {
    matches!(kind, IrNodeKind::Text(_))
        || matches!(semantic, Some(SemanticKind::TextElement | SemanticKind::Image))
}

/// 返 (rich_text_block ir_idx 集合, mixed 诊断)。
/// rich-text-block = display:block 容器 + 直接子全 inline 级且 ≥1。
/// mixed = display:block 容器 + 直接子既有 inline 又有 block 级 → error。
pub fn classify_rich_text(
    tree: &IrTree,
    is_block_container: impl Fn(usize) -> bool, // parent-display 判定，复用 6.5 helper
) -> (Vec<usize>, Vec<Diagnostic>) {
    let mut rich = Vec::new();
    let mut diags = Vec::new();
    for (idx, node) in tree.nodes.iter().enumerate() {
        let IrNodeKind::Element(el) = &node.kind else { continue };
        if !is_block_container(idx) { continue; }
        let direct: Vec<(&IrNodeKind, Option<SemanticKind>)> = node.children.iter()
            .map(|c| (&tree.nodes[c.0].kind, tree.nodes[c.0].kind.as_element().and_then(|e| e.semantic)))
            .collect();
        let inline_cnt = direct.iter().filter(|(k, s)| is_inline_level(k, *s)).count();
        if inline_cnt == 0 { continue; } // 全 block 或空
        if inline_cnt == direct.len() {
            rich.push(idx); // 全 inline → rich-text-block
        } else {
            // mixed → 报错
            diags.push(/* FenceMixedInlineBlock diagnostic @ el.span，教学文案 */);
        }
    }
    (rich, diags)
}
```

- [ ] **Step 5: 管线接入**——`lib.rs`/`pipeline.rs` 在 6.5 前调 `classify_rich_text`，结果写进 `ParsedTemplate.rich_text_blocks` + diagnostics 合并。parent-display 判定复用 6.5 的 helper（inline + tag 默认 + 单 compound class 规则；多 compound 保守）。

- [ ] **Step 6: 6.5 img 豁免**——`inline_context_check.rs` 报 `FenceInlineElementInBlockContext` 前，查 parent ir_idx 是否在 `rich_text_blocks` → 是则 img（SemanticKind::Image）豁免，button 仍报。

- [ ] **Step 7: 跑测试确认通过**

Run: `cargo test -p loomgui_fence --test rich_text_classify`
Expected: PASS

- [ ] **Step 8: 更新 fence.md + 随包副本 + doc_schema_sync**

`docs/design/fence.md` 加 §6.4（分类 + mixed + img 豁免，排 6.5 前）+ §7 DiagnosticCode 表加 `FenceMixedInlineBlock`。cp 随包副本。`doc_schema_sync` 若需加 mixed code 校验则补。

- [ ] **Step 9: 全 fence 门**

Run: `cargo test -p loomgui_fence`
Expected: 全绿（含 doc_schema_sync、pipeline_integration）

- [ ] **Step 10: Commit**

```bash
git add crates/fence docs/design/fence.md unity/package/Editor/Resources/LoomGUI/skill/references/fence.md
git commit -m "feat(fence): stage 6.4 rich-text-block classify + mixed error + img exempt"
```

---

## Task 2: pack 数据模型 — TemplateNode + Node 字段 + pkg v33 + bridge 接线

**依赖 T1**（bridge 读 `ParsedTemplate.rich_text_blocks`）。

**Files:**
- Modify: `crates/core/src/asset/mod.rs:37,119`、`crates/core/src/scene/node.rs:227`、`crates/core/src/scene/dynamic.rs`（instantiate）、`crates/packer/pkg/src/bridge.rs:18`
- Test: `crates/packer/pkg/tests/`（bridge）、`crates/core/src/asset/`（pkg roundtrip）

**Interfaces:**
- Consumes: T1 的 `ParsedTemplate.rich_text_blocks`
- Produces: `TemplateNode.rich_text_block: bool`、`Node.rich_text_block: bool`、pkg v33、instantiate 烘 `Node.rich_text_block`

- [ ] **Step 1: 写 pkg roundtrip + bridge 测试**

```rust
// crates/packer/pkg/tests/rich_text_flag.rs
use loomgui_core::asset::TemplateNode;
use loomgui_core::scene::node::NodeKind;

#[test]
fn bridge_sets_rich_text_block_flag() {
    let html = "<div>hello <span>world</span></div>"; // 根 div = rich-text-block
    let parsed = loomgui_fence::run_pipeline(html).unwrap();
    let nodes = loomgui_pkg::bridge::bridge(&parsed).unwrap();
    // 根节点（div）flag=true
    assert!(nodes[0].rich_text_block, "root rich-text-block flag must be set");
}

#[test]
fn bridge_no_flag_for_structural_block() {
    let parsed = loomgui_fence::run_pipeline("<div><div>a</div><div>b</div></div>").unwrap();
    let nodes = loomgui_pkg::bridge::bridge(&parsed).unwrap();
    assert!(!nodes[0].rich_text_block);
}
```

pkg roundtrip（asset 层）：
```rust
#[test]
fn pkg_v33_roundtrip_preserves_rich_text_block() {
    // 构造含 rich_text_block=true 的 TemplateNode → write → read → 字段保留
    // （照 asset/mod.rs 现有 roundtrip 测试模式）
}
```

- [ ] **Step 2: 跑确认失败**（字段不存在）

- [ ] **Step 3: 加字段**——`asset/mod.rs:119` `TemplateNode` +`pub rich_text_block: bool`；`node.rs:227` `Node` struct +`pub rich_text_block: bool`（Default=false）。`asset/mod.rs:37` `FORMAT_VERSION` 32→33 + `MIN_VERSION` bump。

- [ ] **Step 4: bridge 接线**——`bridge.rs:18` 主循环：建 TemplateNode 时，查该 ir_idx 是否在 `parsed.rich_text_blocks` → 设 `.rich_text_block: rich_text_blocks.contains(&ir_idx)`。

- [ ] **Step 5: instantiate 烘 flag**——`scene/dynamic.rs` 的 instantiate 路径（建 Node 处，~127/201/284 附近）：`rich_text_block: tpl_node.rich_text_block`。

- [ ] **Step 6: 跑测试通过**

Run: `cargo test -p loomgui_pkg --test rich_text_flag && cargo test -p loomgui_core asset`
Expected: PASS

- [ ] **Step 7: 重打 showcase pkg（验 pkg v33 实通）**

Run: `cargo run -p loomgui_pkg -- build showcase` （照实际 workspace 路径）
Expected: 产新 pkg.bin（v33）；旧 reader 拒旧 v32 pkg（MIN_VERSION 验证）。

- [ ] **Step 8: Commit**

```bash
git add crates/core/src/asset crates/core/src/scene crates/packer/pkg
git commit -m "feat(pkg): v33 + TemplateNode/Node rich_text_block flag + bridge/instantiate wiring"
```

---

## Task 3: RichRun +source，摘 serde（runtime-only）

**Files:**
- Modify: `crates/core/src/text/rich.rs:89`
- Test: `crates/core/src/text/rich.rs`（tests mod）

**Interfaces:**
- Produces: `RichRun { kind, color, font_id, size_px, weight, style, deco, link_id, source: NodeId }`；摘 `Serialize/Deserialize` derive（runs 不再进 pkg）

- [ ] **Step 1: 写测试（source 字段 roundtrip 构造）**

```rust
// rich.rs tests
#[test]
fn rich_run_carries_source_node() {
    let r = RichRun {
        kind: RichKind::Text { text: "hi".into() },
        color: [0.,0.,0.,1.], font_id: 0, size_px: 14,
        weight: RichWeight::Normal, style: RichStyle::Normal,
        deco: RichDeco::default(), link_id: None,
        source: NodeId(7),
    };
    assert_eq!(r.source, NodeId(7));
}
```
（摘 serde 后，原 `rich_run_serde_roundtrip` 测试删或改为非 serde 构造断言。）

- [ ] **Step 2: 跑确认失败**（source 字段缺）

- [ ] **Step 3: 加 source 字段 + 摘 serde**——`RichRun` struct +`pub source: NodeId`（`use crate::scene::node::NodeId`）；derive 去 `Serialize, Deserialize`（保留 Debug/Clone）。`RichRun::text` helper 默认 `source: NodeId(0)` 或加参数。

- [ ] **Step 4: 跑通过** — `cargo test -p loomgui_core rich::`

- [ ] **Step 5: Commit** — `git commit -m "refactor(text): RichRun +source NodeId, drop serde (runs runtime-only)"`

---

## Task 4: Run 编译器 — `compile_rich_runs`

**依赖 T3**（RichRun.source）。**算法详见 spec §6。**

**Files:**
- Create: `crates/core/src/text/rich_compile.rs`
- Modify: `crates/core/src/text/mod.rs`（pub mod rich_compile）
- Test: `crates/core/src/text/rich_compile.rs`（tests mod）

**Interfaces:**
- Consumes: `Scene`（children/text_contents/image_srcs/style）、`Node.rich_text_block`
- Produces: `pub fn compile_rich_runs(scene: &Scene, parent: NodeId) -> Vec<RichRun>`

- [ ] **Step 1: 写编译器测试（构造 Scene + 断 runs）**

```rust
// rich_compile.rs tests
use crate::scene::node::{Scene, NodeKind, NodeId};

#[test]
fn plain_text_compiles_to_one_run() {
    let mut s = Scene::default();
    let div = s.create_node(NodeKind::Container, /*style*/, None);
    s.get_mut(div).unwrap().rich_text_block = true;
    let tn = s.create_text_child(div, "hello"); // 照 scene 现有 helper
    let runs = compile_rich_runs(&s, div);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].source, tn);
}

#[test]
fn span_text_run_source_is_span_not_textnode() {
    // <div>a <span>b</span></div> → 2 runs: "a "(source=textnode), "b"(source=span)
    // 断 runs[1].source == span_id（非 span 的 textnode 子）
}

#[test]
fn nested_span_recurses() {
    // <div><span>a<span>b</span></span></div> → 2 runs，各 source=对应 span
}

#[test]
fn image_run_inline() {
    // <div>text <img></div> → text run + image run（source=img_id）
}

#[test]
fn whitespace_textnode_preserved() {
    // <div>a <span>b</span></div> 源里 "a " 含尾空格 → run text 含空格（measure_rich_text 折叠）
}
```

- [ ] **Step 2: 跑确认失败**（fn 不存在）

- [ ] **Step 3: 实现 `compile_rich_runs`**（spec §6 完整算法）——遍历 `scene.children(parent)`（含空白 TextNode，**不套 is_whitespace_only_text**）：

```rust
pub fn compile_rich_runs(scene: &Scene, parent: NodeId) -> Vec<RichRun> {
    let mut runs = Vec::new();
    let p = scene.get(parent).expect("live");
    for &child in &p.children {
        let cn = scene.get(child).expect("live");
        match cn.kind {
            NodeKind::TextNode => {
                let text = scene.text_contents.get(&child).cloned().unwrap_or_default();
                runs.push(run_from_style(&cn.style, RichKind::Text { text }, child));
            }
            NodeKind::TextElement => recurse_span(scene, child, &mut runs),
            NodeKind::Image => {
                let src = scene.image_srcs.get(&child).cloned().unwrap_or_default();
                let (w, h) = /* image_sizes 查或默认 */ (0.0, 0.0);
                runs.push(run_from_style(&cn.style, RichKind::Image { src, w, h, valign: RichVAlign::Baseline }, child));
            }
            _ => { /* unreachable：fence 保证 rich-text-block 全 inline 子 */ }
        }
    }
    runs
}

fn recurse_span(scene: &Scene, span: NodeId, runs: &mut Vec<RichRun>) {
    let sn = scene.get(span).expect("live");
    for &child in &sn.children {
        let cn = scene.get(child).expect("live");
        match cn.kind {
            NodeKind::TextNode => {
                let text = scene.text_contents.get(&child).cloned().unwrap_or_default();
                // source=span（事件命 span），style=span.style
                runs.push(run_from_style(&sn.style, RichKind::Text { text }, span));
            }
            NodeKind::TextElement => recurse_span(scene, child, runs), // 嵌套
            NodeKind::Image => { /* Image run，source=child（img 自己） */ }
            _ => {}
        }
    }
}

fn run_from_style(style: &ResolvedStyle, kind: RichKind, source: NodeId) -> RichRun {
    RichRun { kind, color: style.color, font_id: /*family→id*/, size_px: style.font_size as u16,
              weight: weight_from_font_weight(style.font_weight), style: RichStyle::Normal,
              deco: /*deco 从 style*/, link_id: None, source }
}
```
（font_id / deco 按 `ResolvedStyle` 实际字段调；MVP single-font 填 default_font_id。）

- [ ] **Step 4: 跑通过** — `cargo test -p loomgui_core text::rich_compile`

- [ ] **Step 5: Commit** — `git commit -m "feat(text): compile_rich_runs — standard subtree → Vec<RichRun>"`

---

## Task 5: TextLayout per-run-line rect 输出（命中测试基建）

**依赖 T4**（runs）。**详见 spec §6 run 行 rect。**

**Files:**
- Modify: `crates/core/src/text/layout.rs:95`（`TextLayout`）、`crates/core/src/text/layout.rs:646`（`measure_rich_text`）
- Test: `crates/core/src/text/layout.rs`（tests）

**Interfaces:**
- Produces: `TextLayout.run_rects: Vec<RichRunRect>`（每 run 每行 rect + source NodeId），由 `measure_rich_text` 填或独立 post-pass。

- [ ] **Step 1: 写 rect 计算测试**

```rust
#[test]
fn measure_rich_text_emits_run_rects() {
    let runs = vec![RichRun::text("hello world", [0.,0.,0.,1.], 0, 14)]; // source 默认
    let lay = measure_rich_text(&runs, Some(40.0), 1.2, TextAlign::Left, &stack);
    // "hello world" 在 40px 宽换行 → ≥2 行 → runs 跨行 → run_rects 覆盖
    assert!(!lay.run_rects.is_empty());
    // 每 rect 有 x/y/w/h
}
```

- [ ] **Step 2: 跑确认失败**（run_rects 字段缺）

- [ ] **Step 3: 加 `RichRunRect` + `TextLayout.run_rects`**

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RichRunRect { pub x: f32, pub y: f32, pub w: f32, pub h: f32, pub source: NodeId }
```
`TextLayout` +`pub run_rects: Vec<RichRunRect>`。`measure_rich_text` 末尾从 `lines[].runs[].glyphs`（x/advance）推每 run 每行 rect（跨行 run 拆多 rect），source 取该 run 的 `source`。image run 用 `RichImagePlacement`。

- [ ] **Step 4: 跑通过** — `cargo test -p loomgui_core text::layout::tests`

- [ ] **Step 5: Commit** — `git commit -m "feat(text): TextLayout run_rects for rich-text hit-test"`

---

## Task 6: solve 折叠 + RichText measure arm 接通 + 指纹 memo

**依赖 T4, T5。**

**Files:**
- Modify: `crates/core/src/layout/mod.rs:87,143-260,459`、`crates/core/src/text/layout.rs:372`（+`rich_text_fingerprint`）
- Test: `crates/core/src/layout/mod.rs`（tests）

**Interfaces:**
- Consumes: T4 `compile_rich_runs`、T5 `TextLayout`
- Produces: rich-text-block 节点 solve 为叶子（MeasureContext::RichText），TextLayout 存 `scene.text_layouts[parent]`；`rich_text_fingerprint`

- [ ] **Step 1: 写 solve 测试（rich-text block 测量）**

```rust
#[test]
fn rich_text_block_measures_as_leaf_with_wrapping() {
    // 构造 <div rich_text_block> 含长文本，avail 宽 100 → 换行 → height > 单行
    // solve 后 scene.text_layouts[div] 非空，layout_rect.h 反映多行
}
```

- [ ] **Step 2: 跑确认失败**（RichText arm 仍 dead）

- [ ] **Step 3: build() 跳子**——`layout/mod.rs` `build()`：if `node.rich_text_block` → 编译 runs（T4）→ `MeasureContext::RichText{runs, line_height, align, family, h_inset}` → `new_leaf_with_context`（**children 不递归进 taffy**）。taffy_to_scene 注册 parent nid。

- [ ] **Step 4: measure arm 接通 + memo**——`layout/mod.rs:459` RichText arm 摘 `#[allow(dead_code)]`：

```rust
Some(MeasureContext::RichText { runs, line_height, align, family, h_inset, .. }) => {
    let stack = fonts.stack_for(family.as_deref());
    let mw = known.width.map(|w| (w - *h_inset).max(0.0));
    let fp = crate::text::layout::rich_text_fingerprint(runs, *line_height, *align, family.as_deref(), mw);
    // 查 text_measure_cache[fp]；命中复用，未命中 measure_rich_text + 存
    let layout = measure_or_cached(runs, mw, *line_height, *align, &stack, fp, /*cache*/);
    if let Some(sid) = taffy_to_scene.get(&nid) {
        scene.text_layouts[sid.index()] = Some(layout.clone());
    }
    Size { width: layout.text_width, height: layout.text_height }
}
```

- [ ] **Step 5: 写 `rich_text_fingerprint`**（`text/layout.rs:372` 仿 `text_fingerprint`）——哈希 runs（每 run text/source + color bits + size + weight + style + deco + source）+ line_height/align/family + mw 桶。

- [ ] **Step 6: 跑通过** — `cargo test -p loomgui_core layout`

- [ ] **Step 7: Commit** — `git commit -m "feat(layout): rich-text-block solves as RichText leaf + fingerprint memo"`

---

## Task 7: render 新 arm — Container+flag 渲 TextLayout

**依赖 T6**（TextLayout 存父）。**详见 spec §8。**

**Files:**
- Modify: `crates/core/src/render/mod.rs:1886`（Container 分派）
- Test: `crates/core/src/render/mod.rs`（tests）

**Interfaces:**
- Consumes: `scene.text_layouts[parent]`、`build_text_mesh`、`is_text_sub_page`
- Produces: rich-text-block Container → 多 run mesh（跳过 inline 子递归）

- [ ] **Step 1: 写 render 测试（rich-text block 产多 run mesh）**

```rust
#[test]
fn rich_text_block_renders_text_mesh() {
    // 构造 rich-text-block div + solve（存 TextLayout）→ render → 产含 glyph 顶点的 RenderNode
    // 断：有 text RenderNode（program=glyph），且 inline 子节点不单独产 mesh
}
```

- [ ] **Step 2: 跑确认失败**（Container arm 不认 flag）

- [ ] **Step 3: render Container arm 前置 flag 特判**——`render/mod.rs` 到 `NodeKind::Container`（或通用容器分派前）：

```rust
if node.rich_text_block {
    let layout = scene.text_layouts.get(n.id.index()).cloned().flatten()
        .unwrap_or_else(|| /* fallback measure，同 TextNode arm */);
    let off_left = /* padding/border */; let off_top = /* ... */;
    let mut layout = layout;
    if off_left != 0.0 || off_top != 0.0 { bake_content_offset(&mut layout, off_left, off_top); }
    let meshes = build_text_mesh(&layout, atlas, fonts, rect, /*effects*/, /*gradient*/, /*clip*/);
    // 推 RenderNode（primary 用 n.id，多页用 synth id，复用 is_text_sub_page）
    // **不递归 inline 子**（return，不走 Container 的 children 遍历）
    return /* 推完的 RenderNode 集合 */;
}
// else 原 Container 逻辑
```

- [ ] **Step 4: 跑通过** — `cargo test -p loomgui_core render`

- [ ] **Step 5: Commit** — `git commit -m "feat(render): Container+rich_text_block arm renders multi-run text mesh"`

---

## Task 8: 命中测试 — core `hit_test_rich` + FFI + C# 绑定

**依赖 T5（run_rects）、T6（TextLayout 存父）。**

**Files:**
- Create: `crates/core/src/text/hit_test.rs`（或进 layout.rs）
- Modify: `crates/ffi/src/lib.rs`、`crates/core/src/text/mod.rs`
- Test: `crates/core/src/text/hit_test.rs`、`tests/dotnet/LoomGUI.PublicApi`（FFI 存在性）

**Interfaces:**
- Consumes: `scene.text_layouts[block]`（含 run_rects）
- Produces: `pub fn hit_test_rich(scene: &Scene, block_id: NodeId, local_pt: (f32,f32)) -> Option<NodeId>`；FFI `loomgui_hit_test_rich`

- [ ] **Step 1: 写 core hit_test 测试**

```rust
#[test]
fn hit_test_rich_resolves_to_span_source() {
    // <div rich>text <span id="s">x</span></div>，span run rect 在 (50,0,10,14)
    // 点 (55, 5) → Some(span_id)；点 (5,5)（text 区）→ Some(textnode_id)；点 (200,200)（外）→ None
}
```

- [ ] **Step 2: 跑确认失败**

- [ ] **Step 3: 实现 `hit_test_rich`**

```rust
pub fn hit_test_rich(scene: &Scene, block: NodeId, pt: (f32, f32)) -> Option<NodeId> {
    let layout = scene.text_layouts.get(block.index())?.as_ref()?;
    // local_pt 相对 block 内容区（已 bake content offset）→ 查 run_rects 命中
    for r in &layout.run_rects {
        if pt.0 >= r.x && pt.0 <= r.x + r.w && pt.1 >= r.y && pt.1 <= r.y + r.h {
            return Some(r.source);
        }
    }
    None
}
```

- [ ] **Step 4: FFI `loomgui_hit_test_rich`**——`ffi/src/lib.rs`：

```rust
#[no_mangle]
pub unsafe extern "C" fn loomgui_hit_test_rich(
    scene: *mut Scene, node_id: u32, x: f32, y: f32, out: *mut u32,
) -> bool { /* 调 hit_test_rich，命中写 *out=true 返 true */ }
```

- [ ] **Step 5: sync-bindings + 跑测试**

Run: `cargo run -p xtask -- sync-bindings` → 产 C# `LoomGUIBindings.cs`（含 `loomgui_hit_test_rich`）→ 同步 `unity/package/Plugins/LoomGUI/Bindings/`。
Run: `cargo test -p loomgui_core text::hit_test && dotnet test tests/dotnet/LoomGUI.PublicApi`
Expected: PASS（PublicApi 验 FFI 符号存在）

- [ ] **Step 6: Commit** — `git add crates/core crates/ffi unity/package/Plugins/LoomGUI/Bindings && git commit -m "feat(ffi): hit_test_rich — sub-node event hit-test for rich-text"`

---

## Task 9: showcase 迁移 + 验收

**依赖 T1-T8 全完。两台机：编码机 headless 验核心；Unity 真机视觉验收标「家里机」。**

**Files:**
- Modify: `showcase/showcase/*.html`（mixed 修正）、`showcase.pkg.bin`（重打 v33）
- Test: 手动 + dump_text example

- [ ] **Step 1: 扫 8 页 mixed**——`cargo run -p loomgui_pkg -- build showcase`；fence 报 `FenceMixedInlineBlock` 的逐一修（inline 裹子 div 或容器改 flex）。目标：8 页全过 fence 无 mixed error。

- [ ] **Step 2: headless 验文本 inline flow**——`cargo run --example dump_text`（或新 dump_rich）喂 showcase form/mail 的 pkg.bin，dump 全节点 layout_rect + text metrics，确认 rich-text-block 文本 inline 流动 + 换行（非竖排）。

- [ ] **Step 3: 重打 pkg + 编 dll**（parse-time 改过）

```bash
cargo run -p loomgui_pkg -- build showcase
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```

- [ ] **Step 4: Unity 真机验收（家里机）**——PlayMode 跑 form/mail，确认文本 inline 流动 + 换行对齐浏览器；span click 事件触发（hit_test_rich）。rect-diff 对齐（里程碑 1 任务 1 门）。

- [ ] **Step 5: Commit** — `git add showcase && git commit -m "chore(showcase): migrate to rich-text-block (fix mixed) + repackage v33"`

---

## Self-Review

**1. Spec coverage**（spec 各节 → task）：
- §5 fence 6.4 → T1 ✓｜§4 数据模型 → T2 ✓｜§6 编译器 → T4 ✓｜§6 run rect → T5 ✓｜§7 solve 折叠 → T6 ✓｜§8 render → T7 ✓｜§9 指纹 → T6 ✓｜§10 hit-test FFI → T8 ✓｜§3 RichRun+source → T3 ✓｜§13 showcase 迁移 → T9 ✓｜§12 测试矩阵 → 各 task 内嵌 + T9 ✓
- §11 pkg bump → T2 ✓｜§15 验收 → T9 ✓

**2. Placeholder scan**：无 TBD/TODO；编译器/measure/hit-test 给了签名 + 关键逻辑 + 指 spec §6 完整算法（spec 有伪代码，DRY 非占位）。`/* */` 注释处（font_id/deco/image_sizes/Effects/gradient）是"按 ResolvedStyle/style 实际字段调"——实现者读源码确认字段名，非占位（路径已给）。

**3. Type consistency**：
- `RichRun.source: NodeId`（T3）→ T4 产、T5 `RichRunRect.source: NodeId`、T8 `hit_test_rich -> Option<NodeId>` ✓
- `compile_rich_runs(scene, parent) -> Vec<RichRun>`（T4）→ T6 调 ✓
- `rich_text_fingerprint(runs, line_height, align, family, mw)`（T6）✓
- `hit_test_rich(scene, block, (f32,f32)) -> Option<NodeId>`（T8）✓
- `node.rich_text_block: bool`（T2）→ T6/T7 读 ✓（一致，非 NodeFlags）

**4. 依赖序**：T1→T2→T3→T4→T5→T6→T7→T8→T9（线性；T3 可与 T2 并行）。
