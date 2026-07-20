# showcase 整体打包解锁 + Playwright 布局回归设施 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 showcase 8 页整体打包跑通（标准 B：打包过 + Unity 实例化 + 布局 rect 对齐浏览器，视觉占位）+ 建立 Playwright 布局 rect diff 回归设施（§4 复用）。

**Architecture:** 1 spec 合并 task 序列。fence 扩围（逗号 selector list / 属性 selector 精确匹配靠拆 NodeKind 变体 / `resize` noop）+ packer src-key 归一化 + Playwright 设施（B2 三件分离 + A1 reset）+ showcase defer 注释。属性 selector 精确匹配走拆 `NodeKind::TextField` → `TextField`/`PasswordField`/`SearchField`（碰 core + pkg v19→v20 + 重编 .dll）。nth-child / aria-selected 注释 defer（§13 + roadmap）。

**Tech Stack:** Rust（fence/core/packer crate）、C#（Unity 投影层 + HeadlessTests）、node + Playwright（rect diff 设施）、taffy 0.5、csbindgen。

**Spec:** `docs/superpowers/specs/2026-07-20-showcase-package-unblock-design.md`

## Global Constraints

- Rust edition 2021；依赖钉版本（taffy 0.5 / ttf-parser 0.20 / slotmap 1.1 / csbindgen 1）；CSS 选择器解析器手搓零新依赖。
- node 已在工具链（tauri-cli 走 npm）；Playwright 是**验收期** node 工具，不进 `Cargo.toml`。
- pkg 格式版本一刀切 v19→**v20**，不留后向兼容（`MIN=MAX=20`）。
- 改 fence/packer/core 后重出 GUI exe；改 core 后重编 .dll + binding sync（`cargo run -p xtask -- sync-bindings`）。
- showcase 是浏览器对标基线，**不可改写非标准**；defer 项注释 + TODO 指回 roadmap。
- 用户只读中文；代码/commit 英文。
- push 前本地跑 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`。
- 两台机：编码机（本机）验打包 + core dump rect；家里机验 Unity rect。
- clippy 各 crate root `#![allow]` 放行的可辩护 lint 勿误清。

## File Structure

**Rust crate 改动**：
- `crates/core/src/scene/node.rs` — `NodeKind` enum 加 `PasswordField`/`SearchField` 变体 + `from_u8` + 谓词。
- `crates/core/src/asset/mod.rs` — `PKG_FORMAT_VERSION` 19→20 + kind_tag 写/读映射加新变体。
- `crates/core/src/asset/tests.rs` — pkg v20 + 新变体往返测试。
- `crates/core/src/dump.rs` — `NodeKind` dispatch 加新变体。
- `crates/core/src/style/dynamic.rs` — `compound_matches_node` 消费 `c.attrs`（type→NodeKind 匹配）。
- `crates/fence/src/schema/tag.rs` — `SemanticKind` 加 `PasswordField`/`SearchField` + `resolve_semantic` 拆 text/password/search。
- `crates/fence/src/css_rules.rs` — `parse_selector` 去 `[` 越界 + `parse_compound` 加属性解析 + `parse_style_block` 逗号 list split。
- `crates/fence/src/schema/css.rs` — `CssPropSpec` 表加 `resize`。
- `crates/packer/pkg/src/bridge.rs` — `map_semantic` SemanticKind→NodeKind 拆 + img src 归一化。
- `crates/packer/pkg/src/build.rs` — referenced_sprites 归一化（HTML 路径上下文）。
- `crates/packer/pkg/examples/spec4b_dump.rs` — 扩 dump rect JSON（core rect 导出器）。

**C# 改动**：
- `unity/package/Runtime/Public/LoomGUI.*.cs` — `NodeKind` enum + `NodeFactory` dispatch 加 `PasswordField`/`SearchField`。
- `unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs` — csbindgen 重生成。
- `tests/dotnet/LoomGUI.HeadlessTests/NodeKindTests.cs` — 加 PasswordField/SearchField 判别值断言。

**node 设施（新）**：`showcase/scripts/rect-diff/` — `package.json` / `browser-rect.mjs` / `diff.mjs` / `reset.css`。

**doc / showcase**：
- `docs/design/fence.md`（§311 子代漂移 + §108 TextField 拆）、`docs/design/main-design.md`（:121/:284）、`docs/design/public-api.md`（:296）。
- `docs/roadmap/roadmap.md` — §4 tech-debt 加 nth-child / aria-selected 两条。
- `showcase/showcase/home.html`（:38-41 注释）、`showcase/showcase/settings.html`（:15 注释）。

---

## Task 1: 拆 NodeKind/SemanticKind TextField → 3 变体 + pkg v20

**依赖**：无（全链起点）。后续 Task 2/4/5/6/7 依赖此 task 的 NodeKind 变体。

**Files**:
- Modify: `crates/fence/src/schema/tag.rs:54-67`（SemanticKind enum）、`:104-122`（resolve_semantic）
- Modify: `crates/core/src/scene/node.rs:98`（NodeKind enum）、`:128`（from_u8）、`:192`（谓词）、`:585`（tests）
- Modify: `crates/core/src/asset/mod.rs`（PKG_FORMAT_VERSION + kind_tag 写/读映射，写约 :189、读约 :397）
- Modify: `crates/core/src/dump.rs:39`（NodeKind dispatch）
- Modify: `crates/packer/pkg/src/bridge.rs:106`（map_semantic）

**Interfaces**:
- Produces: `NodeKind::PasswordField`、`NodeKind::SearchField`、`SemanticKind::PasswordField`、`SemanticKind::SearchField`；`resolve_semantic("input", Some("password")) → SemanticKind::PasswordField`；pkg kind_tag 新判别值；`PKG_FORMAT_VERSION = 20`。

- [ ] **Step 1: 写失败测试（fence resolve_semantic 拆）**

`crates/fence/src/schema/tag.rs` 测试模块（或 tag.rs 同文件 `#[cfg(test)]`）加：

```rust
#[test]
fn resolve_input_password_search_split() {
    assert_eq!(resolve_semantic("input", Some("text")), Some(SemanticKind::TextField));
    assert_eq!(resolve_semantic("input", Some("password")), Some(SemanticKind::PasswordField));
    assert_eq!(resolve_semantic("input", Some("search")), Some(SemanticKind::SearchField));
    assert_eq!(resolve_semantic("input", None), Some(SemanticKind::TextField)); // 默认 text
}
```

- [ ] **Step 2: 跑红**

Run: `cargo test -p loomgui_fence resolve_input_password_search_split`
Expected: 编译失败（`SemanticKind::PasswordField` 不存在）。

- [ ] **Step 3: 拆 SemanticKind + resolve_semantic**

`tag.rs:54` `pub enum SemanticKind` 在 `TextField,` 后加 `PasswordField, SearchField,`。
`tag.rs:115-116` 改：

```rust
        "input" => match input_type.unwrap_or("text") {
            "text" => Some(SemanticKind::TextField),
            "password" => Some(SemanticKind::PasswordField),
            "search" => Some(SemanticKind::SearchField),
            "number" => Some(SemanticKind::NumberField),
            "range" => Some(SemanticKind::Slider),
            "checkbox" => Some(SemanticKind::Toggle),
            "radio" => Some(SemanticKind::RadioButton),
            _ => None,
        },
```

- [ ] **Step 4: 跑绿（fence 部分）**

Run: `cargo test -p loomgui_fence`
Expected: PASS。

- [ ] **Step 5: 拆 NodeKind + from_u8 + 谓词（core）**

`node.rs:98` `NodeKind` enum 在 `TextField,` 后加 `PasswordField, SearchField,`（加在末尾也行，但紧跟 TextField 语义清晰——实现者按 enum 当前变体数定判别值，Rust/C#/NodeKindTests 三处一致）。
`node.rs:128` `from_u8` 加对应 `N => Some(NodeKind::PasswordField)` / `M => Some(NodeKind::SearchField)`（N/M = 新判别值）。
`node.rs:192` 谓词（`is_leaf`/`has_children` 等）把新变体归入 leaf 组（同 TextField）。

- [ ] **Step 6: bridge map_semantic 拆**

`bridge.rs:106` 改：

```rust
        Some(SemanticKind::TextField) => Ok(NodeKind::TextField),
        Some(SemanticKind::PasswordField) => Ok(NodeKind::PasswordField),
        Some(SemanticKind::SearchField) => Ok(NodeKind::SearchField),
```

- [ ] **Step 7: pkg v20 + kind_tag 写/读映射**

`asset/mod.rs`：`PKG_FORMAT_VERSION` 改 20（`MIN=MAX=20`，弃 v19 无迁移器）。kind_tag 写映射（约 :189）加 `NodeKind::PasswordField => <N>` / `SearchField => <M>`；读映射（约 :397）加反向。N/M 与 from_u8 一致。

- [ ] **Step 8: dump.rs dispatch + node.rs tests**

`dump.rs:39` 加：
```rust
            NodeKind::PasswordField => ("input", "PasswordField".into()),
            NodeKind::SearchField => ("input", "SearchField".into()),
```
`node.rs:585` tests 加 PasswordField/SearchField 进 NodeKind 全变体枚举测试。

- [ ] **Step 9: 写 pkg v20 往返测试**

`crates/core/src/asset/tests.rs` 加：构造含 PasswordField/SearchField 节点的 pkg，write → read，断言 kind 保真 + 版本=20。照 :497 现有 TextField 往返测试扩。

- [ ] **Step 10: 跑绿 + fmt/clippy**

Run: `cargo test -p loomgui_core` && `cargo test -p loomgui_fence` && `cargo test -p loomgui_pkg`
然后 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`。
Expected: 全绿。

- [ ] **Step 11: Commit**

```bash
git add crates/core/src/scene/node.rs crates/core/src/asset/ crates/core/src/dump.rs \
        crates/fence/src/schema/tag.rs crates/packer/pkg/src/bridge.rs
git commit -m "feat(core,fence): split NodeKind::TextField into TextField/PasswordField/SearchField (pkg v20)"
```

---

## Task 2: C# 投影 NodeFactory + binding sync

**依赖**：Task 1（NodeKind 新变体）。

**Files**:
- Modify: `unity/package/Runtime/Public/LoomGUI.*.cs`（NodeKind enum + NodeFactory dispatch）
- Modify: `unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs`（csbindgen 重生成）
- Modify: `tests/dotnet/LoomGUI.HeadlessTests/NodeKindTests.cs:51`

- [ ] **Step 1: 写失败测试**

`NodeKindTests.cs` 加（值 N/M 与 Rust from_u8 一致）：
```csharp
[Fact]
public void PasswordFieldIsN() => Assert.Equal((byte)N, (byte)NodeKind.PasswordField);
[Fact]
public void SearchFieldIsM() => Assert.Equal((byte)M, (byte)NodeKind.SearchField);
```

- [ ] **Step 2: 跑红**

Run: `dotnet test tests/dotnet/LoomGUI.HeadlessTests`（过滤 NodeKindTests）
Expected: 编译失败（`NodeKind.PasswordField` 不存在）。

- [ ] **Step 3: 重编 .dll + sync bindings**

```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
cargo run -p xtask -- sync-bindings
```
（Unity 必须关着才能拷 .dll。）确认 `LoomGUIBindings.cs` 重生成含新 NodeKind 判别值。

- [ ] **Step 4: C# NodeKind enum + NodeFactory dispatch**

`LoomGUI.Nodes.cs`（或 NodeKind 定义处）：NodeKind enum 加 `PasswordField`/`SearchField`（值 N/M）。`NodeFactory` dispatch 加 `case NodeKind.PasswordField: return new PasswordField(...)`（壳，照 TextField 壳模式；若无 PasswordField class，加壳 class : Node）。

- [ ] **Step 5: 跑绿（HeadlessTests + PublicApi 编译门）**

Run: `dotnet test tests/dotnet/LoomGUI.HeadlessTests` && `dotnet build tests/dotnet/LoomGUI.PublicApi`
Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add unity/package/Runtime/Public/ unity/package/Plugins/LoomGUI/Bindings/ \
        unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll tests/dotnet/LoomGUI.HeadlessTests/
git commit -m "feat(unity): NodeKind PasswordField/SearchField C# projection + dll rebuild"
```

---

## Task 3: fence 逗号 selector list

**依赖**：无。

**Files**:
- Modify: `crates/fence/src/css_rules.rs:226-245`（parse_style_block 普通选择器分支）
- Test: `crates/fence/src/css_rules.rs` 测试模块（:439 附近）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn parse_comma_selector_list_expands_to_shared_declarations() {
    let (rules, _, diags) = parse_style_block(r#"input[type="text"], select, textarea { color: red }"#);
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(rules.len(), 3, "逗号 list 展开为 3 条规则");
    // 三条共享同一声明块
    assert_eq!(rules[0].declarations, rules[1].declarations);
    assert_eq!(rules[1].declarations, rules[2].declarations);
}
```

- [ ] **Step 2: 跑红**

Run: `cargo test -p loomgui_fence parse_comma_selector_list`
Expected: FAIL（当前 `parse_selector("a, b")` 返 None → 3 条都丢 + 诊断，rules 空）。

- [ ] **Step 3: 实现 prelude 逗号 split**

`css_rules.rs` 普通选择器分支（:226-245），把单次 `parse_selector(prelude)` 改成按逗号展开：

```rust
        // 逗号 selector list：`a, b, c { decls }` → 每段独立 parse_selector，共享声明块。
        let declarations = parse_declarations(body, &loc, &mut diagnostics);
        if declarations.is_empty() {
            continue;
        }
        for sel_raw in prelude.split(',') {
            let sel_raw = sel_raw.trim();
            if sel_raw.is_empty() {
                continue;
            }
            let loc = line_map.source_location(sel_start, "<style>".to_string());
            match parse_selector(sel_raw) {
                Some(selector) => rules.push(DynamicRule { selector, declarations: declarations.clone() }),
                None => diagnostics.push(Diagnostic::error(
                    DiagnosticCode::FenceBadCssValue,
                    format!("unsupported selector \"{sel_raw}\" in <style>"),
                    loc,
                )),
            }
        }
```

注意：原代码先 `parse_selector` 再 `parse_declarations`；改成先 parse_declarations 再循环 split parse_selector（声明块只解析一次，复用）。

- [ ] **Step 4: 跑绿**

Run: `cargo test -p loomgui_fence`
Expected: PASS（含原 selector 测试不回归）。

- [ ] **Step 5: fmt/clippy + Commit**

```bash
cargo fmt --all && cargo clippy -p loomgui_fence -- -D warnings
git add crates/fence/src/css_rules.rs
git commit -m "feat(fence): comma selector list expands to shared declarations"
```

---

## Task 4: fence 属性 selector 解析

**依赖**：无（解析层独立，匹配在 Task 5）。

**Files**:
- Modify: `crates/fence/src/css_rules.rs:52`（parse_selector 去越界）、`:88-153`（parse_compound 加 `[attr]`）
- Test: `crates/fence/src/css_rules.rs`（:504-508 测试要改）

- [ ] **Step 1: 改越界测试 + 写新测试**

`:504` 现有 `assert!(parse_selector(r#"[type="text"]"#).is_none());` 改成 `.is_some()`。加：

```rust
#[test]
fn parse_attr_selector_eq() {
    let s = parse_selector(r#"input[type="text"]"#).unwrap();
    assert_eq!(s.compound[0].tag.as_deref(), Some("input"));
    assert_eq!(s.compound[0].attrs.len(), 1);
    let a = &s.compound[0].attrs[0];
    assert_eq!(a.name, "type");
    assert_eq!(a.op, AttrOp::Eq);
    assert_eq!(a.value.as_deref(), Some("text"));
    // specificity：属性 = class 级 → (0, id数=0, class+attr=1, tag=1) → Specificity(0,1,1)
    assert_eq!(s.specificity, Specificity(0, 1, 1));
}

#[test]
fn parse_attr_selector_unquoted_and_exists() {
    assert_eq!(parse_selector(r#"input[type=password]"#).unwrap().compound[0].attrs[0].value.as_deref(), Some("password"));
    // [attr] 存在形式
    let s = parse_selector(r#"[disabled]"#).unwrap();
    assert_eq!(s.compound[0].attrs[0].op, AttrOp::Exists);
    assert!(s.compound[0].attrs[0].value.is_none());
}
```

- [ ] **Step 2: 跑红**

Run: `cargo test -p loomgui_fence parse_attr_selector`
Expected: FAIL（`[` 越界返 None）。

- [ ] **Step 3: parse_selector 去 `[` 越界**

`css_rules.rs:52-58` 删 `|| raw.contains('[')`（保留 `,`/`>`/`+`/`~` 越界——这些本轮不做）。

- [ ] **Step 4: parse_compound 加 `[attr]` 分支**

`parse_compound`（:88）的 while 循环，加 `[` 分支（在 `:` 伪类分支后）：

```rust
        } else if let Some(r) = rest.strip_prefix('[') {
            // 属性选择器：[attr] / [attr="val"] / [attr=val]
            let close = r.find(']').ok_or(())?; // 用 None 语义：返 None 让 parse_compound 返 None
            let inner = r[..close].trim();
            let after = &r[close + 1..];
            let attr = if let Some(eq_pos) = inner.find('=') {
                let name = inner[..eq_pos].trim().trim_end_matches('^').trim_end_matches('~');
                let mut val = inner[eq_pos + 1..].trim();
                val = val.trim_matches('"').trim_matches('\'');
                if name.is_empty() { return None; }
                // 仅等值（=）；^= ~= 等高阶本轮不支持 → 返 None
                if inner[..eq_pos].trim_end_matches(' ').chars().any(|c| c == '^' || c == '~' || c == '$' || c == '*') {
                    return None;
                }
                Compound_attr(name.to_ascii_lowercase(), AttrOp::Eq, Some(val.to_string()))
            } else {
                Compound_attr(inner.to_ascii_lowercase(), AttrOp::Exists, None)
            };
            c.attrs.push(AttrSelector { name: attr.0, op: attr.1, value: attr.2 });
            b += 1;
            rest = after;
        } else {
```

（注：`Compound_attr` 是示意——实际直接构造 `AttrSelector { name, op, value }` push。高阶操作符 `^=`/`~=`/`$=`/`*=` 检测到就返 None，保持围栏外报错。`AttrOp`/`AttrSelector` 从 `loomgui_core::style::dynamic` import。）

- [ ] **Step 5: 跑绿**

Run: `cargo test -p loomgui_fence`
Expected: PASS。

- [ ] **Step 6: fmt/clippy + Commit**

```bash
cargo fmt --all && cargo clippy -p loomgui_fence -- -D warnings
git add crates/fence/src/css_rules.rs
git commit -m "feat(fence): attribute selector parsing [attr]/[attr=\"val\"] (Exists/Eq only)"
```

---

## Task 5: core 属性 selector 匹配（type → NodeKind）

**依赖**：Task 1（NodeKind 拆变体）+ Task 4（fence 产 attrs）。

**Files**:
- Modify: `crates/core/src/style/dynamic.rs:256-260`（compound_matches_node 消费 attrs）
- Test: `crates/core/src/style/dynamic.rs` 测试模块

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn attr_selector_type_matches_nodekind_precisely() {
    // [type="password"] 只匹配 PasswordField，不匹配 TextField
    let sel = parse_selector(r#"[type="password"]"#).unwrap();
    let mut pw_node = test_node(NodeKind::PasswordField);
    assert!(compound_matches_node(&sel.compound[0], &pw_node));
    let mut text_node = test_node(NodeKind::TextField);
    assert!(!compound_matches_node(&sel.compound[0], &text_node));
    // [type="text"] 只匹配 TextField
    let sel_text = parse_selector(r#"[type="text"]"#).unwrap();
    assert!(compound_matches_node(&sel_text.compound[0], &text_node));
    assert!(!compound_matches_node(&sel_text.compound[0], &pw_node));
}

#[test]
fn attr_selector_non_type_attr_does_not_match() {
    // 非 type 属性：本轮不匹配（规则不生效）
    let sel = parse_selector("[disabled]").unwrap();
    let node = test_node(NodeKind::TextField);
    assert!(!compound_matches_node(&sel.compound[0], &node));
}
```

（`test_node` 照现有测试 helper 造 Node。）

- [ ] **Step 2: 跑红**

Run: `cargo test -p loomgui_core attr_selector_type_matches`
Expected: FAIL（当前 `:258-260` 非空 attrs 直接返 false）。

- [ ] **Step 3: 改 compound_matches_node 消费 attrs**

`dynamic.rs:256-260` 把 `if !c.attrs.is_empty() { return false; }` 改成：

```rust
    // 属性选择器：本轮只支持 [type="x"]，查 NodeKind 精确对应。
    // 其他 attr name 不匹配（规则不生效）。
    for a in &c.attrs {
        if !attr_matches_node(a, node) {
            return false;
        }
    }
    true
}

/// `[type="x"]` 查 NodeKind 精确对应。其他 attr name 本轮不匹配（返 false）。
fn attr_matches_node(a: &AttrSelector, node: &Node) -> bool {
    if a.name != "type" {
        return false;
    }
    let Some(val) = &a.value else { return false; }; // [type] 存在形式本轮不匹配
    let expected_kind = match val.as_str() {
        "text" => NodeKind::TextField,
        "password" => NodeKind::PasswordField,
        "search" => NodeKind::SearchField,
        "number" => NodeKind::NumberField,
        "range" => NodeKind::Slider,
        "checkbox" => NodeKind::Toggle,
        "radio" => NodeKind::RadioButton,
        _ => return false,
    };
    node.kind == expected_kind
}
```

- [ ] **Step 4: 跑绿**

Run: `cargo test -p loomgui_core`
Expected: PASS。

- [ ] **Step 5: fmt/clippy + Commit**

```bash
cargo fmt --all && cargo clippy -p loomgui_core -- -D warnings
git add crates/core/src/style/dynamic.rs
git commit -m "feat(core): attribute selector [type=x] matches NodeKind precisely"
```

---

## Task 6: fence `resize` prop noop

**依赖**：无。

**Files**:
- Modify: `crates/fence/src/schema/css.rs`（CSS_PROPS 表 + CssValueParser）
- Test: `crates/fence/src/schema/css.rs` 测试模块

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn resize_prop_accepted_as_noop() {
    // resize 进 CSS_PROPS（find_css_prop 命中），值 none/both/horizontal/vertical 接受
    assert!(find_css_prop("resize").is_some());
    // 通过 parse_style_block 验：含 resize:none 的规则不产 prop 名诊断
    let (_, _, diags) = parse_style_block(r#"textarea { resize: none }"#);
    let resize_diag = diags.iter().find(|d| d.message.contains("resize"));
    assert!(resize_diag.is_none(), "resize 不该报 prop 名错：{diags:?}");
}
```

- [ ] **Step 2: 跑红**

Run: `cargo test -p loomgui_fence resize_prop`
Expected: FAIL（`find_css_prop("resize")` 返 None）。

- [ ] **Step 3: CSS_PROPS 加 resize + CssValueParser::Keyword**

`css.rs` `CssValueParser` enum 加变体（若 `Keyword` 够用则复用）。`CSS_PROPS` 表加（照现有条目模式）：

```rust
    CssPropSpec {
        name: "resize",
        default: "none",
        inherited: false,
        parser: CssValueParser::Keyword(&["none", "both", "horizontal", "vertical"]),
    },
```

core `apply_decl` 不消费 resize（noop，同 transition——apply_decl 对未知 prop 名忽略；resize 进了 CSS_PROPS 只是让 fence 不报 prop 名错，core 自然不 apply）。

- [ ] **Step 4: 跑绿**

Run: `cargo test -p loomgui_fence`
Expected: PASS。

- [ ] **Step 5: fmt/clippy + Commit**

```bash
cargo fmt --all && cargo clippy -p loomgui_fence -- -D warnings
git add crates/fence/src/schema/css.rs
git commit -m "feat(fence): accept resize CSS prop as noop (textarea)"
```

---

## Task 7: doc 漂移修 + TextField 拆 3 文档

**依赖**：Task 1（NodeKind 拆完才有拆 3 可写）。

**Files**:
- Modify: `docs/design/fence.md`（§311 子代→后代空格 + :108 TextField 拆）
- Modify: `docs/design/main-design.md:121,:284`（TextField 拆）
- Modify: `docs/design/public-api.md:296`（TextField 拆 3）
- Modify: `crates/packer/gui/src-tauri/templates/skill/SKILL.md:81`（TextField 拆）

- [ ] **Step 1: fence.md §311 子代漂移**

`fence.md:311`「class/tag/id/后代/子代/伪类」→「class/tag/id/后代空格/伪类」（对齐 `css_rules.rs:506` 测试断言 `.a > .b` 返 None）。

- [ ] **Step 2: TextField 拆 3 文档（四处）**

- `fence.md:108`：`| input[type=text]（含 password、search，默认） | TextField |` → 拆三行：
  ```
  | `input[type=text]`（默认） | TextField |
  | `input[type=password]` | PasswordField |
  | `input[type=search]` | SearchField |
  ```
- `main-design.md:121`：`<input type="text/password/search"> → TextField` → 拆三行（text→TextField, password→PasswordField, search→SearchField）。:284 控件 API 表同。
- `public-api.md:296`：`| input[text/password/search] | TextField : Node | ... |` → 拆三行（TextField/PasswordField/SearchField 各一行，共享 Value/Placeholder/... 事件）。
- `SKILL.md:81`：`| text (default), password, search | TextField |` → 拆三行。

- [ ] **Step 3: 防漂移门**

Run: `cargo test -p loomgui_fence`
Expected: PASS（doc 改不破 fence 测试，确认无 schema 引用断裂）。

- [ ] **Step 4: Commit**

```bash
git add docs/design/fence.md docs/design/main-design.md docs/design/public-api.md \
        crates/packer/gui/src-tauri/templates/skill/SKILL.md
git commit -m "docs: fix fence.md selector subset drift + split TextField into 3 variants"
```

---

## Task 8: packer src-key 路径归一化

**依赖**：无。

**Files**:
- Modify: `crates/packer/pkg/src/build.rs:40-56`（refs 收集循环 + 归一化）
- Modify: `crates/packer/pkg/src/build.rs` 加归一化 helper（或 `bridge.rs`）
- Test: `crates/packer/pkg/tests/build.rs:59` 附近（sprite_key 测试）

**背景**：sprite_key = 图相对 workspace_root（正斜杠，`atlas/collect.rs:8`）。HTML img src 是相对 HTML 文件（如 `showcase/home.html` 里的 `../res/icons/x.png`）。两者前缀不匹配 → `referenced_sprites` 校验挂。设计意图（`workspace-CLAUDE.md:73`）：`../../assets/x` → `assets/x`。

- [ ] **Step 1: 写失败测试**

`crates/packer/pkg/tests/build.rs` 加（照 :59 现有 sprite_key 测试）：

```rust
#[test]
fn img_src_with_dotdot_normalizes_to_workspace_root_relative() {
    // HTML 在 showcase/home.html（workspace_root 相对），img src ../res/icons/x.png
    // → sprite_key res/icons/x.png
    let got = normalize_sprite_key("showcase/home.html", "../res/icons/x.png");
    assert_eq!(got, "res/icons/x.png");
    // 无 ../ 的 src 直接相对 workspace_root
    assert_eq!(normalize_sprite_key("showcase/home.html", "res/icons/y.png"), "res/icons/y.png");
}
```

- [ ] **Step 2: 跑红**

Run: `cargo test -p loomgui_pkg normalize_sprite_key`
Expected: 编译失败（函数不存在）。

- [ ] **Step 3: 实现归一化 helper + 接入 build.rs**

`build.rs` 加（手写 Component 归约，零新依赖）：

```rust
/// 把 img src（相对 HTML 文件）归一化为 sprite_key（相对 workspace_root，正斜杠）。
/// html_rel = HTML 相对 workspace_root（如 "showcase/home.html"）；src = img src 原值。
/// 例：("showcase/home.html", "../res/icons/x.png") → "res/icons/x.png"。
pub fn normalize_sprite_key(html_rel: &str, src: &str) -> String {
    let base = std::path::Path::new(html_rel).parent().unwrap_or(std::path::Path::new(""));
    let joined = base.join(src);
    // 手写归约：Normal 段入栈，CurDir 跳过，ParentDir 弹栈（栈空则丢——不逃出 workspace_root）
    let mut stack: Vec<&str> = Vec::new();
    for comp in joined.components() {
        use std::path::Component;
        match comp {
            Component::Normal(s) => stack.push(s.to_str().unwrap_or("")),
            Component::CurDir => {}
            Component::ParentDir => { stack.pop(); }
            Component::RootDir | Component::Prefix(_) => {} // 绝对路径不归一化（围栏外）
        }
    }
    stack.join("/")
}
```

`build.rs:46-55` 收集循环改成带 HTML 路径归一化（需把当前组件 HTML 路径从循环上下文传入——`resolve_html_list` 已算，循环里每组件对应一个 HTML）：

```rust
        // parsed.referenced_sprites 是相对 HTML 的 src；归一化为 sprite_key（相对 workspace_root）
        for src in &parsed.referenced_sprites {
            refs.insert(normalize_sprite_key(&html_rel, src));
        }
```

（`refs` 从 `Vec` 改 `HashSet<String>` 去重，或保持 Vec——照现有类型。`html_rel` 是当前组件 HTML 相对 workspace_root，循环上下文已有。）

- [ ] **Step 4: 跑绿**

Run: `cargo test -p loomgui_pkg`
Expected: PASS。

- [ ] **Step 5: fmt/clippy + Commit**

```bash
cargo fmt --all && cargo clippy -p loomgui_pkg -- -D warnings
git add crates/packer/pkg/src/build.rs crates/packer/pkg/tests/build.rs
git commit -m "fix(packer): normalize img src (relative to HTML) to sprite_key (workspace-root relative)"
```

---

## Task 9: Playwright 浏览器 rect 导出器

**依赖**：无（设施独立）。

**Files**:
- Create: `showcase/scripts/rect-diff/package.json`
- Create: `showcase/scripts/rect-diff/browser-rect.mjs`
- Create: `showcase/scripts/rect-diff/reset.css`

- [ ] **Step 1: 装 Playwright**

```bash
cd showcase/scripts/rect-diff
npm init -y
npm install -D playwright
npx playwright install chromium
```

`package.json` 加 `"type": "module"`（.mjs 用 ESM）。

- [ ] **Step 2: 写 reset.css（A1 基准对齐）**

`reset.css`：
```css
/* A1 reset：让浏览器侧 ≈ LoomGUI 无 UA 默认，绝对坐标可比 */
* { box-sizing: border-box; }
body { margin: 0; padding: 0; }
h1,h2,h3,h4,h5,h6,p,ul,ol,li,div { margin: 0; padding: 0; }
ul,ol { list-style: none; }
```

- [ ] **Step 3: 写 browser-rect.mjs**

```javascript
// 用法：node browser-rect.mjs <showcase-html-绝对路径> <out.json>
// 加载 showcase HTML（注入 reset）→ 量所有元素 getBoundingClientRect → JSON {id, tag, classes, x, y, w, h}
import { chromium } from 'playwright';
import { readFileSync } from 'fs';

const [, , htmlPath, outPath] = process.argv;
if (!htmlPath || !outPath) { console.error('usage: node browser-rect.mjs <html> <out.json>'); process.exit(1); }

const reset = readFileSync(new URL('./reset.css', import.meta.url), 'utf8');
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
await page.goto('file://' + htmlPath, { waitUntil: 'networkidle' });
await page.addStyleTag({ content: reset });
await page.waitForTimeout(100); // 让 reset 重排稳定

const rects = await page.evaluate(() => {
  const els = document.querySelectorAll('body *');
  return Array.from(els).map((el, i) => {
    const r = el.getBoundingClientRect();
    return {
      domIndex: i,
      tag: el.tagName.toLowerCase(),
      id: el.id || null,
      classes: Array.from(el.classList),
      x: r.x, y: r.y, w: r.width, h: r.height,
    };
  });
});

await browser.close();
writeFileSync(outPath, JSON.stringify(rects, null, 2));
import { writeFileSync } from 'fs';
```

- [ ] **Step 4: 跑（spec4b 单页验设施能导）**

```bash
cd showcase/scripts/rect-diff
node browser-rect.mjs ../../showcase/spec4b/spec4b-acceptance.html spec4b-browser.json
```
Expected: 产出 `spec4b-browser.json`，含每个元素的 rect。人工抽查 root 宽 1280、card 宽 300（spec4b CSS）。

- [ ] **Step 5: Commit**

```bash
git add showcase/scripts/rect-diff/
git commit -m "feat(rect-diff): Playwright browser rect exporter + A1 reset"
```

---

## Task 10: core rect 导出器（spec4b_dump 扩 JSON）

**依赖**：Task 1（NodeKind 拆完，dump 不破）。

**Files**:
- Modify: `crates/packer/pkg/examples/spec4b_dump.rs`（加 JSON rect 导出模式）

**背景**：`spec4b_dump` 已 dump 全节点 `layout_rect`（CLAUDE.md 调试段）。本 task 加 JSON 输出（与 browser-rect.json 同结构，可配对比对）。

- [ ] **Step 1: 加 JSON 导出模式**

`spec4b_dump.rs` 加 CLI 参数 `--json <out>`：dump 全节点 `layout_rect`（border box 口径，`node.layout_rect`）→ JSON 数组 `{domIndex, tag, id, classes, x, y, w, h}`。domIndex = scene 节点 DFS 序（与浏览器 DOM 序对齐需稳定映射——见 Task 12 scheme 确认）。tag 从 `NodeKind` 反查（dump.rs:39 已有映射）。id/classes 从 `node.id_attr`/`node.classes`。

```rust
// 伪代码骨架（实现按 spec4b_dump 现有结构扩）
let nodes_json: Vec<Value> = scene.iter_dfs().enumerate().map(|(i, n)| json!({
    "domIndex": i,
    "tag": kind_to_tag(n.kind),       // 复用 dump.rs:39 映射
    "id": n.id_attr,
    "classes": n.classes,
    "x": n.layout_rect.x, "y": n.layout_rect.y,
    "w": n.layout_rect.w, "h": n.layout_rect.h,
})).collect();
std::fs::write(out_path, serde_json::to_string_pretty(&nodes_json)?)?;
```

（serde_json 是否已在依赖——pkg crate 若无 serde_json，用 手写 JSON 字符串拼接零新依赖，或加 serde_json dev-dependency。实现时按 pkg Cargo.toml 定。）

- [ ] **Step 2: 跑**

```bash
cargo run -p loomgui_pkg --example spec4b_dump -- <pkg.bin> <atlas.json> <font> --json spec4b-core.json
```
Expected: 产出 `spec4b-core.json`，节点 rect。抽查与 browser-rect.json 同元素 rect 接近（绝对坐标，因 A1 reset）。

- [ ] **Step 3: Commit**

```bash
git add crates/packer/pkg/examples/spec4b_dump.rs
git commit -m "feat(dump): spec4b_dump JSON rect export mode (core side of rect diff)"
```

---

## Task 11: diff 工具

**依赖**：Task 9/10（两 JSON 格式）。

**Files**:
- Create: `showcase/scripts/rect-diff/diff.mjs`

- [ ] **Step 1: 写 diff.mjs**

```javascript
// 用法：node diff.mjs <browser.json> <core.json> [--tol-box 1] [--tol-text 3]
// 配对（按 id 优先，否则 domIndex）→ 逐元素比 rect → 报差异 + exit code（有超容差=1）
import { readFileSync } from 'fs';
const args = process.argv.slice(2);
const boxTol = +(args.find(a => a.startsWith('--tol-box'))?.split('=')[1] ?? 1);
const textTol = +(args.find(a => a.startsWith('--tol-text'))?.split('=')[1] ?? 3);

const browser = JSON.parse(readFileSync(args[0], 'utf8'));
const core = JSON.parse(readFileSync(args[1], 'utf8'));

const byKey = (arr) => Object.fromEntries(arr.map(e => [e.id ?? `#${e.domIndex}`, e]));
const bMap = byKey(browser), cMap = byKey(core);
const keys = new Set([...Object.keys(bMap), ...Object.keys(cMap)]);
let diffs = 0, unmatched = 0;
for (const k of keys) {
  const b = bMap[k], c = cMap[k];
  if (!b || !c) { unmatched++; console.log(`UNMATCHED ${k}: ${b?'browser-only':'core-only'}`); continue; }
  const isText = c.tag === 'span'; // 文本节点宽容
  const tol = isText ? textTol : boxTol;
  for (const f of ['x','y','w','h']) {
    if (Math.abs(b[f] - c[f]) > tol) {
      diffs++; console.log(`DIFF ${k}.${f}: browser=${b[f]} core=${c[f]} (tol=${tol})`);
    }
  }
}
console.log(`\nsummary: ${diffs} rect diffs, ${unmatched} unmatched`);
process.exit(diffs + unmatched > 0 ? 1 : 0);
```

- [ ] **Step 2: 跑（spec4b 单页）**

```bash
cd showcase/scripts/rect-diff
node diff.mjs spec4b-browser.json spec4b-core.json
```
Expected: 报告差异（spec4b 简单页应接近 0 diff，或仅文本度量微差）。

- [ ] **Step 3: Commit**

```bash
git add showcase/scripts/rect-diff/diff.mjs
git commit -m "feat(rect-diff): diff tool (id/domIndex pairing + box/text tolerance)"
```

---

## Task 12: 设施自验门 — spec4b 单页 rect diff 绿

**依赖**：Task 9/10/11。

**Files**: 无（串联验证）。

- [ ] **Step 1: 打包 spec4b 单页 pkg**

```bash
cargo run -p loomgui_pkg -- build showcase   # 产 spec4b-acceptance.pkg.bin（loom.workspace.json 已配）
```

- [ ] **Step 2: 三件串联跑**

```bash
cd showcase/scripts/rect-diff
node browser-rect.mjs ../../showcase/spec4b/spec4b-acceptance.html spec4b-browser.json
cargo run -p loomgui_pkg --example spec4b_dump -- <spec4b.pkg.bin 路径> <atlas> <font> --json spec4b-core.json
node diff.mjs spec4b-browser.json spec4b-core.json
```
Expected: diff summary 接近 0 diff（spec4b 是简单页 + no-UA reset 已对齐基准）。若文本节点微差在容差内 OK。

- [ ] **Step 3: 调 scheme（若 domIndex 不对齐）**

若 diff 报大量 UNMATCHED，是 domIndex scheme 不一致（浏览器 DOM 序 vs core DFS 序）。改 diff.mjs 配对策略：优先按 `id` 配对（showcase 元素多有 id），其次按 `tag + classes` 组合。记录 scheme 决策到 spec §10。

- [ ] **Step 4: 设施门判定**

spec4b 单页 rect diff 绿（diffs 在容差内）= 设施门过。否则回 Task 9-11 调。无 commit（验证步骤）。

---

## Task 13: showcase 调整 + roadmap tech-debt

**依赖**：无（注释/doc）。

**Files**:
- Modify: `showcase/showcase/home.html:38-41`（nth-child 注释）
- Modify: `showcase/showcase/settings.html:15`（aria-selected 注释）
- Modify: `docs/roadmap/roadmap.md`（§4 tech-debt 加两条）

- [ ] **Step 1: home.html nth-child 注释**

`home.html:38-41` 的 7 条 `.nav-card:nth-child(N){animation-delay:...}` 整体注释 + TODO：

```html
  <!-- TODO(roadmap §4 tech-debt nth-child): :nth-child(N) defer 到控件束/§4 animation runtime。
       home 7 条错峰 animation-delay 随 nth-child + animation runtime 一同激活。
  .nav-card:nth-child(1){animation-delay:.05s}.nav-card:nth-child(2){animation-delay:.1s}
  ...（7 条全注释）-->
```

- [ ] **Step 2: settings.html aria-selected 注释**

`settings.html:15`：
```css
  /* TODO(roadmap §4 tech-debt aria-selected): state-attr selector + tab 控件 defer 到控件束 TabList。*/
  /* .tab[aria-selected="true"] { background-color:rgba(26,47,69,0.85); color:#5fb4d4; font-weight:700; } */
```

- [ ] **Step 3: roadmap §4 tech-debt 加两条**

`docs/roadmap/roadmap.md` §4 tech-debt 段加（照 spec §9 草稿）：
- `:nth-child(N)` 条（pkg v20→v21 + 与 keyframes runtime 合并）。
- `[aria-selected]` state-attr 条（控件束 TabList）。

- [ ] **Step 4: Commit**

```bash
git add showcase/showcase/home.html showcase/showcase/settings.html docs/roadmap/roadmap.md
git commit -m "docs(showcase): defer nth-child + aria-selected selectors (roadmap tech-debt)"
```

---

## Task 14: 8 页打包门 + rect diff 快照

**依赖**：Task 1-8, 13（fence 扩围 + packer 修 + NodeKind 拆 + showcase 注释都完成）。

**Files**: 无（验收 + 快照报告）。Create: `showcase/scripts/rect-diff/snapshot-2026-07-20.md`（快照报告）。

- [ ] **Step 1: 8 页打包门（硬）**

```bash
cargo run -p loomgui_pkg -- build showcase
```
Expected: 8 页（home/settings/mail/inventory/shop/character/form/lab）全打包过，无 fence diagnostic。若有 diagnostic，回对应 Task 修。

- [ ] **Step 2: 8 页 rect diff 快照（软）**

对每页跑 browser-rect + core rect + diff，记录到 `snapshot-2026-07-20.md`：
- 哪些页 rect diff 接近全绿（LoomGUI 已支持特性：基础 div/flex/img/text）。
- 哪些页大面积红 + 原因（特性 gap：form/settings input 控件布局、inventory ListView、mail 富文本、character/lab filter/animation）。

这是特性 gap 仪表盘基线，绿不是门。

- [ ] **Step 3: 家里机 Unity rect（标准 B 的 Unity 半）**

把 pkg + atlas + fonts 搬家里机，Unity PlayMode 实例化 8 页，Unity rect 与 core rect 比对（特性 gap 同上，软门）。这步家里机做，编码机 commit 快照报告后等家里机反馈。

- [ ] **Step 4: Commit 快照**

```bash
git add showcase/scripts/rect-diff/snapshot-2026-07-20.md
git commit -m "test(rect-diff): 8-page rect diff baseline snapshot (showcase unblock)"
```

---

## Task 15: GUI exe 重出 + 最终全绿门

**依赖**：所有前置 task。

**Files**:
- Modify: `unity/package/Editor/Tools/loomgui_gui.exe`

- [ ] **Step 1: 重出 GUI exe（fence/packer 改动后必须）**

```bash
(cd crates/packer/gui/src-tauri && tauri build --no-bundle)
cp crates/packer/gui/src-tauri/target/release/loomgui_gui.exe unity/package/Editor/Tools/loomgui_gui.exe
```

- [ ] **Step 2: 最终全绿门**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test                            # 全 workspace
cargo test --no-default-features --all-targets   # feature-gate check
dotnet test tests/dotnet/LoomGUI.HeadlessTests
dotnet build tests/dotnet/LoomGUI.PublicApi
```
Expected: 全绿。

- [ ] **Step 3: Commit exe**

```bash
git add unity/package/Editor/Tools/loomgui_gui.exe
git commit -m "chore(gui): rebuild loomgui_gui.exe (fence attribute selector + NodeKind split + resize)"
```

---

## Plan Self-Review

**Spec coverage**（spec section → task）：
- §3 阻塞 #1 逗号 list → Task 3 ✅
- §3 阻塞 #2 属性 selector（A2 拆变体）→ Task 1（拆）+ 4（解析）+ 5（匹配）+ 2（C#）+ 7（doc）✅
- §3 阻塞 #3 resize → Task 6 ✅
- §3 阻塞 #4 src-key → Task 8 ✅
- §3 doc 漂移 §311 → Task 7 ✅
- §4.3 A2 拆 NodeKind + pkg v20 → Task 1 ✅
- §4.5 Playwright A1 reset + B2 三件 → Task 9/10/11 ✅
- §4.7 验收门（打包门硬 + 设施门硬 + 快照软）→ Task 14/12 ✅
- §9 nth-child / aria-selected defer → Task 13 ✅
- §8 避让 image-bg / dll/exe 闭环 → Task 2（dll）+ 15（exe）✅

**Placeholder scan**：无 TBD/TODO 空泛（代码段完整或给精确落点 + 骨架；`<N>`/`<M>` 是 NodeKind 判别值占位，实现者按 enum 顺序定 + Rust/C#/NodeKindTests 三处一致——这是实现参数非 spec 缺陷）。

**Type consistency**：`NodeKind::PasswordField`/`SearchField`、`SemanticKind::PasswordField`/`SearchField`、`AttrSelector{name,op,value}`、`AttrOp::Eq/Exists` 跨 task 命名一致。`normalize_sprite_key(html_rel, src)` 签名 Task 8 定义 + 测试用，一致。

**顺序依赖**：Task 1（拆 NodeKind）是 Task 2/4/5/7 的前置；Task 9-11 独立可早做；Task 12 依赖 9-11；Task 14 依赖 1-8+13；Task 15 最后。
