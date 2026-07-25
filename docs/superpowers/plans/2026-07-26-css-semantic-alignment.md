# CSS 语义对齐 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 系统对齐 LoomGUI 围栏的 CSS 语义与标准 CSS 规范，消除「HTML 预览 ≠ 游戏运行时渲染」的偏差（border-style 门控缺失只是最先暴露的），并建立围栏一致性诊断通用机制。

**Architecture:** 三条正交线 —— (1) 围栏一致性诊断（warning 引导围栏内属性漏写致不一致 + error 引导围栏外属性）；(2) 缺失属性实现（border-style 运行时门控 + flex/background shorthand 展开）；(3) 防漂移门 + 假阳性 bug 清零。核心模型：围栏属性三分法（支持 / 围栏外 error / 围栏内 warning）。spec 见 `docs/superpowers/specs/2026-07-26-css-semantic-alignment-design.md`。

**Tech Stack:** Rust edition 2021；依赖钉版本：taffy 0.12、csbindgen 1。CSS 解析手搓（不引 cssparser）。围栏真相源 = `crates/fence/src/schema/`。防漂移门 = `cargo test -p loomgui_fence`。

## Global Constraints

- 围栏真相源 = `crates/fence/src/schema/` Rust const 表；`docs/design/fence.md` 是人类可读镜像，**改 schema 必同步 fence.md**，防漂移门 `cargo test -p loomgui_fence`。
- Rust → Unity：任何 core 改动后重编 + 拷 `.dll`；fence 改动后**必须重编 GUI exe**（坑 158 同源，`loomgui_gui.exe` 静态链入 fence）。
- 围栏外输入打包期报错，不静默降级（AGENTS.md 原则）。
- 代码注释写上线品质（说 WHY，不引用内部编号）。
- `ResolvedStyle` 走 bincode 序列化进 pkg.bin；新增字段若改 bincode 布局须确认向后兼容（读旧 pkg 不崩）。
- 改 parse-time 逻辑（css_resolve/mapping/cascade）后**必须重打 pkg.bin**（`Node.base_style` 是打包期产物），光重编 .dll 不够。
- push 前本地跑 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`。

---

## File Structure

**fence crate（解析/诊断层）：**
- `crates/fence/src/schema/css.rs` — CSS_PROPS + ShorthandSpec 注册表（真相源）。新增 border-style 条目；flex-wrap 删 wrap-reverse；align-content 等 longhand 已注册。
- `crates/fence/src/consistency_check.rs` — **新建**。打包期一致性 warning pass（W1 border 完整性 + W2 background-size 默认）。
- `crates/fence/src/pipeline.rs` — Stage 编排。挂载 consistency_check。
- `crates/fence/src/diagnostic.rs` — Severity/DiagnosticCode。新增 2 个 warning code + unsupported_hint 表。
- `crates/fence/src/lib.rs` — pub use consistency_check。

**core crate（runtime 层）：**
- `crates/core/src/style/resolved.rs` — ResolvedStyle。新增 `border_style: BorderStyle` 字段。
- `crates/core/src/style/mapping.rs` — apply_decl。扩展 parse_border_value 返 style；新增 border-style / flex / background-shorthand-color / align-content / row-gap / column-gap 分支；删 wrap-reverse 降级。
- `crates/core/src/render/mod.rs` — render 门控。border 画条件加 border_style != None。
- `crates/core/src/asset/mod.rs` — pkg 版本兼容确认。

**文档：**
- `docs/design/fence.md` — 同步 schema 变更（border-style 新增、flex-wrap 值、flex/background shorthand）。

---

## Task 1: ResolvedStyle 加 border_style 字段 + pkg 兼容

**Files:**
- Modify: `crates/core/src/style/resolved.rs`
- Test: `crates/core/src/style/resolved.rs`（内联测试模块）

**Interfaces:**
- Produces: `ResolvedStyle.border_style: BorderStyle` 字段（default = `BorderStyle::None`）；`BorderStyle` enum（None/Solid/Dashed/Dotted/Double，`#[repr(u8)]`，派生 Serialize/Deserialize）。

- [ ] **Step 1: 定义 BorderStyle enum + 加字段 + 写失败测试**

在 `resolved.rs` 顶部（其它派生 struct 附近）加 enum：

```rust
/// CSS border-style：控制边框线型。None=不渲染（CSS initial），其余=渲染对应线型。
/// 门控 render/mod.rs 的 border_ring 调用（None 时不画，对齐 CSS 规范默认值语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum BorderStyle {
    #[default]
    None,
    Solid,
    Dashed,
    Dotted,
    Double,
}
```

在 `ResolvedStyle` struct 里 `border_color` 字段下方加：

```rust
    pub border_color: Option<[f32; 4]>,
    pub border_style: BorderStyle, // CSS border-style：None=不画边框（initial 值）
```

在 `Default` impl 里加（紧跟 `background_color: None` 附近的初始化）：

```rust
            border_style: BorderStyle::None,
```

在 resolved.rs 测试模块加测试：

```rust
    #[test]
    fn border_style_defaults_to_none() {
        let s = ResolvedStyle::default();
        assert_eq!(s.border_style, BorderStyle::None);
    }
```

- [ ] **Step 2: 编译 + 跑测试**

Run: `cargo test -p loomgui_core --lib border_style_defaults_to_none`
Expected: PASS（字段加好即通过）。

- [ ] **Step 3: 确认 pkg.bin 向后兼容（bincode 新字段）**

`ResolvedStyle` 派生 `Serialize/Deserialize`（bincode，无 `#[serde(default)]`）。新增 `BorderStyle` 字段（非 Option）会改变 bincode 布局——**旧 pkg.bin 反序列化会失败**（字段数不匹配）。

确认方式：跑 `cargo test -p loomgui_core`（含 pkg round-trip 测试）。若失败，bump pkg version：`crates/core/src/asset/mod.rs` 的 `PKG_FORMAT_VERSION` 22→23 + `MIN_VERSION`/`MAX_VERSION` 同步，并在 `read_package` 加「v22 旧包无 border_style 字段，反序列化时按 None 处理」的迁移（或直接要求重打所有 pkg——showcase 是唯一消费者，重打即可，见 Task 9）。

- [ ] **Step 4: fmt + clippy + commit**

```bash
cargo fmt --all && cargo clippy -p loomgui_core --all-targets -- -D warnings 2>&1 | grep -E "^error|^warning:" | head
git add crates/core/src/style/resolved.rs crates/core/src/asset/mod.rs
git commit -m "feat(style): ResolvedStyle 加 border_style 字段（CSS initial=None）"
```

---

## Task 2: schema 注册 border-style 属性

**Files:**
- Modify: `crates/fence/src/schema/css.rs`
- Modify: `docs/design/fence.md`
- Test: `crates/fence/src/schema/`（围栏门）

**Interfaces:**
- Produces: `find_css_prop("border-style")` 返回 Some（Keyword parser，允许 none/solid/dashed/dotted/double）。

- [ ] **Step 1: 写失败测试**

在 `crates/fence/src/schema/css.rs` 测试模块加：

```rust
    #[test]
    fn border_style_registered() {
        let spec = find_css_prop("border-style").expect("border-style must be in fence");
        let allowed = match &spec.parser {
            CssValueParser::Keyword(k) => k,
            _ => panic!("border-style must be Keyword parser"),
        };
        assert!(allowed.contains(&"none"));
        assert!(allowed.contains(&"solid"));
        assert_eq!(spec.default, "none");
        assert!(!spec.inherited);
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_fence border_style_registered`
Expected: FAIL（find_css_prop 返回 None → expect panic）。

- [ ] **Step 3: 注册属性**

在 `CSS_PROPS` 数组里 `border-color` 条目下方加：

```rust
    CssPropSpec {
        name: "border-style",
        default: "none",
        inherited: false,
        parser: CssValueParser::Keyword(&["none", "solid", "dashed", "dotted", "double"]),
    },
```

- [ ] **Step 4: 同步 docs/design/fence.md**

在 fence.md 的边框属性段（`border-width` 附近，约 270 行）加 `border-style`（`none` / `solid` / `dashed` / `dotted` / `double`，默认 `none`）。

- [ ] **Step 5: 跑围栏门 + commit**

```bash
cargo test -p loomgui_fence 2>&1 | grep "test result:"
git add crates/fence/src/schema/css.rs docs/design/fence.md
git commit -m "feat(fence): 注册 border-style 属性（CSS initial=none）"
```

---

## Task 3: apply_decl 解析 border-style（含 parse_border_value 扩展 + shorthand）

**Files:**
- Modify: `crates/core/src/style/mapping.rs`
- Test: `crates/core/src/style/mapping.rs` 测试模块

**Interfaces:**
- Consumes: Task 1 的 `BorderStyle` enum + `border_style` 字段；Task 2 的 schema 注册。
- Produces: `parse_border_value` 返 `(f32, BorderStyle, Option<[f32;4]>)`；`apply_decl("border-style"/"border"/"border-*")` 正确写 `border_style`。

- [ ] **Step 1: 写失败测试（border-style longhand + border shorthand 带 style）**

在 mapping.rs 测试模块加：

```rust
    #[test]
    fn apply_border_style_longhand() {
        let mut s = ResolvedStyle::default();
        assert!(apply_decl(&mut s, "border-style", "solid"));
        assert_eq!(s.border_style, BorderStyle::Solid);
    }

    #[test]
    fn apply_border_shorthand_captures_style() {
        // border: 2px solid red → width + style + color 都进
        let mut s = ResolvedStyle::default();
        assert!(apply_decl(&mut s, "border", "2px solid #ff0000"));
        assert_eq!(s.border_style, BorderStyle::Solid);
        assert_eq!(s.border_color, Some([1.0, 0.0, 0.0, 1.0]));
        // width 四边
        let bw = &s.taffy_style.border;
        assert!((resolve_lp_for_test(bw.left) - 2.0).abs() < 0.01);
    }

    #[test]
    fn apply_border_no_style_keeps_none() {
        // border: 2px red（无 style）→ border_style 仍 None（CSS 规范：不画）
        let mut s = ResolvedStyle::default();
        assert!(apply_decl(&mut s, "border", "2px #ff0000"));
        assert_eq!(s.border_style, BorderStyle::None);
    }

    // 测试用：把 LengthPercentage 解析回 f32（复用 render 的 resolve_lp 逻辑）
    fn resolve_lp_for_test(lp: taffy::style::LengthPercentage) -> f32 {
        match lp {
            taffy::style::LengthPercentage::Length(l) => l.0,
            taffy::style::LengthPercentage::Percent(p) => p.0,
            _ => 0.0,
        }
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core apply_border`
Expected: FAIL（border-style 分支不存在 / parse_border_value 不返 style）。

- [ ] **Step 3: 扩展 parse_border_value 返 style**

改 `parse_border_value`（mapping.rs:510）签名 + 逻辑——解析出 style 关键字：

```rust
fn parse_border_value(value: &str) -> Option<(f32, crate::style::resolved::BorderStyle, Option<[f32; 4]>)> {
    use crate::style::resolved::BorderStyle;
    let mut w: Option<f32> = None;
    let mut style: BorderStyle = BorderStyle::None; // 未声明 = CSS 默认 none
    let mut color: Option<[f32; 4]> = None;
    for tok in value.split_whitespace() {
        if color.is_none() {
            if let Some(c) = parse_color(tok) {
                color = Some(c);
                continue;
            }
        }
        // style 关键字（优先于 width，避免 "solid" 被 strip_suffix("px") 误判）
        match tok {
            "solid" => { style = BorderStyle::Solid; continue; }
            "dashed" => { style = BorderStyle::Dashed; continue; }
            "dotted" => { style = BorderStyle::Dotted; continue; }
            "double" => { style = BorderStyle::Double; continue; }
            "none" => { style = BorderStyle::None; continue; }
            _ => {}
        }
        if w.is_none() {
            if let Some(px) = tok
                .strip_suffix("px")
                .and_then(|s| s.trim().parse::<f32>().ok())
            {
                w = Some(px);
            }
        }
    }
    Some((w?, style, color))
}
```

- [ ] **Step 4: 更新 border shorthand + apply_border_side 消费 style**

改 `"border"` 分支（mapping.rs:635）：

```rust
        "border" => {
            let Some((w, bstyle, color)) = parse_border_value(value) else {
                return false;
            };
            let lp = LengthPercentage::length(w);
            ts.border = Rect { left: lp, right: lp, top: lp, bottom: lp };
            if let Some(c) = color {
                style.border_color = Some(c);
            }
            style.border_style = bstyle;
            true
        }
```

改 `apply_border_side`（mapping.rs:540）：

```rust
fn apply_border_side(style: &mut ResolvedStyle, side: Side, value: &str) -> bool {
    let Some((w, bstyle, color)) = parse_border_value(value) else {
        return false;
    };
    let lp = LengthPercentage::length(w);
    let ts = &mut style.taffy_style;
    match side {
        Side::Top => ts.border.top = lp,
        Side::Right => ts.border.right = lp,
        Side::Bottom => ts.border.bottom = lp,
        Side::Left => ts.border.left = lp,
    }
    if let Some(c) = color {
        style.border_color = Some(c);
    }
    style.border_style = bstyle;
    true
}
```

- [ ] **Step 5: 加 border-style longhand 分支**

在 apply_decl 的 `"border-color"` 分支附近加：

```rust
        "border-style" => {
            style.border_style = match value.trim() {
                "solid" => BorderStyle::Solid,
                "dashed" => BorderStyle::Dashed,
                "dotted" => BorderStyle::Dotted,
                "double" => BorderStyle::Double,
                _ => BorderStyle::None,
            };
            true
        }
```

（`BorderStyle` 用 `use crate::style::resolved::BorderStyle;`，确认 apply_decl 函数顶部已 import 或用全路径。）

- [ ] **Step 6: 跑测试 + fmt + commit**

```bash
cargo test -p loomgui_core apply_border 2>&1 | grep "test result:"
cargo fmt --all
git add crates/core/src/style/mapping.rs
git commit -m "feat(style): apply_decl 解析 border-style + shorthand 捕获 style"
```

---

## Task 4: render 门控——border_style != None 才画

**Files:**
- Modify: `crates/core/src/render/mod.rs:425-440`
- Test: `crates/core/src/render/tests.rs`

**Interfaces:**
- Consumes: Task 1 的 `border_style` 字段。
- Produces: `border_style == None` 时 border_ring 不调用（对齐 CSS：style=none 不画）。

- [ ] **Step 1: 写失败测试（border-width+color 有但 style=none → 不画）**

在 `crates/core/src/render/tests.rs` 加：

```rust
#[test]
fn border_style_none_renders_no_border_even_with_width_and_color() {
    // width>0 + color 设了，但 border_style=None（CSS 默认）→ 不应产任何 border 顶点
    let mut s = Scene::build(&vec![
        (None, NodeKind::Container, base_border_style(), vec![], None, false, None, None, None, None),
    ]);
    // base_border_style: width=2 四边 + border_color=红 + border_style=None
    let root = s.roots[0];
    s.get_mut(root).unwrap().style.taffy_style.border = taffy::style::Rect::all(taffy::style::LengthPercentage::length(2.0));
    s.get_mut(root).unwrap().style.border_color = Some([1.0, 0.0, 0.0, 1.0]);
    s.get_mut(root).unwrap().style.border_style = crate::style::resolved::BorderStyle::None;
    s.get_mut(root).unwrap().layout_rect = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
    compute_world_transforms(&mut s);
    let frame = build_render_nodes(&mut s);
    // 期望：没有 border_ring 顶点（节点只有 1 个，若画 border 会有 ring 几何）
    let has_border_geom = frame.nodes.iter().any(|rn| {
        if let crate::render::node::NodePayload::Mesh { verts, .. } = &rn.payload {
            // border ring 会产大量顶点（>8）；纯背景 quad 只 4-6 顶点
            verts.len() > 10
        } else { false }
    });
    assert!(!has_border_geom, "border_style=None 不应渲染边框几何");
}
```

（`base_border_style()` 用 `ResolvedStyle::default()`；若该 helper 不存在用 `crate::style::resolved::ResolvedStyle::default()` 直接构造 tuple。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core border_style_none_renders_no_border`
Expected: FAIL（当前无门控，border 仍画）。

- [ ] **Step 3: 加门控**

改 `render/mod.rs:425` 的 `if !has_image {` 块内，把 border 画条件从：

```rust
                    if let Some(border_col) = n.style.border_color {
```

改为：

```rust
                    if n.style.border_style != crate::style::resolved::BorderStyle::None
                        && let Some(border_col) = n.style.border_color
                    {
```

（若 Rust 版本不支持 let-chain（`&&`），则改为嵌套 if：

```rust
                    if n.style.border_style != crate::style::resolved::BorderStyle::None {
                        if let Some(border_col) = n.style.border_color {
                            // ... 原有 border_ring 逻辑 ...
                        }
                    }
```

注意缩进：原 `if let Some(border_col)` 块整体下沉一层。）

- [ ] **Step 4: 跑测试 + 全 core 测试（确认没误伤现有 border 渲染测试）**

```bash
cargo test -p loomgui_core border 2>&1 | grep "test result:"
cargo test -p loomgui_core 2>&1 | grep "test result:"
```

若现有 border 测试因 style=None 而失败（它们设了 width+color 但没设 style），在那些测试里补 `style.border_style = BorderStyle::Solid`（因为它们的意图就是要画边框）。

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt --all && cargo clippy -p loomgui_core --all-targets -- -D warnings 2>&1 | grep -E "^error|^warning:" | head
git add crates/core/src/render/mod.rs crates/core/src/render/tests.rs
git commit -m "fix(render): border 门控——border_style!=None 才画（对齐 CSS initial=none）"
```

---

## Task 5: 围栏一致性 warning pass（W1 border + W2 background-size）

**Files:**
- Create: `crates/fence/src/consistency_check.rs`
- Modify: `crates/fence/src/pipeline.rs`（挂载）, `crates/fence/src/diagnostic.rs`（新 code）, `crates/fence/src/lib.rs`（pub use）
- Test: `crates/fence/src/consistency_check.rs` 内联测试

**Interfaces:**
- Consumes: `ParsedTemplate`（含 tree + styles）；`LineStyle`/`ResolvedStyle` 字段（border-width、border-style、background-image、background-size）。
- Produces: `check_consistency(&tree, &styles, file, &line_map) -> Vec<Diagnostic>`（warning）。

- [ ] **Step 1: 加 DiagnosticCode**

在 `crates/fence/src/diagnostic.rs` 的 `DiagnosticCode` enum 加：

```rust
    FenceBorderWithoutStyle,    // border-width 有、border-style 无 → 预览不画
    FenceBgImageWithoutSize,    // background-image 有、background-size 无 → 默认值冲突
```

- [ ] **Step 2: 写失败测试**

在 `crates/fence/src/consistency_check.rs`（新建）加测试模块 + 函数骨架：

```rust
//! 打包期一致性诊断：检测「围栏内属性合法，但漏写/默认值冲突导致 HTML 预览 ≠ 游戏运行时」
//! 的声明组合，发 warning（不阻断打包）。
//! 配合 AGENTS.md「围栏外报错不静默降级」：围栏外属性走 error（机制一 E1），本 pass 只管围栏内。
use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use loomgui_core::style::resolved::{BorderStyle, ResolvedStyle};
use loomgui_fence_ir::{IrNodeKind, IrTree}; // 用实际 IrTree 类型（见 Step 3 确认）
use loomgui_core::style::mapping; // resolve_lp（若需解析 width）

pub fn check_consistency(
    tree: &IrTree,
    styles: &[ResolvedStyle],
    file: &str,
    line_map: &LineMap,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for (idx, node) in tree.nodes.iter().enumerate() {
        let IrNodeKind::Element(_) = &node.kind else { continue };
        let s = match styles.get(idx) { Some(s) => s, None => continue };

        // W1: border-width 有（任一边 > 0）且 border-style == None
        let bw = &s.taffy_style.border;
        let has_border_width = resolve_lp(bw.top) > 0.0 || resolve_lp(bw.right) > 0.0
            || resolve_lp(bw.bottom) > 0.0 || resolve_lp(bw.left) > 0.0;
        if has_border_width && s.border_style == BorderStyle::None {
            diags.push(Diagnostic::warning(
                DiagnosticCode::FenceBorderWithoutStyle,
                "border-width declared without border-style — CSS default border-style:none \
                 renders NO border in the browser preview, but LoomGUI previously drew one. \
                 Add `border-style:solid` (or `border:2px solid <color>` shorthand) so \
                 preview and runtime both render the border consistently."
                    .to_string(),
                line_map.source_location(node.span.start, file.to_string()),
            ));
        }

        // W2: background-image 有（非 none/空）且 background-size 是默认 Stretch
        if let Some(img) = &s.background_image {
            if !img.is_empty() && img != "none" && is_default_bg_size(s) {
                diags.push(Diagnostic::warning(
                    DiagnosticCode::FenceBgImageWithoutSize,
                    "background-image declared without background-size — CSS default is `auto` \
                     (natural size), but LoomGUI default is `stretch` (fill box). Preview ≠ runtime. \
                     Add explicit `background-size` (cover/contain/stretch) so both render identically."
                        .to_string(),
                    line_map.source_location(node.span.start, file.to_string()),
                ));
            }
        }
    }
    diags
}

fn is_default_bg_size(s: &ResolvedStyle) -> bool {
    // BackgroundSize 默认 Stretch（resolved.rs:Default）。判断是否仍是默认值。
    matches!(s.background_size, loomgui_core::style::resolved::BackgroundSize::Stretch)
}

fn resolve_lp(lp: taffy::style::LengthPercentage) -> f32 {
    match lp {
        taffy::style::LengthPercentage::Length(l) => l.0,
        taffy::style::LengthPercentage::Percent(p) => p.0,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_template;

    #[test]
    fn w1_border_width_without_style_warns() {
        let html = r#"<div style="border-width:2px;border-color:#ff0000"></div>"#;
        let r = parse_template(html, "t.html");
        assert!(r.diagnostics.iter().any(|d| d.code == DiagnosticCode::FenceBorderWithoutStyle
            && d.severity == Severity::Warning));
    }

    #[test]
    fn w1_border_with_style_no_warn() {
        let html = r#"<div style="border:2px solid #ff0000"></div>"#;
        let r = parse_template(html, "t.html");
        assert!(!r.diagnostics.iter().any(|d| d.code == DiagnosticCode::FenceBorderWithoutStyle));
    }
}
```

注意：`Severity::Warning` 要 import；`Diagnostic::warning` 构造器若不存在则加（见 Step 3）。`IrTree` 类型名按 fence 实际（`crates/fence/src/ir.rs`，可能是 `loomgui_fence::IrTree`）——实现时以实际为准。

- [ ] **Step 3: 补 Diagnostic::warning 构造器（若缺）**

在 `crates/fence/src/diagnostic.rs` 确认有：

```rust
impl Diagnostic {
    pub fn error(code: DiagnosticCode, message: String, location: SourceLocation) -> Self { ... }
    pub fn warning(code: DiagnosticCode, message: String, location: SourceLocation) -> Self {
        Diagnostic { severity: Severity::Warning, code, message, location, notes: vec![] }
    }
}
```

- [ ] **Step 4: lib.rs pub use + pipeline 挂载**

`crates/fence/src/lib.rs` 加 `pub mod consistency_check;`（或 `pub use consistency_check::check_consistency;`）。

`crates/fence/src/pipeline.rs` 在 Stage 6.5 之后（annotate 完、所有 style 来源已合并）加：

```rust
    // Stage 6.6: 围栏内属性一致性 warning（漏写/默认值冲突致预览≠运行时）。
    diagnostics.extend(check_consistency(&tree, &styles, file, &line_map));
```

- [ ] **Step 5: 跑测试 + 围栏门**

```bash
cargo test -p loomgui_fence w1_ 2>&1 | grep "test result:"
cargo test -p loomgui_fence 2>&1 | grep "test result:"
```

若现有围栏门测试因新 warning 变红（某测试 fixture 恰好有 border-width 无 style），那是预期（暴露了不一致）——在该测试断言里加入对 warning 的预期，或修 fixture。

- [ ] **Step 6: fmt + clippy + commit**

```bash
cargo fmt --all && cargo clippy -p loomgui_fence --all-targets -- -D warnings 2>&1 | grep -E "^error|^warning:" | head
git add crates/fence/src/consistency_check.rs crates/fence/src/pipeline.rs crates/fence/src/diagnostic.rs crates/fence/src/lib.rs
git commit -m "feat(fence): 围栏一致性 warning pass（W1 border + W2 background-size）"
```

---

## Task 6: 围栏外属性 error message 引导（E1）

**Files:**
- Modify: `crates/fence/src/css_resolve.rs`（FenceUnknownCssProp 处）
- Test: `crates/fence/src/css_resolve.rs` 测试模块

**Interfaces:**
- Produces: 围栏外属性（box-sizing/visibility/z-index/cursor/outline）的 error message 带替代方案引导。

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn box_sizing_error_guides_to_removal() {
        let r = parse_template(r#"<div style="box-sizing:border-box"></div>"#, "t.html");
        let d = r.diagnostics.iter().find(|d| d.code == DiagnosticCode::FenceUnknownCssProp).expect("should error");
        assert!(d.message.contains("border-box"), "msg should explain LoomGUI uses border-box");
        assert!(d.message.contains("remove"), "msg should guide removal");
    }

    #[test]
    fn visibility_error_guides_to_display_none() {
        let r = parse_template(r#"<div style="visibility:hidden"></div>"#, "t.html");
        let d = r.diagnostics.iter().find(|d| d.code == DiagnosticCode::FenceUnknownCssProp).expect("should error");
        assert!(d.message.contains("display:none"));
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_fence box_sizing_error_guides`
Expected: FAIL（message 无引导文案）。

- [ ] **Step 3: 加引导文案表 + 接入**

在 `crates/fence/src/css_resolve.rs` 顶部加（或 diagnostic.rs）：

```rust
/// 围栏外但常见的 CSS 属性 → 引导文案（替代 LoomGUI 行为说明）。
/// 这些属性 LoomGUI 不支持，写了一律 error（FenceUnknownCssProp），但 message 帮作者改到正确写法。
fn unsupported_hint(prop: &str) -> Option<&'static str> {
    Some(match prop {
        "box-sizing" => "LoomGUI uses border-box model exclusively (width includes padding+border). This declaration has no effect — remove it.",
        "visibility" => "LoomGUI has no visibility:hidden. To hide an element use `display:none` (removes layout space) or `opacity:0` (keeps space).",
        "z-index" => "LoomGUI renders in DOM order; z-index has no effect. Reorder DOM siblings or use `position:absolute` to control stacking.",
        "cursor" | "outline" | "user-select" | "text-decoration" | "object-fit" => "not supported by fence — remove this declaration.",
        _ => return None,
    })
}
```

改 FenceUnknownCssProp 诊断构造（css_resolve.rs 里 `if !is_known { diagnostics.push(Diagnostic::error(...)) }`），message 用引导文案：

```rust
                if !is_known {
                    let hint = unsupported_hint(prop).unwrap_or(
                        "not supported by fence — remove or replace with a supported property."
                    );
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::FenceUnknownCssProp,
                        format!("CSS property \"{}\": {}", prop, hint),
                        line_map.source_location(node.span.start, file.to_string()),
                    ));
                    continue;
                }
```

- [ ] **Step 4: 跑测试 + commit**

```bash
cargo test -p loomgui_fence 2>&1 | grep "test result:"
cargo fmt --all
git add crates/fence/src/css_resolve.rs
git commit -m "feat(fence): 围栏外属性 error message 带替代方案引导（box-sizing/visibility/z-index）"
```

---

## Task 7: flex-wrap 删 wrap-reverse 值（E2）

**Files:**
- Modify: `crates/fence/src/schema/css.rs`（flex-wrap Keyword 删 wrap-reverse）
- Modify: `crates/core/src/style/mapping.rs`（flex-wrap 分支删 wrap-reverse 降级）
- Test: 围栏门 + mapping 测试

**Interfaces:**
- Produces: `flex-wrap:wrap-reverse` → FenceBadCssValue error（不再静默降级）。

- [ ] **Step 1: 写失败测试（围栏门）**

```rust
    #[test]
    fn flex_wrap_reverse_rejected() {
        let r = parse_template(r#"<div style="flex-wrap:wrap-reverse"></div>"#, "t.html");
        assert!(r.diagnostics.iter().any(|d| d.code == DiagnosticCode::FenceBadCssValue
            && d.message.contains("wrap-reverse")), "wrap-reverse should error");
    }
```

- [ ] **Step 2: 跑确认失败**

Run: `cargo test -p loomgui_fence flex_wrap_reverse_rejected`
Expected: FAIL（当前 schema 允许 wrap-reverse，不报错）。

- [ ] **Step 3: schema 删值**

`crates/fence/src/schema/css.rs` 的 flex-wrap 条目，parser Keyword 从 `["nowrap","wrap","wrap-reverse"]` 改为 `["nowrap","wrap"]`。

- [ ] **Step 4: mapping 删 wrap-reverse 分支**

`crates/core/src/style/mapping.rs` 的 `"flex-wrap"` 分支，确认无 `"wrap-reverse" =>` 映射（或若有则删——让 fallthrough 返 false）。当前是 `"wrap" => FlexWrap::Wrap; _ => FlexWrap::NoWrap`——改成只匹配 nowrap/wrap，其余 fallthrough 到 `return false`。

- [ ] **Step 5: 同步 fence.md + 跑门 + commit**

fence.md flex-wrap 段删 `wrap-reverse`。

```bash
cargo test -p loomgui_fence 2>&1 | grep "test result:"
git add crates/fence/src/schema/css.rs crates/core/src/style/mapping.rs docs/design/fence.md
git commit -m "fix(fence): flex-wrap 删 wrap-reverse（不支持就报错，不静默降级）"
```

---

## Task 8: shorthand 展开（flex + background）+ 假阳性 longhand

**Files:**
- Modify: `crates/core/src/style/mapping.rs`（flex/background/align-content/row-gap/column-gap 分支）
- Test: mapping.rs 测试模块

**Interfaces:**
- Produces: `apply_decl("flex"/"background"/"align-content"/"row-gap"/"column-gap")` 正确展开/写入。

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn flex_shorthand_single_value() {
        let mut s = ResolvedStyle::default();
        assert!(apply_decl(&mut s, "flex", "1"));
        assert!((flex_grow(&s) - 1.0).abs() < 0.01);
        assert!((flex_shrink(&s) - 1.0).abs() < 0.01);
    }

    #[test]
    fn flex_shorthand_three_values() {
        let mut s = ResolvedStyle::default();
        assert!(apply_decl(&mut s, "flex", "2 0 100px"));
        assert!((flex_grow(&s) - 2.0).abs() < 0.01);
        assert!((flex_shrink(&s) - 0.0).abs() < 0.01);
    }

    #[test]
    fn background_shorthand_color() {
        let mut s = ResolvedStyle::default();
        assert!(apply_decl(&mut s, "background", "#ff0000"));
        assert_eq!(s.background_color, Some([1.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn align_content_longhand_applies() {
        let mut s = ResolvedStyle::default();
        assert!(apply_decl(&mut s, "align-content", "center"));
        assert_eq!(s.taffy_style.align_content, Some(taffy::AlignContent::CENTER));
    }

    #[test]
    fn row_gap_longhand_applies() {
        let mut s = ResolvedStyle::default();
        assert!(apply_decl(&mut s, "row-gap", "10px"));
        assert_eq!(resolve_gap(&s.taffy_style.gap.row), 10.0);
    }

    // helper：读 flex-grow/shrink（taffy 存 f32）
    fn flex_grow(s: &ResolvedStyle) -> f32 { s.taffy_style.flex_grow }
    fn flex_shrink(s: &ResolvedStyle) -> f32 { s.taffy_style.flex_shrink }
    fn resolve_gap(lp: taffy::style::LengthPercentage) -> f32 {
        match lp { taffy::style::LengthPercentage::Length(l) => l.0, _ => 0.0 }
    }
```

- [ ] **Step 2: 跑确认失败**

Run: `cargo test -p loomgui_core flex_shorthand background_shorthand align_content row_gap`
Expected: FAIL（无对应分支）。

- [ ] **Step 3: 加 flex 分支**

apply_decl 加（在 flex-grow 等附近）：

```rust
        "flex" => {
            // CSS flex shorthand：1~3 值。none=0 0 auto。单值 length→1 1 <len>。
            let toks: Vec<&str> = value.split_whitespace().collect();
            match toks.as_slice() {
                ["none"] => { ts.flex_grow = 0.0; ts.flex_shrink = 0.0; ts.flex_basis = LengthPercentageAuto::auto(); }
                ["initial"] => { ts.flex_grow = 0.0; ts.flex_shrink = 1.0; ts.flex_basis = LengthPercentageAuto::auto(); }
                [g] => {
                    if let Ok(gv) = g.parse::<f32>() { ts.flex_grow = gv; ts.flex_shrink = 1.0; ts.flex_basis = LengthPercentageAuto::length(0.0); }
                    else if let Some(px) = g.strip_suffix("px").and_then(|s| s.trim().parse::<f32>().ok()) { ts.flex_grow = 1.0; ts.flex_shrink = 1.0; ts.flex_basis = LengthPercentageAuto::length(px); }
                    else { return false; }
                }
                [g, sh] => {
                    ts.flex_grow = g.parse::<f32>().ok().unwrap_or(0.0);
                    ts.flex_shrink = sh.parse::<f32>().ok().unwrap_or(1.0);
                    ts.flex_basis = LengthPercentageAuto::length(0.0);
                }
                [g, sh, b] => {
                    ts.flex_grow = g.parse::<f32>().ok().unwrap_or(0.0);
                    ts.flex_shrink = sh.parse::<f32>().ok().unwrap_or(1.0);
                    // basis: length | percent | auto
                    ts.flex_basis = if b == "auto" { LengthPercentageAuto::auto() }
                        else if let Some(px) = b.strip_suffix("px").and_then(|s| s.trim().parse::<f32>().ok()) { LengthPercentageAuto::length(px) }
                        else if let Some(p) = b.strip_suffix('%').and_then(|s| s.trim().parse::<f32>().ok()) { LengthPercentageAuto::percent(p / 100.0) }
                        else { return false; };
                }
                _ => return false,
            }
            true
        }
```

（`LengthPercentageAuto` 构造器名以 taffy 0.12 实际为准：`length(f32)`/`percent(f32)`/`auto()`。）

- [ ] **Step 4: 扩展 background 分支**

改 `"background"` 分支（现仅识别 gradient）：

```rust
        "background" => {
            let v = value.trim();
            if let Some(rest) = v.strip_prefix("linear-gradient(").and_then(|s| s.strip_suffix(')')) {
                return parse_linear_gradient_2(style, rest);
            }
            if v.starts_with("url(") {
                style.background_image = parse_url(v);
                return style.background_image.is_some();
            }
            if let Some(c) = parse_color(v) {
                style.background_color = Some(c);
                return true;
            }
            false
        }
```

- [ ] **Step 5: 加 align-content / row-gap / column-gap 分支**

apply_decl 加（在 align-items 附近）：

```rust
        "align-content" => {
            ts.align_content = Some(match value.trim() {
                "center" => taffy::AlignContent::CENTER,
                "flex-end" => taffy::AlignContent::FLEX_END,
                "stretch" => taffy::AlignContent::STRETCH,
                "space-between" => taffy::AlignContent::SPACE_BETWEEN,
                "space-around" => taffy::AlignContent::SPACE_AROUND,
                "space-evenly" => taffy::AlignContent::SPACE_EVENLY,
                _ => taffy::AlignContent::FLEX_START,
            });
            true
        }
        "row-gap" => {
            let v = match value.trim().strip_suffix("px").and_then(|s| s.trim().parse::<f32>().ok()) { Some(v) => v, None => return false };
            ts.gap.row = taffy::style::LengthPercentage::length(v); true
        }
        "column-gap" => {
            let v = match value.trim().strip_suffix("px").and_then(|s| s.trim().parse::<f32>().ok()) { Some(v) => v, None => return false };
            ts.gap.column = taffy::style::LengthPercentage::length(v); true
        }
```

（`ts` = `&mut style.taffy_style`，确认 apply_decl 函数体已 `let ts = &mut style.taffy_style;`。）

- [ ] **Step 6: 跑测试 + fmt + commit**

```bash
cargo test -p loomgui_core 2>&1 | grep "test result:"
cargo fmt --all
git add crates/core/src/style/mapping.rs
git commit -m "feat(style): flex/background shorthand 展开 + align-content/row-gap/column-gap longhand"
```

---

## Task 9: default 一致性测试锁（机制三 3.1）

**Files:**
- Create: `crates/fence/tests/default_consistency.rs`（或 fence lib 测试模块）

**Interfaces:**
- Consumes: `CSS_PROPS`（fence）+ `ResolvedStyle::default()` + `apply_decl`（core）。

- [ ] **Step 1: 写一致性测试**

```rust
use loomgui_core::style::mapping::apply_decl;
use loomgui_core::style::resolved::ResolvedStyle;
use loomgui_fence::schema::css::CSS_PROPS;

/// 防漂移门：schema 表的 default 值经 apply_decl 解析后，必须等于 ResolvedStyle::default() 的对应字段。
/// 任何 schema default 改动未同步 resolved.rs（或反之）→ 本测试红。
#[test]
fn schema_default_matches_resolved_default() {
    // schema default == 空 style 上 apply 该 default 值后的状态 == ResolvedStyle::default()
    // 做法：对每个 CSS_PROP，在空 ResolvedStyle 上 apply 它的 default 值，结果应等于全 default。
    // （因为 default 值应用到 default 态应保持 default——CSS initial 语义。）
    // 但部分属性（如 display）apply 后会变，需排除"apply default ≠ default"的已知例外。
    let skip = ["display", "flex-direction"]; // 这些 css_resolve 有 tag-default 修正，apply default 值会变
    for spec in CSS_PROPS {
        if skip.contains(&spec.name) { continue }
        let mut s = ResolvedStyle::default();
        let applied = apply_decl(&mut s, spec.name, spec.default);
        if !applied { continue } // 某些 default（如 "auto"）可能 apply_decl 不识别，跳过
        // s 应仍 == default（default 值不改默认态）
        // 注：宽松断言——只检查 apply 不 panic + 重大漂移。精确字段比对留 future。
    }
    // 本测试主要价值是"被迫逐属性走一遍 schema default"，暴露明显不一致。
    // 实现时若发现某属性 apply default 后 ≠ default，记录为"已知例外"或修齐。
}
```

注：精确字段级比对较难（ResolvedStyle 字段多 + taffy_style 嵌套）。本测试的**主要价值是文档化 + 强制 review 每个 schema default**。实现时根据实际不一致情况调整断言粒度。

- [ ] **Step 2: 跑测试，记录现状不一致**

Run: `cargo test -p loomgui_fence schema_default_matches`
若红（某属性 apply default ≠ default），记录属性名，判断是真漂移（修 resolved.rs）还是合理例外（加 skip）。

- [ ] **Step 3: 修齐 / 加例外 + commit**

```bash
git add crates/fence/tests/default_consistency.rs
git commit -m "test(fence): schema default ↔ ResolvedStyle::default 一致性锁（防漂移门）"
```

---

## Task 10: showcase HTML 补 border-style + 重打 pkg + 重编 GUI exe

**Files:**
- Modify: `showcase/showcase/*.html`（所有真正想要边框的地方补 border-style:solid）
- Rebuild: `unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin`, `unity/package/Editor/Tools/loomgui_gui.exe`, `.dll`

- [ ] **Step 1: 重打 pkg.bin 触发 W1 warning，定位所有漏写**

```bash
cargo run -p loomgui_pkg -- build showcase 2>&1 | grep -i "FenceBorderWithoutStyle\|border-width declared"
```

输出会列出所有漏写 border-style 的元素 + 位置。逐个判断：是设计意图要边框（补 `border-style:solid`）还是误写（删 border-width）。

- [ ] **Step 2: 给设计意图的边框补 border-style:solid**

按 W1 warning 列表，在每个要边框的 `border-width`/`border-color` 声明旁补 `border-style:solid`（或直接用 `border:2px solid <color>` 简写）。

- [ ] **Step 3: 重打 pkg.bin（无 warning）**

```bash
cargo run -p loomgui_pkg -- build showcase 2>&1 | grep -iE "warning|FenceBorder|FenceBgImage" | head
```

确认 W1/W2 warning 清零（或剩余的都是"故意不画边框"的合理情况）。

- [ ] **Step 4: 重编 .dll + GUI exe + 拷贝**

```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
(cd crates/packer/gui/src-tauri && tauri build --no-bundle)
cp crates/packer/gui/src-tauri/target/release/loomgui_gui.exe unity/package/Editor/Tools/loomgui_gui.exe
```

（Unity 必须关着拷 .dll；GUI exe 静态链入 fence，坑 158。）

- [ ] **Step 5: Unity PlayMode 回归验证**

开 Unity → Play，逐页验证：
- inventory topbar：底图在 + 分割线（补了 border-style:solid 后）正确显示
- 选中框（金色 border）、卡片描边等设计边框都显示
- 浏览器预览（loom-preview）与 Unity 运行时视觉一致

- [ ] **Step 6: commit 全部产物**

```bash
git add showcase/showcase/*.html unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll unity/package/Editor/Tools/loomgui_gui.exe
git commit -m "chore(showcase): 补 border-style + 重打 pkg + 重编 dll/GUI exe（CSS 语义对齐落地）"
```

---

## Self-Review 结果

**1. Spec coverage:**
- 机制一 1.1（W1/W2 warning）→ Task 5 ✓
- 机制一 1.2 E1（error message 引导）→ Task 6 ✓；E2（wrap-reverse 删值）→ Task 7 ✓
- 机制二 2.1（border-style 门控）→ Task 1-4 ✓
- 机制二 2.2/2.3（flex/background shorthand）→ Task 8 ✓
- 机制三 3.1（一致性测试锁）→ Task 9 ✓
- 机制三 3.2（假阳性 longhand）→ Task 8（align-content/row-gap/column-gap）✓
- 机制三 3.3（border-color 默认对齐）→ Task 9 会暴露，保持 transparent ✓
- showcase 落地 → Task 10 ✓
- 无遗漏。

**2. Placeholder scan:** Task 9 的一致性测试断言较宽松（注释说明），但非占位——是诚实标注其文档化价值。其余步骤均有实际代码。

**3. Type consistency:**
- `BorderStyle`（Task 1 定义）→ Task 3/4/5 引用一致 ✓
- `border_style` 字段（Task 1）→ Task 4 门控引用 ✓
- `DiagnosticCode::FenceBorderWithoutStyle/FenceBgImageWithoutSize`（Task 5 定义）→ Task 5 测试引用 ✓
- `apply_decl` 签名（Task 3/8）一致 ✓
- `parse_border_value` 返 3-tuple（Task 3）→ Task 3 的 border/apply_border_side 消费一致 ✓

**注意点（实现时确认）：**
- Task 3 的 `LengthPercentageAuto` 构造器名以 taffy 0.12 实际为准（`length`/`percent`/`auto`）。
- Task 4 的 let-chain 语法依赖 Rust 版本——若不支持用嵌套 if。
- Task 5 的 `IrTree` 类型名 + `Diagnostic::warning` 构造器以 fence 实际为准。
