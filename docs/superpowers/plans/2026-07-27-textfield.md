# TextField 全家（text/password/search + TextArea）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 TextField/PasswordField/SearchField/TextArea 四个文本输入控件 + IME（中文输入法），让 showcase 文本输入控件可交互。

**Architecture:** TextField 当 leaf（自渲染文本+光标+选区+composition，取 RmlUi 几何内核丢 DOM 外壳）。编辑内核 EditState（value/cursor/anchor/composition）存 ControlState side table（P1 模式）。TextLayout 在 layout 阶段 measure（非 render lazy），光标几何 render 时从缓存取。IME = core 标记子串渲染 + 后端读 `Input.compositionString` 采集。字符输入独立通道（textinput，与 keydown 分离）。

**Tech Stack:** Rust core（taffy 0.12 / slotmap / bincode / ttf-parser 0.20）+ csbindgen FFI + C# 投影层 + 围栏（fence crate）

**Spec:** `docs/superpowers/specs/2026-07-27-textfield-design.md`

## Global Constraints

- Rust edition 2021，依赖钉版本（taffy 0.12 / slotmap 1.1 / csbindgen 1 / ttf-parser 0.20）
- FFI 边界 C-like enum 必须 `#[repr(uN)]`；`size_of::<T>()` 断言 ABI struct 尺寸
- FFI 返字符串一律 ptr+len（不靠 NUL）；getter 用 return-code + out-param（避 Container=0 哨兵）
- pkg 格式一刀切升 v24→v25（MIN=MAX=25，弃 v24，无迁移器），加 bincode 稳定性测试
- 围栏真相源 = `crates/fence/src/schema/` Rust const 表；改 schema 必同步 `docs/design/fence.md`
- 代码注释写上线品质（说 WHY，不引用内部编号）
- push 前跑 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`
- Rust 改动后重编 + 拷 `.dll`：`cargo build -p loomgui_ffi_c --release` → cp 到 `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`（Unity 关着拷）
- 改 parse-time 逻辑（bridge）须重打 pkg：`cargo run -p loomgui_pkg -- build showcase`
- 所有字节偏移严格遵守 UTF-8 边界（光标/选区/composition 都用字节偏移，钳到字符首字节）
- value 存 ControlState.EditState（非 text_contents）；TextField 不用 text_contents

---

## File Structure

**core（Rust）：**
- `crates/core/src/asset/mod.rs` — ControlInit 加 TextField/TextArea 变体；PKG_FORMAT_VERSION bump 25
- `crates/core/src/scene/node.rs` — ControlState 加 TextField/TextArea(EditState)；EditState/Composition struct
- `crates/core/src/scene/dynamic.rs` — instantiate 从 ControlInit 填 EditState
- `crates/core/src/scene/control.rs` — 文本编辑内核（编辑原语 + 命中 + 状态同步），扩现有 control.rs
- `crates/core/src/scene/text_cursor.rs`（新）— 字形位置查询（cursor_pixel_x / hit_byte_offset / line_byte_ranges）
- `crates/core/src/render/mod.rs` — NodeKind::TextField/Password/Search/TextArea arm（文本+光标+选区+composition 渲染）
- `crates/core/src/stage.rs` — tick 插 TextField measure（solve 后）+ 光标闪烁 timer
- `crates/core/src/input.rs` — process_keys 路由控制键给编辑内核；EVT_SUBMITTED 常量

**packer（Rust）：** `crates/packer/pkg/src/bridge.rs` — 提取 input/textarea 属性 → ControlInit

**FFI（Rust）：** `crates/ffi/src/lib.rs` — text input / composition / clipboard / control text+selection+placeholder+readonly FFI

**C#：** `unity/package/Runtime/Public/LoomGUI.Nodes.cs` + `Projection/EventDemuxer.cs` + `LoomGUI.EventType.cs`

**fence：** `crates/fence/src/` + `docs/design/fence.md` — 扩控件 CSS 命中校验到 input/textarea

**showcase：** `showcase/showcase/form.html,settings.html,mail.html` — 文本控件 CSS + 交互演示

---

## Task 1: pkg v25 + ControlInit 加 TextField/TextArea 变体

**Files:**
- Modify: `crates/core/src/asset/mod.rs:21-23,48-60`
- Test: `crates/core/src/asset/tests.rs`

**Interfaces:**
- Produces: `ControlInit::TextField(EditInit)`、`ControlInit::TextArea(EditInit)`；`EditInit { value, placeholder, max_length, readonly }`

- [ ] **Step 1: ControlInit 加共享结构 + 两变体（asset/mod.rs:48）**

enum 上方加：
```rust
/// 文本控件初始值（TextField/TextArea 共用，从 HTML value/placeholder 属性 bake）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EditInit {
    pub value: String,
    pub placeholder: String,
    pub max_length: usize,  // 0 = 无限
    pub readonly: bool,
}
```
enum 追加 `TextField(EditInit)`、`TextArea(EditInit)`。

- [ ] **Step 2: bump 版本（asset/mod.rs:21-23）**

```rust
pub const PKG_FORMAT_VERSION: u32 = 25; // v25: ControlInit TextField/TextArea (bincode layout change)
pub(crate) const MIN_VERSION: u32 = 25;
pub(crate) const MAX_VERSION: u32 = 25;
```

- [ ] **Step 3: write/read_package 同步 + 补遗漏构造点**

`cargo build -p loomgui_core` 报所有构造 `ControlInit` 遗漏处，逐一补。

- [ ] **Step 4: 写失败测试（bincode 稳定性）**

```rust
#[test]
fn pkg_v25_edit_init_roundtrip() {
    let init = ControlInit::TextField(EditInit {
        value: "hi".into(), placeholder: "name".into(), max_length: 20, readonly: false });
    let bytes = bincode::serialize(&init).unwrap();
    let back: ControlInit = bincode::deserialize(&bytes).unwrap();
    assert_eq!(init, back);
}

#[test]
fn pkg_v25_rejects_v24() {
    let mut bad = vec![];
    bad.extend_from_slice(&24u32.to_le_bytes());
    let res: Result<Package, _> = read_package(&bad);
    assert!(res.is_err(), "v24 must be rejected after bump");
}
```

- [ ] **Step 5: 跑测试** — `cargo test -p loomgui_core pkg_v25` PASS，全绿。

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/asset/mod.rs crates/core/src/asset/tests.rs
git commit -m "feat(pkg): bump v25 + ControlInit TextField/TextArea variants"
```

---

## Task 2: bridge 提取文本控件属性 → ControlInit

**Files:**
- Modify: `crates/packer/pkg/src/bridge.rs`（`extract_control_init`，P1 已有）
- Test: `crates/packer/pkg/tests/`

**Interfaces:**
- Consumes: `ControlInit`（Task 1）、`attr` helper
- Produces: bridge 填 `ControlInit::TextField/TextArea`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn bridge_extracts_text_attrs() {
    let html = r#"<input type="text" value="bob" placeholder="name" maxlength="20">"#;
    let node = &run_bridge(html)[0].nodes[0];
    assert_eq!(node.kind, NodeKind::TextField);
    match &node.control_init {
        Some(ControlInit::TextField(e)) => {
            assert_eq!(e.value, "bob"); assert_eq!(e.placeholder, "name");
            assert_eq!(e.max_length, 20); assert!(!e.readonly);
        }
        other => panic!("expected TextField, got {:?}", other),
    }
}

#[test]
fn bridge_extracts_textarea_attrs() {
    let html = r#"<textarea placeholder="body" maxlength="500">hello</textarea>"#;
    let node = &run_bridge(html)[0].nodes[0];
    assert_eq!(node.kind, NodeKind::TextArea);
    match &node.control_init {
        Some(ControlInit::TextArea(e)) => { assert_eq!(e.value, "hello"); assert_eq!(e.placeholder, "body"); }
        other => panic!("expected TextArea, got {:?}", other),
    }
}
```

- [ ] **Step 2: 跑测试验失败** — `cargo test -p loomgui_pkg bridge_extracts_text` FAIL。

- [ ] **Step 3: 实现 bridge 提取（extract_control_init match）**

```rust
NodeKind::TextField | NodeKind::PasswordField | NodeKind::SearchField => Some(ControlInit::TextField(EditInit {
    value: attr(el, "value").unwrap_or_default().to_string(),
    placeholder: attr(el, "placeholder").unwrap_or_default().to_string(),
    max_length: attr(el, "maxlength").and_then(|v| v.parse().ok()).unwrap_or(0),
    readonly: attr(el, "readonly").is_some(),
})),
NodeKind::TextArea => Some(ControlInit::TextArea(EditInit {
    value: element_text(el),  // textarea 内容即 value（非属性）
    placeholder: attr(el, "placeholder").unwrap_or_default().to_string(),
    max_length: attr(el, "maxlength").and_then(|v| v.parse().ok()).unwrap_or(0),
    readonly: attr(el, "readonly").is_some(),
})),
```
（`element_text`：若无则加——遍历 IrElement 子节点收集 text。Password/Search 复用 TextField EditInit，运行时按 NodeKind 掩码。）

- [ ] **Step 4: 跑测试** — `cargo test -p loomgui_pkg` 全绿。

- [ ] **Step 5: Commit**

```bash
git add crates/packer/pkg/src/bridge.rs crates/packer/pkg/tests/
git commit -m "feat(bridge): extract text control attrs into ControlInit"
```

---

## Task 3: ControlState 加 TextField/TextArea(EditState) + instantiate 填值

**Files:**
- Modify: `crates/core/src/scene/node.rs:350-376`、`crates/core/src/scene/dynamic.rs`
- Test: `crates/core/src/scene/node/tests.rs`

**Interfaces:**
- Consumes: `ControlInit`（Task 1）
- Produces: `ControlState::TextField(EditState)` / `TextArea(EditState)`；`EditState` / `Composition`

- [ ] **Step 1: 定义 EditState + Composition（node.rs，ControlState 上方）**

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Composition { pub text: String, pub pos: usize }  // value 字节偏移

#[derive(Debug, Clone, PartialEq)]
pub struct EditState {
    pub value: String,
    pub cursor: usize,        // [0, value.len()]
    pub anchor: usize,        // 选区锚；选区 = [min(anchor,cursor), max]
    pub composition: Option<Composition>,
    pub max_length: usize,    // 0 = 无限（按 UTF-8 字符数）
    pub readonly: bool,
    pub cursor_visible: bool,
    pub cursor_timer: f32,
    pub ideal_cursor_x: f32,  // 上下行 sticky x（TextArea 用）
}
impl EditState {
    pub fn from_init(value: String, _ph: String, max_length: usize, readonly: bool) -> Self {
        let cursor = value.len();
        Self { value, cursor, anchor: cursor, composition: None, max_length, readonly,
            cursor_visible: true, cursor_timer: 0.0, ideal_cursor_x: 0.0 }
    }
    pub fn selection_range(&self) -> (usize, usize) {
        if self.anchor <= self.cursor { (self.anchor, self.cursor) } else { (self.cursor, self.anchor) }
    }
}
```

- [ ] **Step 2: ControlState 加变体（node.rs:350）** — 追加 `TextField(EditState)`、`TextArea(EditState)`。

- [ ] **Step 3: instantiate 填值（dynamic.rs create_node_from_template，P1 match 旁）**

```rust
ControlInit::TextField(e) => ControlState::TextField(EditState::from_init(
    e.value.clone(), e.placeholder.clone(), e.max_length, e.readonly)),
ControlInit::TextArea(e) => ControlState::TextArea(EditState::from_init(
    e.value.clone(), e.placeholder.clone(), e.max_length, e.readonly)),
```

- [ ] **Step 4: 写失败测试**

```rust
#[test]
fn instantiate_fills_textfield_edit_state() {
    let mut scene = Scene::default();
    let template = TemplateNode {
        kind: NodeKind::TextField,
        control_init: Some(ControlInit::TextField(EditInit {
            value: "hi".into(), placeholder: "p".into(), max_length: 10, readonly: false })),
        ..default_template()
    };
    let id = create_node_from_template(&mut scene, &template, None);
    match scene.controls.get(id) {
        Some(ControlState::TextField(e)) => {
            assert_eq!(e.value, "hi"); assert_eq!(e.cursor, 2); assert_eq!(e.anchor, 2);
        }
        other => panic!("expected TextField, got {:?}", other),
    }
}
```

- [ ] **Step 5: 跑测试** — `cargo test -p loomgui_core instantiate_fills_textfield` PASS，全绿。

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/scene/node.rs crates/core/src/scene/dynamic.rs crates/core/src/scene/node/tests.rs
git commit -m "feat(core): ControlState TextField/TextArea + EditState, fill from ControlInit"
```

---

## Task 4: render TextField arm + value 显示 + 密码掩码

**Files:**
- Modify: `crates/core/src/render/mod.rs:321`（match 加 arm；抽 build_container_mesh）
- Test: `crates/core/src/render/tests.rs`

**Interfaces:**
- Consumes: `ControlState`（Task 3）、`measure_text`、`build_text_mesh`
- Produces: TextField/Password/Search/TextArea 渲染出 value；`transform_display_value(kind, &value)`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn textfield_renders_value_text() {
    let (mut scene, id) = make_scene_with_control(NodeKind::TextField,
        ControlState::TextField(EditState::from_init("hello".into(), "".into(), 0, false)));
    let nodes = build_render_nodes(&mut scene, &fonts(), &prev_hashes(), &image_sizes(), &mut atlas());
    let rn = nodes.iter().find(|n| n.node_id == id.0).expect("rendered");
    assert!(matches!(rn.payload, NodePayload::Mesh { ref verts, .. } if !verts.is_empty()),
        "TextField must render value glyphs");
}
```

- [ ] **Step 2: 加 transform_display_value（control.rs）**

```rust
/// 显示变换：PasswordField 掩码（'•' × 字符数）。其他 kind 原样。
pub fn transform_display_value(kind: NodeKind, value: &str) -> String {
    match kind {
        NodeKind::PasswordField => value.chars().map(|_| '•').collect(),
        _ => value.to_string(),
    }
}
```

- [ ] **Step 3: 抽 build_container_mesh（mod.rs，把 `_ =>`（line 618）Container mesh 抽成 fn）**

现有 `_ => RenderNode { ... Container mesh ... }` 抽成 `fn build_container_mesh(...) -> RenderNode`，TextField arm 复用画背景框。

- [ ] **Step 4: render 加 TextField arm（mod.rs:321，TextNode arm 旁）**

```rust
NodeKind::TextField | NodeKind::PasswordField | NodeKind::SearchField | NodeKind::TextArea => {
    let bg = build_container_mesh(scene, n, rect, ...);  // 背景框
    push_text_meshes(&mut nodes, &mut id_to_pos, std::iter::once(bg), n, node_id, ...);
    let Some(ControlState::TextField(e) | ControlState::TextArea(e)) = scene.controls.get(n.id) else { continue };
    let display = if e.value.is_empty() { e.placeholder.clone() } else { transform_display_value(n.kind, &e.value) };
    let s = &n.style;
    let stack = fonts.stack_for(s.font_family.as_deref());
    let off_left = resolve_lp(s.taffy_style.border.left) + resolve_lp(s.taffy_style.padding.left);
    let off_right = resolve_lp(s.taffy_style.border.right) + resolve_lp(s.taffy_style.padding.right);
    let content_w = (rect.w - off_left - off_right).max(0.0);
    let mut layout = scene.text_layouts.get(n.id.index()).cloned().flatten()
        .unwrap_or_else(|| measure_text(&display, s.font_size, s.line_height, s.letter_spacing,
            s.text_align, s.white_space_nowrap, Some(content_w), &stack, s.color,
            crate::text::rich::weight_from_font_weight(s.font_weight)));
    let off_top = resolve_lp(s.taffy_style.border.top) + resolve_lp(s.taffy_style.padding.top);
    if off_left != 0.0 || off_top != 0.0 { bake_content_offset(&mut layout, off_left, off_top); }
    let meshes = build_text_mesh(&layout, atlas, fonts, rect, &[], None, false);
    push_text_meshes(&mut nodes, &mut id_to_pos, meshes, n, node_id, node_id);
}
```

- [ ] **Step 5: 跑测试** — `cargo test -p loomgui_core textfield_renders` PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/render/mod.rs crates/core/src/render/tests.rs crates/core/src/scene/control.rs
git commit -m "feat(render): TextField leaf arm renders value text + password mask"
```

---

## Task 5: TextLayout layout 阶段 measure（非 render lazy）

**Files:**
- Modify: `crates/core/src/stage.rs:716`（solve 后插 measure）、`crates/core/src/scene/control.rs`
- Test: `crates/core/src/stage.rs`

**Interfaces:**
- Consumes: `measure_text`、`ControlState`
- Produces: `measure_text_controls(scene, fonts)` —— tick solve 后 measure 写 text_layouts

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn tick_measures_textfield_layout_after_solve() {
    let mut stage = Stage::new(fonts(), root_size(), image_sizes());
    stage.load_package(&fixture_pkg_with_textfield("hello"));
    stage.instantiate(0, "root");
    stage.tick_and_render();
    let scene = stage.scene().unwrap();
    let tf_id = find_node_by_kind(scene, NodeKind::TextField);
    assert!(scene.text_layouts.get(tf_id.index()).flatten().is_some(),
        "TextField TextLayout must be measured at layout stage");
}
```

- [ ] **Step 2: 实现 measure_text_controls（control.rs）**

```rust
use crate::text::layout::{measure_text, FontTable};
use crate::render::resolve_lp;  // 或重导出

pub fn measure_text_controls(scene: &mut Scene, fonts: &FontTable) {
    let ids: Vec<NodeId> = scene.controls.0.iter()
        .filter(|(_, s)| matches!(s, ControlState::TextField(_) | ControlState::TextArea(_)))
        .map(|(&id, _)| id).collect();
    for id in ids {
        let Some(n) = scene.get(id) else { continue };
        let Some(ControlState::TextField(e) | ControlState::TextArea(e)) = scene.controls.get(id) else { continue };
        let display = transform_display_value(n.kind, &e.value);
        let s = &n.style;
        let stack = fonts.stack_for(s.font_family.as_deref());
        let off_left = resolve_lp(s.taffy_style.border.left) + resolve_lp(s.taffy_style.padding.left);
        let off_right = resolve_lp(s.taffy_style.border.right) + resolve_lp(s.taffy_style.padding.right);
        let content_w = (n.layout_rect.w - off_left - off_right).max(0.0);
        let off_top = resolve_lp(s.taffy_style.border.top) + resolve_lp(s.taffy_style.padding.top);
        let mut layout = measure_text(&display, s.font_size, s.line_height, s.letter_spacing,
            s.text_align, s.white_space_nowrap, Some(content_w), &stack, s.color,
            crate::text::rich::weight_from_font_weight(s.font_weight));
        if off_left != 0.0 || off_top != 0.0 { bake_content_offset(&mut layout, off_left, off_top); }
        scene.text_layouts.insert(id, Some(layout));
    }
}
```
（`bake_content_offset` 当前在 render/mod.rs —— 提到 pub(crate) 或重导出到 control 复用。）

- [ ] **Step 3: stage tick 插步骤（stage.rs，solve 后 line ~716）**

```rust
solve(scene, &self.fonts, self.root_size, &self.image_sizes);
// 5.5 measure 文本控件显示文本（光标命中/几何/render 都用 TextLayout，须 render 前算好）。
crate::scene::control::measure_text_controls(scene, &self.fonts);
crate::scroll::refresh_content_sizes(scene);
```

- [ ] **Step 4: 跑测试** — `cargo test -p loomgui_core tick_measures_textfield` PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/stage.rs crates/core/src/scene/control.rs
git commit -m "feat(core): measure TextField TextLayout at layout stage (not render lazy)"
```

---

## Task 6: 字形位置查询（cursor_pixel_x / hit_byte_offset / line_byte_ranges）

**Files:**
- Create: `crates/core/src/scene/text_cursor.rs`；Modify: `crates/core/src/scene/mod.rs`（pub mod）
- Test: `crates/core/src/scene/text_cursor/tests.rs`（新）

**Interfaces:**
- Consumes: `TextLayout`（glyph.advance/codepoint）
- Produces: `line_byte_ranges`、`cursor_pixel_x`、`hit_byte_offset`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn cursor_pixel_x_at_byte_offsets() {
    let layout = make_layout("abc");  // 等宽 advance=10
    let ranges = line_byte_ranges(&layout, "abc");
    assert_eq!(cursor_pixel_x(&layout, &ranges, 0).0, 0.0);
    assert_eq!(cursor_pixel_x(&layout, &ranges, 1).0, 10.0);
    assert_eq!(cursor_pixel_x(&layout, &ranges, 3).0, 30.0);
}

#[test]
fn hit_byte_offset_finds_nearest() {
    let layout = make_layout("abc");
    let ranges = line_byte_ranges(&layout, "abc");
    assert_eq!(hit_byte_offset(&layout, &ranges, 5.0, 0.0), 0);
    assert_eq!(hit_byte_offset(&layout, &ranges, 15.0, 0.0), 1);
    assert_eq!(hit_byte_offset(&layout, &ranges, 35.0, 0.0), 3);
}
```

- [ ] **Step 2: 实现 line_byte_ranges**

```rust
pub fn line_byte_ranges(layout: &TextLayout, value: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut byte_pos = 0usize;
    let mut chars = value.chars();
    for line in &layout.lines {
        let start = byte_pos;
        for run in &line.runs {
            for _g in &run.glyphs { if let Some(ch) = chars.next() { byte_pos += ch.len_utf8(); } }
        }
        ranges.push((start, byte_pos));
    }
    if ranges.is_empty() { ranges.push((0, 0)); }
    ranges
}
```

- [ ] **Step 3: 实现 cursor_pixel_x**

```rust
pub fn cursor_pixel_x(layout: &TextLayout, ranges: &[(usize, usize)], offset: usize) -> (f32, usize) {
    for (li, &(start, end)) in ranges.iter().enumerate() {
        if offset <= end || li == ranges.len() - 1 {
            let line = &layout.lines[li];
            let mut x = 0.0; let mut cur = start;
            'outer: for run in &line.runs {
                for g in &run.glyphs {
                    if cur >= offset { break 'outer; }
                    x += g.advance;
                    cur += char::from_u32(g.codepoint).map(|c| c.len_utf8()).unwrap_or(1);
                }
            }
            return (x, li);
        }
    }
    (0.0, 0)
}
```

- [ ] **Step 4: 实现 hit_byte_offset**

```rust
pub fn hit_byte_offset(layout: &TextLayout, ranges: &[(usize, usize)], x: f32, y: f32) -> usize {
    let mut li = 0;
    for (i, line) in layout.lines.iter().enumerate() {
        if y >= line.y { li = i; }  // 越下取末行
    }
    let line = &layout.lines[li];
    let (start, _end) = ranges[li];
    let mut pen = 0.0; let mut cur = start;
    for run in &line.runs {
        for g in &run.glyphs {
            let mid = pen + g.advance / 2.0;
            if x < mid { return cur; }
            pen += g.advance;
            cur += char::from_u32(g.codepoint).map(|c| c.len_utf8()).unwrap_or(1);
        }
    }
    cur
}
```

- [ ] **Step 5: 跑测试** — `cargo test -p loomgui_core text_cursor` PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/scene/text_cursor.rs crates/core/src/scene/mod.rs crates/core/src/scene/text_cursor/tests.rs
git commit -m "feat(core): glyph position queries (cursor_pixel_x / hit_byte_offset / line_byte_ranges)"
```

---

## Task 7: 光标点击命中 + 光标闪烁 timer

**Files:**
- Modify: `crates/core/src/scene/control.rs`、`crates/core/src/stage.rs`
- Test: `crates/core/src/scene/control/tests.rs`

**Interfaces:**
- Consumes: `hit_byte_offset`（Task 6）、PointerState
- Produces: `on_text_pointer_down`；`advance_cursor_blink`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn textfield_click_sets_cursor() {
    let (mut scene, id) = make_scene_with_textfield("hello");
    on_text_pointer_down(&mut scene, id, 20.0, 5.0);
    if let Some(ControlState::TextField(e)) = scene.controls.get(id) {
        assert!(e.cursor >= 1 && e.cursor <= 3, "cursor near char 2");
        assert_eq!(e.anchor, e.cursor);
    } else { panic!("not TextField"); }
}
```

- [ ] **Step 2: 实现 on_text_pointer_down（control.rs）**

```rust
use crate::scene::text_cursor::{hit_byte_offset, line_byte_ranges};

pub fn on_text_pointer_down(scene: &mut Scene, id: NodeId, local_x: f32, local_y: f32) {
    let value = match scene.controls.get(id) {
        Some(ControlState::TextField(e) | ControlState::TextArea(e)) => e.value.clone(), _ => return };
    let Some(layout) = scene.text_layouts.get(id.index()).and_then(|o| o.as_ref()).cloned() else { return };
    let ranges = line_byte_ranges(&layout, &value);
    let offset = hit_byte_offset(&layout, &ranges, local_x, local_y);
    if let Some(ControlState::TextField(e) | ControlState::TextArea(e)) = scene.controls.get_mut(id) {
        e.cursor = offset; e.anchor = offset;
        e.cursor_visible = true; e.cursor_timer = 0.0;
    }
}
```

- [ ] **Step 3: stage process 接 hook（参照 P1 Slider 拖拽接入）**

PointerState.process 命中后，若命中节点是 TextField/TextArea，把 world pos 转 local（减节点 rect.xy + border/padding offset）调 `on_text_pointer_down`。

- [ ] **Step 4: 光标闪烁 timer（control.rs + stage.rs）**

```rust
const CURSOR_BLINK_PERIOD: f32 = 0.7;
pub fn advance_cursor_blink(scene: &mut Scene, dt: f32) {
    let focused = scene.focused_node;
    for (_, state) in scene.controls.0.iter_mut() {
        if let ControlState::TextField(e) | ControlState::TextArea(e) = state {
            if Some(state_node_id) == focused {  // 注意借用：先取 focused 再迭代
                e.cursor_timer += dt;
                if e.cursor_timer >= CURSOR_BLINK_PERIOD {
                    e.cursor_timer = 0.0; e.cursor_visible = !e.cursor_visible;
                }
            } else { e.cursor_visible = false; }
        }
    }
}
```
stage tick `self.tweens.update(dt, ...)` 后加 `advance_cursor_blink(scene, dt)`。

- [ ] **Step 5: 跑测试** — `cargo test -p loomgui_core textfield_click` PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/scene/control.rs crates/core/src/stage.rs crates/core/src/scene/control/tests.rs
git commit -m "feat(core): textfield click hit-to-cursor + cursor blink timer"
```

---

## Task 8: 编辑原语（insert / delete / move + UTF-8 边界 + sanitize）

**Files:**
- Modify: `crates/core/src/scene/control.rs`
- Test: `crates/core/src/scene/control/tests.rs`

**Interfaces:**
- Consumes: `EditState`、`NodeKind`
- Produces: `insert_text` / `delete_char` / `delete_selection` / `move_cursor` / `sanitize_value`

- [ ] **Step 1: 写失败测试（核心编辑）**

```rust
#[test] fn insert_at_cursor() { let mut e = EditState::from_init("ac".into(),"".into(),0,false);
    e.cursor=1; e.anchor=1; insert_text(&mut e, NodeKind::TextField, "b");
    assert_eq!(e.value,"abc"); assert_eq!(e.cursor,2); }
#[test] fn insert_replaces_selection() { let mut e = EditState::from_init("hello".into(),"".into(),0,false);
    e.anchor=1; e.cursor=4; insert_text(&mut e, NodeKind::TextField, "X");
    assert_eq!(e.value,"hXo"); assert_eq!(e.cursor,2); }
#[test] fn backspace_deletes_left() { let mut e = EditState::from_init("abc".into(),"".into(),0,false);
    e.cursor=2; e.anchor=2; delete_char(&mut e, NodeKind::TextField, true);
    assert_eq!(e.value,"ac"); assert_eq!(e.cursor,1); }
#[test] fn sanitize_strips_newline_single_line() { let mut e = EditState::from_init("a\nb".into(),"".into(),0,false);
    sanitize_value(&mut e, NodeKind::TextField); assert_eq!(e.value,"ab");
    let mut e2 = EditState::from_init("a\nb".into(),"".into(),0,false);
    sanitize_value(&mut e2, NodeKind::TextArea); assert_eq!(e2.value,"a\nb"); }
#[test] fn utf8_boundary_clamp() { let mut e = EditState::from_init("你好".into(),"".into(),0,false);
    e.cursor=3; move_cursor(&mut e, NodeKind::TextField, true, false); assert_eq!(e.cursor,6); }
#[test] fn max_length_truncates() { let mut e = EditState::from_init("ab".into(),"".into(),2,false);
    e.cursor=2; e.anchor=2; insert_text(&mut e, NodeKind::TextField, "c"); assert_eq!(e.value,"ab"); }
```

- [ ] **Step 2: 实现编辑原语（control.rs）**

```rust
fn prev_char_boundary(value: &str, idx: usize) -> usize {
    let mut i = idx; while i > 0 && !value.is_char_boundary(i) { i -= 1; } i
}
fn next_char_boundary(value: &str, idx: usize) -> usize {
    let mut i = idx + 1; while i < value.len() && !value.is_char_boundary(i) { i += 1; } i.min(value.len())
}
fn clamp_boundary(value: &str, idx: usize) -> usize {
    let mut i = idx.min(value.len()); while i > 0 && !value.is_char_boundary(i) { i -= 1; } i
}
fn sanitize_str(kind: NodeKind, s: &str) -> String {
    match kind {
        NodeKind::TextArea => s.chars().filter(|&c| c != '\r' && c != '\t').collect(),
        _ => s.chars().filter(|&c| !matches!(c,'\n'|'\r'|'\t')).collect(),
    }
}
pub fn insert_text(e: &mut EditState, kind: NodeKind, text: &str) -> bool {
    if e.readonly { return false; }
    let text = sanitize_str(kind, text);
    if text.is_empty() { return false; }
    delete_selection(e);
    if e.max_length > 0 {
        let cur = e.value.chars().count(); let add = text.chars().count();
        if cur + add > e.max_length { return false; }
    }
    e.value.insert_str(e.cursor, &text);
    e.cursor += text.len(); e.anchor = e.cursor;
    e.cursor_visible = true; e.cursor_timer = 0.0;
    true
}
pub fn delete_selection(e: &mut EditState) -> bool {
    let (b, end) = e.selection_range();
    if b == end { return false; }
    e.value.replace_range(b..end, ""); e.cursor = b; e.anchor = b; true
}
pub fn delete_char(e: &mut EditState, _kind: NodeKind, backspace: bool) -> bool {
    if e.readonly { return false; }
    if e.anchor != e.cursor { return delete_selection(e); }
    if backspace && e.cursor > 0 {
        let nc = prev_char_boundary(&e.value, e.cursor);
        e.value.replace_range(nc..e.cursor, ""); e.cursor = nc; e.anchor = nc; true
    } else if !backspace && e.cursor < e.value.len() {
        let end = next_char_boundary(&e.value, e.cursor);
        e.value.replace_range(e.cursor..end, ""); e.anchor = e.cursor; true
    } else { false }
}
pub fn move_cursor(e: &mut EditState, _kind: NodeKind, right: bool, select: bool) {
    let nc = if right { next_char_boundary(&e.value, e.cursor) } else { prev_char_boundary(&e.value, e.cursor) };
    e.cursor = nc;
    if !select { e.anchor = nc; }
    e.cursor_visible = true; e.cursor_timer = 0.0;
}
pub fn sanitize_value(e: &mut EditState, kind: NodeKind) {
    e.value = sanitize_str(kind, &e.value);
    e.cursor = clamp_boundary(&e.value, e.cursor);
    e.anchor = clamp_boundary(&e.value, e.anchor);
}
```

- [ ] **Step 3: 跑测试** — `cargo test -p loomgui_core edit` 全 PASS。

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/scene/control.rs crates/core/src/scene/control/tests.rs
git commit -m "feat(core): text edit primitives (insert/delete/move + UTF-8 + sanitize)"
```

---

## Task 9: 字符输入通道（textinput FFI + process）

**Files:**
- Modify: `crates/ffi/src/lib.rs`、`crates/core/src/stage.rs`（pending_text_input）
- Test: `crates/ffi/src/tests.rs`

**Interfaces:**
- Consumes: `insert_text`（Task 8）、`focused_node`
- Produces: `loomgui_stage_set_text_input` FFI

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn ffi_text_input_inserts_into_focused_textfield() {
    let stage = make_stage_with_focused_textfield("ab");
    unsafe {
        let cps = [b'b' as u32, b'c' as u32];
        assert_eq!(loomgui_stage_set_text_input(stage.handle, cps.as_ptr(), 2), 0);
    }
    stage.tick();
    assert_textfield_value(&stage, "abbc");
}
```

- [ ] **Step 2: Stage 加 pending_text_input + process（stage.rs）**

```rust
pub pending_text_input: Vec<u32>,
pub fn set_text_input(&mut self, cps: &[u32]) { self.pending_text_input = cps.to_vec(); }
// tick（process_keys 后）：
let cps = std::mem::take(&mut self.pending_text_input);
if !cps.is_empty() {
    if let Some(fid) = scene.focused_node {
        if let Some(ControlState::TextField(e) | ControlState::TextArea(e)) = scene.controls.get_mut(fid) {
            let kind = scene.get(fid).map(|n| n.kind).unwrap();
            let s: String = cps.iter().filter_map(|&cp| char::from_u32(cp)).collect();
            if insert_text(e, kind, &s) { /* Task 11 emit ValueChanged */ }
        }
    }
}
```

- [ ] **Step 3: FFI（lib.rs，仿 set_key_input）**

```rust
#[csbindgen]
pub unsafe extern "C" fn loomgui_stage_set_text_input(h: *mut StageHandle, codepoints: *const u32, len: usize) -> i32 {
    let Some(sh) = h.as_ref() else { return -1 };
    if len == 0 { sh.stage.set_text_input(&[]); return 0; }
    sh.stage.set_text_input(std::slice::from_raw_parts(codepoints, len));
    0
}
```

- [ ] **Step 4: 跑测试 + sync bindings** — `cargo test -p loomgui_ffi_c ffi_text_input` PASS；`cargo run -p xtask -- sync-bindings`。

- [ ] **Step 5: Commit**

```bash
git add crates/ffi/src/lib.rs crates/ffi/src/tests.rs crates/core/src/stage.rs
git commit -m "feat(ffi): text input channel (set_text_input -> focused TextField)"
```

---

## Task 10: 控制键路由（keydown → 编辑内核）

**Files:**
- Modify: `crates/core/src/input.rs`
- Test: `crates/core/src/input.rs`

**Interfaces:**
- Consumes: `move_cursor` / `delete_char`（Task 8）、KeyEvent
- Produces: process_keys 路由控制键给 focused TextField

- [ ] **Step 1: 写失败测试**

```rust
#[test] fn textfield_backspace_key_deletes() {
    let mut stage = make_stage_with_focused_textfield("abc");
    stage.set_key_input(&[KeyEvent { key_code: KEY_BACKSPACE, modifiers: 0, is_down: true, ..default() }]);
    stage.tick(); assert_textfield_value(&stage, "ab");
}
#[test] fn textfield_left_arrow_moves_cursor() {
    let mut stage = make_stage_with_focused_textfield("abc");
    stage.set_key_input(&[KeyEvent { key_code: KEY_LEFT, modifiers: 0, is_down: true, ..default() }]);
    stage.tick(); assert_textfield_cursor(&stage, 2);
}
```

- [ ] **Step 2: 核对 + 定义 KeyCode 常量（input.rs）**

⚠️ 实现时查 Unity KeyCode 实际数值（`Editor/Data/Managed/UnityEditor.xml`）+ input.rs:41 modifiers 位定义，勿用记忆值。至少加 KEY_BACKSPACE/RETURN/ESCAPE/LEFT/UP/RIGHT/DOWN/DELETE/HOME/END/A/C/V/X + MOD_CTRL/MOD_SHIFT。

- [ ] **Step 3: process_keys 路由（input.rs，现有 Tab 分支旁）**

```rust
if let Some(fid) = scene.focused_node {
    if let Some(ControlState::TextField(e) | ControlState::TextArea(e)) = scene.controls.get_mut(fid) {
        let kind = scene.get(fid).map(|n| n.kind).unwrap();
        if ke.is_down {
            let ctrl = ke.modifiers & MOD_CTRL != 0; let shift = ke.modifiers & MOD_SHIFT != 0;
            let mut changed = false;
            match ke.key_code {
                KEY_BACKSPACE => { if delete_char(e, kind, true) { changed = true; } continue_with_route }
                KEY_DELETE => { if delete_char(e, kind, false) { changed = true; } continue_with_route }
                KEY_LEFT => { move_cursor(e, kind, false, shift); continue_with_route }
                KEY_RIGHT => { move_cursor(e, kind, true, shift); continue_with_route }
                KEY_HOME => { e.cursor = 0; if !shift { e.anchor = 0; } continue_with_route }
                KEY_END => { e.cursor = e.value.len(); if !shift { e.anchor = e.cursor; } continue_with_route }
                KEY_RETURN => { line_break(e, kind, &mut out, fid); continue_with_route }  // Task 11
                KEY_ESCAPE => { scene.focused_node = None; continue_with_route }
                _ => {}
            }
            if changed { /* Task 11 emit ValueChanged */ }
        }
    }
}
```
（`continue_with_route` = 路由后 `continue` 不发 keydown；对照现有 Tab `continue` 模式。Enter/Escape 走各自语义。）

- [ ] **Step 4: 跑测试** — `cargo test -p loomgui_core textfield_.*_key` PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/input.rs crates/core/src/scene/control.rs
git commit -m "feat(core): route control keys to textfield edit kernel"
```

---

## Task 11: Submitted 事件 + TextArea 换行 + ValueChanged 接线

**Files:**
- Modify: `crates/core/src/input.rs`、`crates/core/src/scene/control.rs`
- Test: `crates/core/src/input.rs`

**Interfaces:**
- Consumes: KeyEvent、EventRecord
- Produces: `EVT_SUBMITTED=25`；`line_break`（单行=Submitted，多行=插 \n）；ValueChanged 接线

- [ ] **Step 1: 定义常量（input.rs）** — `pub const EVT_SUBMITTED: u8 = 25;`

- [ ] **Step 2: 写失败测试**

```rust
#[test] fn singleline_enter_emits_submitted() {
    let mut stage = make_stage_with_focused_textfield("query");
    stage.set_key_input(&[KeyEvent { key_code: KEY_RETURN, is_down: true, ..default() }]);
    stage.tick_and_render();
    assert!(stage.borrow_events().iter().any(|e| e.evt_type == EVT_SUBMITTED));
    assert_textfield_value(&stage, "query");
}
#[test] fn textarea_enter_inserts_newline() {
    let mut stage = make_stage_with_focused_textarea("ab");
    stage.set_key_input(&[KeyEvent { key_code: KEY_RETURN, is_down: true, ..default() }]);
    stage.tick_and_render();
    assert_textfield_value(&stage, "ab\n");
    assert!(!stage.borrow_events().iter().any(|e| e.evt_type == EVT_SUBMITTED));
}
```

- [ ] **Step 3: 实现 line_break + ValueChanged helper（control.rs）**

```rust
pub fn line_break(e: &mut EditState, kind: NodeKind, out: &mut Vec<EventRecord>, node: NodeId) {
    match kind {
        NodeKind::TextArea => { if insert_text(e, kind, "\n") { emit_value_changed(out, node); } }
        _ => { out.push(EventRecord { evt_type: EVT_SUBMITTED, target: node, ..Default::default() }); }
    }
}
pub fn emit_value_changed(out: &mut Vec<EventRecord>, node: NodeId) {
    out.push(EventRecord { evt_type: EVT_VALUE_CHANGED, target: node, ..Default::default() });
}
```

- [ ] **Step 4: ValueChanged 接线** — Task 9/10 的 insert/delete 返回 true 处调 `emit_value_changed(&mut out, fid)`。

- [ ] **Step 5: 跑测试** — `cargo test -p loomgui_core enter_emits\|textarea_enter` PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/input.rs crates/core/src/scene/control.rs
git commit -m "feat(core): Submitted event + TextArea newline + ValueChanged wiring"
```

---

## Task 12: 光标 / 选区 / composition 渲染 mesh

**Files:**
- Modify: `crates/core/src/render/mod.rs`（TextField arm 扩）
- Test: `crates/core/src/render/tests.rs`

**Interfaces:**
- Consumes: `cursor_pixel_x`（Task 6）、EditState、TextLayout
- Produces: render 画光标 + 选区背景 + 选中文本反色 + composition 下划线

- [ ] **Step 1: 写失败测试（光标 mesh 存在）**

```rust
#[test]
fn focused_textfield_renders_cursor_quad() {
    let (mut scene, id) = make_focused_textfield("ab");  // focused + cursor_visible
    scene.text_layouts.insert(id, Some(make_layout("ab")));
    let nodes = build_render_nodes(&mut scene, &fonts(), &prev_hashes(), &image_sizes(), &mut atlas());
    let tf_nodes: Vec<_> = nodes.iter().filter(|n| /* 属于 id */ ).collect();
    assert!(tf_nodes.len() >= 2, "expected text + cursor meshes");
}
```

- [ ] **Step 2: TextField arm 扩渲染（mod.rs，Task 4 文本 mesh 后）**

```rust
// 光标（focused + cursor_visible + !readonly）
if scene.focused_node == Some(n.id) && !e.readonly && e.cursor_visible {
    if let Some(layout) = scene.text_layouts.get(n.id.index()).and_then(|o| o.as_ref()) {
        let ranges = crate::scene::text_cursor::line_byte_ranges(layout, &display);
        let (cx, li) = crate::scene::text_cursor::cursor_pixel_x(layout, &ranges, e.cursor);
        let line = &layout.lines[li];
        let (x, y) = (rect.x + off_left + cx, rect.y + off_top + line.y);
        let caret_color = s.caret_color.unwrap_or(s.color);  // caret-color CSS（Task 15 style mapping）
        push_solid_quad(&mut nodes, /*合成 cursor id*/, n, x, y, 1.0, line.height, caret_color);
    }
}
// 选区背景（sel_begin<sel_end）：每行选区内字符逐段画 selection-background quad
let (sel_b, sel_e) = e.selection_range();
if sel_b < sel_e { /* 逐行算矩形 push mesh，色 = s.selection_background.unwrap_or([0.,0.,1.,0.5]) */ }
// composition 下划线（e.composition.is_some()）：comp 段每行底部 2px quad
if e.composition.is_some() { /* 逐行画下划线 quad */ }
```
（`push_solid_quad` helper：小工具，push 一个纯色 quad RenderNode。选区/composition 逐行矩形算法参照 RmlUi GenerateLine。`caret_color`/`selection_background` 若 style 未加则用常量缺省色，Task 15 补 style 字段。）

- [ ] **Step 3: 跑测试** — `cargo test -p loomgui_core focused_textfield_renders_cursor` PASS。

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/render/mod.rs crates/core/src/render/tests.rs
git commit -m "feat(render): textfield cursor/selection/composition meshes"
```

---

## Task 13: IME composition FFI + 标记子串渲染

**Files:**
- Modify: `crates/ffi/src/lib.rs`、`crates/core/src/scene/control.rs`、`crates/core/src/stage.rs`
- Test: `crates/ffi/src/tests.rs`

**Interfaces:**
- Consumes: `EditState.composition`、measure
- Produces: `loomgui_stage_set_composition` / `commit_composition` / `get_cursor_rect` FFI；core 渲染 composition 拼接 + 下划线

- [ ] **Step 1: 写失败测试（composition 拼进显示文本 + 提交落定）**

```rust
#[test]
fn composition_spliced_into_display() {
    let (mut scene, id) = make_textfield_with_value("ab");  // cursor=2
    unsafe {
        let s = "ni";
        assert_eq!(loomgui_stage_set_composition(scene.handle, id.0, s.as_ptr(), s.len(), 2), 0);
    }
    scene.tick_and_render();
    let layout = scene.text_layouts.get(id.index()).unwrap().as_ref().unwrap();
    assert!(layout.text_width > 0.0);  // 测到 "abni" 宽度
}
#[test]
fn commit_composition_appends_to_value() {
    let (mut scene, id) = make_textfield_with_value("ab");
    unsafe {
        loomgui_stage_set_composition(scene.handle, id.0, "ni".as_ptr(), 2, 2);
        loomgui_stage_commit_composition(scene.handle, id.0);
    }
    scene.tick_and_render();
    assert_textfield_value(&scene, "abni");
}
```

- [ ] **Step 2: core composition 处理（control.rs）**

```rust
/// 设置 composition（后端读 Input.compositionString 回灌）。
pub fn set_composition(e: &mut EditState, text: &str, pos: usize) {
    let pos = pos.min(e.value.len());
    e.composition = Some(Composition { text: text.to_string(), pos });
    e.cursor_visible = true; e.cursor_timer = 0.0;
}
/// 提交 composition：落定进 value（删选区 + 在 pos 插 composition.text）。
pub fn commit_composition(e: &mut EditState, kind: NodeKind) -> bool {
    if let Some(comp) = e.composition.take() {
        e.cursor = comp.pos; e.anchor = comp.pos;
        return insert_text(e, kind, &comp.text);
    }
    false
}
/// 显示文本 = value 拼上 composition（measure/render 用）。
pub fn display_value(e: &EditState, kind: NodeKind) -> String {
    let base = transform_display_value(kind, &e.value);
    match &e.composition {
        Some(c) => {
            // value 掩码后再插 composition（composition 不掩码——拼音该可见）
            // 注意：掩码后 byte pos 与原 value 不同。简化：composition pos 基于 original value，
            // 掩码下用 char 计数定位。PasswordField 的 composition 罕见，本轮按 char 对齐。
            let mut chars: Vec<char> = base.chars().collect();
            let pos_char = e.value[..c.pos.min(e.value.len())].chars().count();
            let comp_chars: Vec<char> = c.text.chars().collect();
            for (i, ch) in comp_chars.into_iter().enumerate() {
                if pos_char + i <= chars.len() { chars.insert(pos_char + i, ch); }
            }
            chars.into_iter().collect()
        }
        None => base,
    }
}
```
（`measure_text_controls`（Task 5）+ render（Task 4）改用 `display_value(e, kind)` 取显示文本，替代纯 `transform_display_value`。）

- [ ] **Step 3: FFI（lib.rs）**

```rust
#[csbindgen]
pub unsafe extern "C" fn loomgui_stage_set_composition(
    h: *mut StageHandle, node: u32, text: *const u8, text_len: usize, pos: usize,
) -> i32 {
    let Some(sh) = h.as_ref() else { return -1 };
    let s = if text_len == 0 { String::new() }
        else { String::from_utf8_lossy(std::slice::from_raw_parts(text, text_len)).into_owned() };
    sh.stage.with_text_edit_mut(NodeId(node), |e, kind| set_composition(e, &s, pos))
}
#[csbindgen]
pub unsafe extern "C" fn loomgui_stage_commit_composition(h: *mut StageHandle, node: u32) -> i32 {
    let Some(sh) = h.as_ref() else { return -1 };
    sh.stage.with_text_edit_mut(NodeId(node), |e, kind| commit_composition(e, kind)).map(|c| c as i32).unwrap_or(-1)
}
#[csbindgen]
pub unsafe extern "C" fn loomgui_stage_get_cursor_rect(
    h: *mut StageHandle, node: u32, out: *mut CursorRectRepr,
) -> i32 {
    // 读光标 world 矩形（后端摆 IME 候选窗 Input.compositionCursorPos 用）
    // 复用 text_cursor::cursor_pixel_x + 节点 world transform → world rect
    ...
}
```
（`with_text_edit_mut`：Stage helper，按 node 取 ControlState::TextField/TextArea 的 EditState 闭包编辑。`CursorRectRepr`：`#[repr(C)] struct { x, y, w, h: f32 }`。）

- [ ] **Step 4: render 画 composition 下划线（Task 12 的 composition 分支填实）**

comp 段每行底部 2px 下划线 quad（用 display_value 的 TextLayout + composition pos 算 comp 字节范围 → 逐行矩形）。

- [ ] **Step 5: 跑测试 + sync bindings** — `cargo test -p loomgui_ffi_c composition` PASS；`cargo run -p xtask -- sync-bindings`。

- [ ] **Step 6: Commit**

```bash
git add crates/ffi/src/lib.rs crates/core/src/scene/control.rs crates/core/src/stage.rs crates/ffi/src/tests.rs
git commit -m "feat(ffi): IME composition (set/commit/get_cursor_rect) + mark-substring render"
```

---

## Task 14: 剪贴板 FFI

**Files:**
- Modify: `crates/ffi/src/lib.rs`、`crates/core/src/scene/control.rs`（copy/cut/paste 编辑原语）
- Test: `crates/ffi/src/tests.rs`

**Interfaces:**
- Consumes: `EditState`、FFI 剪贴板回调
- Produces: `loomgui_clipboard_set` / `clipboard_get` FFI；`copy_selection` / `cut_selection` / `paste`

- [ ] **Step 1: 写失败测试**

```rust
#[test] fn paste_inserts_clipboard_text() {
    let (mut scene, id) = make_focused_textfield_with_clipboard("XY", "hi");  // 剪贴板 "hi"
    paste(&mut scene, id, NodeKind::TextField);
    assert_textfield_value(&scene, "XYhi");
}
#[test] fn copy_fills_clipboard() {
    let (mut scene, id) = make_textfield_with_selection("hello", 0, 3);  // 选 "hel"
    let cb = copy_selection(&mut scene, id);
    assert_eq!(cb, "hel");
}
```

- [ ] **Step 2: core copy/cut/paste（control.rs）**

```rust
/// core 侧调 FFI 剪贴板（后端实现转 Unity GUIUtility.systemCopyBuffer）。
extern "C" { fn loomgui_clipboard_set(text: *const u8, len: usize) -> i32;
            fn loomgui_clipboard_get(out: *mut *const u8, out_len: *mut usize) -> i32; }
pub fn read_clipboard() -> String { /* 调 loomgui_clipboard_get，ptr+len 转 String */ }
pub fn write_clipboard(s: &str) { /* 调 loomgui_clipboard_set */ }

pub fn selected_text(e: &EditState) -> String {
    let (b, end) = e.selection_range(); e.value[b..end].to_string()
}
pub fn copy_selection(e: &EditState) -> String { let s = selected_text(e); write_clipboard(&s); s }
pub fn cut_selection(e: &mut EditState, kind: NodeKind) -> bool {
    let s = selected_text(e); write_clipboard(&s); delete_selection(e)
}
pub fn paste(e: &mut EditState, kind: NodeKind) -> bool {
    insert_text(e, kind, &read_clipboard())
}
```

- [ ] **Step 3: FFI（lib.rs，后端实现转 Unity systemCopyBuffer）**

```rust
#[csbindgen]
pub unsafe extern "C" fn loomgui_clipboard_set(text: *const u8, len: usize) -> i32 { ... }
#[csbindgen]
pub unsafe extern "C" fn loomgui_clipboard_get(out: *mut *const u8, out_len: *mut usize) -> i32 { ... }
```
（C# 侧 UnityLoomBackend 实现：set/get 转 `GUIUtility.systemCopyBuffer`。本轮纯文本。）

- [ ] **Step 4: process_keys 接 Ctrl+C/X/V（Task 10 路由 match 加）**

```rust
KEY_C if ctrl => { copy_selection(e); continue_with_route }
KEY_X if ctrl => { if cut_selection(e, kind) { changed = true; } continue_with_route }
KEY_V if ctrl => { if paste(e, kind) { changed = true; } continue_with_route }
KEY_A if ctrl => { e.anchor = 0; e.cursor = e.value.len(); continue_with_route }
```

- [ ] **Step 5: 跑测试 + sync bindings** — `cargo test -p loomgui_ffi_c clipboard` PASS；`cargo run -p xtask -- sync-bindings`。

- [ ] **Step 6: Commit**

```bash
git add crates/ffi/src/lib.rs crates/core/src/scene/control.rs crates/ffi/src/tests.rs
git commit -m "feat(ffi): clipboard (copy/cut/paste) + Ctrl+A/C/X/V routing"
```

---

## Task 15: value/selection/placeholder/readonly FFI + caret-color/selection-background style

**Files:**
- Modify: `crates/ffi/src/lib.rs`、`crates/core/src/style/resolved.rs`（加 caret_color/selection_background 字段）、`crates/core/src/style/mapping.rs`（CSS mapping）
- Test: `crates/ffi/src/tests.rs`

**Interfaces:**
- Consumes: `EditState`、ResolvedStyle
- Produces: `set/get_control_text`、`set/get_selection`、`set/get_control_placeholder`、`set_control_readonly/maxlength`；style 加 `caret_color`/`selection_background`

- [ ] **Step 1: 写失败测试**

```rust
#[test] fn ffi_set_get_control_text() {
    let stage = make_stage_with_textfield("old");
    unsafe { assert_eq!(loomgui_stage_set_control_text(stage.handle, id, str_ptr("new"), 3), 0); }
    stage.tick();
    let mut buf = [0u8; 16]; let mut len = 0usize;
    unsafe { loomgui_stage_get_control_text(stage.handle, id, buf.as_mut_ptr(), &mut len); }
    assert_eq!(&buf[..len], b"new");
}
#[test] fn ffi_set_selection() {
    let stage = make_stage_with_textfield("hello");
    unsafe { loomgui_stage_set_selection(stage.handle, id, 1, 3); }  // 选 "el"
    assert_textfield_selection(&stage, 1, 3);
}
```

- [ ] **Step 2: style 加字段（resolved.rs ResolvedStyle）**

```rust
pub caret_color: Option<[f32; 4]>,           // caret-color CSS（缺省 = color）
pub selection_background: Option<[f32; 4]>,   // ::selection bg（缺省 蓝半透）
pub selection_color: Option<[f32; 4]>,        // ::selection 文本色（缺省 白）
```
mapping.rs 加 `caret-color`/`selection-background`/`selection-color` CSS prop 解析。fence schema 加属性白名单。

- [ ] **Step 3: FFI（lib.rs，return-code + out-param）**

```rust
#[csbindgen] pub unsafe extern "C" fn loomgui_stage_set_control_text(h, node, text, len) -> i32 { ... }
#[csbindgen] pub unsafe extern "C" fn loomgui_stage_get_control_text(h, node, out: *mut u8, out_len: *mut usize) -> i32 { ... }
#[csbindgen] pub unsafe extern "C" fn loomgui_stage_set_selection(h, node, start, end) -> i32 { ... }
#[csbindgen] pub unsafe extern "C" fn loomgui_stage_get_selection(h, node, *start, *end) -> i32 { ... }
#[csbindgen] pub unsafe extern "C" fn loomgui_stage_set_control_placeholder(h, node, text, len) -> i32 { ... }
#[csbindgen] pub unsafe extern "C" fn loomgui_stage_set_control_readonly(h, node, bool) -> i32 { ... }
#[csbindgen] pub unsafe extern "C" fn loomgui_stage_set_control_maxlength(h, node, usize) -> i32 { ... }
```
（set_control_text 改 EditState.value + cursor 末尾；get_control_text 用 ptr+len out-param。）

- [ ] **Step 4: 跑测试 + sync bindings** — `cargo test -p loomgui_ffi_c control_text\|selection` PASS；`cargo run -p xtask -- sync-bindings`。

- [ ] **Step 5: Commit**

```bash
git add crates/ffi/src/lib.rs crates/core/src/style/resolved.rs crates/core/src/style/mapping.rs crates/ffi/src/tests.rs
git commit -m "feat(ffi): control text/selection/placeholder/readonly + caret-color/selection style"
```

---

## Task 16: C# 投影填壳 + Submitted 事件 demux

**Files:**
- Modify: `unity/package/Runtime/Public/LoomGUI.Nodes.cs`（TextField/Password/Search/TextArea 壳 line 1279-1640）、`unity/package/Runtime/LoomGUI.EventType.cs`、`unity/package/Runtime/Projection/EventDemuxer.cs`
- Test: `tests/dotnet/LoomGUI.HeadlessTests/`

**Interfaces:**
- Consumes: FFI（Task 13/15）、事件 demux
- Produces: 控件 class 填实 + Submitted demux

- [ ] **Step 1: 填壳（Nodes.cs，仿 P1 Slider/ProgressBar）**

```csharp
public class TextField : Node {
    public string Value { get => Ffi.get_control_text(ctx, id); set => Ffi.set_control_text(ctx, id, value); }
    public string Placeholder { get => ...; set => ...; }
    public TextSelection Selection { get => ...; set => Ffi.set_selection(ctx, id, value.Start, value.End); }
    public bool ReadOnly { get => ...; set => Ffi.set_control_readonly(ctx, id, value); }
    public bool Disabled { /* 同 P1 */ }
    public event Action<ValueChangedEvent<string>> ValueChanged { add{...} remove{...} }  // 接 demux EVT_VALUE_CHANGED
    public event Action<string> Submitted { add{...} remove{...} }  // 接 demux EVT_SUBMITTED
}
// PasswordField / SearchField / TextArea 同构（TextArea 无 Submitted）
```

- [ ] **Step 2: EventType.cs 加 Submitted**

```csharp
public enum EventType : byte { ..., Submitted = 25 }
```

- [ ] **Step 3: EventDemuxer demux（按 evt_type 分发）**

Submitted → TextField.Submitted；ValueChanged → TextField.ValueChanged（payload = current value via get_control_text）。

- [ ] **Step 4: Headless 测试**

```csharp
[Fact] public void textfield_value_roundtrips_via_ffi() { ... }
[Fact] public void textfield_submitted_fires_on_enter() { ... }  // set_key_input RETURN + tick + borrow_events
[Fact] public void textfield_textinput_appends_chars() { ... }  // set_text_input + tick
```

- [ ] **Step 5: 跑 Headless + PublicApi 编译门** — `dotnet test` HeadlessTests PASS；`dotnet build` PublicApi 编译门 PASS。

- [ ] **Step 6: Commit**

```bash
git add unity/package/Runtime/Public/LoomGUI.Nodes.cs unity/package/Runtime/LoomGUI.EventType.cs unity/package/Runtime/Projection/EventDemuxer.cs tests/dotnet/LoomGUI.HeadlessTests/
git commit -m "feat(csharp): fill TextField/Password/Search/TextArea projection + Submitted demux"
```

---

## Task 17: 围栏控件 CSS 命中校验扩到 input/textarea

**Files:**
- Modify: `crates/fence/src/`（P1 已有控件 CSS 命中校验 pass）、`docs/design/fence.md`
- Test: `crates/fence/tests/`

**Interfaces:**
- Consumes: fence cascade resolve（控件节点是否被规则匹配）
- Produces: input/textarea 无 CSS 命中 → 打包期报错 + 教学

- [ ] **Step 1: 写失败测试**

```rust
#[test] fn text_input_without_css_errors() {
    let html = r#"<input type="text" value="x">"#;
    let diags = run_fence(html);
    assert!(diags.iter().any(|d| d.message.contains("input") && d.message.contains("CSS")));
}
#[test] fn textarea_with_css_passes() {
    let html = r#"<style>textarea{background:#fff}</style><textarea></textarea>"#;
    assert!(run_fence(html).is_empty());
}
```

- [ ] **Step 2: 扩校验 pass（P1 的控件 CSS 命中校验，加 input/textarea/PasswordField/SearchField 到控件 kind 列表）**

教学文案：LoomGUI 控件不带默认样式，`<input>` / `<textarea>` 需 CSS 规则（建议为它们提供 background/border + caret-color）。

- [ ] **Step 3: 同步 fence.md**

- [ ] **Step 4: 跑测试** — `cargo test -p loomgui_fence control_css` PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/fence/src/ crates/fence/tests/ docs/design/fence.md
git commit -m "feat(fence): require CSS match for input/textarea (no UA stylesheet)"
```

---

## Task 18: showcase 文本控件 CSS + 交互演示 + 重打 pkg + 重编 dll

**Files:**
- Modify: `showcase/showcase/form.html,settings.html,mail.html`
- Test: 人工 Unity PlayMode + dump 验证

**Interfaces:**
- Consumes: 全部前序 task

- [ ] **Step 1: 给 showcase 文本控件配 CSS**

每个用 input/textarea 的页面加 `<style>`：标准边框、caret-color、selection-background、placeholder 色。form 覆盖 text/password/search/textarea，settings 用 search/text，mail 用 textarea。

- [ ] **Step 2: 加交互演示**

form 的 text 输入回车触发 Submitted；textarea 多行编辑；password 掩码显示。

- [ ] **Step 3: 重打 pkg + 重编 dll**

```bash
cargo run -p loomgui_pkg -- build showcase
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```

- [ ] **Step 4: dump_page 验证 core 状态**

`cargo run -p loomgui_core --example dump_page -- <pkg>` 验证 TextField value 渲染 + 光标位置 + 选区。

- [ ] **Step 5: Commit**

```bash
git add showcase/ unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
git commit -m "feat(showcase): text control CSS + interactive demos (textfield/textarea/IME)"
```

---

## Self-Review 记录

**Spec 覆盖**：§1-2（背景+渲染模型）→ 全局约束；§3（数据模型 EditState/行模型/密码）→ Task 3/6/4；§4（渲染链路+几何）→ Task 4/5/6/12；§5（字符输入通道）→ Task 9/10；§6（编辑原语）→ Task 8；§7（光标闪烁）→ Task 7；§8（FFI+攒批）→ Task 9/13/14/15；§9（C# 投影）→ Task 16；§10（事件）→ Task 11；§2.4（IME 架构）→ Task 13；§11（defer）→ 不在 plan；§12（showcase）→ Task 18；§2.3（围栏校验）→ Task 17。全覆盖。

**类型一致性**：`EditInit`（Task1）↔ `EditState`（Task3）字段对齐（value/placeholder/max_length/readonly）；`transform_display_value`（Task4）→ `display_value`（Task13，加 composition 拼接）；`cursor_pixel_x`/`hit_byte_offset`/`line_byte_ranges`（Task6）在 Task7/12 复用一致；`EVT_SUBMITTED=25`（Task11）→ C# demux（Task16）对齐；`set_composition`/`commit_composition`（Task13）FFI ↔ core control.rs 一致。

**Placeholder 检查**：无 TBD/TODO； KeyCode 数值明确标注「实现时查 UnityEditor.xml 勿用记忆值」；`with_text_edit_mut`/`push_solid_quad`/`CursorRectRepr` 等 helper 标注了定义位置。

**注意点**（实现时留意）：
- Task 5 `bake_content_offset` 当前在 render/mod.rs，需 pub(crate) 或重导出到 control 复用
- Task 7 stage process 接 hook 要参照 P1 Slider 拖拽接入方式（PointerState.process 返回值）
- Task 12 caret-color/selection-background 在 Task 15 才加 style 字段，Task 12 先用常量缺省色
- Task 13 PasswordField 的 composition pos 在掩码后用 char 计数定位（byte pos 失效），实现时注意
- Task 10 KeyCode 数值必须查 UnityEditor.xml + input.rs:41 modifiers 位定义，勿用记忆值
- TextArea 跨行导航（上下方向键按行盒 y + sticky ideal_x）本轮做基础（Task 8 move_cursor 只做左右，上下在 Task 10 接但理想 x sticky 简化）
