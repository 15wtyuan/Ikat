# Cascade Spike (阶段 S) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在旧 5 变体 `NodeKind` enum 上打通 `div/span` HTML → `<style>` 解析 → cascade → rect，用两个 headless 集成测试证明三个致命假设（选择器解析器、通用继承、编排接通）可走通。

**Architecture:** fence 新增 `<style>` 留存 + 选择器解析 + 规则表产物（路径 c：硬化手搓解析器，零新依赖，直产 core 的 `ParsedSelector`/`DynamicRule`）。core 把 color-only 继承 hack 换成通用继承 pass（cascade 期 transient set-ness bitmask，不碰 `ResolvedStyle` 序列化、不升 pkg 版本）。cascade 引擎 `rematch_pseudo_classes` 已是完整引擎（遍历全规则 + specificity 合并 + base_style 重起）——S1 只产规则表、S3 用 throwaway mini-bridge 接通 IrTree→Scene。

**Tech Stack:** Rust 2021；`html5gum 0.8`（fence 现有）；`taffy 0.5`（core 现有）；无新依赖（路径 c）。

## Global Constraints

- Rust edition 2021。依赖钉版本：`taffy 0.5`、`html5gum 0.8`（路径 c 不引 cssparser/scraper；若 S1.0 改路径 a/b，才按 CLAUDE.md 钉 `cssparser 0.34`/`scraper 0.19`）。
- **core 不新增运行时依赖**（保持引擎无关纯库）。fence 已依赖 core，可产 core 类型。
- **不改 `ResolvedStyle` 字段集**（会改 bincode 形状、连带升 pkg v17——已推迟 Spec-2）。S2 set-ness 用 cascade 期 transient 局部 map，不进 `ResolvedStyle`、不进 `Scene` 持久字段。
- CI 门禁严：`cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` 必过。clippy 各 crate root 有 `#![allow]` 白名单，可辩护的测试/FFI lint 在那里加，勿误清。
- 代码注释写上线品质：自包含、说 WHY、不引用坑号。注释可中文，标识符英文。
- 全程纯 Rust headless（`cargo test`），不碰 FFI/.dll/Unity。两台机串行：本机唯一编码机，验收靠本机测试。
- 遇编译错按 crate 实际源码调路径/签名，勿硬改依赖版本（CLAUDE.md API 适配方法论）。

---

## File Structure

- **CREATE** `crates/fence/src/css_rules.rs` — 选择器解析器（路径 c）+ `<style>` 文本 → `Vec<DynamicRule>` 解析。fence 新模块，产 core 的 `ParsedSelector`/`DynamicRule`。
- **CREATE** `crates/fence/tests/cascade_spike.rs` — S3 验收门：throwaway mini-bridge（IrTree→Scene）+ 两个集成测试（cascade 继承/class 命中；layout rect/display:none 剪枝）。
- **MODIFY** `crates/fence/src/tree_builder.rs` — 留存 `<style>` 文本（`in_style` + `style_texts`），`parse_html_to_ir_named` 多返一个 `Vec<String>`。
- **MODIFY** `crates/fence/src/pipeline.rs` — 接收 `style_texts`，加 CSS 规则解析阶段，`ParsedTemplate` 加 `dynamic_rules: Vec<DynamicRule>` 字段。
- **MODIFY** `crates/fence/src/lib.rs` — `pub mod css_rules;` + re-export。
- **MODIFY** `crates/core/src/style/dynamic.rs` — S2：set-ness bitmask + 通用继承 pass 替换 `propagate_color_inheritance`；修正文件头 stale 注释（:8-9）。
- **DELETE** `unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin` — 旧范式旧包（P0.2）。

---

## Task 1: 清理（删旧包 + 修 stale 注释）

**Files:**
- Delete: `unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin`
- Modify: `crates/core/src/style/dynamic.rs:8-9`

**Interfaces:** 无（独立清理）。

- [ ] **Step 1: 删旧 showcase 包**

```bash
git rm unity/showcase-unity/Assets/Bundles/ui/showcase.pkg.bin
```

预期：文件删除（旧范式时期打的、装旧 showcase 内容，当前 packer 已无法重打，个人项目不兼容）。

- [ ] **Step 2: 修 dynamic.rs 头部 stale 注释**

`crates/core/src/style/dynamic.rs:8-9` 现写"字符串 → 这些结构的解析器在 fence crate（loomgui_fence）"——这是愿景不是现实（解析器本任务之后才写）。改成现状：

把：
```rust
//! bincode 反序列化的 \.pkg.bin\ 就是这些结构，runtime 不再 parse，直接用反序列化结构。
//! 字符串 → 这些结构的解析器在 fence crate（\loomgui_fence\）。
```
改为：
```rust
//! bincode 反序列化的 \.pkg.bin\ 就是这些结构，runtime 不再 parse 选择器，直接用反序列化结构。
//! 字符串 → 这些结构的解析器在 fence crate（\loomgui_fence\）——由 spike（css_rules.rs）落地。
```

- [ ] **Step 3: 验证编译**

Run: `cargo build -p loomgui_core`
Expected: 编译过（仅注释改动）。

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: drop stale showcase.pkg.bin, fix stale dynamic.rs header comment"
```

---

## Task 2: fence 留存 `<style>` 文本

**Files:**
- Modify: `crates/fence/src/tree_builder.rs`
- Test: `crates/fence/src/tree_builder.rs`（`#[cfg(test)] mod tests`）

**Interfaces:**
- Produces: `parse_html_to_ir_named(html, file) -> (IrTree, Vec<Diagnostic>, Vec<String>)`（第 3 个返回值 = 各 `<style>` 块的文本，顺序为出现序）。下游 Task 4 消费。

- [ ] **Step 1: 写失败测试**

在 `crates/fence/src/tree_builder.rs` 的 `mod tests` 末尾加：

```rust
#[test]
fn style_text_is_captured_not_dropped() {
    let html = r#"<html><head><style>.foo { color: red }</style></head><body><div>hi</div></body></html>"#;
    let (tree, diags, style_texts) = parse_html_to_ir_named(html, "x.html".into());
    assert!(diags.is_empty(), "unexpected: {diags:?}");
    // <style> 元素本身不进树（shell 标签）
    assert!(
        !tree.nodes.iter().any(|n| matches!(&n.kind, crate::ir::IrNodeKind::Element(e) if e.tag == "style")),
        "<style> 不应进 IrTree"
    );
    // 但文本留下来了
    assert_eq!(style_texts, vec![".foo { color: red }".to_string()]);
}

#[test]
fn style_in_body_also_captured() {
    let (tree, _diags, style_texts) = parse_html_to_ir_named(
        r#"<div><style>.a { width: 10px }</style></div>"#,
        "x.html".into(),
    );
    let _ = tree;
    assert_eq!(style_texts, vec![".a { width: 10px }".to_string()]);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_fence style_text_is_captured 2>&1 | tail -5`
Expected: 编译失败（`parse_html_to_ir_named` 现返 2-tuple，测试解构 3-tuple）。

- [ ] **Step 3: 改 TreeBuilder 留存 `<style>`**

在 `struct TreeBuilder`（约 :147）加两个字段：

```rust
struct TreeBuilder {
    tree: IrTree,
    stack: Vec<IrNodeId>,
    diagnostics: Vec<Diagnostic>,
    line_map: LineMap,
    file: String,
    in_body: bool,
    in_head: bool,
    in_style: bool,          // NEW: 正在 <style> 内
    style_texts: Vec<String>, // NEW: 各 <style> 块文本
}
```

`TreeBuilder::new`（约 :158）的 `Self { .. }` 加 `in_style: false, style_texts: Vec::new(),`。

`process_token` 的 `IrToken::String` 分支（:191）改为：

```rust
IrToken::String { text, span } => {
    if self.in_style {
        self.style_texts.push(text);
    } else if !self.in_head && !text.is_empty() {
        self.tree.push_text(text, span, self.current_parent());
    }
}
```

`handle_start_tag`（:207）在最前面（`if name == "body"` 之前）加：

```rust
fn handle_start_tag(&mut self, name: String, attributes: Vec<IrAttribute>, self_closing: bool, span: Span) {
    if name == "style" {
        self.in_style = true;
        return; // 不建元素、不入栈；文本由 String 分支捕获
    }
    if name == "body" {
        // ...（原逻辑不动）
```

`handle_end_tag`（:245）在最前面加：

```rust
fn handle_end_tag(&mut self, name: String, span: Span) {
    if name == "style" {
        self.in_style = false;
        return;
    }
    if name == "body" {
        // ...（原逻辑不动）
```

`finish`（:295）返回三元组：

```rust
fn finish(mut self) -> (IrTree, Vec<Diagnostic>, Vec<String>) {
    for &id in self.stack.iter().rev() {
        // ...（原 unclosed 诊断逻辑不动）
    }
    (self.tree, self.diagnostics, self.style_texts)
}
```

`parse_html_to_ir_named`（:316）改为：

```rust
pub fn parse_html_to_ir_named(html: &str, file: String) -> (IrTree, Vec<Diagnostic>, Vec<String>) {
    let tokens = tokenize(html);
    let mut builder = TreeBuilder::new(html, file);
    for token in tokens {
        builder.process_token(token);
    }
    builder.finish()
}
```

`parse_html_to_ir`（:311，test helper）改为丢弃第 3 个返回值，保持旧 2-tuple 签名给现有测试：

```rust
pub fn parse_html_to_ir(html: &str) -> (IrTree, Vec<Diagnostic>) {
    let (tree, diags, _style_texts) = parse_html_to_ir_named(html, "<inline>".to_string());
    (tree, diags)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p loomgui_fence`
Expected: PASS（含两个新测试 + 既有 tree_builder 测试）。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(fence): retain <style> text via in_style capture in tree_builder"
```

---

## Task 3: 选择器解析器（路径 c：硬化手搓 + specificity）

> **S1.0 阻抗闸门（先做这步，~半天）**：本任务默认路径 (c)——零新依赖、直产 core 类型、`crates/fence/src/css_rules.rs` 写一个生产版选择器解析器（逻辑参考 core `dynamic.rs:505` 的测试 helper `hand_selector`，补 specificity）。开工前先验证路径 (c) 可行：core 的 `ParsedSelector`/`Compound`/`Combinator` 是普通 struct，fence 能直接构造（fence 已依赖 core）。若发现子集解析有硬阻挡（如必须支持转义、且手搓代价失控），停下改走 (a) scraper 0.19 或 (b) cssparser 0.34+selectors——届时调整本任务代码，但 Task 4-6 消费 `Vec<DynamicRule>` 的接口不变。

**Files:**
- Create: `crates/fence/src/css_rules.rs`
- Modify: `crates/fence/src/lib.rs`（`pub mod css_rules;`）

**Interfaces:**
- Produces: `crate::css_rules::parse_selector(raw: &str) -> Option<ParsedSelector>`——返回 core 的 `loomgui_core::style::dynamic::ParsedSelector`，含 specificity。子集：`class`/`tag`/`id`/后代组合/伪类（`:hover`/`:active`/`:disabled`/`:focus`/`:checked`）。越界返回 `None`（调用方报错）。

- [ ] **Step 1: 写失败测试**

创建 `crates/fence/src/css_rules.rs`，先只放测试模块（实现留空让测试失败）：

```rust
//! `<style>` 选择器解析 + 规则表产物（fence = 纯解析器）。
//!
//! 路径 c：手搓解析器，直产 core 的 ParsedSelector/Compound（fence 已依赖 core）。
//! 子集：class / tag / id / 后代组合（空格）/ 伪类（hover/active/disabled/focus/checked）。
//! 越界（属性选择器、nth-child、+ ~ 组合子、逗号多选等）返 None，由调用方报错。
use loomgui_core::style::dynamic::{Combinator, ParsedSelector};

/// 解析单条选择器串 → ParsedSelector（含 specificity）。越界返 None。
pub fn parse_selector(raw: &str) -> Option<ParsedSelector> {
    None // TODO Task 3 Step 3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(raw: &str) -> ParsedSelector {
        parse_selector(raw).unwrap_or_else(|| panic!("parse_selector({raw:?}) 返回 None"))
    }

    #[test]
    fn class_selector() {
        let s = spec(".foo");
        assert_eq!(s.compound.len(), 1);
        assert_eq!(s.compound[0].classes, vec!["foo".to_string()]);
        // specificity (id, class, tag) = (0,1,0)
        assert_eq!(s.specificity.0, 0);
        assert_eq!(s.specificity.1, 1);
        assert_eq!(s.specificity.2, 0);
    }

    #[test]
    fn tag_selector() {
        let s = spec("div");
        assert_eq!(s.compound[0].tag.as_deref(), Some("div"));
        assert_eq!(s.specificity, loomgui_core::style::dynamic::Specificity(0, 0, 1));
    }

    #[test]
    fn id_selector() {
        let s = spec("#bar");
        assert_eq!(s.compound[0].id.as_deref(), Some("bar"));
        assert_eq!(s.specificity.1, 0);
        assert_eq!(s.specificity.0, 1);
    }

    #[test]
    fn compound_class_tag_id() {
        // div.foo#bar → (id=1, class=1, tag=1)
        let s = spec("div.foo#bar");
        assert_eq!(s.compound[0].tag.as_deref(), Some("div"));
        assert_eq!(s.compound[0].classes, vec!["foo".to_string()]);
        assert_eq!(s.compound[0].id.as_deref(), Some("bar"));
        assert_eq!(s.specificity, loomgui_core::style::dynamic::Specificity(1, 1, 1));
    }

    #[test]
    fn descendant_combinator() {
        // .a .b → 两个 compound，后者 combinator = Descendant
        let s = spec(".a .b");
        assert_eq!(s.compound.len(), 2);
        assert_eq!(s.compound[1].combinator, Combinator::Descendant);
        assert_eq!(s.specificity.1, 2); // 两个 class
    }

    #[test]
    fn pseudo_class_sets_flag_and_specificity() {
        let s = spec(".btn:hover");
        assert!(s.compound[0].pseudo_hover);
        // 伪类算 class 级 specificity → (0, 2, 0)
        assert_eq!(s.specificity.1, 2);
    }

    #[test]
    fn out_of_subset_returns_none() {
        // 属性选择器、逗号、+ ~ 组合子都不在本子集
        assert!(parse_selector(r#"[type="text"]"#).is_none());
        assert!(parse_selector(".a, .b").is_none());
        assert!(parse_selector(".a > .b").is_none()); // Child 组合子本轮不做（仅后代空格）
        assert!(parse_selector(".a + .b").is_none());
        assert!(parse_selector(":nth-child(2)").is_none());
    }
}
```

> 注：`Specificity` 需从 core 导出。若 `loomgui_core::style::dynamic::Specificity` 未 pub，先用元组比较（测试里 `s.specificity.0/1/2`），具体性 struct 字段是 pub 的（`pub struct Specificity(pub u32, pub u32, pub u32)`，见 dynamic.rs:72）——元组字段访问 OK。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_fence css_rules 2>&1 | tail -5`
Expected: 测试失败（`parse_selector` 恒返 None）。

- [ ] **Step 3: 实现解析器**

把 `crates/fence/src/css_rules.rs` 顶部的 `parse_selector` 占位换成实现（并补 use）：

```rust
use loomgui_core::style::dynamic::{Combinator, Compound, ParsedSelector, Specificity};

/// 解析单条选择器串 → ParsedSelector（含 specificity）。越界返 None。
///
/// 子集：空格分隔的若干 compound（后代组合）；每个 compound = tag? + (class/id/pseudo)*。
/// 越界：属性选择器 `[...]`、Child `>`、相邻 `+`/`~`、逗号多选 → None。
pub fn parse_selector(raw: &str) -> Option<ParsedSelector> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // 越界字符快速判定（本子集不含这些）
    if raw.contains('[') || raw.contains(',') || raw.contains('>') || raw.contains('+')
        || raw.contains('~')
    {
        return None;
    }

    let mut specificity_a = 0u32; // id 数
    let mut specificity_b = 0u32; // class + 伪类 + 属性 数
    let mut specificity_c = 0u32; // tag 数
    let mut compounds: Vec<Compound> = Vec::new();

    for (idx, part) in raw.split_whitespace().enumerate() {
        if part.is_empty() {
            continue;
        }
        let (c, a, b, cc) = parse_compound(part)?;
        specificity_a += a;
        specificity_b += b;
        specificity_c += cc;
        // 第一个 compound 的 combinator 无意义（无前驱）；rematch 只用 comps[1..].combinator
        let combinator = if idx == 0 { Combinator::Descendant } else { Combinator::Descendant };
        let mut c = c;
        c.combinator = combinator;
        compounds.push(c);
    }

    if compounds.is_empty() {
        return None;
    }
    Some(ParsedSelector {
        raw: raw.to_string(),
        compound: compounds,
        specificity: Specificity(specificity_a, specificity_b, specificity_c),
    })
}

/// 解析单个 compound（无空格的一段）。返 (compound, a, b, c) specificity 贡献。
fn parse_compound(part: &str) -> Option<(Compound, u32, u32, u32)> {
    let mut c = Compound {
        tag: None,
        classes: Vec::new(),
        id: None,
        combinator: Combinator::Descendant,
        pseudo_hover: false,
        pseudo_active: false,
        pseudo_disabled: false,
        pseudo_focus: false,
        attrs: Vec::new(),
    };
    let mut a = 0u32;
    let mut b = 0u32;
    let mut cc = 0u32;
    let mut rest = part;
    let mut consumed_tag = false;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('.') {
            let (name, next) = take_ident(r);
            if name.is_empty() {
                return None;
            }
            c.classes.push(name.to_string());
            b += 1;
            rest = next;
        } else if let Some(r) = rest.strip_prefix('#') {
            let (name, next) = take_ident(r);
            if name.is_empty() {
                return None;
            }
            c.id = Some(name.to_string());
            a += 1;
            rest = next;
        } else if let Some(r) = rest.strip_prefix(':') {
            let (name, next) = take_ident(r);
            match name {
                "hover" => c.pseudo_hover = true,
                "active" => c.pseudo_active = true,
                "disabled" => c.pseudo_disabled = true,
                "focus" => c.pseudo_focus = true,
                "checked" => {
                    // checked 复用 disabled 槽？不——core Compound 无 pseudo_checked 字段。
                    // 本轮 :checked 映射到伪类 specificity（b+=1），但 core 无独立 checked 布尔。
                    // spike 子集声明含 :checked 但 core 无存储 → 记 specificity、不存状态门
                    // （checked 控件态由控件束处理，Spec-4）。这里只计 specificity。
                }
                _ => return None, // 未知伪类越界
            }
            b += 1; // 伪类算 class 级
            rest = next;
        } else {
            // tag（必须出现在 compound 最前）
            if consumed_tag {
                return None; // tag 后面跟了非 .#: 的字符 → 非法形态
            }
            let (name, next) = take_ident(rest);
            if name.is_empty() {
                return None;
            }
            c.tag = Some(name.to_string());
            cc += 1;
            consumed_tag = true;
            rest = next;
        }
    }
    Some((c, a, b, cc))
}

/// 取一个标识符（字母/数字/`-`/`_`），返回 (标识符, 剩余)。
fn take_ident(s: &str) -> (&str, &str) {
    let end = s
        .find(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
        .unwrap_or(s.len());
    (&s[..end], &s[end..])
}
```

> 说明：`:checked` 在 core 的 `Compound` 无独立布尔字段（控件态归控件束 Spec-4）。本子集解析器对 `:checked` 仅计 specificity、不存状态门——测试不覆盖 checked 行为，只保证不返 None。若实现期发现需要 checked 状态，记 ponytail 欠债推 Spec-4。

- [ ] **Step 4: 注册模块**

`crates/fence/src/lib.rs` 加（在现有 `pub mod` 列表里，字母序插 `css_resolve` 后）：

```rust
pub mod css_rules;
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p loomgui_fence css_rules`
Expected: 7 个测试全 PASS。

- [ ] **Step 6: clippy + fmt**

Run: `cargo clippy -p loomgui_fence --all-targets -- -D warnings` then `cargo fmt -p loomgui_fence -- --check`
Expected: 无警告、格式过。若 clippy 报 `combinator` 赋值冗余（idx 分支两支相同），删掉 `if idx == 0` 直接 `Combinator::Descendant`。

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(fence): hand-rolled selector parser (path c) with specificity"
```

---

## Task 4: `<style>` → 规则表 + 接进 pipeline

**Files:**
- Modify: `crates/fence/src/css_rules.rs`（加 `parse_style_block`）
- Modify: `crates/fence/src/pipeline.rs`（加规则解析阶段 + `ParsedTemplate.dynamic_rules`）

**Interfaces:**
- Produces: `crate::css_rules::parse_style_block(css: &str) -> (Vec<DynamicRule>, Vec<Diagnostic>)`——解析一段 `<style>` 文本为规则；不可解析的选择器/声明进 diagnostic。
- Produces: `ParsedTemplate { tree, styles, dynamic_rules: Vec<DynamicRule>, diagnostics, referenced_sprites }`。

- [ ] **Step 1: 写失败测试**

在 `crates/fence/src/css_rules.rs` 的 `mod tests` 加：

```rust
use loomgui_core::style::dynamic::{Declaration, DynamicRule};

#[test]
fn parse_style_block_basic() {
    let css = ".foo { color: red; font-size: 24px }\ndiv.bar { width: 100px }";
    let (rules, diags) = parse_style_block(css);
    assert!(diags.is_empty(), "diags: {diags:?}");
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0].selector.raw, ".foo");
    assert_eq!(rules[0].declarations.len(), 2);
    assert_eq!(rules[0].declarations[0], Declaration { prop: "color".into(), value: "red".into() });
    assert_eq!(rules[0].declarations[1].prop, "font-size");
    assert_eq!(rules[1].selector.raw, "div.bar");
    assert_eq!(rules[1].declarations[0].prop, "width");
}

#[test]
fn parse_style_block_skips_unparseable_selector() {
    // .a > .b 越界 → 该规则进 diagnostic，其他规则照常
    let (rules, diags) = parse_style_block(".a > .b { color: red }\n.ok { color: blue }");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].selector.raw, ".ok");
    assert!(diags.iter().any(|d| d.message.contains(".a > .b")), "越界选择器应报错: {diags:?}");
}

#[test]
fn parse_style_block_ignores_comments() {
    let (rules, _diags) = parse_style_block("/* c */ .x { color: red } /* tail */");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].selector.raw, ".x");
}
```

（`Diagnostic` 的字段名按 `crates/fence/src/diagnostic.rs` 实际——若 `message` 字段名不同，按实际改测试。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_fence parse_style_block 2>&1 | tail -5`
Expected: 编译失败（`parse_style_block` 未定义）。

- [ ] **Step 3: 实现 parse_style_block**

`crates/fence/src/css_rules.rs` 加（顶部 use 补 `DynamicRule`、`Diagnostic` 等）：

```rust
use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use loomgui_core::style::dynamic::{Declaration, DynamicRule};

/// 解析一段 `<style>` 文本 → 规则表 + 诊断。
///
/// 文法（子集）：`selector_list? { decl_list }` 重复；`selector_list` 为单选择器（不支持逗号）；
/// `decl_list` = `prop: value;` 重复。CSS 注释 `/* ... */` 剥除。越界选择器 → 该规则丢弃 + 诊断；
/// 声明 prop 名不在 schema（find_css_prop/find_shorthand）→ 诊断（与 css_resolve 一致）。
pub fn parse_style_block(css: &str) -> (Vec<DynamicRule>, Vec<Diagnostic>) {
    let stripped = strip_comments(css);
    let line_map = LineMap::new(&stripped); // 诊断定位用（粗略）
    let mut rules = Vec::new();
    let mut diagnostics = Vec::new();
    let mut pos = 0;
    let bytes = stripped.as_bytes();
    while pos < bytes.len() {
        // 找下一个 '{'
        let Some(brace_open) = stripped[pos..].find('{') else { break };
        let sel_start = pos;
        let sel_raw = stripped[pos..pos + brace_open].trim();
        let after_open = pos + brace_open + 1;
        let Some(brace_close_rel) = stripped[after_open..].find('}') else { break };
        let body = &stripped[after_open..after_open + brace_close_rel];
        pos = after_open + brace_close_rel + 1;

        if sel_raw.is_empty() {
            continue;
        }
        // <style> 内无精确 per-token span（strip_comments 后 offset 已偏）——定位用选择器起点近似。
        let loc = line_map.source_location(sel_start, "<style>".to_string());
        let Some(selector) = parse_selector(sel_raw) else {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::FenceBadCssValue,
                format!("unsupported selector \"{}\" in <style>", sel_raw),
                loc.clone(),
            ));
            continue;
        };
        let declarations = parse_declarations(body, &loc, &mut diagnostics);
        if !declarations.is_empty() {
            rules.push(DynamicRule { selector, declarations });
        }
    }
    (rules, diagnostics)
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let bytes = css.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // 跳到 */
            if let Some(rel) = css[i + 2..].find("*/") {
                i = i + 2 + rel + 2;
            } else {
                break; // 未闭合注释 → 丢到末尾
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// 解析声明块体 → Vec<Declaration>。prop 名校验同 css_resolve（find_css_prop/find_shorthand）。
/// `loc` = 本规则块的近似 SourceLocation（diagnostic 定位用）。
fn parse_declarations(
    body: &str,
    loc: &crate::diagnostic::SourceLocation,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Declaration> {
    use crate::schema::css::{find_css_prop, find_shorthand};
    let mut decls = Vec::new();
    for raw_decl in body.split(';') {
        let raw_decl = raw_decl.trim();
        if raw_decl.is_empty() {
            continue;
        }
        let Some((prop, value)) = raw_decl.split_once(':') else { continue };
        let prop = prop.trim();
        let value = value.trim();
        if prop.is_empty() || value.is_empty() {
            continue;
        }
        if find_css_prop(prop).is_none() && find_shorthand(prop).is_none() {
            diagnostics.push(Diagnostic::error(
                DiagnosticCode::FenceUnknownCssProp,
                format!("CSS property \"{}\" is not in the fence", prop),
                loc.clone(),
            ));
            continue;
        }
        decls.push(Declaration { prop: prop.to_string(), value: value.to_string() });
    }
    decls
}
```

> 诊断字段/构造按 `crates/fence/src/diagnostic.rs` 实际签名调（`Diagnostic::error(code, message, location)`、`SourceLocation::default()`、`DiagnosticCode::FenceUnknownCssProp`/`FenceBadCssValue`——这些在 css_resolve.rs 已用，照搬）。若 `Diagnostic::error` 签名不同，按实际改。

- [ ] **Step 4: 跑 css_rules 测试确认通过**

Run: `cargo test -p loomgui_fence css_rules`
Expected: 全 PASS（含新 3 个 parse_style_block 测试）。

- [ ] **Step 5: 接进 ParsedTemplate + pipeline**

`crates/fence/src/pipeline.rs`：
- 顶部 use 加 `use crate::css_rules::parse_style_block;` 和 `use loomgui_core::style::dynamic::DynamicRule;`
- `ParsedTemplate`（:12）加字段：

```rust
pub struct ParsedTemplate {
    pub tree: IrTree,
    pub styles: Vec<ResolvedStyle>,
    pub dynamic_rules: Vec<DynamicRule>,
    pub diagnostics: Vec<Diagnostic>,
    pub referenced_sprites: Vec<String>,
}
```

- `parse_template`（:23）：Stage 1+2 改解构三元组，加规则解析阶段，构造补 `dynamic_rules`：

把 `let (mut tree, mut diagnostics) = parse_html_to_ir_named(html, file.to_string());` 改为：

```rust
let (mut tree, mut diagnostics, style_texts) = parse_html_to_ir_named(html, file.to_string());
```

在 Stage 6 annotate 之后、`extract_sprites` 之前加：

```rust
// Stage 4.5: <style> → 动态规则表（CSS cascade 规则，运行时 rematch 消费）
let mut dynamic_rules = Vec::new();
for css in &style_texts {
    let (rules, css_diags) = parse_style_block(css);
    dynamic_rules.extend(rules);
    diagnostics.extend(css_diags);
}
```

构造 `ParsedTemplate { tree, styles, dynamic_rules, diagnostics, referenced_sprites }`（补 `dynamic_rules`）。

- [ ] **Step 6: 跑 fence 全测确认通过**

Run: `cargo test -p loomgui_fence`
Expected: 全 PASS（pipeline 测试不受影响——它们不读 dynamic_rules）。

- [ ] **Step 7: clippy + fmt + commit**

Run: `cargo clippy -p loomgui_fence --all-targets -- -D warnings && cargo fmt -p loomgui_fence -- --check`
Expected: 过。

```bash
git add -A
git commit -m "feat(fence): parse <style> into dynamic rule table, wire into pipeline"
```

---

## Task 5: S2 通用继承 pass（推翻 color-only hack）

**Files:**
- Modify: `crates/core/src/style/dynamic.rs`

**Interfaces:**
- Consumes: `rematch_pseudo_classes` 现有 cascade（每节点 base_style 重起 + 规则合并）。
- Produces: `rematch_pseudo_classes` 末尾改调通用 `propagate_inherited(scene, &set_map)`，替代 `propagate_color_inheritance`。set-ness 为 cascade 期局部 `HashMap<NodeId, InheritedSet>`，不进 `ResolvedStyle`、不进 `Scene` 持久字段。

**Inherited 字段集（core 侧硬编码，本 spike）**：`font_size`、`color`、`font_family`、`font_weight`、`text_align`、`line_height`、`letter_spacing`、`white_space_nowrap`。（`text_effects`/`-webkit-text-stroke` 等复合字段本轮不进继承 pass，记 ponytail 推 Spec-3——它们是 Vec，propagate 语义需定，超 spike 范围。）

- [ ] **Step 1: 写失败测试**

在 `crates/core/src/style/dynamic.rs` 的 `mod tests`（:466）加。用现有 `rule()` helper（:582，手搓 ParsedSelector）：

```rust
#[test]
fn child_inherits_parent_font_size() {
    // root(.par font-size:24) > Text child。child 无 font-size 规则 → 该继承 24。
    // 证明通用继承（非 color-only）。
    let mut root = Node::default();
    root.classes = vec!["par".to_string()];
    root.layout_rect = Rect { x: 0.0, y: 0.0, w: 200.0, h: 200.0 };
    let mut child = Node::default();
    child.kind = NodeKind::Text { content: "hi".into() };
    child.layout_rect = Rect { x: 0.0, y: 0.0, w: 50.0, h: 20.0 };
    let mut s = Scene::from_nodes(vec![root, child], vec![(0, 1)]);
    s.dynamic_rules.rules.push(rule(".par", "font-size", "24px"));
    rematch_pseudo_classes(&mut s);
    let cid = s.get(s.roots[0]).unwrap().children[0];
    assert_eq!(
        s.get(cid).unwrap().style.font_size,
        24.0,
        "child Text 该继承 parent .par 的 font-size:24"
    );
}

#[test]
fn child_explicit_font_size_not_overridden_by_inheritance() {
    // child 自己声明 font-size:12 → 不被父的 24 覆盖（set-ness 阻断继承）。
    let mut root = Node::default();
    root.classes = vec!["par".to_string()];
    let mut child = Node::default();
    child.classes = vec!["c".to_string()];
    child.kind = NodeKind::Text { content: "hi".into() };
    let mut s = Scene::from_nodes(vec![root, child], vec![(0, 1)]);
    s.dynamic_rules.rules.push(rule(".par", "font-size", "24px"));
    s.dynamic_rules.rules.push(rule(".c", "font-size", "12px"));
    rematch_pseudo_classes(&mut s);
    let cid = s.get(s.roots[0]).unwrap().children[0];
    assert_eq!(s.get(cid).unwrap().style.font_size, 12.0, "child 显式声明 12 不被继承覆盖");
}

#[test]
fn inheritance_cascades_two_levels() {
    // root(.a font-size:20) > mid > leaf(Text)。mid/leaf 都不声明 → leaf 继承 20（跨两级）。
    let mut root = Node::default();
    root.classes = vec!["a".to_string()];
    let mut mid = Node::default();
    let mut leaf = Node::default();
    leaf.kind = NodeKind::Text { content: "x".into() };
    let mut s = Scene::from_nodes(vec![root, mid, leaf], vec![(0, 1), (1, 2)]);
    s.dynamic_rules.rules.push(rule(".a", "font-size", "20px"));
    rematch_pseudo_classes(&mut s);
    let mid_id = s.get(s.roots[0]).unwrap().children[0];
    let leaf_id = s.get(mid_id).unwrap().children[0];
    assert_eq!(s.get(leaf_id).unwrap().style.font_size, 20.0, "leaf 跨级继承 20");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core child_inherits_parent_font_size 2>&1 | tail -8`
Expected: FAIL（现 `propagate_color_inheritance` 只传 color，child.font_size 仍 = base default 16.0，断言 24 失败）。

- [ ] **Step 3: 加 InheritedSet + set-ness 收集**

`crates/core/src/style/dynamic.rs`，在 `use` 区（:76 附近）后加：

```rust
use std::collections::HashMap;

/// cascade 期 transient：节点显式声明了哪些可继承属性（bitmask）。
/// 不进 ResolvedStyle（避免改 bincode/pkg 格式），不进 Scene 持久字段。
#[derive(Default, Clone, Copy)]
struct InheritedSet(u16);

const INH_FONT_SIZE: u16 = 1 << 0;
const INH_COLOR: u16 = 1 << 1;
const INH_FONT_FAMILY: u16 = 1 << 2;
const INH_FONT_WEIGHT: u16 = 1 << 3;
const INH_TEXT_ALIGN: u16 = 1 << 4;
const INH_LINE_HEIGHT: u16 = 1 << 5;
const INH_LETTER_SPACING: u16 = 1 << 6;
const INH_WHITE_SPACE_NOWRAP: u16 = 1 << 7;

/// prop 名 → 可继承属性 bit（非可继承返 None）。core 侧硬编码（fence schema 的
/// inherited 标志是 build-time 校验用的另一份；本 spike core 用此局部表，Spec-3 再统一）。
fn inherited_bit(prop: &str) -> Option<u16> {
    match prop.trim() {
        "font-size" => Some(INH_FONT_SIZE),
        "color" => Some(INH_COLOR),
        "font-family" => Some(INH_FONT_FAMILY),
        "font-weight" => Some(INH_FONT_WEIGHT),
        "text-align" => Some(INH_TEXT_ALIGN),
        "line-height" => Some(INH_LINE_HEIGHT),
        "letter-spacing" => Some(INH_LETTER_SPACING),
        "white-space" => Some(INH_WHITE_SPACE_NOWRAP),
        _ => None,
    }
}
```

- [ ] **Step 4: 在 cascade 循环里收集 set-ness**

`rematch_pseudo_classes`（:301）。当前循环对每节点：算 old_style/cascaded_once/transition_decl、`new_style = base_style.clone()`、收集 matched、sort、apply_decl 写 new_style、transition 检测、写 node.style。

在 `let mut matched` 之前声明局部 map（函数顶部）：在 `let node_ids: Vec<NodeId> = ...;`（:317）后加：

```rust
let mut set_map: HashMap<NodeId, InheritedSet> = HashMap::new();
```

在 apply declarations 的内层循环（:340-344）里，apply 成功后记 set-ness。把：

```rust
for (_, _, _, r) in &matched {
    for decl in &r.declarations {
        apply_decl(&mut new_style, &decl.prop, &decl.value);
    }
}
```

改为：

```rust
let mut inh: InheritedSet = InheritedSet::default();
for (_, _, _, r) in &matched {
    for decl in &r.declarations {
        if apply_decl(&mut new_style, &decl.prop, &decl.value) {
            if let Some(bit) = inherited_bit(&decl.prop) {
                inh.0 |= bit;
            }
        }
    }
}
set_map.insert(node_id, inh);
```

- [ ] **Step 5: 替换 propagate_color_inheritance 为通用 propagate_inherited**

把 `rematch_pseudo_classes` 末尾（:357-360）的：

```rust
// runtime color 继承：...
propagate_color_inheritance(scene);
```

改为：

```rust
// 通用可继承属性传播：每节点从 base_style 独立 cascade（不读父），故继承须 rematch 后
// 按 tree order 补一次：子未显式声明（set_map 无该 bit）→ 取父 effective 值。
propagate_inherited(scene, &set_map);
```

删掉旧 `propagate_color_inheritance`（:362-400）和 `propagate_color_rec`（:377-400），换成：

```rust
/// 通用可继承属性传播（tree-order DFS）。子未显式声明的可继承字段 → 取父 effective 值。
/// `effective` = 节点当前 style 值（anim override 本轮仅 color 用过，font 等无 anim）。
fn propagate_inherited(scene: &mut Scene, set_map: &HashMap<NodeId, InheritedSet>) {
    let roots = scene.roots.clone();
    for root in roots {
        propagate_inherited_rec(scene, root, None, set_map);
    }
}

fn propagate_inherited_rec(
    scene: &mut Scene,
    id: NodeId,
    parent_style: Option<ResolvedStyle>,
    set_map: &HashMap<NodeId, InheritedSet>,
) {
    let (my_style, my_base, children) = {
        let n = scene.get(id).expect("live node");
        (n.style.clone(), n.base_style.clone(), n.children.clone())
    };
    // 父 effective = 父传下来的 style 快照（已含父自己的继承结果，因 tree order）
    if let Some(parent_eff) = parent_style {
        let inh = set_map.get(&id).copied().unwrap_or_default();
        let mut new_style = my_style.clone();
        macro_rules! copy_if_unset {
            ($field:ident, $bit:expr) => {
                if (inh.0 & $bit) == 0 {
                    new_style.$field = parent_eff.$field;
                }
            };
        }
        copy_if_unset!(font_size, INH_FONT_SIZE);
        copy_if_unset!(color, INH_COLOR);
        copy_if_unset!(font_family, INH_FONT_FAMILY);
        copy_if_unset!(font_weight, INH_FONT_WEIGHT);
        copy_if_unset!(text_align, INH_TEXT_ALIGN);
        copy_if_unset!(line_height, INH_LINE_HEIGHT);
        copy_if_unset!(letter_spacing, INH_LETTER_SPACING);
        copy_if_unset!(white_space_nowrap, INH_WHITE_SPACE_NOWRAP);
        scene.get_mut(id).expect("live node").style = new_style;
        // 向下传我更新后的 style 作为子 effective
        for c in children {
            propagate_inherited_rec(scene, c, Some(new_style.clone()), set_map);
        }
    } else {
        // 根节点：无父继承，effective = 自己 style，直接向下传
        for c in children {
            propagate_inherited_rec(scene, c, Some(my_style.clone()), set_map);
        }
    }
    let _ = my_base; // base_style 本 pass 不读（rematch 已用）
}
```

> `propagate_inherited_rec` 每层 clone style——spike 可接受（节点少）；热循环优化推后续。标 `ponytail: per-clone, 节点多时换就地改 + 父快照`。

- [ ] **Step 6: 跑新测试确认通过**

Run: `cargo test -p loomgui_core child_ 2>&1 | tail -8 && cargo test -p loomgui_core inheritance_cascades 2>&1 | tail -5`
Expected: 3 个新测试 PASS。

- [ ] **Step 7: 跑 dynamic.rs 既有测试，确认 color 继承仍工作**

现 `child_text_inherits_parent_runtime_color`（:610）测的是 color 继承——通用 pass 也要让 color 继承工作。Run: `cargo test -p loomgui_core`
Expected: 全 PASS。**若 color 继承测试红**：检查 `propagate_color_inheritance` 的 anim-text override 逻辑是否被丢了——旧代码对 color 有 `scene.anim.get(id).text_color` 优先。本轮 color 继承走通用 pass（无 anim override），若该测试依赖 anim override，把它标 `#[ignore]` 并记 ponytail（anim-text-color override 推 Spec-3），或在本 pass 里为 color 单独补 anim 读取。优先：让基础 color 继承过，anim override 单独跟进。

- [ ] **Step 8: clippy + fmt + commit**

Run: `cargo clippy -p loomgui_core --all-targets -- -D warnings && cargo fmt -p loomgui_core -- --check`
Expected: 过（`macro_rules! copy_if_unset` 若 clippy 报 `clone_on_copy`/`too_many_lines`，按 crate root `#![allow]` 惯例加白名单带理由注释）。

```bash
git add -A
git commit -m "feat(core): general inherited-property propagation, drop color-only hack"
```

---

## Task 6: S3 throwaway mini-bridge + 验收集成测试

**Files:**
- Create: `crates/fence/tests/cascade_spike.rs`

**Interfaces:**
- Consumes: `loomgui_fence::parse_template`（产 `ParsedTemplate.dynamic_rules`）、`loomgui_core::style::dynamic::{rematch_pseudo_classes, DynamicRule}`、`loomgui_core::scene::node::{Scene, NodeKind}`、`loomgui_core::layout::{solve, ImageSizeTable}`、`loomgui_core::style::resolved::ResolvedStyle`、`loomgui_core::text::layout::FontTable`。

> mini-bridge = 测试内部 throwaway（标 `ponytail:`）：只映射 div→Container、span/p/h*/strong/em/label/a/button 的元素 + 折叠文本为 Text{content}，抽 class/id。生产桥（IrTree→新 NodeKind，SemanticKind 24 total 映射）是 Spec-3 ②。

- [ ] **Step 1: 写集成测试文件（含 mini-bridge + 两个测试）**

创建 `crates/fence/tests/cascade_spike.rs`：

```rust
//! Spec-1 (阶段 S spike) 验收门：div/span HTML → <style> cascade → rect/语义。
//!
//! throwaway mini-bridge（标 ponytail）：fence ParsedTemplate.tree(IrTree) → core Scene。
//! 生产 IrTree→新 NodeKind 桥是 Spec-3 ②，本测试用最小映射（div→Container、文本→Text）。
use loomgui_core::layout::{solve, ImageSizeTable};
use loomgui_core::scene::node::{NodeKind, Scene};
use loomgui_core::style::dynamic::rematch_pseudo_classes;
use loomgui_core::style::resolved::ResolvedStyle;
use loomgui_core::text::layout::FontTable;
use loomgui_fence::{IrNodeKind, parse_template};
use std::collections::HashMap;

/// ponytail: throwaway mini-bridge for spike; replaced by production bridge (Spec-3 ②) on new enum.
/// 把 fence ParsedTemplate 的 IrTree 折叠成 core Scene：
/// - div/main/section 等 block → Container；span/p/h*/strong/em/label/a/button → 看语义
/// - 纯文本 IrNode 折叠进最近 Text 元素的 content
/// 只映射 div→Container、span→Text{content}（测试 HTML 限定）。
fn bridge(html: &str) -> Scene {
    let parsed = parse_template(html, "spike.html");
    assert!(
        parsed.diagnostics.is_empty(),
        "fence diagnostics: {:?}",
        parsed.diagnostics
    );

    let tree = &parsed.tree;
    // 给每个 Element IrNode 分配一个 Scene 节点 index（DFS 前序，跳过 Text IrNode）。
    // ponytail: 简化——假设测试 HTML 元素都是 div/span，文本是叶子。
    let mut entries: Vec<(
        Option<usize>,
        NodeKind,
        ResolvedStyle,
        Vec<String>,
        Option<String>,
        bool,
        Option<i32>,
        Option<String>,
    )> = Vec::new();
    // ir_index → scene_index 映射
    let mut ir_to_scene: std::collections::HashMap<usize, usize> = HashMap::new();

    // DFS 前序遍历 IrTree 元素
    let mut stack: Vec<(usize, Option<usize>)> = tree.roots.iter().map(|r| (r.0, None)).collect();
    while let Some((ir_idx, parent_scene_idx)) = stack.pop() {
        let node = &tree.nodes[ir_idx];
        let IrNodeKind::Element(el) = &node.kind else { continue };
        let scene_idx = entries.len();
        ir_to_scene.insert(ir_idx, scene_idx);

        // 收集本元素直接文本子节点为 content
        let mut content = String::new();
        let mut child_elems: Vec<usize> = Vec::new();
        for &child_id in &node.children {
            match &tree.nodes[child_id.0].kind {
                IrNodeKind::Text(t) => content.push_str(t),
                IrNodeKind::Element(_) => child_elems.push(child_id.0),
            }
        }

        let classes: Vec<String> = el
            .attributes
            .iter()
            .find(|a| a.name == "class")
            .map(|a| a.value.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default();
        let id_attr = el
            .attributes
            .iter()
            .find(|a| a.name == "id")
            .map(|a| a.value.clone());

        let kind = match el.tag.as_str() {
            "div" | "main" | "section" | "header" | "footer" | "nav" | "article" | "aside" => {
                NodeKind::Container
            }
            // span/p/h*/strong/em/label/a → Text 叶子（折叠文本）
            _ => NodeKind::Text { content },
        };
        entries.push((
            parent_scene_idx,
            kind,
            ResolvedStyle::default(), // base_style = UA default（spike 无打包期 bake）
            classes,
            id_attr,
            false,
            None,
            None,
        ));
        // 子元素入栈（逆序保前序）
        for &ce in child_elems.iter().rev() {
            stack.push((ce, Some(scene_idx)));
        }
    }

    let mut scene = Scene::build(&entries);
    // 规则表喂给现成 cascade 引擎
    scene.dynamic_rules.rules.extend(parsed.dynamic_rules);
    scene
}

#[test]
fn cascade_class_hit_and_font_size_inheritance() {
    // 断言 3（class 命中）+ 断言 2（font-size 继承）。只跑 cascade，不跑 solve（无字体依赖）。
    let html = r#"<style>.par { font-size: 24px } .hit { color: #ff0000 }</style>
        <div class="par"><span class="hit">Hello</span></div>"#;
    let mut scene = bridge(html);

    rematch_pseudo_classes(&mut scene);

    let root = scene.roots[0];
    let span_id = scene.get(root).unwrap().children[0];
    let span = scene.get(span_id).unwrap();
    // 断言 3：.hit class 命中 → color 红
    assert_eq!(span.style.color, [1.0, 0.0, 0.0, 1.0], ".hit class 命中应设 color 红");
    // 断言 2：span 无 font-size 规则 → 继承 .par 的 24px
    assert_eq!(span.style.font_size, 24.0, "span 该继承 parent .par 的 font-size:24");
}

#[test]
fn layout_rect_and_display_none_pruning() {
    // 断言 1（rect）+ 断言 4（display:none 剪枝）。只用 Container（无 Text），solve 不触发 measure。
    // root flex column 200x200 > [.hidden(display:none w:100 h:50), .vis(w:100 h:50)]
    // .hidden 被剪枝 → .vis 落在 y=0（若没剪枝会落在 y=50）。
    let html = r#"<style>
        .hidden { display: none; width: 100px; height: 50px }
        .vis { width: 100px; height: 50px }
        .root { width: 200px; height: 200px }
    </style>
    <div class="root"><div class="hidden"></div><div class="vis"></div></div>"#;
    let mut scene = bridge(html);

    rematch_pseudo_classes(&mut scene);
    // Container-only 树，无 Text → measure 不触发 → 空字体表即可
    let fonts = FontTable::new();
    let sizes: ImageSizeTable = HashMap::new();
    solve(&mut scene, &fonts, (200.0, 200.0), &sizes);

    let root = scene.roots[0];
    let children = &scene.get(root).unwrap().children;
    // 子节点文档序：.hidden(idx0)、.vis(idx1)
    let vis_id = children[1];
    let vis = scene.get(vis_id).unwrap().layout_rect;
    // 断言 1：.vis 尺寸正确
    assert!((vis.w - 100.0).abs() < 0.5, ".vis width 应 100，实际 {}", vis.w);
    assert!((vis.h - 50.0).abs() < 0.5, ".vis height 应 50，实际 {}", vis.h);
    // 断言 4：.hidden 被 display:none 剪枝 → .vis 落在 y=0（而非 y=50）
    assert!(
        vis.y.abs() < 0.5,
        ".hidden 剪枝后 .vis 应在 y=0，实际 y={}（display:none 未剪枝？）",
        vis.y
    );
}
```

- [ ] **Step 2: 跑集成测试确认状态**

Run: `cargo test -p loomgui_fence --test cascade_spike 2>&1 | tail -20`
Expected: 两个测试 PASS。**若 FAIL**：逐断言排查——
- `.hit color` 红 failed → 查规则表是否进 `dynamic_rules`（Task 4）、cascade 是否跑（Task 5）。
- `font-size 24` failed → 查继承 pass（Task 5）。
- `.vis w/h` failed → 查 solve/taffy：root 是 flex column，子 .vis width/height 是否经 cascade 写进 taffy_style.size（apply_decl "width"/"height"）。
- `.vis y` 非 0 → display:none 未剪枝：查 `.hidden` 是否经 cascade 设 `display_mode=None`（apply_decl "display" "none"）、taffy 是否跳过 Display::None。
- 编译错（`FontTable`/`ImageSizeTable`/`solve` 路径）→ 按 core 实际 mod 路径调（`loomgui_core::text::layout::FontTable`、`loomgui_core::layout::{solve, ImageSizeTable}`——若路径不同按 `cargo build` 报错改）。

- [ ] **Step 3: clippy + fmt**

Run: `cargo clippy -p loomgui_fence --all-targets -- -D warnings && cargo fmt -p loomgui_fence -- --check`
Expected: 过。

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "test(fence): S3 spike acceptance — cascade inheritance/class hit + layout/display:none"
```

---

## Task 7: 全门禁 + 验收清单核对

**Files:** 无（验证）。

- [ ] **Step 1: 全 workspace 门禁**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test -p loomgui_core
cargo test -p loomgui_fence
cargo test -p loomgui_fence --test cascade_spike
```
Expected: 全过。**feature-gate 门**也跑（CI 跑）：`cargo clippy --workspace --no-default-features --all-targets -- -D warnings`（若 workspace 有 feature 配置；按 `.github/workflows/rust-ci.yml` 实际）。

- [ ] **Step 2: 核对 spec §9 验收清单**

逐条核对（在 PR 描述或 commit 里勾）：
- [x] S1.0 阻抗 spike：路径 (c) 选定（手搓直产 core 类型），类型归属 = fence 产 core `DynamicRule`（fence 已依赖 core），无适配层。回填 spec §4。
- [x] 改 fence 留存 `<style>` 文本（Task 2）。
- [x] `<style>` → rule table，支持 class/tag/id/后代/伪类 + specificity（Task 3-4）。
- [x] S2 set-ness + 通用继承，删 color-only hack（Task 5）。
- [x] base_style = UA 默认；cascade 每帧基线正确（Task 6 测试隐式验证）。
- [x] S3 集成测试绿（§3 断言：rect / font-size 继承 / class 命中 / display:none 剪枝，Task 6）。
- [x] 删旧 `showcase.pkg.bin`（Task 1）。
- [x] 修 `dynamic.rs:8-9` stale 注释（Task 1）。
- [x] fmt + clippy 严门过（Step 1）。
- [x] core + fence 全测绿（Step 1）。

- [ ] **Step 3: 回填 spec §4（类型归属决定）**

`docs/superpowers/specs/2026-07-15-cascade-spike-design.md` §4 末尾"类型归属由 S1.0 定"——补一句定论：**S1.0 选定路径 (c)，selector/rule 类型留在 core（`style/dynamic.rs`），fence 产 core 类型（fence 已依赖 core，无适配层、无共享 crate）**。

- [ ] **Step 4: Commit spec 回填**

```bash
git add docs/superpowers/specs/2026-07-15-cascade-spike-design.md
git commit -m "docs: backfill S1.0 path-(c) type-ownership decision into spike spec"
```

---

## Notes for the implementer

- **顺序依赖**：Task 1 独立可先做。Task 2→3→4（fence 解析链）顺序。Task 5（core 继承）独立于 fence，可与 Task 2-4 并行。Task 6 依赖 Task 2-5 全部完成。
- **若 S1.0（Task 3 Step 0）改路径**：Task 3 换实现（scraper 或 cssparser+selectors），但 `parse_selector(raw) -> Option<ParsedSelector>` 签名不变；Task 4-6 消费 `Vec<DynamicRule>` 不受影响。
- **两台机**：本计划全程 `cargo test`（本机），不重编 .dll、不进 Unity。Spec-4 才需搬家里机。
- **`color` anim override**：Task 5 Step 7 若 `child_text_inherits_parent_runtime_color` 因丢 anim-text override 红，标 `#[ignore]` + ponytail，不阻塞 spike（anim-text-color 联动归 Spec-3）。
