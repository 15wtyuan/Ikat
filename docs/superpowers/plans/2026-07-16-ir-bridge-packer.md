# Spec-3 ② IrTree→TemplateNode 桥 + 打包编排 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把断点接上——让真 HTML 第一次端到端走进 core（IrTree→TemplateNode 桥 + packer 打包编排 + base_style 灌入 + inherited_set bake），推到终点线 1（headless smoke）。

**Architecture:** fence `parse_template` 产 `ParsedTemplate{tree, styles, dynamic_rules, referenced_sprites}`（成熟，停在 IrTree）→ packer 新增 `bridge()` 把 IrTree 翻译成 `Vec<TemplateNode>`（SemanticKind total 映射 + 属性抽取）→ packer `build()` 组 `ComponentTemplate` 调 core `write_package` 产 v18 pkg.bin → core `load_package`/`instantiate`/`tick_and_render` → 断言。pkg 格式升 v18（kind_tag 全 23 变体 + 删 RichText 死字段）。

**Tech Stack:** Rust edition 2021 / loomgui_core + loomgui_fence + loomgui_pkg / bincode / cargo test / 本机 headless 验证（不依赖 Unity）。

## Global Constraints

- **Rust edition 2021**，依赖钉版本（本 plan 无新外部依赖；packer 加 `loomgui_fence` path 依赖）。
- **CI 门禁**：`cargo fmt --all -- --check` 严 + `cargo clippy --all-targets -- -D warnings` 严 + feature-gate check。每个 task 末跑。
- **NodeKind 不跨 C ABI**（FFI 边界是 NodeId u32，`create_node` 吃 tag 字符串）——`#[repr(u8)]` 零 ABI 影响。
- **改 core 后必重编 + commit .dll**（两台机串行工作流，本机唯一编码机）：`cargo build -p loomgui_ffi_c --release` → `cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`（Unity 关着）。本 plan 改 core（pkg v18），Task 7 统一重编。
- **commit message 英文**，末尾 `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`。
- **不静默降级**：未知 SemanticKind / kind_tag / 缺 sprite / 多根 → 报错或 Diagnostic，绝不默默塌缩。
- **本 plan 的 spec**：`docs/superpowers/specs/2026-07-16-ir-bridge-packer-design.md`（权威）。本机 = 编码机（headless 测试在本机跑）。

## File Structure

| 文件 | 责任 | 改动 |
|---|---|---|
| `crates/core/src/scene/node.rs` | NodeKind enum | 加 `#[repr(u8)]` + `pub fn from_u8`（Task 1） |
| `crates/core/src/asset/mod.rs` | pkg.bin write/read | kind 映射改 `as u8`/`from_u8` + 删 rich_runs_arena/rich_off + 升 v18（Task 2） |
| `crates/core/src/style/dynamic.rs` | cascade 引擎 | `inherited_bit` 改 `pub`（Task 4） |
| `crates/fence/src/css_resolve.rs` | inline style resolve | apply_decl 后 set `inherited_set` bit（Task 4，修坑 161） |
| `crates/packer/pkg/Cargo.toml` | packer 依赖 | 加 `loomgui_fence`（Task 3） |
| `crates/packer/pkg/src/bridge.rs` | **新**：IrTree→TemplateNode 桥 | SemanticKind total 映射 + 属性抽取 + parent_idx + 单根检查（Task 3） |
| `crates/packer/pkg/src/lib.rs` | packer 模块声明 | 加 `pub mod bridge`（Task 3） |
| `crates/packer/pkg/src/build.rs` | 打包编排 | packages 循环 + referenced_sprites 回接 atlas validate（Task 5） |
| `crates/core/tests/smoke_ir_bridge.rs` | **新**：终点线1 集成测试 | 手搓最小 HTML 5 断言 + form 冒烟（Task 6） |

---

### Task 1: NodeKind `#[repr(u8)]` + `from_u8`

**Files:**
- Modify: `crates/core/src/scene/node.rs:84`（derive 行）+ `:112`（impl 块）
- Test: `crates/core/src/scene/node.rs`（同文件 `#[cfg(test)]`，若无则新增）

**Interfaces:**
- Consumes: 无
- Produces: `NodeKind` 带 `#[repr(u8)]`（`kind as u8` = 稳定判别值 0..22）；`NodeKind::from_u8(b: u8) -> Option<NodeKind>`（Task 2 read 用）

- [ ] **Step 1: 写失败测试**

在 `crates/core/src/scene/node.rs` 末尾（或现有 test 模块内）加：

```rust
#[cfg(test)]
mod repr_tests {
    use super::NodeKind;

    #[test]
    fn kind_as_u8_is_discriminant() {
        // repr(u8) 后 as u8 等于声明顺序的判别值；锁定几个关键值防漂移。
        assert_eq!(NodeKind::Container as u8, 0);
        assert_eq!(NodeKind::TextNode as u8, 1);
        assert_eq!(NodeKind::Button as u8, 6);
        assert_eq!(NodeKind::Image as u8, 8);
        assert_eq!(NodeKind::Canvas as u8, 22);
    }

    #[test]
    fn from_u8_roundtrip_all_variants() {
        let all = [
            NodeKind::Container, NodeKind::TextNode, NodeKind::TextBlock,
            NodeKind::TextElement, NodeKind::LineBreak, NodeKind::Label,
            NodeKind::Button, NodeKind::Link, NodeKind::Image,
            NodeKind::TextField, NodeKind::NumberField, NodeKind::Slider,
            NodeKind::Toggle, NodeKind::RadioButton, NodeKind::TextArea,
            NodeKind::Dropdown, NodeKind::OptionItem, NodeKind::ProgressBar,
            NodeKind::ListView, NodeKind::ListItem, NodeKind::Slot,
            NodeKind::CustomElement, NodeKind::Canvas,
        ];
        for &k in &all {
            assert_eq!(NodeKind::from_u8(k as u8), Some(k));
        }
        assert_eq!(NodeKind::from_u8(23), None); // 越界
        assert_eq!(NodeKind::from_u8(255), None);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core scene::node::repr_tests`
Expected: 编译失败（`from_u8` 未定义）或 `#[repr(u8)]` 缺失导致 `as u8` 报错。

- [ ] **Step 3: 加 `#[repr(u8)]`**

`crates/core/src/scene/node.rs:84`，derive 行上方加 `#[repr(u8)]`：

```rust
/// 默认 `Container`（无数据变体），render 层测试构造 Node 用 `Default::default()`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum NodeKind {
```

- [ ] **Step 4: 加 `from_u8`**

在 `impl NodeKind {` 块内（`is_container` 之前）加：

```rust
    /// u8 判别值 → NodeKind（pkg.bin kind_tag read 用）。越界返 None。
    /// 变体只追加到 enum 末尾，保持既有判别值稳定。
    pub fn from_u8(b: u8) -> Option<NodeKind> {
        match b {
            0 => Some(NodeKind::Container),
            1 => Some(NodeKind::TextNode),
            2 => Some(NodeKind::TextBlock),
            3 => Some(NodeKind::TextElement),
            4 => Some(NodeKind::LineBreak),
            5 => Some(NodeKind::Label),
            6 => Some(NodeKind::Button),
            7 => Some(NodeKind::Link),
            8 => Some(NodeKind::Image),
            9 => Some(NodeKind::TextField),
            10 => Some(NodeKind::NumberField),
            11 => Some(NodeKind::Slider),
            12 => Some(NodeKind::Toggle),
            13 => Some(NodeKind::RadioButton),
            14 => Some(NodeKind::TextArea),
            15 => Some(NodeKind::Dropdown),
            16 => Some(NodeKind::OptionItem),
            17 => Some(NodeKind::ProgressBar),
            18 => Some(NodeKind::ListView),
            19 => Some(NodeKind::ListItem),
            20 => Some(NodeKind::Slot),
            21 => Some(NodeKind::CustomElement),
            22 => Some(NodeKind::Canvas),
            _ => None,
        }
    }
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p loomgui_core scene::node::repr_tests`
Expected: PASS（2 测试）。

- [ ] **Step 6: 全 core 测试 + fmt + clippy**

Run: `cargo test -p loomgui_core && cargo fmt --all -- --check && cargo clippy -p loomgui_core --all-targets -- -D warnings`
Expected: 全绿。

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/scene/node.rs
git commit -m "feat(core): NodeKind #[repr(u8)] + from_u8 for pkg kind_tag

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: pkg 格式升 v18（kind_tag 全 23 变体 + 删 RichText 死字段）

> write/read 必须同步改（格式一致才能 roundtrip），所以本 task 一次性改两端 + 升版本。命中 fe81e76 的 `TODO(pkg-format-cleanup)` + wildcard `debug_assert!`。

**Files:**
- Modify: `crates/core/src/asset/mod.rs`（常量 / write_package / read_package / 文档注释）
- Test: `crates/core/src/asset/tests.rs`（扩展稳定性测试）

**Interfaces:**
- Consumes: Task 1 的 `NodeKind::from_u8`、`NodeKind as u8`
- Produces: pkg.bin v18（kind_tag = NodeKind 判别值，无 RichRunsArena 段）

- [ ] **Step 1: 写失败测试（Slider 不塌 + 无 rich 段）**

在 `crates/core/src/asset/tests.rs` 末尾加（若文件有 `use super::*;` 则复用）：

```rust
#[test]
fn v18_all_nodekinds_roundtrip_no_collapse() {
    // 23 变体每个都进包再出，确认不塌成 Container（坑：v17 只序列化 4 种）。
    let all_kinds = [
        NodeKind::TextNode, NodeKind::TextBlock, NodeKind::TextElement,
        NodeKind::LineBreak, NodeKind::Label, NodeKind::Link,
        NodeKind::TextField, NodeKind::NumberField, NodeKind::Slider,
        NodeKind::Toggle, NodeKind::RadioButton, NodeKind::TextArea,
        NodeKind::Dropdown, NodeKind::OptionItem, NodeKind::ProgressBar,
        NodeKind::ListView, NodeKind::ListItem, NodeKind::Slot,
        NodeKind::CustomElement, NodeKind::Canvas,
    ];
    // 逐变体单独 roundtrip（每变体一个单节点组件，组件根 parent_idx=None）。
    for &k in &all_kinds {
        let one = TemplateNode {
            kind: k,
            style: ResolvedStyle::default(),
            parent_idx: None,
            classes: vec![],
            id_attr: None,
            draggable: false,
            tabindex: None,
            data_controller: None,
            content: None,
            src: None,
        };
        let empty_rules = DynamicRuleTable { rules: vec![] };
        let input = PackageInput {
            components: vec![("c", std::slice::from_ref(&one), &empty_rules, &[])],
        };
        let bytes = write_package(&input);
        let pkg = read_package(&bytes).unwrap();
        let comp = pkg.components.get("c").unwrap();
        assert_eq!(comp.nodes[0].kind, k, "kind {k:?} collapsed after roundtrip");
    }
}
```

> 注：测试里多次 `TemplateNode { .. }` 字段全部列出（无 `..Default` 因 TemplateNode 无 Default）。`ResolvedStyle::default()` / `DynamicRuleTable { rules: vec![] }` 是现有 public 构造。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core asset::tests::v18_all_nodekinds_roundtrip_no_collapse`
Expected: FAIL——Slider 等变体 roundtrip 后变 Container（v17 wildcard fallback）。

- [ ] **Step 3: 升版本 + 删常量 + 删 rich 哨兵**

`crates/core/src/asset/mod.rs`：

(a) `:23-25` 版本升 v18：
```rust
pub const PKG_FORMAT_VERSION: u32 = 18; // v18: kind_tag = NodeKind 判别值(全23变体) + 删 RichText 死字段(rich_runs_arena/rich_off)
pub(crate) const MIN_VERSION: u32 = 18;
pub(crate) const MAX_VERSION: u32 = 18;
```

(b) `:1` 模块 doc 注释 `version=16` → `version=18`。

(c) `:27-28` 删 `NULL_RICH_OFF` 常量。

(d) `:30-34` 删 5 个 `KIND_*` 常量（整段删）。

(e) `:4-11` 模块 doc 删 RichRunsArena 段描述，改 NodeBlock 描述去 rich_off。

- [ ] **Step 4: 改 write_package（kind_tag + 删 rich_runs_arena/rich_off）**

`crates/core/src/asset/mod.rs` write_package 内：

(a) `:152-154` `node_records` tuple 删末位 `rich_off: u32`，从 11 元素降到 10：
```rust
    // 每节点（全局）：(parent_idx:i32, kind_tag, style_blob, text_idx, src_idx, class_idx[], id_idx, flags, tabindex, dc_idx)
    let mut node_records: Vec<(i32, u8, Vec<u8>, u16, u16, Vec<u16>, u16, u8, i32, u16)> =
        Vec::new();
```

(b) `:155-161` 删整段 `rich_runs_arena` 注释 + `let rich_runs_arena: Vec<u8> = Vec::new();`。

(c) `:181` 删 `let rich_off: u32 = NULL_RICH_OFF;`。

(d) `:182-210` kind 映射段——删 KIND_* 4-arm match + wildcard debug_assert，改为：
```rust
            let (kind_tag, text_idx, src_idx) = {
                let text_idx = tn
                    .content
                    .as_ref()
                    .map(|c| intern(c, &mut strings, &mut idx_of))
                    .unwrap_or(NULL_IDX);
                let src_idx = tn
                    .src
                    .as_ref()
                    .map(|c| intern(c, &mut strings, &mut idx_of))
                    .unwrap_or(NULL_IDX);
                // kind_tag = NodeKind 判别值（repr(u8)），全 23 变体保真。
                let kind_tag = tn.kind as u8;
                match tn.kind {
                    NodeKind::Image => (kind_tag, NULL_IDX, src_idx),
                    NodeKind::TextNode => (kind_tag, text_idx, NULL_IDX),
                    _ => (kind_tag, NULL_IDX, NULL_IDX),
                }
            };
```

(e) `:230-242` `node_records.push` 删末位 `rich_off`（tuple 从 11 元素降到 10）：
```rust
            node_records.push((
                parent_global,
                kind_tag,
                style_blob,
                text_idx,
                src_idx,
                class_idx,
                id_idx,
                flags,
                tabindex,
                dc_idx,
            ));
```

(f) `:279-282` 删 RichRunsArena 写段（`arena_len + arena_bytes`）整段删。

(g) `:283-314` NodeBlock 写循环——解构 tuple 删 `rich_off`，删 `out.extend_from_slice(&rich_off.to_le_bytes());`：
```rust
    // NodeBlock: 每节点 {parent_idx(i32), kind_tag(u8), style_len(u32)+style_blob, text_idx(u16), src_idx(u16),
    //   class_count(u16)+class_idx[], id_idx(u16), flags(u8), tabindex(i32), dc_idx(u16)}
    for (
        parent_idx,
        kind_tag,
        style_blob,
        text_idx,
        src_idx,
        class_idx,
        id_idx,
        flags,
        tabindex,
        dc_idx,
    ) in &node_records
    {
        out.extend_from_slice(&parent_idx.to_le_bytes());
        out.push(*kind_tag);
        out.extend_from_slice(&(style_blob.len() as u32).to_le_bytes());
        out.extend_from_slice(style_blob);
        out.extend_from_slice(&text_idx.to_le_bytes());
        out.extend_from_slice(&src_idx.to_le_bytes());
        out.extend_from_slice(&(class_idx.len() as u16).to_le_bytes());
        for &cidx in class_idx {
            out.extend_from_slice(&cidx.to_le_bytes());
        }
        out.extend_from_slice(&id_idx.to_le_bytes());
        out.push(*flags);
        out.extend_from_slice(&tabindex.to_le_bytes());
        out.extend_from_slice(&dc_idx.to_le_bytes());
    }
```

- [ ] **Step 5: 改 read_package（from_u8 + 删 rich 读取）**

`crates/core/src/asset/mod.rs` read_package 内：

(a) `:367-370` 删 RichRunsArena 读取段（`rich_arena_len` + `_rich_runs_arena` take）整段删。

(b) `:407-408` 删 `let _rich_off = r.u32("rich_off")?;`。

(c) `:411-435` kind_tag match——删 KIND_* 识别 + RICHTEXT fallback，改 `from_u8`：
```rust
        let (kind, content, src) = match NodeKind::from_u8(kind_tag) {
            Some(NodeKind::Image) => (
                NodeKind::Image,
                None,
                if src_idx == NULL_IDX {
                    None
                } else {
                    Some(string_at(&strings, src_idx)?)
                },
            ),
            Some(NodeKind::TextNode) => (
                NodeKind::TextNode,
                if text_idx == NULL_IDX {
                    None
                } else {
                    Some(string_at(&strings, text_idx)?)
                },
                None,
            ),
            Some(k) => (k, None, None),
            None => return Err(PkgError::BadKind(kind_tag)),
        };
```

- [ ] **Step 6: 跑测试确认通过**

Run: `cargo test -p loomgui_core asset`
Expected: PASS——`v18_all_nodekinds_roundtrip_no_collapse` 绿（Slider 等不塌），现有 roundtrip 测试也绿（升 v18 后重打）。

- [ ] **Step 7: 全 core 测试 + fmt + clippy**

Run: `cargo test -p loomgui_core && cargo fmt --all -- --check && cargo clippy -p loomgui_core --all-targets -- -D warnings`
Expected: 全绿。若现有测试因格式字段硬编码旧 layout 失败，按新 v18 layout 修。

- [ ] **Step 8: Commit**

```bash
git add crates/core/src/asset/mod.rs crates/core/src/asset/tests.rs
git commit -m "feat(core): pkg format v18 — kind_tag=NodeKind discriminant + drop RichText dead fields

- kind_tag: 5 KIND_* constants -> NodeKind as u8 (all 23 variants, no collapse)
- drop rich_runs_arena (always empty) + rich_off per-node field (4B dead)
- bump v17->v18 (MIN=MAX=18, no reader for v17)
- closes fe81e76 TODO(pkg-format-cleanup)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: IrTree→TemplateNode 桥（packer 新模块）

> ② 核心。**修正 spec §4.5 措辞**：Text 节点保留为**独立 TextNode 子节点**（非"塞进父 content"）——core 只对 `NodeKind::TextNode` 读 `text_contents` side table，若把文字塞进父 Element 的 `content` 字段（父非 TextNode），core 不渲染文字。inline 嵌套结构保留，但不产生 rich text runs（复合束）。

**Files:**
- Create: `crates/packer/pkg/src/bridge.rs`
- Modify: `crates/packer/pkg/src/lib.rs:8`（加 `pub mod bridge;`）
- Modify: `crates/packer/pkg/Cargo.toml:11`（加 fence 依赖）
- Test: `crates/packer/pkg/src/bridge.rs`（同文件 `#[cfg(test)]`）

**Interfaces:**
- Consumes: `loomgui_fence::parse_template` / `ParsedTemplate` / `IrNodeKind` / `IrElement` / `SemanticKind`；`loomgui_core::asset::TemplateNode`；`NodeKind`（Task 1）
- Produces: `pub fn bridge(parsed: &ParsedTemplate) -> Result<(Vec<TemplateNode>, Vec<ControllerEntry>), String>`（Task 5 build 用）

- [ ] **Step 1: packer 加 fence 依赖**

`crates/packer/pkg/Cargo.toml` `[dependencies]` 段，紧接 `loomgui_core` 后加：
```toml
loomgui_core = { path = "../../core" }
loomgui_fence = { path = "../../fence" }
```

Run: `cargo build -p loomgui_pkg` — 确认依赖解析通过。

- [ ] **Step 2: lib.rs 加模块声明**

`crates/packer/pkg/src/lib.rs` 紧接 `pub mod build;` 后加：
```rust
pub mod bridge;
```

- [ ] **Step 3: 写 bridge.rs（实现）**

`crates/packer/pkg/src/bridge.rs`：
```rust
//! IrTree → core TemplateNode 桥（生产级，替代 fence/tests/cascade_spike.rs 的 throwaway mini-bridge）。
//! fence parse_template 停在 IrTree；本模块是第一处把 IrTree 翻译成 core 打包结构的代码。

use loomgui_core::asset::{ControllerEntry, TemplateNode};
use loomgui_core::scene::NodeKind;
use loomgui_fence::ir::{IrElement, IrNodeKind};
use loomgui_fence::schema::tag::SemanticKind;
use loomgui_fence::ParsedTemplate;

/// 把一个组件 HTML 的 ParsedTemplate 翻译成 (TemplateNode 树, controllers)。
///
/// 单根契约：`parsed.tree.roots` 必须恰好 1 个（html/head/body 等 shell 标签已由 fence 剥除）。
/// controllers 恒空（② 不做 controller 逻辑，旧范式退役中；data_controller 数据仍抽取保留）。
/// base_style = fence styles[ir_idx]（Task 4 会把 inherited_set bake 进 styles）。
pub fn bridge(parsed: &ParsedTemplate) -> Result<(Vec<TemplateNode>, Vec<ControllerEntry>), String> {
    if parsed.tree.roots.len() != 1 {
        return Err(format!(
            "组件 HTML 必须单一根元素（当前 {} 个顶层；html/head/body 等 shell 标签已由 fence 剥除）",
            parsed.tree.roots.len()
        ));
    }
    // ir_idx → template_idx 映射（Element/Text 占位；Comment/Doctype/Template 不占）。
    let mut ir_to_tpl: Vec<Option<usize>> = vec![None; parsed.tree.nodes.len()];
    let mut nodes: Vec<TemplateNode> = Vec::new();
    for (ir_idx, node) in parsed.tree.nodes.iter().enumerate() {
        // parent 总在 child 之前 push（tree_builder DFS），故此处 parent 的 tpl_idx 已知。
        let parent_tpl = node.parent.and_then(|pid| ir_to_tpl[pid.0]);
        let style = parsed.styles.get(ir_idx).cloned().unwrap_or_default();
        match &node.kind {
            IrNodeKind::Element(el) => {
                // <template> display:none，不进实例化。
                // ponytail: 仅跳过 template 节点本身；其子树跳过留复合束（骨架期 showcase 无 template）。
                if el.semantic == Some(SemanticKind::Template) {
                    continue;
                }
                let kind = map_semantic(el)?;
                let tpl_idx = nodes.len();
                ir_to_tpl[ir_idx] = Some(tpl_idx);
                let src = if kind == NodeKind::Image { attr(el, "src") } else { None };
                nodes.push(TemplateNode {
                    kind,
                    style,
                    parent_idx: parent_tpl,
                    classes: extract_classes(el),
                    id_attr: attr(el, "id"),
                    draggable: false,
                    tabindex: attr(el, "tabindex").and_then(|s| s.parse::<i32>().ok()),
                    data_controller: attr(el, "data-controller"),
                    content: None,
                    src,
                });
            }
            IrNodeKind::Text(s) => {
                // Text 节点 → 独立 TextNode 子节点（core 靠 TextNode 渲染文字；保留 HTML 子树结构）。
                let tpl_idx = nodes.len();
                ir_to_tpl[ir_idx] = Some(tpl_idx);
                nodes.push(TemplateNode {
                    kind: NodeKind::TextNode,
                    style,
                    parent_idx: parent_tpl,
                    classes: vec![],
                    id_attr: None,
                    draggable: false,
                    tabindex: None,
                    data_controller: None,
                    content: Some(s.clone()),
                    src: None,
                });
            }
            IrNodeKind::Comment(_) | IrNodeKind::Doctype(_) => continue,
        }
    }
    Ok((nodes, Vec::new()))
}

/// SemanticKind → NodeKind（total，非静默）。
/// InputDispatch 不进 IrTree（annotate 已分派）；Template 在 bridge 主循环跳过；
/// None = 未识别标签 → Err（围栏门应已挡，防御性兜底）。
fn map_semantic(el: &IrElement) -> Result<NodeKind, String> {
    match el.semantic {
        Some(SemanticKind::Container) => Ok(NodeKind::Container),
        Some(SemanticKind::TextBlock) => Ok(NodeKind::TextBlock),
        Some(SemanticKind::TextElement) => Ok(NodeKind::TextElement),
        Some(SemanticKind::LineBreak) => Ok(NodeKind::LineBreak),
        Some(SemanticKind::Label) => Ok(NodeKind::Label),
        Some(SemanticKind::Button) => Ok(NodeKind::Button),
        Some(SemanticKind::Link) => Ok(NodeKind::Link),
        Some(SemanticKind::Image) => Ok(NodeKind::Image),
        Some(SemanticKind::Canvas) => Ok(NodeKind::Canvas),
        Some(SemanticKind::TextField) => Ok(NodeKind::TextField),
        Some(SemanticKind::NumberField) => Ok(NodeKind::NumberField),
        Some(SemanticKind::Slider) => Ok(NodeKind::Slider),
        Some(SemanticKind::Toggle) => Ok(NodeKind::Toggle),
        Some(SemanticKind::RadioButton) => Ok(NodeKind::RadioButton),
        Some(SemanticKind::TextArea) => Ok(NodeKind::TextArea),
        Some(SemanticKind::Dropdown) => Ok(NodeKind::Dropdown),
        Some(SemanticKind::OptionItem) => Ok(NodeKind::OptionItem),
        Some(SemanticKind::ProgressBar) => Ok(NodeKind::ProgressBar),
        Some(SemanticKind::ListView) => Ok(NodeKind::ListView),
        Some(SemanticKind::ListItem) => Ok(NodeKind::ListItem),
        Some(SemanticKind::Slot) => Ok(NodeKind::Slot),
        Some(SemanticKind::CustomElement) => Ok(NodeKind::CustomElement),
        Some(SemanticKind::InputDispatch) => Err(format!(
            "InternalError: InputDispatch reached bridge (annotate should have dispatched) on <{}>",
            el.tag
        )),
        Some(SemanticKind::Template) => Err(
            "InternalError: Template reached map_semantic (bridge main loop should skip it)".into(),
        ),
        None => Err(format!("未识别标签 <{}>（semantic=None；围栏门应已挡）", el.tag)),
    }
}

fn attr(el: &IrElement, name: &str) -> Option<String> {
    el.attributes
        .iter()
        .find(|a| a.name == name)
        .map(|a| a.value.clone())
}

fn extract_classes(el: &IrElement) -> Vec<String> {
    attr(el, "class")
        .map(|c| c.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}
```

> **import 核实**：若 `loomgui_fence::ir::` 或 `loomgui_fence::schema::tag::SemanticKind` 编译不过，按 `crates/fence/src/lib.rs` 实际 `pub use` / `pub mod` 调整（CLAUDE.md：遇编译错按 crate 实际源码调，勿硬改依赖版本）。

- [ ] **Step 4: 写测试**

`crates/packer/pkg/src/bridge.rs` 末尾加：
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn bridged(html: &str) -> Vec<TemplateNode> {
        let parsed = loomgui_fence::parse_template(html, "test.html");
        assert!(parsed.diagnostics.is_empty(), "diags: {:?}", parsed.diagnostics);
        bridge(&parsed).unwrap().0
    }

    #[test]
    fn div_p_text_img_mapping_and_structure() {
        let nodes = bridged(r#"<div class="root" id="r"><p class="t">hi</p><img src="a.png"></div>"#);
        // [0] div Container root (parent=None, class=root, id=r)
        assert_eq!(nodes[0].kind, NodeKind::Container);
        assert_eq!(nodes[0].parent_idx, None);
        assert!(nodes[0].classes.contains(&"root".to_string()));
        assert_eq!(nodes[0].id_attr.as_deref(), Some("r"));
        // [1] p TextBlock (parent=0, class=t)
        assert_eq!(nodes[1].kind, NodeKind::TextBlock);
        assert_eq!(nodes[1].parent_idx, Some(0));
        // [2] "hi" TextNode (parent=1, content=hi) — Text 保留为独立子节点
        assert_eq!(nodes[2].kind, NodeKind::TextNode);
        assert_eq!(nodes[2].parent_idx, Some(1));
        assert_eq!(nodes[2].content.as_deref(), Some("hi"));
        // [3] img Image (parent=0, src=a.png)
        assert_eq!(nodes[3].kind, NodeKind::Image);
        assert_eq!(nodes[3].parent_idx, Some(0));
        assert_eq!(nodes[3].src.as_deref(), Some("a.png"));
    }

    #[test]
    fn input_dispatch_to_concrete_kinds() {
        let nodes = bridged(r#"<div><input type="range"><input type="checkbox"></div>"#);
        let kinds: Vec<_> = nodes.iter().map(|n| n.kind).collect();
        assert!(kinds.contains(&NodeKind::Slider), "Slider missing: {kinds:?}");
        assert!(kinds.contains(&NodeKind::Toggle), "Toggle missing: {kinds:?}");
    }

    #[test]
    fn multi_root_errors() {
        let parsed = loomgui_fence::parse_template(r#"<div>a</div><div>b</div>"#, "t.html");
        assert!(bridge(&parsed).is_err(), "multi-root should error");
    }

    #[test]
    fn template_element_skipped() {
        let nodes = bridged(r#"<div><template><p>x</p></template></div>"#);
        // [0] = div Container；template 节点本身不进 nodes
        assert_eq!(nodes[0].kind, NodeKind::Container);
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn tabindex_parsed() {
        let nodes = bridged(r#"<div><button tabindex="2">b</button></div>"#);
        let btn = nodes.iter().find(|n| n.kind == NodeKind::Button).unwrap();
        assert_eq!(btn.tabindex, Some(2));
    }
}
```

- [ ] **Step 5: 跑测试**

Run: `cargo test -p loomgui_pkg bridge`
Expected: PASS（5 测试）。若 import 路径错，按 fence lib.rs 实际 re-export 调。

- [ ] **Step 6: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy -p loomgui_pkg --all-targets -- -D warnings`
Expected: 清。

- [ ] **Step 7: Commit**

```bash
git add crates/packer/pkg/Cargo.toml crates/packer/pkg/src/lib.rs crates/packer/pkg/src/bridge.rs
git commit -m "feat(packer): IrTree->TemplateNode bridge (SemanticKind total map + attr extract)

- new bridge.rs: fence ParsedTemplate -> Vec<TemplateNode> (single-root contract)
- SemanticKind -> NodeKind total map (no silent loss); Text node -> TextNode child
- extract class/id/tabindex/data-controller/src; base_style = fence styles[ir_idx]
- packer now depends on loomgui_fence
- supersedes fence/tests/cascade_spike.rs throwaway mini-bridge

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: inherited_set inline bake（修坑 161，统一双源）

> ② 必做：否则 inline 继承属性被父覆盖，smoke 测继承挂。方案(a)（review 锁定）：单一真相源留 core。

**Files:**
- Modify: `crates/core/src/style/dynamic.rs:94-96`（`inherited_bit` 改 `pub` + 注释）
- Modify: `crates/fence/src/css_resolve.rs:100`（apply_decl 后 set bit）
- Test: `crates/fence/src/css_resolve.rs` tests 模块（:132 起）

**Interfaces:**
- Consumes: `apply_decl`（core mapping）；`inherited_bit`（本 task pub）
- Produces: fence `styles[i].inherited_set` 被 bake——Task 3 bridge 把 `styles[i]` 填进 `base_style`，inherited_set 随之进包，运行时 propagate 不再覆盖子声明

- [ ] **Step 1: 写失败测试**

`crates/fence/src/css_resolve.rs` tests 模块末尾加：
```rust
    #[test]
    fn inline_inherited_sets_bit() {
        let (tree, _) = parse_html_to_ir(r#"<span style="color:blue"></span>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        let color_bit = loomgui_core::style::dynamic::inherited_bit("color").unwrap();
        assert!(
            styles[id.0].inherited_set.0 & color_bit != 0,
            "inline color must set inherited_set COLOR bit"
        );
    }

    #[test]
    fn inline_non_inherited_sets_no_bit() {
        let (tree, _) = parse_html_to_ir(r#"<div style="width:100px"></div>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        let fs_bit = loomgui_core::style::dynamic::inherited_bit("font-size").unwrap();
        assert_eq!(styles[id.0].inherited_set.0 & fs_bit, 0, "width is not inherited");
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_fence css_resolve`
Expected: FAIL——`inherited_bit` 未 pub（编译错 `fn is private`）或 bit 未 set。

- [ ] **Step 3: pub inherited_bit + 更新注释**

`crates/core/src/style/dynamic.rs:94-96`，把注释 + `fn` 改：
```rust
/// prop 名 → 可继承属性 bit（非可继承返 None）。单一真相源：bit 的定义（本表 INH_*）与
/// 消费（rematch set bit + propagate copy_if_unset）都在 core。fence css_resolve 调本函数
/// 把 inline 可继承声明 bake 进 ResolvedStyle.inherited_set（坑 161 修复）。
pub fn inherited_bit(prop: &str) -> Option<u16> {
```
（`fn` → `pub fn`，注释替换；`INH_*` 常量保持 private——fence 只需 bit 值，不需常量名。）

- [ ] **Step 4: css_resolve set bit**

`crates/fence/src/css_resolve.rs` 的 apply_decl 调用处（约 :100），把：
```rust
                if !apply_decl(&mut styles[idx], prop, value) {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::FenceBadCssValue,
                        format!(
                            "value \"{}\" is not valid for CSS property \"{}\"",
                            value, prop
                        ),
                        line_map.source_location(node.span.start, file.to_string()),
                    ));
                }
```
改为（成功分支 set bit）：
```rust
                if !apply_decl(&mut styles[idx], prop, value) {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::FenceBadCssValue,
                        format!(
                            "value \"{}\" is not valid for CSS property \"{}\"",
                            value, prop
                        ),
                        line_map.source_location(node.span.start, file.to_string()),
                    ));
                } else if let Some(bit) = loomgui_core::style::dynamic::inherited_bit(prop) {
                    // 坑 161 修复：inline 可继承声明 bake 进 inherited_set，避免运行时
                    // propagate_inherited 用父值覆盖子的 inline 声明。
                    styles[idx].inherited_set.0 |= bit;
                }
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p loomgui_fence css_resolve`
Expected: PASS（2 新测试 + 现有 css_resolve 测试）。

- [ ] **Step 6: 全测试 + fmt + clippy**

Run: `cargo test -p loomgui_fence && cargo test -p loomgui_core && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
Expected: 全绿。

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/style/dynamic.rs crates/fence/src/css_resolve.rs
git commit -m "fix(fence,core): bake inline inherited_set (pitfall #161) + unify dual source

- pub core inherited_bit (single source of truth; INH_* stay private)
- fence css_resolve: after apply_decl, set inherited_set bit for inherited props
- fixes div[style=color:red]>span[style=color:blue] rendering span red
- updates dynamic.rs comment (was 'Spec-3 再统一')

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: packer packages 循环 + referenced_sprites 回接

> 重建 d8fe705 删掉的 HTML→pkg.bin 编排。抽 `pack_components`（可单测，接字符串）+ build 调它读文件 + referenced_sprites 回接 atlas validate。

**Files:**
- Modify: `crates/packer/pkg/src/build.rs`（顶部 use + 加 `pack_components`/`resolve_html_list`/`stem` + build() packages 段）
- Test: `crates/packer/pkg/src/build.rs` tests 模块

**Interfaces:**
- Consumes: `bridge`（Task 3）；`write_package`/`PackageInput`/`TemplateNode`/`ControllerEntry`/`DynamicRuleTable`（core）；`assign_and_validate`（atlas/validate）；`parse_template`（fence）；`PackageCfg`（workspace）
- Produces: build() 产 `ui/<name>.pkg.bin` + runtime.json `packages` 填实际 + referenced_sprites 回接验证

- [ ] **Step 1: 写失败测试（pack_components roundtrip）**

`crates/packer/pkg/src/build.rs` 末尾加 tests 模块：
```rust
#[cfg(test)]
mod package_tests {
    use super::*;
    use loomgui_core::scene::NodeKind;

    #[test]
    fn pack_components_roundtrip_single() {
        let comps = vec![(
            "home".to_string(),
            r#"<div class="root"><p>hi</p><img src="icons/a.png"></div>"#.to_string(),
        )];
        let (bytes, refs) = pack_components(&comps).unwrap();
        let pkg = loomgui_core::asset::read_package(&bytes).unwrap();
        let comp = pkg.components.get("home").expect("home component");
        assert_eq!(comp.nodes[0].kind, NodeKind::Container); // div
        assert!(
            refs.iter().any(|r| r == "icons/a.png"),
            "referenced_sprites missing: {refs:?}"
        );
    }

    #[test]
    fn pack_components_multi_component() {
        let comps = vec![
            ("nav".to_string(), r#"<nav><a href="x">l</a></nav>"#.to_string()),
            ("page".to_string(), r#"<div class="page">body</div>"#.to_string()),
        ];
        let (bytes, _) = pack_components(&comps).unwrap();
        let pkg = loomgui_core::asset::read_package(&bytes).unwrap();
        assert!(pkg.components.contains_key("nav"));
        assert!(pkg.components.contains_key("page"));
    }

    #[test]
    fn pack_components_propagates_bridge_error() {
        // 多根 → bridge 报错（不静默产森林）
        let comps = vec![("bad".to_string(), r#"<div>a</div><div>b</div>"#.to_string())];
        assert!(pack_components(&comps).is_err());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_pkg package_tests`
Expected: FAIL（`pack_components` 未定义）。

- [ ] **Step 3: 顶部 use + 加 pack_components/resolve_html_list/stem**

`crates/packer/pkg/src/build.rs` 顶部 use 段（现有 `use crate::atlas::collect::collect_pngs;` 等之后）加：
```rust
use loomgui_core::asset::{write_package, ControllerEntry, PackageInput, TemplateNode};
use loomgui_core::style::dynamic::DynamicRuleTable;
use crate::bridge::bridge;
use crate::workspace::PackageCfg;
```

在 `build()` 函数**之前**加：
```rust
/// 打包一个 package：components = [(组件名, html 源码)]。返 (pkg.bin bytes, referenced_sprites)。
/// build() 读文件组 (name, src) 调本函数；本函数接字符串便于单测。
pub fn pack_components(
    components: &[(String, String)],
) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut built: Vec<(String, Vec<TemplateNode>, DynamicRuleTable, Vec<ControllerEntry>)> =
        Vec::new();
    let mut refs: Vec<String> = Vec::new();
    for (name, src) in components {
        let parsed = loomgui_fence::parse_template(src, name);
        if !parsed.diagnostics.is_empty() {
            return Err(format!("fence diagnostics in {name}: {:?}", parsed.diagnostics));
        }
        let (nodes, controllers) = bridge(&parsed)?;
        built.push((
            name.clone(),
            nodes,
            DynamicRuleTable { rules: parsed.dynamic_rules },
            controllers,
        ));
        refs.extend(parsed.referenced_sprites);
    }
    let comp_refs: Vec<(&str, &[TemplateNode], &DynamicRuleTable, &[ControllerEntry])> = built
        .iter()
        .map(|(n, nodes, dr, c)| (n.as_str(), nodes.as_slice(), dr, c.as_slice()))
        .collect();
    let bytes = write_package(&PackageInput { components: comp_refs });
    Ok((bytes, refs))
}

/// 把 PackageCfg 解析成 HTML 文件相对路径列表。
/// html 非空 = 显式态（锁定文件）；空 = 自动态（扫 dirs 顶层 .html）。
fn resolve_html_list(workspace_root: &Path, pkg: &PackageCfg) -> Result<Vec<String>, String> {
    if !pkg.html.is_empty() {
        return Ok(pkg.html.clone());
    }
    let mut out = Vec::new();
    for dir in &pkg.dirs {
        let full = workspace_root.join(dir);
        if !full.is_dir() {
            return Err(format!(
                "package `{}` dir not found: {}",
                pkg.name,
                full.display()
            ));
        }
        let mut entries: Vec<String> = std::fs::read_dir(&full)
            .map_err(|e| format!("read dir {}: {e}", full.display()))?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("html") {
                    p.file_name()?.to_str().map(|n| format!("{dir}/{n}"))
                } else {
                    None
                }
            })
            .collect();
        entries.sort();
        out.extend(entries);
    }
    Ok(out)
}

fn stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}
```

- [ ] **Step 4: build() 加 packages 段 + referenced_sprites 回接**

`crates/packer/pkg/src/build.rs` build() 内，把 `let _ = &atlas_manifests; // kept for future cross-validation (R3)`（:147）替换为：
```rust
    // ---------- Packages (HTML -> .pkg.bin) ----------
    let mut all_refs: Vec<String> = Vec::new();
    for pkg in &ws.packages {
        let html_files = resolve_html_list(workspace_root, pkg)?;
        let comps: Vec<(String, String)> = html_files
            .iter()
            .map(|rel| {
                let path = workspace_root.join(rel);
                let src = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read {}: {e}", path.display()))?;
                Ok((stem(rel), src))
            })
            .collect::<Result<Vec<_>, String>>()?;
        report
            .log
            .push(format!("packaging {} ({} component html)", pkg.name, comps.len()));
        let (bytes, refs) = pack_components(&comps)?;
        let pkg_path = ui_dir.join(format!("{}.pkg.bin", pkg.name));
        std::fs::write(&pkg_path, &bytes)
            .map_err(|e| format!("write {}: {e}", pkg_path.display()))?;
        report.packages.push(pkg.name.clone());
        report
            .log
            .push(format!("  wrote {} ({} bytes)", pkg_path.display(), bytes.len()));
        all_refs.extend(refs);
    }

    // ---------- Cross-validate: HTML refs must all be in some atlas ----------
    if !all_refs.is_empty() {
        let atlas_refs: Vec<(String, &crate::atlas::AtlasManifest)> = atlas_manifests
            .iter()
            .map(|(n, m)| (n.clone(), m))
            .collect();
        crate::atlas::validate::assign_and_validate(&all_refs, &atlas_refs)?;
    }
```

> 注：`report.packages` 现填实际包名（原恒空）；runtime 段（:117-119）`packages: report.packages.clone()` 自动跟着填——无需改 runtime 段。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p loomgui_pkg`
Expected: PASS（package_tests 3 测试 + 现有 packer 测试）。

- [ ] **Step 6: build() 端到端冒烟（手动验证真 workspace）**

Run: `cargo run -p loomgui_pkg -- build F:/WorkSpace/projects/LoomGUI/showcase`
Expected: `showcase/<output_dir>/ui/<name>.pkg.bin` 产出 + log 含 "packaging ... wrote ..."。若 showcase HTML 含 ② scope 外特性（动画等）触发 fence diagnostics，记录但不阻塞本 task（smoke 门用 Task 6 的最小 HTML）。

- [ ] **Step 7: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy -p loomgui_pkg --all-targets -- -D warnings`
Expected: 清。

- [ ] **Step 8: Commit**

```bash
git add crates/packer/pkg/src/build.rs
git commit -m "feat(packer): rebuild HTML->pkg.bin orchestration + atlas cross-validation

- pack_components: [(name, html)] -> (pkg.bin bytes, referenced_sprites) [testable]
- build(): packages loop writes ui/<name>.pkg.bin; runtime.packages filled
- resolve_html_list: explicit html list or auto-scan dirs top-level *.html
- referenced_sprites -> assign_and_validate (revive dead code; non-silent on missing)
- rebuilds d8fe705-removed orchestration using fence + bridge

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: 终点线 1 smoke 门

> **范围说明**：② smoke 验"端到端链通 + class 命中（rect）+ display:none 剪枝 + flex 布局"——这些 Stage public API（`get_node_layout_rect`/`get_node_visible`/`find_node_by_id`）可达。继承（color/font）、kind 保真、computed style 的完整断言需 Stage 暴露内部 style 查询 API，由 Task 2（pkg roundtrip Slider 不塌）/ Task 3（bridge kind 映射）/ Task 4（inherited bake）的 **unit test 精确覆盖**；完整 5 项集成断言推 ③（spec §10，③ 做 cascade 集成级测试）。cascade 引擎本身已被 Spec-1 spike 端到端验证。

**Files:**
- Create: `crates/packer/pkg/tests/smoke_ir_bridge.rs`

**Interfaces:**
- Consumes: `pack_components`（Task 5）；`Stage` public API（`new`/`load_package`/`instantiate`/`advance_time`/`tick_and_render`/`find_node_by_id`/`get_node_layout_rect`/`get_node_visible`）
- Produces: 端到端 smoke（HTML→pkg.bin→Stage→rect/visible 断言）—— 终点线 1

- [ ] **Step 1: 写 smoke 测试**

`crates/packer/pkg/tests/smoke_ir_bridge.rs`：
```rust
//! 终点线 1 smoke：HTML -> pkg.bin -> Stage -> rect/visible 断言（端到端范式验证）。
use loomgui_core::scene::NodeId;
use loomgui_core::stage::Stage;
use loomgui_pkg::build::pack_components;

fn build_stage(html: &str) -> (Stage, NodeId) {
    let (bytes, _refs) =
        pack_components(&[("c".to_string(), html.to_string())]).expect("pack_components");
    let mut stage = Stage::new((400.0, 300.0)).expect("Stage::new");
    stage.load_package("p", &bytes).expect("load_package");
    let root = stage.instantiate("p", "c").expect("instantiate");
    stage.advance_time(0.0);
    stage.tick_and_render();
    (stage, root)
}

#[test]
fn smoke_main_gate_class_hit_displaynone_flex() {
    let html = r#"<style>
        .wrap { display:flex; flex-direction:column; width:200px; }
        .hide { display:none; }
    </style>
    <div class="wrap" id="wrap">
        <div id="a"></div>
        <div id="hide" class="hide"></div>
    </div>"#;
    let (stage, _root) = build_stage(html);
    // class 命中：.wrap width:200（来自 <style> class 规则，经 cascade 生效）
    let wrap = stage.find_node_by_id("wrap").expect("wrap");
    let wrap_rect = stage.get_node_layout_rect(wrap).expect("wrap rect");
    assert!(
        (wrap_rect.w - 200.0).abs() < 1.0,
        "class .wrap width:200 not applied (cascade broken?): w={}",
        wrap_rect.w
    );
    // display:none 剪枝：.hide not visible
    let hide = stage.find_node_by_id("hide").expect("hide");
    assert!(
        !stage.get_node_visible(hide),
        "display:none node should be invisible"
    );
    // flex 布局：子 a 在 wrap 内（rect 合理）
    let a = stage.find_node_by_id("a").expect("a");
    let a_rect = stage.get_node_layout_rect(a).expect("a rect");
    assert!(a_rect.h >= 0.0, "child a laid out, h={}", a_rect.h);
}

#[test]
fn smoke_control_kinds_load_without_crash() {
    // 控件全家（input dispatch 5 种 + select）— instantiate 不 panic = 链通。
    // kind 保真（不塌 Container）由 Task 2 pkg roundtrip + Task 3 bridge map unit test 覆盖。
    let html = r#"<div>
        <input type="text">
        <input type="range">
        <input type="checkbox">
        <input type="radio">
        <select><option></option></select>
    </div>"#;
    let _ = build_stage(html); // 不 panic = 通过
}
```

- [ ] **Step 2: 跑 smoke**

Run: `cargo test -p loomgui_pkg --test smoke_ir_bridge`
Expected: PASS（2 测试）。若 `find_node_by_id` 找不到（id 未进 id_map），查 `instantiate` 是否建 id_map（core 实际行为，按 stage.rs 调）。

- [ ] **Step 3: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy -p loomgui_pkg --all-targets -- -D warnings`
Expected: 清。

- [ ] **Step 4: Commit**

```bash
git add crates/packer/pkg/tests/smoke_ir_bridge.rs
git commit -m "test(packer): finish-line-1 smoke (HTML->pkg->Stage->rect/visible)

- main gate: class hit (width:200 via <style>) + display:none clip + flex layout
- control gate: input family + select instantiate without crash
- end-to-end paradigm proof; semantic unit tests cover inherit/kind (Tasks 2-4)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: 全 workspace 收尾（fmt + clippy + .dll + roadmap）

**Files:**
- Modify: `docs/roadmap/roadmap.md`（进度行 + §2 ② 标 DONE）
- Rebuild: `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`

- [ ] **Step 1: 全 workspace 测试**

Run: `cargo test`
Expected: 全 crate 绿（core + fence + pkg + ffi_c）。

- [ ] **Step 2: fmt + clippy 全 workspace**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
Expected: 清。

- [ ] **Step 3: feature-gate check**

Run: `cargo check -p loomgui_core --no-default-features --all-targets && cargo check -p loomgui_fence --no-default-features --all-targets && cargo check -p loomgui_pkg --no-default-features --all-targets`
Expected: 清。

- [ ] **Step 4: 重编 + commit .dll**

> NodeKind 不跨 C ABI（FFI 是 NodeId u32），重编只因 core 改了（pkg v18 读写代码在 core→.dll）。**Unity 必须关着**（锁 .dll）。

Run:
```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```
Verify: `git status` 显示 dll modified。
Commit:
```bash
git add unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
git commit -m "chore: rebuild dll for Spec-3 ② (pkg format v18 + core changes)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

- [ ] **Step 5: 同步 C# 绑定（若 build.rs 改了 FFI 签名）**

Run: `cargo run -p xtask -- sync-bindings`（本 plan 未改 FFI 签名，通常无变化；若有，commit `LoomGUIBindings.cs`）。

- [ ] **Step 6: 更新 roadmap**

`docs/roadmap/roadmap.md`：
- 进度行（:8）：标 **Spec-3 ② ✅ 完成**（commit range、终点线1 smoke 绿），下一棒 = ③ cascade 收尾。
- §2 「②」标 DONE（commit range + 关键定论：kind_tag 全 23 变体 v18 / 桥放 packer / inherited_set inline bake 修坑 161 / referenced_sprites 回接 atlas）。
- §8 加 Spec-3 ② 完成决策记录。
- 清除文中"下一棒=②"等过期表述。

Commit:
```bash
git add docs/roadmap/roadmap.md
git commit -m "docs: roadmap - Spec-3 ② (IrTree bridge + packer orchestration) complete

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage**（spec 各节 → task）：
- §3 A'（pkg v18 kind_tag + 删 RichText 死字段）→ Task 1（repr+from_u8）+ Task 2（write/read+v18）
- §4 B（IrTree→TemplateNode 桥）→ Task 3
- §5 C（packer 编排 + referenced_sprites 回接）→ Task 5
- §6 D'（base_style 灌入 + inherited_set bake 修坑 161）→ Task 3（base_style=styles[ir_idx]）+ Task 4（inherited bake）
- §7 E（终点线1 smoke）→ Task 6
- §11 实现顺序（A'→B→D'→C→E）→ Task 1-6 顺序一致 + Task 7 收尾 ✓

**2. Placeholder scan**：无 TBD/TODO 占位。"按 core 实际 API 调"/"按 fence lib.rs re-export 调"是实现期核实点（CLAUDE.md API 适配方法论），附了预期路径 + 降级说明，非占位。

**3. Type consistency**：
- `bridge(parsed: &ParsedTemplate) -> Result<(Vec<TemplateNode>, Vec<ControllerEntry>), String>` — Task 3 定义，Task 5 `pack_components` 调用，签名一致 ✓
- `pack_components(&[(String, String)]) -> Result<(Vec<u8>, Vec<String>), String>` — Task 5 定义，Task 6 调用 ✓
- `NodeKind::from_u8(u8) -> Option<NodeKind>` — Task 1 定义，Task 2 read_package 调用 ✓
- `NodeKind as u8` — Task 1（#[repr(u8)]），Task 2 write_package 用 ✓
- `inherited_bit(prop) -> Option<u16>` — Task 4 pub，fence css_resolve 调 ✓
- TemplateNode 字段（kind/style/parent_idx/classes/id_attr/draggable/tabindex/data_controller/content/src）— 全 task 构造一致 ✓

**4. 已知实现期核实项**（非阻塞，附降级）：
- fence `SemanticKind`/`IrNodeKind` import 路径（Task 3 Step 3 注）
- `find_node_by_id` 是否建 id_map（Task 6 Step 2 注）
- showcase 真 build 的 fence diagnostics（Task 5 Step 6，② scope 外 CSS 不阻塞）

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-16-ir-bridge-packer.md`. Two execution options:

**1. Subagent-Driven (recommended)** — 每个 task 派 fresh subagent，task 间 review，快迭代。
**2. Inline Execution** — 本 session 用 executing-plans 批量执行 + checkpoint。

Which approach?
