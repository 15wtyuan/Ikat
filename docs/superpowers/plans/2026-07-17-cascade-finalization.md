# Cascade 收尾（③）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 Stage 补 computed-style + node-kind 查询出口（core public + FFI），把 cascade 探路 HTML 升级成完整语义断言（继承/specificity/class/kind 保真），锁死终点线1（headless）。

**Architecture:** core 新增 `ComputedNodeStyle` typed 快照（从 `ResolvedStyle` 投影 curated 子集，排除 internal set-ness / 几何 / 复杂视觉）+ Stage public 查询方法；FFI 用 return-code + out-param 导出（kind 用 out+rc 避免 Container=0 撞 0 哨兵）；probe HTML 集成断言做 ③ 验收。④（后端对象层）待 ③ FFI 落地后另写 plan。

**Tech Stack:** Rust 2021，loomgui_core（stage/style/scene），loomgui_ffi_c（csbindgen），loomgui_pkg（probe 测试）。

## Global Constraints

- Rust edition 2021；依赖钉版本（taffy 0.5）。
- FFI 边界 enum 必须 `#[repr(uN)]`；新 FFI struct（`ComputedNodeStyleRepr`）是 `#[repr(C)]` POD；csbindgen 不生成 struct C# stub（C# 镜像 ④ 手写，本 plan 不碰 C#）。
- 任何 Rust 改动后重编 `.dll` + commit（Task 5 收尾）。
- push 前本地跑 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`。
- 代码注释上线品质（说 WHY，不引用坑号/内部编号）。
- 用户只读中文；plan 中文叙述、代码英文。

## File Structure

- Create `crates/core/src/style/computed.rs`：`ComputedNodeStyle` typed struct + `from_resolved` 投影（Task 1）。
- Modify `crates/core/src/style/mod.rs`：`pub mod computed;`（Task 1）。
- Modify `crates/core/src/stage.rs`：加 `get_node_kind` + `get_node_computed_style` public 方法（Task 2）。
- Modify `crates/packer/pkg/tests/cascade_probe.rs`：升级断言（Task 3，③ 验收主体）。
- Modify `crates/ffi/src/lib.rs`：加 2 个 FFI 函数 + `ComputedNodeStyleRepr` `#[repr(C)]`（Task 4）。
- `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`：重编 + commit（Task 5）。

---

## Task 1: ComputedNodeStyle typed 快照 + 投影

**Files:**
- Create: `crates/core/src/style/computed.rs`
- Modify: `crates/core/src/style/mod.rs`（加 `pub mod computed;`）

**Interfaces:**
- Consumes: `loomgui_core::style::resolved::{ResolvedStyle, DisplayMode, OverflowMode, TextAlign}`
- Produces: `ComputedNodeStyle` struct + `ComputedNodeStyle::from_resolved(&ResolvedStyle) -> Self`（Task 2/4 消费）

- [ ] **Step 1: 写失败测试（投影 curated 子集）**

Create `crates/core/src/style/computed.rs`：

```rust
//! Cascade 解析后的对外只读样式快照（typed，core 内部用）。
//!
//! 从 `ResolvedStyle` 投影一个 curated 子集——排除 internal set-ness 位图（cascade 实现
//! 细节，不泄漏出 core）、taffy 几何（size/min/max/margin/padding 是 layout 产物，走
//! `get_node_layout_rect` 出口）、复杂视觉（gradient/filter/shadow/transform/text-effects，
//! 留视觉束）。供 `Stage::get_node_computed_style` + 集成断言消费。
use crate::style::resolved::{DisplayMode, OverflowMode, ResolvedStyle, TextAlign};

/// Cascade 解析后的非几何样式快照（typed）。跨 FFI 由 `ComputedNodeStyleRepr` 稳定化（Task 4）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComputedNodeStyle {
    pub display_mode: DisplayMode,
    pub flex_direction: taffy::FlexDirection,
    pub overflow_x: OverflowMode,
    pub overflow_y: OverflowMode,
    pub color: [f32; 4],
    pub background_color: Option<[f32; 4]>,
    pub opacity: f32,
    pub border_color: Option<[f32; 4]>,
    pub font_size: f32,
    pub font_weight: u16,
    pub text_align: TextAlign,
    pub line_height: f32,
    pub letter_spacing: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_resolved_projects_set_fields() {
        let mut r = ResolvedStyle::default();
        r.display_mode = DisplayMode::None;
        r.taffy_style.flex_direction = taffy::FlexDirection::Row;
        r.overflow_x = OverflowMode::Hidden;
        r.color = [0.1, 0.2, 0.3, 1.0];
        r.background_color = Some([1.0, 0.0, 0.0, 1.0]);
        r.border_color = Some([0.0, 1.0, 0.0, 1.0]);
        r.opacity = 0.5;
        r.font_size = 24.0;
        r.font_weight = 700;
        r.text_align = TextAlign::Center;
        r.line_height = 1.5;
        r.letter_spacing = 2.0;
        let c = ComputedNodeStyle::from_resolved(&r);
        assert_eq!(c.display_mode, DisplayMode::None);
        assert_eq!(c.flex_direction, taffy::FlexDirection::Row);
        assert_eq!(c.overflow_x, OverflowMode::Hidden);
        assert_eq!(c.color, [0.1, 0.2, 0.3, 1.0]);
        assert_eq!(c.background_color, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(c.border_color, Some([0.0, 1.0, 0.0, 1.0]));
        assert_eq!(c.opacity, 0.5);
        assert_eq!(c.font_size, 24.0);
        assert_eq!(c.font_weight, 700);
        assert_eq!(c.text_align, TextAlign::Center);
        assert_eq!(c.line_height, 1.5);
        assert_eq!(c.letter_spacing, 2.0);
    }

    #[test]
    fn from_resolved_defaults_match_resolved_default() {
        // 默认 ResolvedStyle → 投影反映默认（flex column / opacity 1 / font 16 / 无背景）。
        let r = ResolvedStyle::default();
        let c = ComputedNodeStyle::from_resolved(&r);
        assert_eq!(c.display_mode, DisplayMode::Flex);
        assert_eq!(c.flex_direction, taffy::FlexDirection::Column);
        assert_eq!(c.opacity, 1.0);
        assert_eq!(c.font_size, 16.0);
        assert_eq!(c.background_color, None);
        assert_eq!(c.border_color, None);
    }
}
```

- [ ] **Step 2: 注册 module（让编译看见 computed.rs）**

Modify `crates/core/src/style/mod.rs`：加 `pub mod computed;`（与其他 `pub mod` 同列）。

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test -p loomgui_core --lib style::computed`
Expected: FAIL，`from_resolved` 未定义（struct 有但 impl 缺）。

- [ ] **Step 4: 实现 from_resolved 投影**

在 `crates/core/src/style/computed.rs` 的 `ComputedNodeStyle` struct 后追加：

```rust
impl ComputedNodeStyle {
    /// 从 cascade 后的 `ResolvedStyle`（`Node.style`，rematch 覆写值）投影对外子集。
    pub fn from_resolved(r: &ResolvedStyle) -> Self {
        Self {
            display_mode: r.display_mode,
            flex_direction: r.taffy_style.flex_direction,
            overflow_x: r.overflow_x,
            overflow_y: r.overflow_y,
            color: r.color,
            background_color: r.background_color,
            opacity: r.opacity,
            border_color: r.border_color,
            font_size: r.font_size,
            font_weight: r.font_weight,
            text_align: r.text_align,
            line_height: r.line_height,
            letter_spacing: r.letter_spacing,
        }
    }
}
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p loomgui_core --lib style::computed`
Expected: PASS（2 tests）。

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/style/computed.rs crates/core/src/style/mod.rs
git commit -m "feat(core): ComputedNodeStyle typed snapshot + from_resolved projection"
```

---

## Task 2: Stage public 查询方法（get_node_kind / get_node_computed_style）

**Files:**
- Modify: `crates/core/src/stage.rs`（加 2 方法 + inline 测试）

**Interfaces:**
- Consumes: Task 1 的 `ComputedNodeStyle::from_resolved`；`Node.kind` / `Node.style`（`crates/core/src/scene/node.rs:218`）；`Scene::get(NodeId) -> Option<&Node>`（与 `get_node_layout_rect` 同模式，`stage.rs:306`）
- Produces: `Stage::get_node_kind(NodeId) -> Option<NodeKind>` + `Stage::get_node_computed_style(NodeId) -> Option<ComputedNodeStyle>`（Task 3/4 消费）

- [ ] **Step 1: 写失败测试（builtin kind + computed 非 None）**

在 `crates/core/src/stage.rs` 的 `#[cfg(test)] mod tests` 内追加。`kind_from_tag` 只认 `div/button/img/span`（`scene/dynamic.rs:24`），故 core 单测验机制用这四个；控件 kind 保真（Slider≠Container）的完整测试走 probe HTML（Task 3）。

```rust
    #[test]
    fn get_node_kind_returns_builtin_kinds() {
        let mut s = Stage::new((100.0, 100.0)).unwrap();
        let root = s.create_root("div", "").unwrap();
        let btn = s.create_node("button", "").unwrap();
        s.append_child(root, btn).unwrap();
        let img = s.create_node("img", "").unwrap();
        s.append_child(root, img).unwrap();
        let sp = s.create_node("span", "").unwrap();
        s.append_child(root, sp).unwrap();
        use crate::scene::node::NodeKind;
        assert_eq!(s.get_node_kind(root), Some(NodeKind::Container));
        assert_eq!(s.get_node_kind(btn), Some(NodeKind::Button));
        assert_eq!(s.get_node_kind(img), Some(NodeKind::Image));
        assert_eq!(s.get_node_kind(sp), Some(NodeKind::TextNode));
        // 无效句柄 → None（不撞 Container=0）。
        assert_eq!(s.get_node_kind(crate::scene::NodeId::INVALID), None);
    }

    #[test]
    fn get_node_computed_style_returns_snapshot() {
        let mut s = Stage::new((100.0, 100.0)).unwrap();
        let root = s.create_root("div", "").unwrap();
        let c = s.get_node_computed_style(root).expect("root computed style");
        // 默认值（不依赖 rematch 时机）：opacity 1.0、display Flex。精确 cascade 值由 Task 3 验。
        assert_eq!(c.opacity, 1.0);
        assert_eq!(
            s.get_node_computed_style(crate::scene::NodeId::INVALID),
            None,
            "invalid node -> None"
        );
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_core --lib stage::tests::get_node_kind_returns_builtin_kinds`
Expected: FAIL，`get_node_kind` 未定义。

- [ ] **Step 3: 实现 2 个 public 方法**

在 `crates/core/src/stage.rs` 的 `get_node_layout_rect`（`:306`）附近追加（照其 `scene.as_ref()?.get(node).map(...)` 模式）：

```rust
    /// 节点语义类型（围栏 tag + 结构属性决定，CSS 不改变）。None = 节点不存在。
    pub fn get_node_kind(
        &self,
        node: NodeId,
    ) -> Option<crate::scene::node::NodeKind> {
        self.scene.as_ref()?.get(node).map(|n| n.kind)
    }

    /// cascade 解析后的非几何样式快照（`Node.style`，rematch 覆写值）。None = 节点不存在。
    /// 几何（w/h/x/y）走 `get_node_layout_rect`；internal set-ness/复杂视觉不暴露。
    pub fn get_node_computed_style(
        &self,
        node: NodeId,
    ) -> Option<crate::style::computed::ComputedNodeStyle> {
        self.scene
            .as_ref()?
            .get(node)
            .map(|n| crate::style::computed::ComputedNodeStyle::from_resolved(&n.style))
    }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p loomgui_core --lib stage::tests`
Expected: PASS（含新增 2 tests）。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/stage.rs
git commit -m "feat(core): Stage::get_node_kind + get_node_computed_style public query exits"
```

## Task 3: probe HTML 升级断言（③ 验收主体）

**Files:**
- Modify: `crates/packer/pkg/tests/cascade_probe.rs`（加 `use NodeKind` + 2 测试函数）

**Interfaces:**
- Consumes: Task 2 的 `Stage::get_node_kind` + `get_node_computed_style`；现有 `build_stage(HTML)` helper（`cascade_probe.rs`）+ `cascade-probe.html` fixture（已含 `#root{font-size:14;color:#222}` / `.title` / `#root .title` / `.muted` / `.row .lbl` / 全量控件）。

**预期**：cascade 引擎已产品化（Spec-1 spike + Spec-3 已验），这些断言应 **PASS**。若 FAIL = cascade 有未覆盖 bug，按 systematic-debugging 查根因（不在本 plan 的「锁死」范围，作为发现点上报）。

- [ ] **Step 1: 加 use + 继承/specificity/class 断言**

Modify `crates/packer/pkg/tests/cascade_probe.rs`——顶部 use 区加：

```rust
use loomgui_core::scene::node::NodeKind;
```

文件末尾追加：

```rust
#[test]
fn probe_cascade_inheritance_and_specificity() {
    let (stage, _) = build_stage(HTML);
    // 继承 + 后代覆盖：`.row .lbl { font-size:12 }` 命中 span.lbl-master，
    // 覆盖继承自 #root 的 14。一次断言同时验「后代选择器匹配」+「继承基线」+「显式声明胜继承」。
    let lbl = stage.find_node_by_id("lbl-master").expect("lbl-master");
    let c = stage.get_node_computed_style(lbl).expect("lbl computed");
    assert!(
        (c.font_size - 12.0).abs() < 0.5,
        ".row .lbl should set font-size 12 (overriding inherited 14): got {}",
        c.font_size
    );
    // 继承（无覆盖）：span.vol-val 无 font-size 规则 → 继承 #root 的 14。
    let vol_val = stage.find_node_by_id("vol-val").expect("vol-val");
    let c = stage.get_node_computed_style(vol_val).expect("vol-val computed");
    assert!(
        (c.font_size - 14.0).abs() < 0.5,
        "vol-val inherits #root font-size 14: got {}",
        c.font_size
    );
    // class 命中：`.muted { color:#888 }` 命中 vol-val（r=136/255≈0.533）。
    assert!(
        (c.color[0] - 136.0 / 255.0).abs() < 0.01,
        ".muted color #888 (r≈0.533): got {}",
        c.color[0]
    );
    // specificity：`#root .title { color:#0066aa }`（id+class=0,2,0）胜 `.title { color:#114488 }`（class=0,1,0）。
    // #0066aa = r=0, b=170/255≈0.667。
    let title = stage.find_node_by_id("title").expect("title");
    let c = stage.get_node_computed_style(title).expect("title computed");
    assert!(
        c.color[0] < 0.01 && (c.color[2] - 170.0 / 255.0).abs() < 0.01,
        "#root .title should win specificity (color #0066aa): got {:?}",
        c.color
    );
}

#[test]
fn probe_control_kinds_do_not_collapse() {
    // kind 保真（防 §3.3「假绿」）：控件不塌成 Container。get_node_kind 是 ③ 新出口，
    // smoke 推迟的「kind 保真」断言在此兑现。
    let (stage, _) = build_stage(HTML);
    let kind = |id: &str| stage.get_node_kind(stage.find_node_by_id(id).expect(id));
    assert_eq!(kind("vol"), Some(NodeKind::Slider), "vol == Slider");
    assert_eq!(kind("mute"), Some(NodeKind::Toggle), "mute == Toggle");
    assert_eq!(kind("quality"), Some(NodeKind::Dropdown), "quality == Dropdown");
    assert_eq!(kind("pb"), Some(NodeKind::ProgressBar), "pb == ProgressBar");
    assert_eq!(kind("save"), Some(NodeKind::Button), "save == Button");
    assert_eq!(kind("li1"), Some(NodeKind::ListItem), "li1 == ListItem");
    assert_eq!(kind("root"), Some(NodeKind::Container), "root still Container");
}
```

- [ ] **Step 2: 跑测试**

Run: `cargo test -p loomgui_pkg --test cascade_probe`
Expected: 5 tests PASS（原 3 + 新 2）。若新断言 FAIL，cascade 有 bug——读 FAIL 信息定位（继承? specificity? kind 映射?），按 systematic-debugging 查，不在本 plan 范围内贴补偿。

- [ ] **Step 3: Commit**

```bash
git add crates/packer/pkg/tests/cascade_probe.rs
git commit -m "test(packer): probe cascade assertions — inheritance/specificity/kind fidelity"
```

---

## Task 4: FFI 导出（get_node_kind + get_node_computed_style）

**Files:**
- Modify: `crates/ffi/src/lib.rs`（加 `ComputedNodeStyleRepr` + 2 FFI 函数 + 投影 helper + 测试）

**Interfaces:**
- Consumes: Task 2 的 `Stage::get_node_kind` / `get_node_computed_style`；`NodeKind`（`#[repr(u8)]`，`node.rs:85`，`as u8` 稳定）；`loomgui_core::style::computed::ComputedNodeStyle`（Task 1）。
- Produces: `loomgui_stage_get_node_kind` + `loomgui_stage_get_node_computed_style` extern "C"（④ C# 投影层消费；csbindgen 自动生成函数 stub，struct C# 镜像 ④ 手写）。

- [ ] **Step 1: 写失败测试（FFI round-trip + 哨兵不撞）**

在 `crates/ffi/src/lib.rs` 的 `#[cfg(test)]` 测试 mod 内追加（用 `test_helpers::stage_new_with_dejavu`）：

```rust
    use crate::test_helpers::stage_new_with_dejavu;
    use loomgui_core::scene::node::NodeKind;
    use loomgui_core::style::resolved::DisplayMode;

    #[test]
    fn ffi_get_node_kind_div_and_invalid() {
        let h = stage_new_with_dejavu(100.0, 100.0);
        let root = unsafe { loomgui_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0) };
        let mut kind: u8 = 255;
        let rc = unsafe { loomgui_stage_get_node_kind(h, root, &mut kind) };
        assert_eq!(rc, 0, "div kind rc");
        assert_eq!(kind, NodeKind::Container as u8, "div == Container(0)");
        // 无效 node -> rc 非 0（关键：不撞 Container=0 哨兵）。
        let rc_bad = unsafe { loomgui_stage_get_node_kind(h, 0xFFFF_FFFF, &mut kind) };
        assert_ne!(rc_bad, 0, "invalid node must not return 0 (collides with Container)");
    }

    #[test]
    fn ffi_get_node_computed_style_div() {
        let h = stage_new_with_dejavu(100.0, 100.0);
        let root = unsafe { loomgui_stage_create_root(h, b"div".as_ptr(), 3, b"".as_ptr(), 0) };
        let mut repr = ComputedNodeStyleRepr::default();
        let rc = unsafe { loomgui_stage_get_node_computed_style(h, root, &mut repr) };
        assert_eq!(rc, 0, "computed style rc");
        assert_eq!(repr.opacity, 1.0);
        assert_eq!(repr.display_mode, DisplayMode::Flex as u8);
        // 无效 node -> rc 非 0。
        let rc_bad = unsafe { loomgui_stage_get_node_computed_style(h, 0xFFFF_FFFF, &mut repr) };
        assert_ne!(rc_bad, 0);
    }

    #[test]
    fn computed_style_repr_is_aligned_pod() {
        // repr(C) POD，max align = 4（f32/[f32;4]）→ size 必 4 对齐。
        // ④ 实现 C# 镜像时，把 C# Marshal.SizeOf 与 Rust size_of 锁等（手写 struct 逐字段对齐须匹配）。
        let sz = std::mem::size_of::<ComputedNodeStyleRepr>();
        assert!(
            sz > 0 && sz % 4 == 0,
            "repr(C) POD must be 4-byte aligned: got {sz}"
        );
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p loomgui_ffi_c --lib ffi_get_node_kind_div_and_invalid`
Expected: FAIL，`ComputedNodeStyleRepr` / `loomgui_stage_get_node_kind` 未定义。

- [ ] **Step 3: 实现 ComputedNodeStyleRepr + 投影 + 2 FFI 函数**

在 `crates/ffi/src/lib.rs`（与现有 `loomgui_stage_get_node_*` 函数同区块）追加。`DisplayMode`/`OverflowMode` 已 `#[repr(u8)]` 可 `as u8`；`TextAlign`（无 repr）+ `taffy::FlexDirection`（外部类型）用 `match` 稳定化。

```rust
use loomgui_core::scene::node::NodeKind;
use loomgui_core::style::computed::ComputedNodeStyle;
use loomgui_core::style::resolved::{DisplayMode, OverflowMode, TextAlign};

/// FFI 稳定快照（#[repr(C)] POD）。enum→u8（match 稳定化，不靠 enum 隐式 repr），
/// Option<[f32;4]>→present flag + 数组。csbindgen 不生成 struct C# stub，C# 镜像 ④ 手写。
#[repr(C)]
#[derive(Default)]
pub struct ComputedNodeStyleRepr {
    pub display_mode: u8,
    pub flex_direction: u8,
    pub overflow_x: u8,
    pub overflow_y: u8,
    pub color: [f32; 4],
    pub bg_present: u8,
    pub background_color: [f32; 4],
    pub opacity: f32,
    pub border_present: u8,
    pub border_color: [f32; 4],
    pub font_size: f32,
    pub font_weight: u16,
    pub text_align: u8,
    pub line_height: f32,
    pub letter_spacing: f32,
}

impl ComputedNodeStyleRepr {
    fn from_computed(c: &ComputedNodeStyle) -> Self {
        let (bg_present, background_color) = match c.background_color {
            Some(col) => (1, col),
            None => (0, [0.0; 4]),
        };
        let (border_present, border_color) = match c.border_color {
            Some(col) => (1, col),
            None => (0, [0.0; 4]),
        };
        Self {
            display_mode: c.display_mode as u8, // #[repr(u8)]
            flex_direction: match c.flex_direction {
                taffy::FlexDirection::Row => 0,
                taffy::FlexDirection::Column => 1,
                taffy::FlexDirection::RowReverse => 2,
                taffy::FlexDirection::ColumnReverse => 3,
            },
            overflow_x: c.overflow_x as u8, // #[repr(u8)]
            overflow_y: c.overflow_y as u8,
            color: c.color,
            bg_present,
            background_color,
            opacity: c.opacity,
            border_present,
            border_color,
            font_size: c.font_size,
            font_weight: c.font_weight,
            text_align: match c.text_align {
                TextAlign::Left => 0,
                TextAlign::Center => 1,
                TextAlign::Right => 2,
            },
            line_height: c.line_height,
            letter_spacing: c.letter_spacing,
        }
    }
}

/// 读节点 kind。return code：0 = ok 且 `*out` = kind 判别值，非 0 = 节点不存在。
/// 不用 `-> u8` + 0 哨兵：NodeKind 首变体 Container 判别值 = 0，会与「不存在」撞。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_kind(
    h: *const StageHandle,
    node_id: u32,
    out: *mut u8,
) -> i32 {
    if h.is_null() {
        return 1;
    }
    let sh = unsafe { &*h };
    match sh.stage.get_node_kind(NodeId(node_id)) {
        Some(k) => {
            if !out.is_null() {
                unsafe { *out = k as u8 }; // NodeKind #[repr(u8)]，as u8 稳定
            }
            0
        }
        None => 1,
    }
}

/// 读节点 computed style 快照。return code：0 = ok 且 `*out` 填好，非 0 = 节点不存在。
#[no_mangle]
pub extern "C" fn loomgui_stage_get_node_computed_style(
    h: *const StageHandle,
    node_id: u32,
    out: *mut ComputedNodeStyleRepr,
) -> i32 {
    if h.is_null() {
        return 1;
    }
    let sh = unsafe { &*h };
    match sh.stage.get_node_computed_style(NodeId(node_id)) {
        Some(c) => {
            if !out.is_null() {
                unsafe { *out = ComputedNodeStyleRepr::from_computed(&c) };
            }
            0
        }
        None => 1,
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p loomgui_ffi_c --lib`（含新 3 tests）
Expected: PASS。`computed_style_repr_is_aligned_pod` 验证 POD 4 对齐；④ 实现 C# 镜像时把 `Marshal.SizeOf` 与 Rust `size_of` 锁等。

- [ ] **Step 5: 重新生成 csbindgen binding**

Run: `cargo build -p loomgui_ffi_c`
（`build.rs` 用 `input_extern_file("src/lib.rs")` 自动扫描，新 2 函数 stub 进 `OUT_DIR/LoomGUIBindings.cs`。struct stub 不生成——C# 镜像 ④ 手写，本 plan 不碰。`cargo run -p xtask -- sync-bindings` 同步到 Unity 也推到 ④。）

- [ ] **Step 6: Commit**

```bash
git add crates/ffi/src/lib.rs
git commit -m "feat(ffi): get_node_kind + get_node_computed_style (return-code + out-param)"
```

---

## Task 5: 重编 .dll + 全 workspace 验证

**Files:**
- Modify: `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`（重编产物）

- [ ] **Step 1: 全 workspace 测试 + 门禁**

Run:
```bash
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```
Expected: 全绿。若 clippy 报新代码 lint，修源（clippy 各 crate root 有 `#![allow]` 放行可辩护模式，勿误清——新增可辩护 lint 在那加，带理由注释）。

- [ ] **Step 2: 重编 release .dll（Unity 必须关着——它锁 .dll）**

Run:
```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```
验证非 stale：`md5sum target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`（两者须等）。

- [ ] **Step 3: Commit .dll**

```bash
git add unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
git commit -m "chore: rebuild dll for ③ query exits (get_node_kind + computed_style)"
```

---

## ③ 验收（终点线1 加固）

全部 task 完成后：
- `cascade_probe.rs` 5 测试绿（rect/visible/不 panic + 继承/specificity/class + kind 保真）。
- core 新查询出口单测绿 + FFI round-trip 测试绿。
- 全 workspace `cargo test` + fmt + clippy 清。
- `.dll` 重编 + commit。
- 核心范式（cascade 正确性）在 headless 完全可断言——「rect 对 ≠ 语义对」盲区消除。

**④（后端对象层）另写 plan**，待本 plan 落地后基于真实 FFI 签名（`loomgui_stage_get_node_kind` / `get_node_computed_style` + `ComputedNodeStyleRepr` 字段集）写 C# 投影层，冲终点线2。

