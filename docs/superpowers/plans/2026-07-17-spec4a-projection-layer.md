# Spec-4a 投影层 + core inline override 层 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 填满 `Public/LoomGUI.*.cs` 的 336 个 `NotImplementedException` 壳，建成"真身 Rust + C# OOP 投影"的后端对象层第 1 棒：core inline override（便签层）+ Rust FFI 缺口 + C# 投影壳 + typed 事件层 + headless harness，全部编码机 headless 验收。

**Architecture:** core 加一个运行时 inline_override 维度（折进现有 set_map，propagate 复用不改）；C# 投影壳裹 NodeId 强引用缓存 + 稀疏镜像 + seam（即时过桥，攒批留 seam）；事件层在旧 EventHandler demux 之上加 typed struct + On<T>；headless harness 直接 P/Invoke native dll，不碰 Unity 渲染。

**Tech Stack:** Rust edition 2021（core + ffi_c，依赖钉版本 taffy 0.5/cssparser 0.34/csbindgen 1）；C# net10.0（xUnit 2.9.2）；csbindgen FFI；pkg.bin v18（不动）。

**Spec:** `docs/superpowers/specs/2026-07-17-spec4a-projection-layer-design.md`

## Global Constraints

- **Rust edition 2021**，依赖钉版本：taffy 0.5、ttf-parser 0.20、cssparser 0.34、scraper 0.19、slotmap 1.1、csbindgen 1。勿改版本。
- **inline_override 纯运行时 transient，不进 pkg.bin**——不改 pkg 格式版本（仍 v18），不改 TemplateNode 结构。只动运行时 `Node`。
- **C# `NodeKind` enum 对齐 Rust `NodeKind` 的 `#[repr(u8)]` 判别值**——值必须与 `crates/core/src/scene/node.rs` 的 NodeKind 变体顺序一致。
- **FFI return-code + out-param 模式**（Spec-3 ③ 定）：导出函数返 `i32`（0=ok），输出走 `*mut T` out-param，不靠 0 哨兵。
- **任何 Rust 改动后必须重编 .dll + commit + binding sync**：`cargo build -p loomgui_ffi_c --release` → 拷 dll → `cargo run -p xtask -- sync-bindings`（CLAUDE.md）。
- **push 前本地跑 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`**，否则 CI 红。
- **公共签名冻结**：`Public/LoomGUI.*.cs` 的 public 成员签名不许改（编译门 `tests/dotnet/LoomGUI.PublicApi`）。只许改方法体 + 加 internal 字段。
- **C# Style 路径只走 `set_inline_override`，严禁 `set_style`**（set_style 写 base_style，污染设计期基线）。
- **headless harness 不碰 Unity 渲染**（MirrorPool/MaterialManager 不链接），直接 P/Invoke。
- **坐标 y-down**（核心），y-flip 是后端根一次变换，harness 不 flip。
- 注释写上线品质（自包含、说 WHY、不引用坑号）。

---

## File Structure

**Rust（core + ffi）**
- `crates/core/src/scene/node.rs` — Modify：`Node` 加 `inline_override: ResolvedStyle` + `inline_set: InheritedSet` 字段（+ Default + 构造点初始化）。
- `crates/core/src/style/dynamic.rs` — Modify：`rematch_pseudo_classes` 加 inline_override 应用步（折进 set_map）。
- `crates/core/src/scene/dynamic.rs` — Modify：加 `set_inline_override` / `unset_inline_override` / `get_children` / `get_child_count` / `add_class` / `remove_class` / `has_class` core API。
- `crates/core/src/stage.rs` — Modify：Stage 方法 wrap 上述 core API（参照现有 `set_style`/`find_node_by_id` 模式）。
- `crates/ffi/src/lib.rs` — Modify：导出 7 个新 FFI（参照 `loomgui_stage_set_style` / `loomgui_stage_get_node_kind` 模式）。

**C#（投影层实现，填 Public 壳）**
- `unity/package/Runtime/Public/LoomGUI.Nodes.cs` — Modify：Node/Container/NodeStyle/NodeTransform/NodeGeometry/ClassList 方法体 throw NE → 实现 + 加 internal 字段。
- `unity/package/Runtime/Public/LoomGUI.Events.cs` — Modify：16 event struct 加 RouteEventCore 字段 + IRouteEvent 转发。
- `unity/package/Runtime/Public/LoomGUI.Types.cs` — Modify：加 `NodeKind` enum（如未在别处）。
- `unity/package/Runtime/Projection/NodeRegistry.cs` — Create：internal，`Dictionary<uint, Node>` 强引用缓存 + 生命周期。
- `unity/package/Runtime/Projection/NodeFactory.cs` — Create：internal，NodeKind→Type 子类工厂 + lazy 构造。
- `unity/package/Runtime/Projection/StyleMirror.cs` — Create：internal，稀疏镜像 + `FlushInline` seam。
- `unity/package/Runtime/Projection/CssValueConvert.cs` — Create：internal，typed↔CSS 串转换。
- `unity/package/Runtime/Projection/EventBus.cs` — Create：internal，On<T> 订阅表 + demux 接线。
- `unity/package/Runtime/Projection/LoomBindings.cs` — Create：internal，集中 P/Invoke 声明（或复用 Plugins LoomGUIBindings.cs）。

**测试**
- `tests/dotnet/LoomGUI.HeadlessTests/` — Create：新 xUnit csproj + harness（Stage handle 工厂）+ 验收测试。
- `tests/dotnet/LoomGUI.HeadlessTests/fixtures/` — Create：预打 fixture pkg.bin + HTML 源。
- `tests/dotnet/LoomGUI.PublicApi/LoomGUI.PublicApi.csproj` — Modify：加 Projection + Bindings 链接（实现依赖）。

**pkg fixture**
- 用 `loom-pkg` 预打 1–2 个最小测试 workspace → `tests/dotnet/LoomGUI.HeadlessTests/fixtures/*.pkg.bin`（入库）。

---

## Task 依赖概览

```
阶段 A（Rust core）：A1 字段 → A2 rematch → A3 inline FFI → A4 child FFI → A5 class FFI → A6 .dll+bindings
阶段 B（C# 骨架）：B1 值转换 → B2 NodeKind → B3 harness（依赖 A6 .dll）
阶段 C（C# 投影壳）：C1 Node基础 → C2 工厂 → C3 Style镜像 → C4 Geometry/Transform → C5 ClassList → C6 Container树 → C7 Get/Query
阶段 D（事件）：D1 RouteEventCore+16struct → D2 On<T>订阅 → D3 demux+语义糖
阶段 E（收尾）：E1 UIContext/Package → E2 fixture pkg → E3 验收门
```

每阶段结束有独立可验交付（A: cargo test core；B: harness smoke；C: 投影壳单测；D: 事件单测；E: 端到端验收门）。

---

## 阶段 A：core inline override 层 + FFI 缺口（Rust）

> 核心难点都在这阶段（便签层 + 继承交互）。C# 投影壳（阶段 B–E）是建立在 A 的 .dll 之上。

### Task A1: Node 加 inline_override/inline_set 字段 + InlineSet 位图基础设施

**Files:**
- Modify: `crates/core/src/scene/node.rs:217`（Node struct 加字段）
- Modify: `crates/core/src/scene/node.rs:244`（Default impl 加初始化）
- Modify: 所有 `Node {` 构造点（grep `rg "Node \{" crates/core/src` 找全：至少 `scene/dynamic.rs:83` create_node、instantiate 路径）
- Modify: `crates/core/src/style/dynamic.rs`（INH_* 定义处，加 INLINE_* 非继承 bit + INH_ALL_MASK + InlineSet 类型）
- Test: `crates/core/src/scene/node/tests.rs`

**Interfaces:**
- Produces: `Node.inline_override: ResolvedStyle`、`Node.inline_set: InlineSet`、`InlineSet(pub u32)` 类型、`inline_bit(prop: &str) -> Option<u32>`、`INH_ALL_MASK: u32`

- [ ] **Step 1: 定位 INH_* 位图定义，加非继承属性 bit**

`rg "INH_FONT_SIZE" crates/core/src` 找到 InheritedSet + INH_* 常量定义处。InheritedSet 现覆盖 9 个继承属性（font_size/color/font_family/font_weight/text_align/line_height/letter_spacing/white_space_nowrap）。在同处加：

```rust
/// inline override 的 set-ness 位图。复用 INH_* 给继承属性（前 9 bit），
/// 其后是非继承属性 bit。rematch 用它应用便签层；继承子集 OR 进 set_map 让 propagate 自动传播。
pub struct InlineSet(pub u32);

/// 所有继承属性 bit 的 OR——rematch 用它把 inline 的继承部分并进 set_map。
pub const INH_ALL_MASK: u32 = INH_FONT_SIZE | INH_COLOR | INH_FONT_FAMILY
    | INH_FONT_WEIGHT | INH_TEXT_ALIGN | INH_LINE_HEIGHT
    | INH_LETTER_SPACING | INH_WHITE_SPACE_NOWRAP;  // 按 InheritedSet 实际常量补齐

// 非继承属性 bit（编号接在继承属性之后，避开 INH_* 占用位）
pub const INLINE_WIDTH: u32 = 1 << 9;
pub const INLINE_HEIGHT: u32 = 1 << 10;
pub const INLINE_MIN_WIDTH: u32 = 1 << 11;
// ... 其余 NodeStyle 非继承属性：min/max_height、display、flex_direction、flex_wrap、
//     justify_content、align_items、gap、padding、margin、border_width、overflow_x/y、
//     left/top/right/bottom、position、z_index、background_color、opacity、visibility。
//     每个 1 个 bit，编号递增不撞 INH_*。

/// prop 名 → InlineSet bit。继承属性复用 inherited_bit，非继承属性走 INLINE_*。
/// 返回 None = 该属性不可 inline（不应发生，apply_decl 能处理的都能 inline）。
pub fn inline_bit(prop: &str) -> Option<u32> {
    if let Some(b) = inherited_bit(prop) { return Some(b); }
    match prop {
        "width" => Some(INLINE_WIDTH),
        "height" => Some(INLINE_HEIGHT),
        // ... 其余非继承属性映射（与 INLINE_* 常量一致）
        _ => None,
    }
}
```

- [ ] **Step 2: Node struct 加字段**

`crates/core/src/scene/node.rs:217` 的 `pub struct Node { ... }`，在 `data_controller: Option<String>,` 后加：

```rust
    /// 运行时 inline override（便签层）。C# Style.X=v 经 set_inline_override 写入；
    /// rematch 在动态规则后应用（最高优先级）。默认空 = 无 inline override。
    /// 纯运行时 transient，不进 pkg.bin。
    pub inline_override: ResolvedStyle,
    /// inline_override 里哪些字段被设了（继承属性复用 INH_* bit，非继承用 INLINE_*）。
    pub inline_set: InlineSet,
```

- [ ] **Step 3: Default + 所有构造点初始化**

`node.rs:244` Default impl + 每个 `Node { ... }` 构造点加：
```rust
            inline_override: ResolvedStyle::default(),
            inline_set: InlineSet(0),
```

- [ ] **Step 4: 写失败测试 + 跑**

```rust
#[test]
fn node_inline_override_defaults_empty() {
    let n = Node::default();
    assert_eq!(n.inline_set.0, 0, "inline_set 默认空");
}
```
Run: `cargo test -p loomgui_core node_inline_override -- --nocapture`
Expected: 先编译错（字段不存在）→ 加字段后 PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/scene/node.rs crates/core/src/style/dynamic.rs crates/core/src/scene/node/tests.rs
git commit -m "feat(core): add Node.inline_override + inline_set (便签层基础设施)"
```

---

### Task A2: rematch 折进 inline_override（propagate 零改）

**Files:**
- Modify: `crates/core/src/style/dynamic.rs:329`（`rematch_pseudo_classes`）
- Test: `crates/core/src/style/dynamic_tests.rs`（或同 module tests）

**Interfaces:**
- Consumes: A1 的 `Node.inline_override` / `inline_set` / `INH_ALL_MASK`
- Produces: inline_override 在 rematch 中生效（最高优先级）；继承属性经 set_map 自动传播

- [ ] **Step 1: 写失败测试——父 inline color → 子继承 + ③ probe 不回归**

```rust
#[test]
fn inline_override_color_inherits_to_child() {
    // 建树 root → child（child 无自身 color 声明）
    let (mut scene, root, child) = build_parent_child();
    // 父 inline 设 color:red
    set_inline_override(&mut scene, root, "color:#ff0000").unwrap();
    rematch_pseudo_classes(&mut scene);  // 应用 inline + cascade
    // 子（未自设 color）应继承父的 inline 值
    let child_color = scene.get(child).unwrap().style.color;
    assert_eq!(child_color, Some([1.0, 0.0, 0.0, 1.0]));
}

#[test]
fn inline_override_unset_falls_back() {
    let (mut scene, root, _child) = build_parent_child();
    set_inline_override(&mut scene, root, "color:#ff0000").unwrap();
    unset_inline_override(&mut scene, root, "color").unwrap();
    rematch_pseudo_classes(&mut scene);
    // color 回落 base/rules（非 red）
    assert_ne!(scene.get(root).unwrap().style.color, Some([1.0,0.0,0.0,1.0]));
}

#[test]
fn spec3_probe_no_regress_when_no_inline() {
    // 复用 Spec-3 ③ probe：无 inline 写，rematch 行为不变（已有 probe 测试重跑应仍绿）
    // 这条是回归门：inline_set 空时 inline 应用步 no-op
    let (mut scene, root) = build_simple_tree();
    assert_eq!(scene.get(root).unwrap().inline_set.0, 0);
    rematch_pseudo_classes(&mut scene);  // 不 panic、行为同 Spec-3
}
```
Run: `cargo test -p loomgui_core inline_override -- --nocapture`
Expected: FAIL（inline_override 还没在 rematch 应用，color 不继承）。

- [ ] **Step 2: 在 rematch 动态规则应用后、`set_map.insert` 前加 inline 应用步**

`style/dynamic.rs`，在 `for (_, _, _, r) in &matched { ... }` 循环（377-385）之后、`set_map.insert(node_id, inh);`（386）之前插入：

```rust
        // inline_override 应用（最高优先级，动态规则之后）。
        // 按 inline_set 把 inline_override 字段拷进 new_style；继承子集 OR 进 set_map，
        // 使 propagate 把含 inline 的父值传给子、且本节点自身不被父覆盖。
        // inline_set 默认空 → 对没设 inline 的节点 no-op（Spec-3 probe 不回归）。
        {
            let n_ref = scene.get(node_id).expect("live node");
            let inline_set = n_ref.inline_set;
            if inline_set.0 != 0 {
                let inline = n_ref.inline_override.clone();
                apply_inline_override(&mut new_style, &inline, inline_set);
                inh.0 |= inline_set.0 & INH_ALL_MASK;  // 只把继承子集并进 set_map
            }
        }
```

同文件加 helper：

```rust
/// 按 set 位图把 inline_override 字段拷进 style（最高优先级覆盖，全属性）。
fn apply_inline_override(style: &mut ResolvedStyle, inline: &ResolvedStyle, set: InlineSet) {
    macro_rules! cpy { ($f:ident, $bit:expr) => { if set.0 & $bit != 0 { style.$f = inline.$f.clone(); } }; }
    // 继承属性（复用 INH_*）
    cpy!(font_size, INH_FONT_SIZE); cpy!(color, INH_COLOR); cpy!(font_family, INH_FONT_FAMILY);
    cpy!(font_weight, INH_FONT_WEIGHT); cpy!(text_align, INH_TEXT_ALIGN); cpy!(line_height, INH_LINE_HEIGHT);
    cpy!(letter_spacing, INH_LETTER_SPACING); cpy!(white_space_nowrap, INH_WHITE_SPACE_NOWRAP);
    // 非继承属性（INLINE_*，与 NodeStyle 一一对应）
    cpy!(width, INLINE_WIDTH); cpy!(height, INLINE_HEIGHT);
    // ... 其余 INLINE_* 字段（min/max_*, display, flex_*, gap, padding, margin, border_width,
    //     overflow_x/y, left/top/right/bottom, position, z_index, background_color, opacity, visibility）
}
```

> 注：字段名以 `ResolvedStyle` 实际成员为准（`rg "pub struct ResolvedStyle"` 核对）。`propagate_inherited` **不改**——它已读父 effective 值（含 inline）传子，继承自动正确。

- [ ] **Step 3: 跑测试通过**

Run: `cargo test -p loomgui_core inline_override spec3_probe -- --nocapture`
Expected: 3 条全 PASS。`cargo test -p loomgui_core`（全 core）仍绿（无回归）。

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/style/dynamic.rs crates/core/src/style/dynamic_tests.rs
git commit -m "feat(core): apply inline_override in rematch (便签层, propagate 零改)"
```

---

### Task A3: set_inline_override / unset_inline_override core API

**Files:**
- Modify: `crates/core/src/scene/dynamic.rs`（加 core API，紧邻现有 `set_style` at line 260）
- Test: `crates/core/src/scene/dynamic_tests.rs`

**Interfaces:**
- Consumes: A1 的 `inline_bit` / `InlineSet`
- Produces: `set_inline_override(scene, node, css) -> Result<(), String>`、`unset_inline_override(scene, node, prop) -> Result<(), String>`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn set_inline_override_sets_bit_and_value() {
    let (mut scene, root) = build_simple_tree();
    set_inline_override(&mut scene, root, "width:100px").unwrap();
    let n = scene.get(root).unwrap();
    assert!(n.inline_set.0 & INLINE_WIDTH != 0, "width bit 置位");
    assert_eq!(n.inline_override.width, Length::Px(100.0));  // 按 ResolvedStyle.width 实际类型
}

#[test]
fn unset_inline_override_clears_bit() {
    let (mut scene, root) = build_simple_tree();
    set_inline_override(&mut scene, root, "width:100px").unwrap();
    unset_inline_override(&mut scene, root, "width").unwrap();
    assert_eq!(scene.get(root).unwrap().inline_set.0 & INLINE_WIDTH, 0, "width bit 清");
}
```
Run: `cargo test -p loomgui_core set_inline_override -- --nocapture`
Expected: FAIL（函数未定义）。

- [ ] **Step 2: 实现 core API**

`scene/dynamic.rs`，紧邻 `set_style`（line 260）加：

```rust
/// 运行时 inline override（便签层）。apply_css 到 inline_override + 置 inline_set bit。
/// 下帧 rematch 以最高优先级应用。C# Style.X=v 走这里（不走 set_style）。
pub fn set_inline_override(scene: &mut Scene, node: NodeId, css: &str) -> Result<(), String> {
    let n = scene.get_mut(node).ok_or("node not live")?;
    for decl in css.split(';') {
        let decl = decl.trim();
        if decl.is_empty() { continue; }
        if let Some((prop, val)) = decl.split_once(':') {
            let prop = prop.trim();
            if apply_decl(&mut n.inline_override, prop, val.trim()) {
                if let Some(bit) = inline_bit(prop) {
                    n.inline_set.0 |= bit;
                }
            }
        }
    }
    n.dirty_mesh = true;
    Ok(())
}

/// 撤销单个 inline 属性：清 inline_set bit → 下帧 rematch 该属性回落 base/rules。
pub fn unset_inline_override(scene: &mut Scene, node: NodeId, prop: &str) -> Result<(), String> {
    let n = scene.get_mut(node).ok_or("node not live")?;
    if let Some(bit) = inline_bit(prop) {
        n.inline_set.0 &= !bit;
    }
    n.dirty_mesh = true;
    Ok(())
}
```

- [ ] **Step 3: 跑测试通过**

Run: `cargo test -p loomgui_core set_inline_override unset_inline_override -- --nocapture`
Expected: PASS。

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/scene/dynamic.rs crates/core/src/scene/dynamic_tests.rs
git commit -m "feat(core): set/unset_inline_override API"
```

---

### Task A4: get_children / get_child_count core API

**Files:**
- Modify: `crates/core/src/scene/dynamic.rs`（加 API）
- Test: `crates/core/src/scene/dynamic_tests.rs`

**Interfaces:**
- Consumes: `Node.children: Vec<NodeId>`（已有）
- Produces: `get_child_count(scene, node) -> Option<usize>`、`get_children(scene, node) -> Option<Vec<NodeId>>`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn get_children_returns_node_children() {
    let (mut scene, root, child) = build_parent_child();
    assert_eq!(get_child_count(&scene, root), Some(1));
    assert_eq!(get_children(&scene, root), Some(vec![child]));
    assert_eq!(get_child_count(&scene, child), Some(0));
}
```
Run: `cargo test -p loomgui_core get_children -- --nocapture` → FAIL（未定义）。

- [ ] **Step 2: 实现**（`scene/dynamic.rs`）

```rust
pub fn get_child_count(scene: &Scene, node: NodeId) -> Option<usize> {
    scene.get(node).map(|n| n.children.len())
}
pub fn get_children(scene: &Scene, node: NodeId) -> Option<Vec<NodeId>> {
    scene.get(node).map(|n| n.children.clone())
}
```

- [ ] **Step 3: 跑过 + commit**

```bash
cargo test -p loomgui_core get_children -- --nocapture
git add crates/core/src/scene/dynamic.rs crates/core/src/scene/dynamic_tests.rs
git commit -m "feat(core): get_children/get_child_count API"
```

---

### Task A5: add_class / remove_class / has_class core API

**Files:**
- Modify: `crates/core/src/scene/dynamic.rs`
- Test: `crates/core/src/scene/dynamic_tests.rs`

**Interfaces:**
- Consumes: `Node.classes: Vec<String>`（已有）
- Produces: `add_class` / `remove_class` / `has_class`（add/remove 标 dirty 触发 rematch）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn class_ops_mutate_and_flag_dirty() {
    let (mut scene, root) = build_simple_tree();
    add_class(&mut scene, root, "active").unwrap();
    assert!(has_class(&scene, root, "active").unwrap());
    assert!(scene.get(root).unwrap().dirty_mesh, "add 标 dirty 触发 rematch");
    remove_class(&mut scene, root, "active").unwrap();
    assert!(!has_class(&scene, root, "active").unwrap());
    // 重复 add 不重复 push
    add_class(&mut scene, root, "x").unwrap();
    add_class(&mut scene, root, "x").unwrap();
    assert_eq!(scene.get(root).unwrap().classes.iter().filter(|c| **c=="x").count(), 1);
}
```
Run: `cargo test -p loomgui_core class_ops -- --nocapture` → FAIL。

- [ ] **Step 2: 实现**

```rust
pub fn add_class(scene: &mut Scene, node: NodeId, name: &str) -> Result<(), String> {
    let n = scene.get_mut(node).ok_or("node not live")?;
    if !n.classes.iter().any(|c| c == name) { n.classes.push(name.to_string()); }
    n.dirty_mesh = true;
    Ok(())
}
pub fn remove_class(scene: &mut Scene, node: NodeId, name: &str) -> Result<(), String> {
    let n = scene.get_mut(node).ok_or("node not live")?;
    n.classes.retain(|c| c != name);
    n.dirty_mesh = true;
    Ok(())
}
pub fn has_class(scene: &Scene, node: NodeId, name: &str) -> Option<bool> {
    scene.get(node).map(|n| n.classes.iter().any(|c| c == name))
}
```

- [ ] **Step 3: 跑过 + commit**

```bash
cargo test -p loomgui_core class_ops -- --nocapture
git add crates/core/src/scene/dynamic.rs crates/core/src/scene/dynamic_tests.rs
git commit -m "feat(core): add/remove/has_class API"
```

---

### Task A6: Stage wrap + FFI 导出 + .dll 重编 + binding sync

**Files:**
- Modify: `crates/core/src/stage.rs`（Stage 方法 wrap，参照 `set_style`/`find_node_by_id` 模式）
- Modify: `crates/ffi/src/lib.rs`（导出 8 个新 FFI，参照 `loomgui_stage_set_style` line 1308 / `loomgui_stage_get_node_kind` line 864 模式）
- Out: `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll` + `Bindings/LoomGUIBindings.cs`

**Interfaces:**
- Consumes: A3/A4/A5 的 core API
- Produces: 8 个 `loomgui_stage_*` FFI（set_inline_override / unset_inline_override / get_child_count / get_children / add_class / remove_class / has_class）+ Stage 方法

- [ ] **Step 1: stage.rs wrap**（紧邻现有 `set_style`/`find_node_by_id`）

```rust
pub fn set_inline_override(&mut self, node: NodeId, css: &str) -> Result<(), String> {
    loomgui_core::scene::set_inline_override(&mut self.scene, node, css)
}
pub fn unset_inline_override(&mut self, node: NodeId, prop: &str) -> Result<(), String> {
    loomgui_core::scene::unset_inline_override(&mut self.scene, node, prop)
}
pub fn get_child_count(&self, node: NodeId) -> Option<usize> {
    loomgui_core::scene::get_child_count(&self.scene, node)
}
pub fn get_children(&self, node: NodeId) -> Option<Vec<NodeId>> {
    loomgui_core::scene::get_children(&self.scene, node)
}
pub fn add_class(&mut self, node: NodeId, name: &str) -> Result<(), String> {
    loomgui_core::scene::add_class(&mut self.scene, node, name)
}
pub fn remove_class(&mut self, node: NodeId, name: &str) -> Result<(), String> {
    loomgui_core::scene::remove_class(&mut self.scene, node, name)
}
pub fn has_class(&self, node: NodeId, name: &str) -> Option<bool> {
    loomgui_core::scene::has_class(&self.scene, node, name)
}
```

- [ ] **Step 2: ffi/lib.rs 导出**（return-code + out-param 模式，参照 line 1308/864）

```rust
#[no_mangle]
pub extern "C" fn loomgui_stage_set_inline_override(
    h: *mut StageHandle, node: u32, css: *const u8, len: usize,
) -> i32 {
    if h.is_null() { return -1; }
    let sh = unsafe { &mut *h };
    let css = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(css, len) }) {
        Ok(s) => s, Err(_) => return -1,
    };
    sh.stage.set_inline_override(NodeId(node), css).map(|_| 0).unwrap_or(-1)
}

#[no_mangle]
pub extern "C" fn loomgui_stage_unset_inline_override(
    h: *mut StageHandle, node: u32, prop: *const u8, len: usize,
) -> i32 { /* 同上模式，调 unset_inline_override */ }

#[no_mangle]
pub extern "C" fn loomgui_stage_get_child_count(h: *const StageHandle, node: u32) -> i32 {
    if h.is_null() { return -1; }
    let sh = unsafe { &*h };
    sh.stage.get_child_count(NodeId(node)).map(|c| c as i32).unwrap_or(-1)
}

/// 写子 NodeId 到 out buffer，返写入数；cap 不够返 -2。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_children(
    h: *const StageHandle, node: u32, out: *mut u32, cap: usize,
) -> i32 {
    if h.is_null() { return -1; }
    let sh = unsafe { &*h };
    match sh.stage.get_children(NodeId(node)) {
        None => -1,
        Some kids => {
            if kids.len() > cap { return -(kids.len() as i32 + 2); }  // 负值表所需 cap
            for (i, k) in kids.iter().enumerate() {
                unsafe { *out.add(i) = k.0; }
            }
            kids.len() as i32
        }
    }
}

#[no_mangle]
pub extern "C" fn loomgui_stage_add_class(h: *mut StageHandle, node: u32, name: *const u8, len: usize) -> i32 {
    /* 同 set_inline_override 模式，调 add_class */ }

#[no_mangle]
pub extern "C" fn loomgui_stage_remove_class(h: *mut StageHandle, node: u32, name: *const u8, len: usize) -> i32 {
    /* 同上，调 remove_class */ }

#[no_mangle]
pub extern "C" fn loomgui_stage_has_class(h: *const StageHandle, node: u32, name: *const u8, len: usize) -> i32 {
    /* 返 0/1，-1 = error；调 has_class */ }
```

- [ ] **Step 3: 加 FFI smoke 测试**（`crates/ffi/src/abi_tests.rs`，验 ABI 不 panic）

```rust
#[test]
fn ffi_inline_override_roundtrip() {
    let (mut stage, root) = build_test_stage_with_root();  // 复用现有 abi_tests helper
    let css = "width:100px";
    assert_eq!(0, loomgui_stage_set_inline_override(stage_ptr, root.0, css.as_ptr(), css.len()));
    // unset + get_child_count/get_children/add_class/remove_class/has_class 各 smoke 一次
}
```

- [ ] **Step 4: 重编 + 拷 dll + sync bindings**（CLAUDE.md 强制，Unity 必须关着）

```bash
cargo fmt --all && cargo clippy -p loomgui_core -p loomgui_ffi_c --all-targets -- -D warnings
cargo test -p loomgui_core -p loomgui_ffi_c
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
cargo run -p xtask -- sync-bindings
```
Expected: 全 core/ffi 测试绿；dll 拷贝成功（Unity 关着）；`LoomGUIBindings.cs` 含 8 个新 entry。

- [ ] **Step 5: Commit**（dll + bindings + 源码一起）

```bash
git add crates/ unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs
git commit -m "feat(ffi): export inline_override/children/class ops + rebuild dll + sync bindings"
```

> **阶段 A 完成门**：`cargo test`（全 workspace）绿；8 个新 FFI 在 bindings；.dll 已更新。core 便签层 + FFI 缺口就绪，C# 可建在上面。

---

## 阶段 B：C# 骨架（值转换 + NodeKind + headless harness）

### Task B1: CssValueConvert（typed ↔ CSS 串）

**Files:** Create `unity/package/Runtime/Projection/CssValueConvert.cs` · Test: `tests/dotnet/LoomGUI.HeadlessTests/CssValueConvertTests.cs`

**Interfaces:** Produces `internal static class CssValueConvert { string ToCss(Length); string ToCss(Color); string ToCss(Thickness); string ToCss(float); }`

- [ ] **Step 1: 写失败测试**
```csharp
[Fact] public void LengthPx() => Assert.Equal("100px", CssValueConvert.ToCss(Length.Px(100)));
[Fact] public void LengthPct() => Assert.Equal("50%", CssValueConvert.ToCss(Length.Pct(50)));
[Fact] public void ColorHex() => Assert.Equal("#ff0000ff", CssValueConvert.ToCss(new Color(1f,0f,0f,1f)));
```
- [ ] **Step 2: 实现** `CssValueConvert`：
  - typed 重载：`Length`（Px→`"{n}px"`、Pct→"{n}%"、Auto→"auto"）、`Color`（→"#rrggbbaa"）、`Thickness`（→"{t} {r} {b} {l}"）、`float`（→ InvariantCulture 串）。
  - **enum → CSS keyword**：NodeStyle 的 enum 属性（DisplayMode/FlexDirection/FlexWrap/JustifyContent/AlignItems/Overflow/Position/Visibility）各映射到 CSS keyword（`DisplayMode.Block→"block"`、`FlexDirection.Row→"row"`、…），照 CSS 标准 + 对照 `crates/core/src/style/mapping.rs` apply_decl 的反向。
  - **`ToCss(object)` 派发重载**（switch 运行时类型派发到上述 typed 重载）——供 StyleMirror（C3）拼 CSS 用。Unset 值由调用方跳过（不进 flush）。
- [ ] **Step 3: 跑过 + commit** `feat(c#): CssValueConvert typed↔css`

---

### Task B2: C# NodeKind enum（对齐 Rust u8）

**Files:** Create `unity/package/Runtime/Projection/NodeKind.cs`

- [ ] **Step 1: 从 Rust 拷变体顺序** —— `rg "pub enum NodeKind" crates/core/src/scene/node.rs -A 30`，按变体声明顺序定义 C# `internal enum NodeKind : byte { Container=0, ... }`，值与 Rust `#[repr(u8)]` 判别值一一对应。
- [ ] **Step 2: 写校验测试**（`get_node_kind` 返回值 == 预期 enum）：建 `<div>` → kind == Container；建文本 → kind == TextNode（对照 Rust 变体）。
- [ ] **Step 3: commit** `feat(c#): NodeKind enum aligned to Rust u8`

---

### Task B3: headless harness（csproj + UIContext internal + Stage 工厂）

**Files:**
- Create: `tests/dotnet/LoomGUI.HeadlessTests/LoomGUI.HeadlessTests.csproj`
- Create: `tests/dotnet/LoomGUI.HeadlessTests/Harness/StageHarness.cs`
- Modify: `unity/package/Runtime/Public/LoomGUI.Nodes.cs`（UIContext 加 internal 构造）

**Interfaces:** Produces `internal static class StageHarness { (IntPtr stage, UIContext ctx) Create(...); }` + `UIContext` internal 构造接 `IntPtr stage`

- [ ] **Step 1: 写 csproj**（链接 Public + Projection + Bindings，拷 dll 到输出）
```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <Nullable>disable</Nullable>
    <IsPackable>false</IsPackable>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Microsoft.NET.Test.Sdk" Version="17.11.1" />
    <PackageReference Include="xunit" Version="2.9.2" />
    <PackageReference Include="xunit.runner.visualstudio" Version="2.8.2" />
  </ItemGroup>
  <ItemGroup>
    <!-- Public 壳 + 投影实现 + bindings -->
    <Compile Include="..\..\..\unity\package\Runtime\Public\LoomGUI.*.cs" Link="Public\%(Filename)%(Extension)" />
    <Compile Include="..\..\..\unity\package\Runtime\Projection\*.cs" Link="Projection\%(Filename)%(Extension)" />
    <Compile Include="..\..\..\unity\package\Plugins\LoomGUI\Bindings\LoomGUIBindings.cs" Link="Bindings\LoomGUIBindings.cs" />
    <Compile Include="..\..\..\unity\package\Runtime\LoomGUIBindings.cs" Link="Bindings\LoomGUIBindings.cs" />  <!-- 实际路径以 grep 为准 -->
  </ItemGroup>
  <ItemGroup>
    <None Include="..\..\..\unity\package\Plugins\LoomGUI\loomgui_ffi_c.dll" CopyToOutputDirectory="PreserveNewest" />
  </ItemGroup>
</Project>
```
（Path/文件名以 `rg "LoomGUIBindings.cs" unity/package` 实际为准；Stubs 若 Public using UnityEngine 则加 `Stubs/UnityEngine.cs`。）

- [ ] **Step 2: UIContext internal 构造**（`LoomGUI.Nodes.cs` UIContext 加）
```csharp
public sealed class UIContext {
    internal IntPtr _stage;            // native StageHandle
    internal NodeRegistry _registry;   // 强引用缓存（C1 实现）
    internal UIContext(IntPtr stage) { _stage = stage; _registry = new NodeRegistry(this); }
    // ... 现有 public 成员方法体在 C/E 阶段填
}
```

- [ ] **Step 3: StageHarness**（P/Invoke loomgui_stage_new，不碰 Unity）
```csharp
internal static class StageHarness {
    public static (IntPtr stage, UIContext ctx) Create(float w = 1280, float h = 720) {
        IntPtr stage = Native.loomgui_stage_new(w, h);
        if (stage == IntPtr.Zero) throw new Exception("stage_new failed");
        return (stage, new UIContext(stage));
    }
    public static void Destroy(IntPtr stage) => Native.loomgui_stage_free(stage);
}
```

- [ ] **Step 4: smoke 测试**（建 stage、create_node、tick，验 P/Invoke 通）
```csharp
[Fact] public void StageCreatesAndTicks() {
    var (stage, ctx) = StageHarness.Create();
    try {
        uint root = Native.loomgui_stage_create_root(stage, "div", "");
        Assert.NotEqual(0xFFFFFFFFu, root);
        Native.loomgui_stage_tick(stage, 0.016f);
    } finally { StageHarness.Destroy(stage); }
}
```
- [ ] **Step 5: 跑过 + commit**
```bash
dotnet test tests/dotnet/LoomGUI.HeadlessTests
git add tests/dotnet/LoomGUI.HeadlessTests/ unity/package/Runtime/Public/LoomGUI.Nodes.cs unity/package/Runtime/Projection/
git commit -m "feat(c#): headless harness + UIContext internal ctor (破两台机瓶颈)"
```

> **阶段 B 完成门**：`dotnet test` harness 绿——编码机能直接 P/Invoke 真 dll 驱动 Stage，不启动 Unity。

---

## 阶段 C：C# 投影壳核心

> 模式：所有 Public 方法体从 `throw NE()` 换成"经 owner NodeId 调 FFI + 维护 NodeRegistry/镜像"。`Node`/`Container` 等 class 加 `internal` 字段（NodeId、Context、IsDisposed 等），public 签名不变。每个 task 一组相关方法 + 验收测试 + commit。

### Task C1: NodeRegistry + Node 基础 + 生命周期

**Files:** Create `Projection/NodeRegistry.cs` · Modify `Public/LoomGUI.Nodes.cs`（Node 类 internal 字段 + Context/Id/Parent/IsDisposed/Dispose/RemoveFromParent）

**Interfaces:** Produces `NodeRegistry.Get/Add/Remove(uint)→Node`、`Node._id/_ctx/_disposed`

- [ ] **Step 1: 写失败测试**
```csharp
[Fact] public void DisposeMarksAndThrows() {
    var (stage, ctx) = StageHarness.Create();
    uint id = Native.loomgui_stage_create_root(stage, "div", "");
    var node = ctx._registry.GetOrCreate(id);
    node.Dispose();
    Assert.True(node.IsDisposed);
    Assert.Throws<ObjectDisposedException>(() => node.Focus());  // 任意操作抛
}
```
- [ ] **Step 2: NodeRegistry**（`Dictionary<uint, Node>` 强引用 + Get/Add/Remove + disposed 检查 helper）
- [ ] **Step 3: Node 加 internal 字段 + 实现 Dispose（递归子 via get_children + remove_node + registry.Remove + 标 _disposed）/ RemoveFromParent（remove_child，不清订阅）/ IsDisposed / Context / Parent（get_parent via FFI）**
  - `Parent`：P/Invoke `loomgui_node_parent`（agent 报告 §2.5 已有），返 NodeId → registry.GetOrCreate；根返 null。
- [ ] **Step 4: 跑过 + commit** `feat(c#): NodeRegistry + Node lifecycle`

---

### Task C2: NodeFactory + 节点子类 + lazy 构造

**Files:** Create `Projection/NodeFactory.cs` · Modify Node 各子类 internal 构造

**Interfaces:** Produces `NodeFactory.CreateTyped(ctx, uint id) -> Node`（get_node_kind → switch → new Container/Button/...）

- [ ] **Step 1: 写失败测试**（Instantiate/create 返回正确类型）
```csharp
[Fact] public void KindDispatchesToType() {
    uint id = create_div();  // <div>
    Node n = ctx._registry.GetOrCreate(id);
    Assert.IsType<Container>(n);
    // <button> → Button；<img> → Image；etc.
}
```
- [ ] **Step 2: NodeFactory**（`loomgui_stage_get_node_kind(ctx._stage, id, out byte kind)` → `(NodeKind)kind` → switch 造对应子类；子类 internal 构造接 `(UIContext ctx, uint id)`）
- [ ] **Step 3: lazy 构造** —— `Children`/`Get` 首次访问时按 `get_children` + get_node_kind 递归 GetOrCreate 子节点。GetOrCreate 内部：缓存命中返回已有，否则 NodeFactory.CreateTyped。
- [ ] **Step 4: 跑过 + commit** `feat(c#): NodeFactory + typed subclass + lazy construct`

---

### Task C3: NodeStyle 稀疏镜像 + FlushInline seam（核心）

**Files:** Create `Projection/StyleMirror.cs` · Modify `Public/LoomGUI.Nodes.cs` NodeStyle

**Interfaces:** Produces `StyleMirror`（稀疏 `Dictionary<string,object>` + `FlushInline()` → set_inline_override）

- [ ] **Step 1: 写失败测试**（写→读镜像 + flush 后 Geometry 变）
```csharp
[Fact] public void StyleWriteReadsBackAndFlushes() {
    var node = ctx.GetRoot();
    node.Style.Width = Length.Px(100);
    Assert.Equal(Length.Px(100), node.Style.Width);   // 读镜像即时
    node.Style.FlushInlineForTest();                   // 即时过桥
    Native.loomgui_stage_tick(ctx._stage, 0.016f);
    // Geometry.LayoutRect.w ≈ 100（滞后一帧，验 seam→set_inline_override→rematch→solve 通）
    var r = Native.loomgui_stage_get_node_layout_rect(ctx._stage, node._id, ...);
    Assert.InRange(r.w, 99, 101);
}
[Fact] public void StyleUnsetFallsBack() {
    node.Style.Width = Length.Px(100); node.Style.FlushInlineForTest();
    node.Style.Width = Length.Unset();  node.Style.FlushInlineForTest();  // unset_inline_override
    // 回落（不再是 100）
}
```
- [ ] **Step 2: StyleMirror**（`Dictionary<string,object>` 存 CSS-prop→typed 值；getter 查字典，无→Unset；setter 写字典；`Style.X = Unset()` → 移除 key）
```csharp
internal sealed class StyleMirror {
    readonly Node _owner;
    readonly Dictionary<string, object> _set = new();
    internal StyleMirror(Node owner) { _owner = owner; }
    internal T? Get<T>(string prop) where T : struct => _set.TryGetValue(prop, out var v) ? (T)v : null;
    internal bool IsSet(string prop) => _set.ContainsKey(prop);
    internal void Set(string prop, object v) => _set[prop] = v;
    internal void Unset(string prop) => _set.Remove(prop);
    internal void FlushInline() {
        if (_set.Count == 0) return;
        var css = string.Join(";", _set.Select(kv => $"{kv.Key}:{CssValueConvert.ToCss(kv.Value)}"));
        Native.loomgui_stage_set_inline_override(_owner._ctx._stage, _owner._id, css, css.Length);
        // ponytail: 即时过桥——攒批版改这里为标脏 + 帧末 flush
    }
}
```
- [ ] **Step 3: NodeStyle 每个 typed setter/getter** 接 StyleMirror（`set => _mirror.Set("width", value); _mirror.FlushInline();` / `get => _mirror.IsSet("width") ? _mirror.Get<Length>("width") : Length.Unset()`）。**严禁走 set_style**。
- [ ] **Step 4: 跑过 + commit** `feat(c#): NodeStyle sparse mirror + FlushInline seam`

---

### Task C4: NodeGeometry（直读 FFI）+ NodeTransform（标脏不 flush）

**Files:** Modify NodeGeometry / NodeTransform

- [ ] **Step 1: NodeGeometry** —— readonly struct 持 NodeId + ctx ref（通过 Node），`LayoutRect`→`loomgui_stage_get_node_layout_rect`，`WorldRect`→`get_node_world_matrix`。4a 直接 FFI 读（ponytail: blob 缓存推后，升级路径见 spec §5）。
- [ ] **Step 2: NodeTransform** —— Position/Scale/Rotation/Origin setter **标脏不 flush**（set_transform FFI 推后），加 `// ponytail: set_transform 未实现，4a 不 flush，留第一个逐帧 transform 控件`。getter 读镜像默认值。
- [ ] **Step 3: 测试 + commit** `feat(c#): Geometry direct-FFI read + Transform deferred`

---

### Task C5: ClassList（→ class FFI）

**Files:** Modify ClassList

- [ ] **Step 1: 测试** `node.Classes.Add("hi") → has_class FFI 返 true → tick 后 computed 变`
- [ ] **Step 2: 实现** Add/Remove/Contains/Toggle/Set/Replace → `loomgui_stage_add_class`/`remove_class`/`has_class`（即时过桥，class 低频）
- [ ] **Step 3: commit** `feat(c#): ClassList via class FFI`

---

### Task C6: Container 树操作

**Files:** Modify Container

- [ ] **Step 1: 测试** `ChildCount/Children/GetChildAt/AddChild/InsertChild/RemoveChild` 与 create_node/append_child + get_children 一致
- [ ] **Step 2: 实现**：ChildCount→`get_child_count`；Children/GetChildAt→`get_children` + lazy GetOrCreate；AddChild→`append_child`；InsertChild→`insert_before`；RemoveChild→`remove_child`。TextContent→set_text（TextNode）/清子换单文本。
- [ ] **Step 3: commit** `feat(c#): Container tree ops`

---

### Task C7: Get<T> / TryGet / Query（作用域查找）

**Files:** Modify Node（Get/TryGet/Query）

- [ ] **Step 1: 测试** `root.Get<Container>("child-id")` 命中 / `TryGet` 不命中返 false / 作用域不穿透 IsScopeRoot（4a 简化：先做子树内查找，作用域边界完整版可标 ponytail 推 4b——见 spec §3.1 Get<T> 契约）
- [ ] **Step 2: 实现**：DFS 子树（get_children 递归 + lazy 构造），按 id_attr 匹配（`find_node_by_id` FFI 已有，或 C# 遍历）。Query<T>() 按类型文档序；Query(selector) 解析 ".cls"/"tag.cls" 遍历。
- [ ] **Step 3: commit** `feat(c#): Get/TryGet/Query scope lookup`

> **阶段 C 完成门**：投影壳单测绿——建树、Get、Style 写/Unset、Geometry 读、Class、树操作、生命周期全可在 harness 验。

---

## 阶段 D：事件 typed 层

### Task D1: RouteEventCore + 16 event struct 接 core

**Files:** Create `Projection/RouteEventCore.cs` · Modify `Public/LoomGUI.Events.cs`（16 struct 加字段 + 转发）

- [ ] **Step 1: RouteEventCore**（持 Target/CurrentTarget/flags，实现 IRouteEvent 6 成员）
```csharp
internal struct RouteEventCore {
    internal Node Target, CurrentTarget;
    internal bool _defaultPrevented, _propagationStopped;
    internal void StopPropagation() => _propagationStopped = true;
    internal void PreventDefault() => _defaultPrevented = true;
}
```
- [ ] **Step 2: 16 struct 各加 `internal RouteEventCore _core;` + IRouteEvent 成员转发 `_core`**（如 `ClickEvent`：`public Node Target => _core.Target; public void StopPropagation() => _core.StopPropagation();` + 业务属性 Position/Button）。每个 struct 关联一个 EventType（D2 用）。
- [ ] **Step 3: 测试 + commit** `feat(c#): RouteEventCore + typed event structs`

---

### Task D2: On<T> 订阅表 + EventRegistration + capture/bubble/once

**Files:** Create `Projection/EventBus.cs` · Modify Node（On<T>）· EventRegistration

- [ ] **Step 1: EventBus**（`Dictionary<(uint nodeId, byte eventType, bool capture), List<HandlerEntry>>`；HandlerEntry 持 typed 回调 + once flag）
```csharp
internal sealed class EventBus {
    // T→EventType 映射：每个 typed event struct 关联一个 EventType（如 ClickEvent→Click）
    internal EventRegistration Subscribe<T>(uint nodeId, Action<T> h, bool capture, bool once) where T:IRouteEvent { ... }
    internal void Dispatch(uint nodeId, byte eventType, LoomEvent raw, NodeRegistry reg) { ... }  // 翻译+路由
}
```
- [ ] **Step 2: On<T>**（`node.On<ClickEvent>(h, capture, once)` → ctx.EventBus.Subscribe）+ EventRegistration.Dispose 退订 + once 触发后退订。
- [ ] **Step 3: 测试**（构造 LoomEvent 喂 Dispatch → handler 收 typed event，Target 正确；once 退订；Dispose 退订）
- [ ] **Step 4: commit** `feat(c#): On<T> subscription + EventRegistration`

---

### Task D3: demux 接线 + 语义糖（Clicked/Activated/Scrolled）

**Files:** Modify EventBus / LoomEventHandler 接线 · Button/Link/Container 语义糖

- [ ] **Step 1: demux 接线** —— 复用 `LoomEventHandler.DispatchPending`（EventType+LoomEvent），在它 dispatch 时调 `ctx.EventBus.Dispatch(nodeId, type, raw, registry)`（翻译 LoomEvent→typed struct：Target=registry.GetOrCreate(nodeId) + 业务字段从 LoomEvent）。路由复用 `tests/dotnet/EventRouter.cs` 纯算法（capture/bubble）。
- [ ] **Step 2: 语义糖** —— `button.Clicked += h` 内部 = `On<ClickEvent>(e => h())` 冒泡到自身；`link.Activated`；`container.Scrolled`。
- [ ] **Step 3: 测试**（capture/bubble 顺序；StopPropagation 生效；Clicked 糖触发）+ commit `feat(c#): event demux wiring + semantic sugar`

> **阶段 D 完成门**：事件单测绿——typed 事件 demux/路由/退订/语义糖全在 harness 验。

---

## 阶段 E：UIContext/Package + 端到端验收门

### Task E1: UIContext / UIPackage / UITemplate

**Files:** Modify UIContext / UIPackage / UITemplate

- [ ] **Step 1: LoadPackage**（bytes → `loomgui_stage_load_package`）+ Instantiate（`loomgui_stage_instantiate` 返 NodeId → NodeFactory 造根 + 入 registry）+ GetTemplate。
- [ ] **Step 2: Create<T>** 白名单（Container/AbsolutePanel/TextNode/Image → create_node；非法 T 抛 UIContractException）；Root（首个根）；FocusedNode（focused_node FFI）；Pick/IsPointerOnUI（FFI 已有）；StyleSheet.Add（推后可 stub）。
- [ ] **Step 3: commit** `feat(c#): UIContext/Package/Template`

---

### Task E2: fixture pkg.bin 准备

**Files:** Create `tests/dotnet/LoomGUI.HeadlessTests/fixtures/`（pkg.bin + HTML 源）

- [ ] **Step 1: 写最小测试 workspace**（`fixtures/ws/loom.workspace.json` + 一个 HTML：div > 子 div[id=child] + `<style>.hi{background-color:red}</style>`）
- [ ] **Step 2: 打包** `cargo run -p loomgui_pkg -- build fixtures/ws` → 产 pkg.bin 入库 fixtures/
- [ ] **Step 3: commit** `test: fixture pkg for 4a acceptance`

---

### Task E3: 验收门（spec §4 全 9 条）

**Files:** `tests/dotnet/LoomGUI.HeadlessTests/AcceptanceTests.cs`

- [ ] **Step 1: 写 9 条验收测试**（对应 spec §4）：
  1. 类型保真（Instantiate 返真实类型）
  2. 作用域查找（Get/TryGet）
  3. 写→读 Geometry（Style.Width=Px(100) → tick → LayoutRect.w≈100）
  4. Unset 撤销回落
  5. class 改 computed（Classes.Add → get_node_computed_style 变）
  6. 树结构（ChildCount/Children/GetChildAt）
  7. 生命周期（Dispose → IsDisposed → 抛 ObjectDisposedException）
  8. 事件（click LoomEvent → Clicked → Target 正确；capture/bubble；StopPropagation；once/Dispose 退订）
  9. inline 继承传播（父 Style.Color=红 → tick → 子 computed color=红）
- [ ] **Step 2: 跑全绿**
```bash
dotnet test tests/dotnet/LoomGUI.HeadlessTests
cargo test  # 全 workspace 无回归
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings
```
- [ ] **Step 3: commit** `test: Spec-4a acceptance gate (9 criteria green)`

> **阶段 E 完成门 = Spec-4a DONE**：编码机 headless 全 9 条绿。终点线2（Unity 真机）= Spec-4b。

---
