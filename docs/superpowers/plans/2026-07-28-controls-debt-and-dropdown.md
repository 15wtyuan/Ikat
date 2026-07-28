# 控件债收口 + Dropdown 全栈 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 收口控件束剩余债务（NumberField / 文本控件 getter / 投影类 / UIStyleException / 攒批 flush + set_transform）并实现 Dropdown/OptionItem 全栈（含 scrollbar 模式浮层基建）。

**Architecture:** 全部沿用项目既有模式——浮层用 scrollbar thumb 的 render-末尾追加模式；控件子树注入用 `inject_control_children` 的 `make_child`+`append_child` 模式；运行时状态用 `ControlState` side table；打包期载荷用 `ControlInit`；攒批用 projection-layer §2 预留的 StyleMirror/NodeTransform flush seam。不发明新机制。

**Tech Stack:** Rust core（edition 2021，taffy 0.12，bincode）+ fence crate（html5gum）+ packer（bridge）+ csbindgen FFI + C# 投影层（Unity Runtime）。

**Spec:** `docs/superpowers/specs/2026-07-28-controls-debt-and-dropdown-design.md`

## Global Constraints

- Rust edition 2021；依赖钉版本（taffy 0.12 / csbindgen 1）。CSS 选择器解析器手搓零新依赖。
- Rust FFI 返字符串一律 ptr+len（不靠 NUL）；C-like enum 必须 `#[repr(uN)]`；FFI 边界 struct 须 `size_of` 断言。
- 改 parse-time 逻辑（bridge / css_resolve / fence schema）后必须重打 pkg；纯 runtime 改只重编 .dll。任何 Rust 改动后重编 + commit .dll。
- 控件不带 UA 默认样式——围栏强制要求 CSS 命中（`FenceControlWithoutCss`）。
- pkg 格式版本一刀切升（MIN=MAX），不留迁移器；加 bincode 稳定性测试。
- push 前本地跑 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`。
- 编码机（本机）跑 headless 测试（Rust + dotnet）；Unity 真机验收在家用机串行做。
- 用户只读中文——commit message 英文，问答中文。
- NodeKind enum 变体只追加到末尾（保 `from_u8` 判别值稳定）。

---

## File Structure

**Rust core（`crates/core/src/`）：**
- `scene/node.rs` — `ControlState` 加 `Dropdown`/`NumberField` 变体；`NodeFlags`（已有 DISABLED/FOCUSED，不改）。
- `scene/control.rs` — `inject_control_children` 加 Dropdown 分支（注入 `.loom-value`+`.loom-popup`）；`sync_control_visuals` 加 Dropdown 分支；新常量 `VALUE`/`POPUP`。
- `asset/mod.rs` — `ControlInit` 加 `Dropdown`/`NumberField` 变体；`PKG_FORMAT_VERSION` 25→26；instantiate 映射。
- `render/mod.rs` — `build_render_nodes` 末尾追加 open popup 子树 DFS。
- `render/batch.rs` — `assign_sort_keys` 不改主体；新 helper `assign_popup_sort_keys`（续号 + mask=0）。
- `hit.rs` — `hit_test` 前置 popup 命中 check。
- `stage.rs` — 已有 `blur()`（:471），不改。
- `input.rs` — NumberField 字符输入 guard（filter 非数字）。

**FFI（`crates/ffi/src/lib.rs`）：**
- 新增 `loomgui_stage_get_node_disabled` / `loomgui_stage_get_control_readonly` / `loomgui_stage_blur` / `loomgui_stage_get_dropdown_selected_index` / `loomgui_stage_set_dropdown_selected_index` / `loomgui_stage_get_dropdown_open` / `loomgui_stage_set_dropdown_open` / `loomgui_stage_get_number_value` / `loomgui_stage_set_number_value`。
- 扩展 `loomgui_stage_set_transform`（加 ox/oy）。

**fence（`crates/fence/src/`）：**
- `control_css_check.rs` — `CONTROL_KINDS` 加 Dropdown/NumberField；`has_injected_children` 加 Dropdown；`loom_children_hint` + 教学文案加分支。

**packer（`crates/packer/pkg/src/bridge.rs`）：**
- `extract_control_init` 加 Dropdown/NumberField arm。

**C# 投影（`unity/package/Runtime/`）：**
- `Public/LoomGUI.Nodes.cs` — NumberField/Dropdown 填实；OptionItem/Slot/CustomElement class；TextField/TextArea getter 改读 FFI。
- `Public/LoomGUI.Types.cs` — `UIStyleException`。
- `Public/LoomGUI.Events.cs` — `SelectionChangedEvent`。
- `Public/LoomGUI.EventType.cs` — 加 `SelectionChanged`。
- `Projection/NodeFactory.cs` — OptionItem/Slot/CustomElement arm 改 dispatch。
- `Projection/StyleMirror.cs` — setter 标脏（攒批）。
- `Host/LoomHost.cs` — Step 帧末 flush seam。
- `Projection/EventDemuxer.cs` — SelectionChanged 分支。

---

## Part A：控件债收口

### Task 1: pkg v26 + ControlInit/ControlState 加 Dropdown/NumberField 变体

**Files:**
- Modify: `crates/core/src/asset/mod.rs:22`（PKG_FORMAT_VERSION）+ `:57`（ControlInit enum）
- Modify: `crates/core/src/scene/node.rs:413`（ControlState enum）
- Test: `crates/core/src/asset/tests.rs`

**Interfaces:**
- Produces: `ControlInit::Dropdown { selected_index: u32 }`、`ControlInit::NumberField(EditInit, f32, f32, f32)`（min/max/step）、`ControlState::Dropdown { selected_index, open, value_lock }`、`ControlState::NumberField { edit: EditState, min, max, step }`。后续 Task 消费这些变体。

- [ ] **Step 1: 写 ControlState 新变体的失败测试**

加到 `crates/core/src/scene/node.rs` 测试模块（或 `tests.rs`）：

```rust
#[test]
fn control_state_dropdown_variant() {
    let s = ControlState::Dropdown { selected_index: 2, open: false, value_lock: false };
    assert!(matches!(s, ControlState::Dropdown { selected_index: 2, open: false, .. }));
}

#[test]
fn control_state_number_field_variant() {
    let edit = EditState::from_init("3.14".into(), String::new(), 0, false);
    let s = ControlState::NumberField { edit, min: 0.0, max: 100.0, step: 1.0 };
    assert!(matches!(s, ControlState::NumberField { min: 0.0, max: 100.0, step: 1.0, .. }));
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core control_state_`
Expected: 编译失败（变体不存在）。

- [ ] **Step 3: 加 ControlState 变体**

`crates/core/src/scene/node.rs:413` 的 `ControlState` enum，在 `TextArea(EditState)` 后追加：

```rust
    /// `<select>` 下拉。selected_index=当前选中项；open=popup 是否展开；value_lock 防反馈环。
    Dropdown {
        selected_index: usize,
        open: bool,
        value_lock: bool,
    },
    /// `<input type="number">`。edit 复用 EditState（value 是数字的文本形式）；
    /// min/max/step 是数值约束，读写门做 clamp + 量化。
    NumberField {
        edit: EditState,
        min: f32,
        max: f32,
        step: f32,
    },
```

- [ ] **Step 4: 加 ControlInit 变体 + 升 pkg 版本**

`crates/core/src/asset/mod.rs:22` 改版本：
```rust
pub const PKG_FORMAT_VERSION: u32 = 26; // v26: ControlInit Dropdown/NumberField (bincode layout change)
```
同步改 MIN/MAX 常量（搜 `PKG_FORMAT_MIN` / `PKG_FORMAT_MAX`，都设 26）。

`crates/core/src/asset/mod.rs:57` 的 `ControlInit` enum，在 `TextArea(EditInit)` 后追加：
```rust
    /// `<select>` 初始选中项索引（打包期扫 option[selected] 算出，无 selected 则 0）。
    Dropdown { selected_index: u32 },
    /// `<input type="number">`。edit 是 value/placeholder/maxlength/readonly；min/max/step 数值约束。
    NumberField {
        edit: EditInit,
        min: f32,
        max: f32,
        step: f32,
    },
```

- [ ] **Step 5: 加 ControlInit→ControlState instantiate 映射**

搜 `ControlInit::TextArea` 在 instantiate 路径的 match（`crates/core/src/asset/mod.rs` 或 `scene/`），加：
```rust
        ControlInit::Dropdown { selected_index } => {
            ControlState::Dropdown { selected_index: selected_index as usize, open: false, value_lock: false }
        }
        ControlInit::NumberField { edit, min, max, step } => {
            ControlState::NumberField {
                edit: EditState::from_init(edit.value, edit.placeholder, edit.max_length, edit.readonly),
                min, max, step,
            }
        }
```

- [ ] **Step 6: 更新 bincode 稳定性测试**

`crates/core/src/asset/tests.rs` 找 `CONTROL_INIT` 或 bincode 往返测试，加 Dropdown/NumberField 的往返断言（照现有 Progress/Slider 测试模板）：
```rust
    let mut sel = tn(NodeKind::Dropdown);
    sel.control_init = Some(ControlInit::Dropdown { selected_index: 1 });
    // ... write_package + read_package 往返，断言 selected_index 保真
```

- [ ] **Step 7: 跑测试确认通过**

Run: `cargo test -p loomgui_core`
Expected: PASS（含新变体 + bincode 往返）。

- [ ] **Step 8: Commit**

```bash
git add crates/core/src/asset/mod.rs crates/core/src/scene/node.rs crates/core/src/asset/tests.rs
git commit -m "core: add ControlInit/State Dropdown+NumberField variants, bump pkg v26"
```

---

### Task 2: fence ControlCssCheck 加 Dropdown/NumberField

**Files:**
- Modify: `crates/fence/src/control_css_check.rs:32`（CONTROL_KINDS）+ `:47`（has_injected_children）+ `:180`（loom_children_hint）+ `:219`（kind_name）+ fix_hint 分支
- Test: `crates/fence/src/control_css_check.rs`（tests 模块）

**Interfaces:**
- Produces: Dropdown/NumberField 进 `FenceControlWithoutCss` 受校验集；Dropdown 教学文案含"NO built-in arrow"。

- [ ] **Step 1: 写失败测试**

加到 `control_css_check.rs` 的 `#[cfg(test)] mod tests`：

```rust
#[test]
fn bare_select_no_rules_errors() {
    let diags = check(r#"<select><option value="a">A</option></select>"#);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, DiagnosticCode::FenceControlWithoutCss);
    assert!(diags[0].message.contains("NO built-in arrow"), "msg: {}", diags[0].message);
}

#[test]
fn select_with_tag_rule_passes() {
    let diags = check(r#"<style>select{background:#ddd}</style><select><option value="a">A</option></select>"#);
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn bare_number_input_no_rules_errors() {
    let diags = check(r#"<input type="number" min="0" max="10">"#);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, DiagnosticCode::FenceControlWithoutCss);
}

#[test]
fn number_input_with_rule_passes() {
    let diags = check(r#"<style>input[type="number"]{background:#ddd}</style><input type="number" min="0" max="10">"#);
    assert!(diags.is_empty(), "{diags:?}");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_fence --lib control_css_check`
Expected: FAIL（Dropdown/NumberField 不在校验集，bare select/number 不报错）。

- [ ] **Step 3: CONTROL_KINDS 加 Dropdown/NumberField**

`control_css_check.rs:32`：
```rust
const CONTROL_KINDS: &[SemanticKind] = &[
    SemanticKind::ProgressBar,
    SemanticKind::Slider,
    SemanticKind::Toggle,
    SemanticKind::RadioButton,
    SemanticKind::TextField,
    SemanticKind::PasswordField,
    SemanticKind::SearchField,
    SemanticKind::TextArea,
    SemanticKind::Dropdown,
    SemanticKind::NumberField,
];
```

- [ ] **Step 4: has_injected_children 加 Dropdown**

`control_css_check.rs:47`：
```rust
fn has_injected_children(semantic: SemanticKind) -> bool {
    matches!(
        semantic,
        SemanticKind::ProgressBar
            | SemanticKind::Slider
            | SemanticKind::Toggle
            | SemanticKind::RadioButton
            | SemanticKind::Dropdown
    )
}
```

- [ ] **Step 5: loom_children_hint 加 Dropdown**

`control_css_check.rs` 的 `loom_children_hint` 函数，在 Toggle/Radio 分支后加：
```rust
        SemanticKind::Dropdown => "`.loom-value` (shows selected text) and `.loom-popup` (the popup list container); `<option>` children also need CSS",
```

- [ ] **Step 6: kind_name + Dropdown 专属教学文案**

`check_control_css` 函数里 `kind_name` match 加：
```rust
            SemanticKind::Dropdown => "dropdown (select)",
            SemanticKind::NumberField => "number field",
```

在 `fix_hint` 计算处，给 Dropdown 单独分支（在 `has_injected_children` 判断内或之前）：
```rust
        let fix_hint = if semantic == SemanticKind::Dropdown {
            format!(
                "Provide CSS for <{tag}> (background/border so the box is visible) and for its \
                 internal `.loom-value` and `.loom-popup` child elements. LoomGUI dropdowns have \
                 NO built-in arrow indicator — if you want one, draw it yourself via CSS (e.g. a \
                 background-image on `.loom-value`, or an extra child element). `<option>` children \
                 also need CSS (they are normal DOM children of <{tag}>)."
            )
        } else if has_injected_children(semantic) {
            // ... 现有注入子节点分支（不变）
        } else {
            // ... 现有文本控件分支（不变）
        };
```

- [ ] **Step 7: 跑测试确认通过**

Run: `cargo test -p loomgui_fence`
Expected: PASS（4 个新测试 + 现有全绿）。

- [ ] **Step 8: Commit**

```bash
git add crates/fence/src/control_css_check.rs
git commit -m "fence: add Dropdown/NumberField to FenceControlWithoutCss with no-arrow teaching"
```

---

### Task 3: packer bridge 提取 Dropdown/NumberField ControlInit

**Files:**
- Modify: `crates/packer/pkg/src/bridge.rs:164`（extract_control_init）
- Test: `crates/packer/pkg/src/bridge.rs` 或 `tests/`

**Interfaces:**
- Produces: `<select>` 的 option[selected] → `ControlInit::Dropdown{selected_index}`；`<input type="number">` 的 min/max/step/value → `ControlInit::NumberField`。

- [ ] **Step 1: 写失败测试**

加 bridge 测试（照现有 Slider arm 测试模板）：

```rust
#[test]
fn dropdown_selected_index_from_option_selected() {
    // <select><option value="a">A</option><option value="b" selected>B</option></select>
    // 期望 ControlInit::Dropdown { selected_index: 1 }
    let html = r#"<select id="s"><option value="a">A</option><option value="b" selected>B</option></select>"#;
    // ... 跑 fence + bridge，断言 template 的 control_init 是 Dropdown{selected_index:1}
}

#[test]
fn dropdown_no_selected_defaults_to_zero() {
    let html = r#"<select id="s"><option value="a">A</option><option value="b">B</option></select>"#;
    // 期望 selected_index: 0
}

#[test]
fn number_field_extracts_min_max_step_value() {
    let html = r#"<input type="number" id="n" value="5" min="0" max="10" step="2">"#;
    // 期望 ControlInit::NumberField { edit: {value:"5",...}, min:0, max:10, step:2 }
}
```

（具体跑 fence+bridge 的 helper 照现有 bridge 测试 fixture。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_pkg bridge`
Expected: FAIL（Dropdown/NumberField arm 返 None，control_init 为空）。

- [ ] **Step 3: 加 Dropdown arm（扫 option 子节点找 selected）**

`bridge.rs` 的 `extract_control_init` match，在 `_ => None` 前加。Dropdown 要遍历子节点找 `IrNodeKind::Element` + tag=="option" + 有 selected 属性：

```rust
        NodeKind::Dropdown => {
            // 扫 select 的 option 子节点，找带 selected 属性的索引；无则默认 0（首项）。
            let mut selected_index: u32 = 0;
            for (i, child_id) in tree.nodes[ir_idx].children.iter().enumerate() {
                if let IrNodeKind::Element(child) = &tree.nodes[child_id.0].kind {
                    if child.tag == "option"
                        && child.attributes.iter().any(|a| a.name == "selected")
                    {
                        selected_index = i as u32;
                        break;
                    }
                }
            }
            Some(ControlInit::Dropdown { selected_index })
        }
        NodeKind::NumberField => {
            let edit = extract_edit_init(el, ir_idx, tree);
            let min = attr(el, "min").and_then(|v| v.parse::<f32>().ok()).unwrap_or(f32::MIN);
            let max = attr(el, "max").and_then(|v| v.parse::<f32>().ok()).unwrap_or(f32::MAX);
            let step = attr(el, "step").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
            edit.map(|e| ControlInit::NumberField { edit: e, min, max, step })
        }
```

（`extract_edit_init` 和 `attr` 是现有 helper，照 TextField arm 用法。如果 TextField arm 内联了 edit 提取，抽成 `extract_edit_init` 复用——见现有代码。）

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p loomgui_pkg`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/packer/pkg/src/bridge.rs
git commit -m "packer: bridge extracts Dropdown selected_index + NumberField constraints"
```

---

### Task 4: core inject_control_children 注入 Dropdown 子树

**Files:**
- Modify: `crates/core/src/scene/control.rs:19`（常量）+ `:163`（inject_control_children）
- Test: `crates/core/src/scene/control.rs`（tests）或 `scene/tests.rs`

**Interfaces:**
- Produces: Dropdown 实例化时注入 `.loom-value` + `.loom-popup` 子节点（position:absolute 锚定 select）。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn inject_dropdown_children_creates_value_and_popup() {
    let mut scene = test_scene();
    let sel = create_node(&mut scene, "select", "").unwrap();
    // 模拟 instantiate：先把 select 加入 controls 表为 Dropdown 状态
    scene.controls.ensure(sel, ControlState::Dropdown { selected_index: 0, open: false, value_lock: false });
    inject_control_children(&mut scene, sel, NodeKind::Dropdown);
    let value = find_child_by_class(&scene, sel, "loom-value").expect("loom-value injected");
    let popup = find_child_by_class(&scene, sel, "loom-popup").expect("loom-popup injected");
    assert!(scene.get(value).unwrap().classes.iter().any(|c| c == "loom-value"));
    assert!(scene.get(popup).unwrap().classes.iter().any(|c| c == "loom-popup"));
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core inject_dropdown`
Expected: FAIL（inject_control_children 的 Dropdown 走 `_ => {}` 空分支）。

- [ ] **Step 3: 加常量 + Dropdown 分支**

`control.rs:19` 常量区，在 `CHECK` 后加：
```rust
const VALUE: &str = "loom-value";
const POPUP: &str = "loom-popup";
```

`control.rs:163` `inject_control_children` 的 match，在 `_ => {}` 前加：
```rust
        NodeKind::Dropdown => {
            // select 设 position:relative 作 absolute containing block（同 Slider 模式）。
            let _ = set_inline_override(scene, id, "position:relative");
            let value = make_child(scene, VALUE);
            append_child(scene, id, value).expect("fresh child has no parent");
            let popup = make_child(scene, POPUP);
            append_child(scene, id, popup).expect("fresh child has no parent");
            // popup 默认收起（display:none），展开由 sync_control_visuals 移除。
            let _ = set_inline_override(scene, popup, "display:none;position:absolute");
        }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p loomgui_core inject_dropdown`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/scene/control.rs
git commit -m "core: inject .loom-value/.loom-popup for Dropdown"
```

---

### Task 5: core sync_control_visuals 加 Dropdown 分支

**Files:**
- Modify: `crates/core/src/scene/control.rs:283`（sync_control_visuals）
- Test: `crates/core/src/scene/control.rs`

**Interfaces:**
- Produces: `selected_index` → `.loom-value` 显示对应 option 文本；`open` → `.loom-popup` display 切换。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn sync_dropdown_shows_selected_option_text_in_value() {
    let mut scene = test_scene();
    let sel = create_dropdown_with_options(&mut scene, &["A", "B", "C"], 1); // selected=B
    sync_control_visuals(&mut scene, sel);
    let value = find_child_by_class(&scene, sel, "loom-value").unwrap();
    // .loom-value 的文本内容应是 "B"（选中项）。验证方式照 sync_progress 测 fill width 的模式。
    // （具体读 scene.text_contents[value] 或等价查询）
}

#[test]
fn sync_dropdown_open_toggles_popup_display() {
    let mut scene = test_scene();
    let sel = create_dropdown_with_options(&mut scene, &["A"], 0);
    // open=true
    scene.controls.get_mut(sel).unwrap(); // 设 open=true
    sync_control_visuals(&mut scene, sel);
    let popup = find_child_by_class(&scene, sel, "loom-popup").unwrap();
    // 验证 popup 的 inline override 不含 display:none（展开）
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core sync_dropdown`
Expected: FAIL。

- [ ] **Step 3: 加 Dropdown 分支到 sync_control_visuals**

`control.rs:283` 的 match，加：
```rust
        ControlState::Dropdown { selected_index, open, .. } => {
            // popup display 切换
            if let Some(popup) = find_child_by_class(scene, id, POPUP) {
                let decl = if open { "display:flex" } else { "display:none" };
                let _ = set_inline_override(scene, popup, decl);
            }
            // value 显示选中 option 的文本：option 是 select 的 DOM 子节点，按 class/tag 找。
            // 读第 selected_index 个 option 子节点的 text content 写进 .loom-value。
            if let Some(value) = find_child_by_class(scene, id, VALUE) {
                let option_text = nth_option_text(scene, id, selected_index).unwrap_or_default();
                set_node_text(scene, value, &option_text);
            }
        }
```

`nth_option_text` 是新 helper（读 select 的第 n 个 option 子节点的 text content）。`set_node_text` 走现有文本节点内容设置（搜 `scene.text_contents`）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p loomgui_core sync_dropdown`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/scene/control.rs
git commit -m "core: sync_control_visuals Dropdown — value text + popup display toggle"
```

---

### Task 6: FFI 导出 — 文本控件 getter + blur + Dropdown 读写 + Number 读写

**Files:**
- Modify: `crates/ffi/src/lib.rs`
- Test: `crates/ffi/src/lib.rs`（tests）或 core 集成测试

**Interfaces:**
- Produces: `loomgui_stage_get_node_disabled` / `get_control_readonly` / `blur` / `get/set_dropdown_selected_index` / `get/set_dropdown_open` / `get/set_number_value`。set_transform 扩展（Task 7）。

- [ ] **Step 1: 写 get_node_disabled 失败测试**

照 `loomgui_stage_get_node_visible`（lib.rs:811）模式，加 FFI 测试：
```rust
#[test]
fn ffi_get_node_disabled_reads_flag() {
    let mut sh = test_stage_handle();
    let id = create_node(&mut sh.stage, "button", "").unwrap();
    set_node_disabled(&mut sh.stage.scene, id, true);
    let mut out: u8 = 0;
    loomgui_stage_get_node_disabled(sh.as_ptr(), id.0, &mut out);
    assert_eq!(out, 1);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_ffi get_node_disabled`
Expected: FAIL（函数不存在）。

- [ ] **Step 3: 加 get_node_disabled / get_control_readonly / blur FFI**

`lib.rs` 照 `loomgui_stage_get_node_visible`（:811）模式：
```rust
/// 读节点 disabled 伪类态（NodeFlags::DISABLED）。非 live 节点返 0。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_disabled(h: *const StageHandle, node_id: u32, out: *mut u8) {
    if h.is_null() { return; }
    let sh = unsafe { &*h };
    if let Some(scene) = sh.stage.scene.as_ref() {
        if let Some(n) = scene.get(NodeId(node_id)) {
            unsafe { *out = if n.interaction.flags.contains(NodeFlags::DISABLED) { 1 } else { 0 }; }
        }
    }
}

/// 读 TextField/TextArea/NumberField 的 readonly（EditState.readonly）。非文本控件返 0。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_control_readonly(h: *const StageHandle, node_id: u32, out: *mut u8) -> i32 {
    if h.is_null() { return -1; }
    let sh = unsafe { &*h };
    if let Some(scene) = sh.stage.scene.as_ref() {
        if let Some(ControlState::TextField(e) | ControlState::TextArea(e) | ControlState::NumberField { edit: e, .. }) = scene.controls.get(NodeId(node_id)) {
            unsafe { *out = if e.readonly { 1 } else { 0 }; }
            return 0;
        }
    }
    -1
}

/// 清除当前 focus（Stage::blur 的 FFI 包装）。
#[no_mangle]
pub extern "C" fn loomgui_stage_blur(h: *mut StageHandle) -> i32 {
    if h.is_null() { return -1; }
    let sh = unsafe { &mut *h };
    sh.stage.blur();
    0
}
```

- [ ] **Step 4: 加 Dropdown / NumberField 读写 FFI**

```rust
/// 读 Dropdown 当前选中项索引。非 Dropdown 返 -1。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_dropdown_selected_index(h: *const StageHandle, node_id: u32, out: *mut u32) -> i32 {
    if h.is_null() { return -1; }
    let sh = unsafe { &*h };
    if let Some(scene) = sh.stage.scene.as_ref() {
        if let Some(ControlState::Dropdown { selected_index, .. }) = scene.controls.get(NodeId(node_id)) {
            unsafe { *out = *selected_index as u32; }
            return 0;
        }
    }
    -1
}

/// 设 Dropdown 选中项（触发 SelectionChanged 事件 + value_lock 防反馈环）。
#[no_mangle]
pub extern "C" fn loomgui_stage_set_dropdown_selected_index(h: *mut StageHandle, node_id: u32, index: u32) -> i32 {
    if h.is_null() { return -1; }
    let sh = unsafe { &mut *h };
    if let Some(scene) = sh.stage.scene.as_mut() {
        if let Some(ControlState::Dropdown { selected_index, value_lock, .. }) = scene.controls.get_mut(NodeId(node_id)) {
            *selected_index = index as usize;
            *value_lock = true; // 防本轮 cascade 回写
            // 事件发射在 tick（EVT_SELECTION_CHANGED）——照 ValueChanged 模式
            return 0;
        }
    }
    -1
}

/// 读/写 Dropdown open 状态、NumberField value —— 同模式（get out-param bool/u32，set 直写）。
```
（完整实现含 get/set_dropdown_open、get/set_number_value，照上面模式；NumberField 的 set 要做 clamp+量化。）

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p loomgui_ffi`
Expected: PASS。

- [ ] **Step 6: 重编 dll + sync bindings**

```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
cargo run -p xtask -- sync-bindings
```

- [ ] **Step 7: Commit**

```bash
git add crates/ffi/src/lib.rs unity/package/Plugins/LoomGUI/
git commit -m "ffi: export get_node_disabled/get_control_readonly/blur + Dropdown/NumberField getters"
```

---

### Task 7: set_transform FFI 加 origin 参数

**Files:**
- Modify: `crates/ffi/src/lib.rs:2282`（loomgui_stage_set_transform）+ `crates/core/src/scene/dynamic.rs:412`（set_user_transform）
- Modify: `crates/core/src/scene/node.rs`（NodeTransform struct 加 origin？——查现状，可能已有）
- Test: `crates/ffi/src/lib.rs`

**Interfaces:**
- Produces: `loomgui_stage_set_transform(h, node_id, tx, ty, sx, sy, rot, ox, oy)` —— C# NodeTransform.Origin 接通。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn ffi_set_transform_with_origin() {
    let mut sh = test_stage_handle();
    let id = create_node(&mut sh.stage, "div", "").unwrap();
    let rc = loomgui_stage_set_transform(sh.as_mut_ptr(), id.0, 10.0, 0.0, 1.0, 1.0, 0.0, 5.0, 5.0);
    assert_eq!(rc, 0);
    // 验证 user_transform.translate=[10,0] origin=[5,5]（或 core 侧已应用 origin 偏移）
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_ffi set_transform_with_origin`
Expected: FAIL（签名不匹配，无 ox/oy 参数）。

- [ ] **Step 3: 扩展 FFI 签名 + core set_user_transform**

`lib.rs:2282` 加 ox/oy 参数：
```rust
#[no_mangle]
pub extern "C" fn loomgui_stage_set_transform(
    h: *mut StageHandle, node_id: u32,
    tx: f32, ty: f32, sx: f32, sy: f32, rot: f32,
    ox: f32, oy: f32,
) -> i32 {
    // ... 构造 NodeTransform { translate:[tx,ty], scale:[sx,sy], rotation:rot, origin:[ox,oy] }
    // ... 调 set_user_transform（dynamic.rs，查它是否已接收 origin；若 NodeTransform 已有 origin 字段则零改）
}
```

查 `NodeTransform` struct（`scene/node.rs`）是否已有 `origin` 字段。若有，FFI 直接透传；若无，加 `pub origin: [f32; 2]` 字段 + `set_user_transform` 应用 origin（translate/scale/rotate 前先减 origin）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p loomgui_ffi`
Expected: PASS。

- [ ] **Step 5: 重编 dll + sync bindings + Commit**

```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
cargo run -p xtask -- sync-bindings
git add crates/ffi/src/lib.rs crates/core/src/scene/ unity/package/Plugins/LoomGUI/
git commit -m "ffi: set_transform adds origin (ox,oy) params"
```

---

### Task 8: C# UIStyleException + 文本控件 getter + 投影类

**Files:**
- Modify: `unity/package/Runtime/Public/LoomGUI.Types.cs`（UIStyleException）
- Modify: `unity/package/Runtime/Public/LoomGUI.Nodes.cs`（TextField/TextArea ReadOnly/Disabled getter + Blur；NumberField 填实；OptionItem/Slot/CustomElement class）
- Modify: `unity/package/Runtime/Projection/NodeFactory.cs:65-67`（OptionItem/Slot/CustomElement dispatch）
- Test: `tests/dotnet/LoomGUI.HeadlessTests/`

**Interfaces:**
- Produces: 文本控件 ReadOnly/Disabled getter 读 FFI；UIStyleException 类；OptionItem/Slot/CustomElement public class。

- [ ] **Step 1: 写 UIStyleException + getter 失败测试**

```csharp
[Fact]
public void TextField_ReadOnly_Getter_Reads_Core() {
    var root = _fixture.Root;
    var tf = root.Get<TextField>("tf");
    tf.ReadOnly = true;   // setter 已工作
    Assert.True(tf.ReadOnly);  // getter 改读 FFI 后应通过
}

[Fact]
public void Node_Disabled_Getter_Reads_Core() {
    var btn = _fixture.Root.Get<Button>("btn");
    btn.Disabled = true;
    Assert.True(btn.Disabled);
}

[Fact]
public void OptionItem_Dispatches_To_OptionItem_Class() {
    // 实例化含 <option> 的 pkg，Get<OptionItem> 应返回 OptionItem 实例（非 Container）
    var opt = _fixture.Root.Get<Dropdown>("sel").Get<OptionItem>(0);
    Assert.IsType<OptionItem>(opt);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `dotnet test tests/dotnet/LoomGUI.HeadlessTests`
Expected: FAIL（getter throw NE；OptionItem 类不存在）。

- [ ] **Step 3: 加 UIStyleException**

`LoomGUI.Types.cs` 照 `UIContractException` 模板：
```csharp
public class UIStyleException : Exception
{
    public UIStyleException() { }
    public UIStyleException(string message) : base(message) { }
    public UIStyleException(string message, Exception inner) : base(message, inner) { }
}
```

- [ ] **Step 4: 文本控件 getter 改读 FFI**

`LoomGUI.Nodes.cs` TextField 的 ReadOnly getter（:1407 附近）：
```csharp
public bool ReadOnly {
    get { ThrowIfDisposed(); return GetControlReadonly(); }
    set { ThrowIfDisposed(); SetControlReadonly(value); }
}
public bool Disabled {
    set { ThrowIfDisposed(); SetNodeDisabled(value); }
    get { ThrowIfDisposed(); return GetNodeDisabled(); }
}
```
加 helper `GetControlReadonly()` / `GetNodeDisabled()`（P/Invoke `loomgui_stage_get_control_readonly` / `get_node_disabled`）。同理 TextArea、Slider/Toggle/RadioButton/NumberField 的 Disabled getter。TextField/TextArea 加 `Blur()` 方法（调 `loomgui_stage_blur`）。

- [ ] **Step 5: 加 OptionItem/Slot/CustomElement class + NodeFactory dispatch**

`LoomGUI.Nodes.cs` 加：
```csharp
public class OptionItem : Container {
    internal OptionItem(UIContext ctx, uint id) : base(ctx, id) { }
    public string Value { get { ThrowIfDisposed(); return GetAttr("value") ?? ""; } }
    public bool Selected { get; }
    public bool Disabled { get; }
}
public class Slot : Container { internal Slot(UIContext ctx, uint id) : base(ctx, id) { } }
public class CustomElement : Container { internal CustomElement(UIContext ctx, uint id) : base(ctx, id) { } }
```
`NodeFactory.cs:65-67` 改：
```csharp
NodeKind.OptionItem    => new OptionItem(ctx, id),
NodeKind.Slot          => new Slot(ctx, id),
NodeKind.CustomElement => new CustomElement(ctx, id),
```

- [ ] **Step 6: 跑测试确认通过**

Run: `dotnet test tests/dotnet/LoomGUI.HeadlessTests`
Expected: PASS。

- [ ] **Step 7: Commit**

```bash
git add unity/package/Runtime/Public/ unity/package/Runtime/Projection/ tests/dotnet/
git commit -m "c#: UIStyleException + TextField getter FFI + OptionItem/Slot/CustomElement classes"
```

---

### Task 9: C# 攒批 flush（StyleMirror + NodeTransform + LoomHost seam）

**Files:**
- Modify: `unity/package/Runtime/Projection/StyleMirror.cs:57`（Set 标脏）
- Modify: `unity/package/Runtime/Public/LoomGUI.Nodes.cs:652`（NodeTransform.Store 接通 FFI）
- Modify: `unity/package/Runtime/Host/LoomHost.cs:98`（帧末 flush）
- Test: `tests/dotnet/LoomGUI.HeadlessTests/`

**Interfaces:**
- Produces: setter 只标脏；LoomHost.Step tick 前 flush 所有脏 StyleMirror + NodeTransform。

- [ ] **Step 1: 写失败测试**

```csharp
[Fact]
public void StyleMirror_Set_Batches_Until_Frame_Flush() {
    // 连续 Set 3 次，断言只触发一次 FFI set_inline_override（用 counter/spy）
}
[Fact]
public void NodeTransform_Store_Flushes_To_FFI() {
    var n = _fixture.Root.Get<Node>("n");
    n.Transform.Position = (10, 20);
    _fixture.Host.Step(0.016f);  // 触发帧末 flush
    // 断言 core 侧 user_transform.translate == [10,20]（经 FFI 查询或下一帧 geometry）
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `dotnet test`
Expected: FAIL（StyleMirror 立即 flush；NodeTransform 不调 FFI）。

- [ ] **Step 3: StyleMirror 标脏改造**

`StyleMirror.cs`：加 `internal bool _dirty;` 字段。`Set`（:60）删 `FlushInline()` 改 `_dirty = true;`。`Unset` 同理。`FlushInline` 改 `public`（供 LoomHost 调）并在末尾 `_dirty = false;`。加 `internal bool IsDirty => _dirty;`。

- [ ] **Step 4: NodeTransform.Store 接通 FFI**

`LoomGUI.Nodes.cs:690` `Store`：标脏 + 注册到 NodeRegistry 的 dirty transform 集。
```csharp
void Store<T>(ref T field, T value) {
    field = value; _dirty = true;
    _ctx._registry.MarkTransformDirty(this);  // NodeRegistry 持 dirty 集合
}
```
加 `internal void FlushTransform()`：调 `loomgui_stage_set_transform(h, id, pos.x, pos.y, scale.x, scale.y, rot, origin.x, origin.y)` + `_dirty=false`。

- [ ] **Step 5: LoomHost.Step 帧末 flush**

`LoomHost.cs:98`（现有 flush seam 占位注释处）改为：
```csharp
// 帧末 flush：攒批回写（StyleMirror + NodeTransform）。
_ctx._registry.FlushDirtyStyles(_stage);   // 扫所有 dirty StyleMirror 调 FlushInline
_ctx._registry.FlushDirtyTransforms(_stage); // 扫所有 dirty NodeTransform 调 FlushTransform
```
`NodeRegistry` 加 dirty 集合（`HashSet<NodeStyle>` / `HashSet<Node>`）+ `MarkStyleDirty`/`MarkTransformDirty` + Flush 方法。

- [ ] **Step 6: 跑测试确认通过**

Run: `dotnet test tests/dotnet/LoomGUI.HeadlessTests`
Expected: PASS。

- [ ] **Step 7: Commit**

```bash
git add unity/package/Runtime/ tests/dotnet/
git commit -m "c#: batch StyleMirror/NodeTransform flush via LoomHost frame seam"
```

---

### Task 10: C# NumberField 投影填实

**Files:**
- Modify: `unity/package/Runtime/Public/LoomGUI.Nodes.cs:1651`（NumberField class）
- Test: `tests/dotnet/LoomGUI.HeadlessTests/`

**Interfaces:**
- Produces: NumberField Value/Min/Max/Step/Disabled 读 FFI；ValueChanged 事件。

- [ ] **Step 1: 写失败测试**

```csharp
[Fact]
public void NumberField_Value_Clamps_To_MinMax() {
    var nf = _fixture.Root.Get<NumberField>("nf");  // min=0 max=10 step=2
    nf.Value = 15f;
    Assert.Equal(10f, nf.Value, 0.01f);  // clamp 到 max
}
[Fact]
public void NumberField_Value_Quantizes_To_Step() {
    var nf = _fixture.Root.Get<NumberField>("nf");  // step=2
    nf.Value = 3f;
    Assert.Equal(2f, nf.Value, 0.01f);  // 量化到 2（或 4，看 round 策略）
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `dotnet test`
Expected: FAIL（NumberField.Value throw NE）。

- [ ] **Step 3: NumberField 填实（照 Slider 模板）**

`LoomGUI.Nodes.cs:1651` NumberField class：
```csharp
public float Value {
    get { ThrowIfDisposed(); return GetNumberValue(); }
    set { ThrowIfDisposed(); SetNumberValue(value); }  // FFI 侧 clamp+量化
}
public float Min { get; }      // 从 ControlInit 烘焙，C# 读 core 查询
public float Max { get; }
public float Step { get; }
public bool Disabled { set { ThrowIfDisposed(); SetNodeDisabled(value); } get { ThrowIfDisposed(); return GetNodeDisabled(); } }
public bool ReadOnly { get { ThrowIfDisposed(); return GetControlReadonly(); } set { ThrowIfDisposed(); SetControlReadonly(value); } }
// ValueChanged backing-dict（照 Slider :1714 模式，订阅 ControlValueChangedEvent）
```
`GetNumberValue`/`SetNumberValue` P/Invoke Task 6 的 FFI。

- [ ] **Step 4: 跑测试确认通过 + Commit**

```bash
dotnet test tests/dotnet/LoomGUI.HeadlessTests
git add unity/package/Runtime/Public/LoomGUI.Nodes.cs tests/dotnet/
git commit -m "c#: NumberField projection — Value/Min/Max/Step + ValueChanged"
```

---

## Part B：Dropdown 全栈（浮层基建）

### Task 11: 浮层渲染 — build_render_nodes 末尾追加 open popup 子树

**Files:**
- Modify: `crates/core/src/render/mod.rs:1011`（max_sort 后追加 popup DFS）
- Modify: `crates/core/src/render/batch.rs`（新 `assign_popup_sort_keys` helper）
- Test: `crates/core/src/render/tests.rs`

**Interfaces:**
- Produces: open Dropdown 的 `.loom-popup` 子树在正常 DFS 跳过、末尾追加渲染（sort_key > max_sort，mask_context=0 跳出祖先 clip）。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn open_popup_renders_above_all_with_no_clip() {
    let mut scene = test_scene();
    // select 在 overflow:hidden 容器里，open=true
    let outer = create_node(&mut scene, "div", "overflow:hidden;width:100;height:100");
    let sel = create_dropdown(&mut scene, &[("A",false),("B",false)], 0, true /*open*/);
    append_child(&mut scene, outer, sel);
    // ... build_render_nodes
    let (frame, _, _) = build_render_nodes(&scene, &fonts, &prev, &sizes, &mut atlas);
    // 断言：popup 子树（option A/B）的 sort_key > outer/select/value 的 sort_key
    // 断言：popup 节点 mask_context == MaskContext(0)（跳出 outer 的 overflow:hidden clip）
    let popup_rns: Vec<_> = frame.nodes.iter().filter(|rn| /* 是 popup 子树 */).collect();
    assert!(popup_rns.iter().all(|rn| rn.sort_key > max_normal_sort));
    assert!(popup_rns.iter().all(|rn| rn.mask_context == MaskContext(0)));
}

#[test]
fn closed_popup_not_rendered() {
    // open=false 时 popup 子树 display:none，不进 RenderNode（display:none override 已 pruned）
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core open_popup`
Expected: FAIL（popup 无特殊处理，走正常 DFS 被 outer clip 裁）。

- [ ] **Step 3: 正常 DFS 跳过 open popup 子树**

`build_render_nodes` 的主遍历（产 RenderNode + id_to_pos）处，对 open Dropdown 的 `.loom-popup` 子节点跳过（不进 id_to_pos，像 pruned display:none）：
```rust
// 在主遍历决定是否收录某节点时，加判断：
// 若该节点是 open Dropdown 的 .loom-popup → skip（不 push RenderNode，不进 id_to_pos）
```
（`assign_sort_keys` 的 dfs 已对 `!id_to_pos.contains_key(&id)` 早返——所以只要 popup 不进 id_to_pos，主 DFS 自然跳过它。）

- [ ] **Step 4: 末尾追加 popup 子树 DFS**

`render/mod.rs` scrollbar thumb 追加（:1014）**之后**，加 popup 追加：
```rust
    // open Dropdown 的 popup 子树：跳出正常 DFS，末尾追加（sort_key 续 max_sort，mask=0 跳出祖先 clip）。
    // 模式同 scrollbar thumb（:1014），但追加的是子树 DFS 而非单 quad。
    let mut popup_counter = max_sort + 1;
    for n in scene.nodes.values() {
        if let Some(ControlState::Dropdown { open: true, .. }) = scene.controls.get(n.id) {
            if let Some(popup) = find_child_by_class(scene, n.id, "loom-popup-const") {
                // 注：常量 POPUP 在 control.rs，render mod 需 import 或用字面量
                batch::assign_popup_sort_keys(
                    scene, &mut nodes, &mut id_to_pos_popup,
                    &mut sort_keys, popup, &mut popup_counter,
                );
            }
        }
    }
```

- [ ] **Step 5: 新增 assign_popup_sort_keys helper**

`batch.rs` 新函数（复用 `assign_sort_keys` 的 dfs 逻辑，但强制 `parent_mask=MaskContext(0)` + `accumulated=None` + 起始 counter 续号）：
```rust
/// 对 open popup 子树跑 DFS，sort_key 从 start_counter 续，mask_context 强制 0（跳出祖先 clip）。
/// 产出的 RenderNode 已在 nodes vec 里（由调用方后续 merge 或直 push）。
pub fn assign_popup_sort_keys(
    scene: &Scene,
    nodes: &mut Vec<RenderNode>,
    id_to_pos: &mut HashMap<NodeId, usize>,
    sort_keys: &mut [u32],
    root: NodeId,
    counter: &mut u32,
) {
    // 内部 dfs，parent_mask=MaskContext(0), accumulated=None（不继承祖先 clip）
    // 每个 popup 子树节点：push RenderNode（含几何/文本，照 build_render_nodes 单节点逻辑）
    //   + sort_key=*counter + mask_context=MaskContext(0) + *counter+=1
}
```
（注意：popup 子树节点也要产完整 RenderNode——几何、文本、图片。这里复用 `build_render_nodes` 单节点的填充逻辑，可能需要把单节点填充抽成 `fn fill_render_node(scene, id) -> RenderNode` 供主遍历和 popup 遍历共用。）

- [ ] **Step 6: 跑测试确认通过**

Run: `cargo test -p loomgui_core open_popup`
Expected: PASS。

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/render/
git commit -m "core: render open popup subtree after main DFS (scrollbar-thumb pattern, mask=0)"
```

---

### Task 12: 命中层 — hit_test 前置 popup check

**Files:**
- Modify: `crates/core/src/hit.rs:48`（hit_test 前置 popup 命中）
- Test: `crates/core/src/hit.rs`

**Interfaces:**
- Produces: open 时 popup 区域优先命中；popup 外点击 → outside-click 关闭信号。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn hit_inside_open_popup_returns_option() {
    // open dropdown，点击落在某 option 的 layout_rect 内 → 返回该 option NodeId
}
#[test]
fn hit_outside_open_popup_signals_close() {
    // open dropdown，点击落在 popup 外 → 返回特殊标记或由调用方判定关闭
    // （hit_test 本身只返 NodeId；outside-click 关闭逻辑在 input.rs 的点击处理里：若点中的不是 popup/select 子树 → 关闭）
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core hit_inside_open_popup`
Expected: FAIL。

- [ ] **Step 3: hit_test 前置 popup check**

`hit.rs:48` `hit_test`，在 `hit_scrollbar_grip` 后、主 roots DFS 前，加：
```rust
    // open popup 命中前置（顶层优先，同 scrollbar grip 模式）。
    // popup 子树是真 DOM 节点（option/.loom-popup 有真 NodeId + layout_rect），
    // 复用 hit_subtree——只是要在主 roots DFS 之前测，保证 popup 顶层命中。
    for n in scene.nodes.values() {
        if let Some(ControlState::Dropdown { open: true, .. }) = scene.controls.get(n.id) {
            if let Some(popup) = find_child_by_class(scene, n.id, "loom-popup") {
                if let Some(hit) = hit_subtree(scene, popup, point) {
                    return Some(hit);
                }
            }
        }
    }
```
outside-click 关闭：在 input.rs 的点击事件处理里，若 `hit_test` 返回的节点**不是**任何 open dropdown 的 select/popup 子树 → 触发该 dropdown close。这逻辑放 input.rs 点击路由（不在 hit_test 内）。

- [ ] **Step 4: outside-click 关闭逻辑（input.rs）**

`crates/core/src/input.rs` 点击事件处理：click 事件落地后，遍历 open Dropdown，若 click 目标不在该 dropdown 的 select 子树 → 设 open=false + 发 EVT（或直接在 stage tick 处理）。

- [ ] **Step 5: 跑测试确认通过 + Commit**

```bash
cargo test -p loomgui_core hit_
git add crates/core/src/hit.rs crates/core/src/input.rs
git commit -m "core: hit_test popup front-check + outside-click close"
```

---

### Task 13: Dropdown 交互 — 点击/键盘/选中/事件

**Files:**
- Modify: `crates/core/src/input.rs`（点击 select toggle open / 点 option 选中 / 键盘 Up/Down/Enter/Esc）
- Modify: `crates/core/src/scene/control.rs` 或 stage.rs（SelectionChanged 事件发射）
- Modify: `crates/core/src/scene/node.rs`（EventType 加 SelectionChanged，如未有）
- Test: `crates/core/src/input.rs` 或 `scene/tests.rs`

**Interfaces:**
- Produces: Dropdown 完整交互闭环；EVT_SELECTION_CHANGED 事件。

- [ ] **Step 1: 写失败测试（交互场景）**

```rust
#[test]
fn click_select_toggles_open() { /* 收起→点 select→open=true */ }
#[test]
fn click_option_selects_and_closes() { /* open→点 option B→selected_index=1, open=false, 事件发出 */ }
#[test]
fn escape_closes_and_reverts() { /* open, 改高亮到 B, Esc→open=false, selected_index 回到打开时值 */ }
#[test]
fn arrow_down_seeks_non_disabled_option() { /* 跳过 disabled option */ }
```

- [ ] **Step 2-4: 实现交互（照 RmlUi WidgetDropDown 语义）**

`input.rs` 点击/键盘路由加 Dropdown 分支。关键：
- click select（收起）→ open=true，记 `open_selected_index = selected_index`（供 Esc 回滚）。
- click option → selected_index=idx + value_lock=true + 发 EVT_SELECTION_CHANGED(idx) + open=false。
- Up/Down（open）→ seek 选中超前一/后一个非 disabled option（高亮，不提交）。
- Enter（open）→ 提交当前高亮 + close。
- Esc（open）→ selected_index 回滚 open_selected_index + open=false。

事件：`EventType` 加 `SelectionChanged`（值=新 index），照 `ValueChanged`（:22）模式。

- [ ] **Step 5: 跑测试确认通过 + Commit**

```bash
cargo test -p loomgui_core dropdown_
git add crates/core/src/input.rs crates/core/src/scene/ crates/core/src/stage.rs
git commit -m "core: Dropdown interaction — click/keyboard/selection-changed event"
```

---

### Task 14: C# Dropdown 投影 + SelectionChanged 事件

**Files:**
- Modify: `unity/package/Runtime/Public/LoomGUI.Nodes.cs:1990`（Dropdown class）
- Modify: `unity/package/Runtime/Public/LoomGUI.Events.cs`（SelectionChangedEvent）
- Modify: `unity/package/Runtime/Public/LoomGUI.EventType.cs`（加 SelectionChanged）
- Modify: `unity/package/Runtime/Projection/EventDemuxer.cs`（SelectionChanged 分支）
- Test: `tests/dotnet/LoomGUI.HeadlessTests/`

**Interfaces:**
- Produces: `Dropdown.SelectedIndex`/`SelectedValue`/`Disabled` + `SelectionChanged` typed event。

- [ ] **Step 1: 写失败测试**

```csharp
[Fact]
public void Dropdown_SelectedIndexChanged_Round_Trip() {
    var sel = _fixture.Root.Get<Dropdown>("sel");
    sel.SelectedIndex = 2;
    Assert.Equal(2, sel.SelectedIndex);
}
[Fact]
public void Dropdown_SelectionChanged_Event_Fires() {
    var sel = _fixture.Root.Get<Dropdown>("sel");
    int received = -1;
    sel.SelectionChanged += e => received = e.NewIndex;
    // 注入 SelectionChanged 原始事件
    var buf = new NativeEventBuffer();
    buf.Add(sel._id, (byte)EventType.SelectionChanged, x: 2f);
    _fixture.Context._eventDemuxer.Pump(buf.Ptr, buf.Count);
    Assert.Equal(2, received);
}
```

- [ ] **Step 2-4: 实现 Dropdown + 事件管线**

- `EventType.cs` 加 `SelectionChanged = <next>`。
- `Events.cs` 加 `SelectionChangedEvent { public int NewIndex; public int OldIndex; }`（字段从 raw event 填）。
- `EventDemuxer.cs` 加分支：`case EventType.SelectionChanged: dispatch SelectionChangedEvent { NewIndex = (int)raw.x }`。
- `Nodes.cs` Dropdown：SelectedIndex get/set 调 Task 6 FFI；SelectionChanged backing-dict（照 Slider ValueChanged :1714 模式，订阅 typed event）。

- [ ] **Step 5: 跑测试确认通过 + Commit**

```bash
dotnet test tests/dotnet/LoomGUI.HeadlessTests
git add unity/package/Runtime/ tests/dotnet/
git commit -m "c#: Dropdown projection + SelectionChanged typed event"
```

---

### Task 15: NumberField 字符输入 guard（filter 非数字）

**Files:**
- Modify: `crates/core/src/input.rs`（字符输入路由加 NumberField guard）
- Test: `crates/core/src/input.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn number_field_rejects_non_digit_input() {
    // NumberField 收 'a' → value 不变；收 '5' → value 追加 '5'
}
#[test]
fn number_field_accepts_minus_dot_e() {
    // 收 '-'/'.'/'e' 在合法位置 → 接受
}
```

- [ ] **Step 2-3: guard 实现**

`input.rs` 字符输入路由：若目标 focused 节点是 NumberField，filter 字符（允许 0-9 / `-` / `.` / `e` / `E`；IME composition 期间不 filter，commit 时校验）。

- [ ] **Step 4: 跑测试 + Commit**

```bash
cargo test -p loomgui_core number_field_rejects
git add crates/core/src/input.rs
git commit -m "core: NumberField input guard — filter non-numeric chars"
```

---

### Task 16: 重打 showcase pkg + 整包验收

**Files:**
- 无源码改动；重打 pkg + 跑全测试。

- [ ] **Step 1: 重编 release dll**

```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
cargo run -p xtask -- sync-bindings
```

- [ ] **Step 2: 重打 showcase pkg**

```bash
cargo run -p loomgui_pkg -- build showcase
```
Expected: exit 0（showcase 的 select/number 不再是空行为，围栏校验通过）。

- [ ] **Step 3: 全 workspace 测试**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
dotnet test tests/dotnet/LoomGUI.HeadlessTests
dotnet build tests/dotnet/LoomGUI.PublicApi  # 公共 API 编译门
```
Expected: 全绿。

- [ ] **Step 4: Commit**

```bash
git add unity/package/ showcase/showcase.pkg.bin  # 若 showcase.pkg.bin 入库
git commit -m "chore: rebuild dll + showcase pkg v26, full test suite green"
```

---

### Task 17: Unity 真机验收（家用机）

**Files:** 无源码改动；PlayMode 验收。

- [ ] **Step 1: 建 Dropdown 验收页**

`showcase/spec4b/dropdown-acceptance.html`：最小 select + option + CSS（含箭头自绘，验教学文案有效）+ NumberField。

- [ ] **Step 2: 打 pkg + Unity PlayMode 跑**

四门：① 渲染（select 收起显示选中项 + 展开浮层溢出父 overflow:hidden 正确）；② SelectedIndex 读写 + SelectionChanged 事件；③ 键盘 Up/Down/Enter/Esc；④ NumberField 输入 + clamp。

- [ ] **Step 3: 记录结果到 roadmap / pitfalls**

视觉验收结果回写 roadmap tech-debt（若有遗留）。

---

## Self-Review

**Spec coverage:**
- §3.1 NumberField → Task 1（变体）+ 3（bridge）+ 6（FFI）+ 10（C#）+ 15（输入 guard）。✓
- §3.2 文本控件 getter + Blur → Task 6（FFI）+ 8（C#）。✓
- §3.3 投影类 OptionItem/Slot/CustomElement → Task 8。✓
- §3.4 UIStyleException → Task 8。✓
- §3.5 攒批 flush + set_transform → Task 7（FFI origin）+ 9（C# 攒批）。✓
- §4.1 子树注入 → Task 4。✓
- §4.2 浮层渲染 → Task 11。✓
- §4.3 命中 → Task 12。✓
- §4.4 选中态 → Task 1（ControlState）+ 5（sync）+ 3（bridge selected_index）。✓
- §4.5 交互 → Task 13。✓
- §4.6 不注入箭头 → Task 2（教学文案）。✓
- §4.7 pkg v26 → Task 1。✓
- §5 围栏 → Task 2。✓
- §6 数据流 → Task 1+3+4+5+11+13 端到端覆盖。✓

**Placeholder scan:** Task 5 的 `nth_option_text`/`set_node_text`、Task 6 的 Number 读写、Task 11 的 `fill_render_node` 抽取——这些是"照现有 helper 模式"的指引，不是占位符（实现者按现有代码模式写）。其余步骤均有具体代码。

**Type consistency:** `ControlState::Dropdown { selected_index: usize }`（Task 1）vs FFI `get_dropdown_selected_index` out-param `*mut u32`（Task 6）——usize→u32 cast 已在 Task 6 Step 3 标注（`*selected_index as u32`）。`ControlInit::Dropdown { selected_index: u32 }`（pkg）vs ControlState usize（runtime）——Task 1 Step 5 标注 `as usize` cast。一致。

**Scope:** 单一子系统（控件束 P3），依赖链线性（ControlInit→bridge→inject→render→hit→interaction→C#），适合单 plan。pkg v26 一次 bump 覆盖全部。
