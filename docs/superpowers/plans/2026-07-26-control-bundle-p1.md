# 控件束 P1（ProgressBar + Toggle + Slider）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 ProgressBar / Toggle / Slider 三个控件，建立控件地基（side table + 子节点注入 + 状态绑定 + set_transform 还债），让 showcase 控件可交互。

**Architecture:** 控件当容器，core 实例化时注入约定 class 的视觉子节点（.loom-fill/.loom-track/.loom-thumb/.loom-check）。状态存统一 ControlState side table（按 NodeId）；core 按状态写子节点 inline style（fill width / check display / thumb transform）。HTML 属性经 bridge 提取 → TemplateNode.control_init → instantiate 填 side table。Slider thumb 走通用 set_transform（还债，绕开 solve）。

**Tech Stack:** Rust core（taffy 0.12 / slotmap / bincode）+ csbindgen FFI + C# 投影层 + 围栏（fence crate）

**Spec:** `docs/superpowers/specs/2026-07-26-control-bundle-p1-design.md`

## Global Constraints

- Rust edition 2021，依赖钉版本（taffy 0.12 / slotmap 1.1 / csbindgen 1）
- FFI 边界 C-like enum 必须 `#[repr(uN)]`；`size_of::<T>()` 断言 ABI struct 尺寸
- FFI 返字符串一律 ptr+len（不靠 NUL）；getter 用 return-code + out-param（避 Container=0 哨兵）
- pkg 格式一刀切升 v23→v24（MIN=MAX=24，弃 v23，无迁移器），加 bincode 稳定性测试
- 围栏真相源 = `crates/fence/src/schema/` Rust const 表；改 schema 必同步 `docs/design/fence.md`
- 代码注释写上线品质（说 WHY，不引用内部编号）
- push 前跑 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`
- Rust 改动后重编 + 拷 `.dll`：`cargo build -p loomgui_ffi_c --release` → cp 到 `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`（Unity 关着拷）
- 改 parse-time 逻辑（bridge）须重打 pkg：`cargo run -p loomgui_pkg -- build showcase`
- 子节点命名 `.loom-` 前缀；只 class 无 id
- value 语义优先：core 写子节点 inline style（inline 优先级最高，用户 CSS 改不了状态驱动的几何/可见性）

---

## File Structure

**core（Rust）：**
- `crates/core/src/asset/mod.rs` — TemplateNode 加 control_init 字段；PKG_FORMAT_VERSION bump 24
- `crates/core/src/scene/node.rs` — Scene 加 controls side table；ControlState enum 定义
- `crates/core/src/scene/dynamic.rs` — instantiate 时填 side table + 注入视觉子节点
- `crates/core/src/scene/control.rs`（新）— 控件状态→子节点 inline style 绑定 + 交互逻辑
- `crates/core/src/transform.rs` — NodeTransform 写入（set_transform 还债）
- `crates/core/src/input.rs` — EVT_* 控件事件常量 + EventRecord 产生
- `crates/core/src/stage.rs` — process 指针输入接控件交互；instantiate hook

**packer（Rust）：**
- `crates/packer/pkg/src/bridge.rs` — 提取控件属性 → ControlInit

**FFI（Rust）：**
- `crates/ffi/src/lib.rs` — set/get control value/checked/max/min/step + set_transform

**C#：**
- `unity/package/Runtime/Projection/NodeKind.cs` — （无改，仅参考）
- `unity/package/Runtime/Public/LoomGUI.Nodes.cs` — 填 ProgressBar/Toggle/Slider/RadioButton 壳
- `unity/package/Runtime/Projection/EventDemuxer.cs` / `LoomGUI.EventType.cs` — 控件事件 demux

**fence：**
- `crates/fence/src/` — 控件 CSS 命中校验（pipeline pass）
- `docs/design/fence.md` — 同步校验规则

**showcase：**
- `showcase/showcase/*.html` — 控件 CSS + 交互演示

---

## Task 1: pkg 格式 bump v23→v24 + TemplateNode.control_init 字段

**Files:**
- Modify: `crates/core/src/asset/mod.rs:21-23,46-56,~110,~125-180,~275-345`
- Test: `crates/core/src/asset/tests.rs`

**Interfaces:**
- Produces: `TemplateNode.control_init: Option<ControlInit>`（ControlInit 在 Task 3 定义；本 task 先用占位类型让字段存在，Task 3 填实）。为避免循环依赖，本 task 先定义 `ControlInit` enum 骨架放 asset/mod.rs，Task 3 再迁到 node.rs 或原地完善。

- [ ] **Step 1: 定义 ControlInit enum（asset/mod.rs）**

在 TemplateNode 上方加：
```rust
/// 控件初始值（从 HTML 属性 bake）。按 NodeKind 分派。
/// 打包期 bridge 提取 → 进 pkg.bin → core instantiate 填 side table。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ControlInit {
    Progress { value: f32, max: f32, indeterminate: bool },
    Toggle { checked: bool },
    Radio { checked: bool, name: String },
    Slider { value: f32, min: f32, max: f32, step: f32 },
}
```

- [ ] **Step 2: TemplateNode 加字段 + bump 版本**

```rust
// asset/mod.rs:21-23
pub const PKG_FORMAT_VERSION: u32 = 24; // v24: TemplateNode.control_init (bincode layout change)
pub(crate) const MIN_VERSION: u32 = 24;
pub(crate) const MAX_VERSION: u32 = 24;
```
TemplateNode struct（line 46-56）末尾加：
```rust
    pub control_init: Option<ControlInit>,
```

- [ ] **Step 3: write_package / read_package 同步字段**

write 段（~line 110 node_records tuple + ~line 290-305）加 `control_init` 序列化；read 段（~line 327-340 `TemplateNode{...}`）加 `control_init: ...` 反序列化。**所有构造 TemplateNode 的地方都要加 control_init: None（测试 helper / 其它）**——`cargo build` 会报所有遗漏点。

- [ ] **Step 4: 写失败测试（bincode 稳定性）**

`crates/core/src/asset/tests.rs` 加：
```rust
#[test]
fn pkg_v24_control_init_roundtrip() {
    let node = TemplateNode {
        kind: NodeKind::ProgressBar,
        style: Default::default(),
        parent_idx: None,
        classes: vec![],
        id_attr: None,
        draggable: false,
        tabindex: -1,
        content: None,
        src: None,
        control_init: Some(ControlInit::Progress { value: 70.0, max: 100.0, indeterminate: false }),
    };
    let bytes = bincode::serialize(&node).unwrap();
    let back: TemplateNode = bincode::deserialize(&bytes).unwrap();
    assert_eq!(back.control_init, node.control_init);
}

#[test]
fn pkg_v24_rejects_v23() {
    // read 一个 version=23 的包应失败
    let mut bad = vec![];
    bad.extend_from_slice(&23u32.to_le_bytes()); // version
    // ... 最小骨架让 read_package 进到 version check
    // 断言 read_package 返 Err（版本不匹配）
}
```

- [ ] **Step 5: 跑测试验证**

`cargo test -p loomgui_core pkg_v24` — 两个新测试 PASS；`cargo test -p loomgui_core` 全绿。

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/asset/mod.rs crates/core/src/asset/tests.rs
git commit -m "feat(pkg): bump v24 + TemplateNode.control_init field"
```

---

## Task 2: bridge 提取控件属性 → ControlInit

**Files:**
- Modify: `crates/packer/pkg/src/bridge.rs:62-80,131`
- Test: `crates/packer/pkg/tests/`

**Interfaces:**
- Consumes: `ControlInit`（Task 1）、`NodeKind`、`attr(el, name)` helper（bridge.rs:131）
- Produces: bridge 翻译时填 `TemplateNode.control_init`

- [ ] **Step 1: 写失败测试（bridge 提取控件属性）**

`crates/packer/pkg/tests/` 加测试（仿现有 smoke_ir_bridge.rs）：
```rust
#[test]
fn bridge_extracts_progress_attrs() {
    let html = r#"<progress value="70" max="100"></progress>"#;
    let templates = run_bridge(html); // helper：fence parse → bridge
    let node = &templates[0].nodes[0];
    assert_eq!(node.kind, NodeKind::ProgressBar);
    let init = node.control_init.as_ref().expect("control_init set");
    assert!(matches!(init, ControlInit::Progress { value: 70.0, max: 100.0, indeterminate: false }));
}

#[test]
fn bridge_extracts_slider_attrs() {
    let html = r#"<input type="range" min="0" max="100" step="5" value="50">"#;
    let node = &run_bridge(html)[0].nodes[0];
    assert!(matches!(node.control_init, Some(ControlInit::Slider { value: 50.0, min: 0.0, max: 100.0, step: 5.0 })));
}

#[test]
fn bridge_extracts_checkbox_attrs() {
    let html = r#"<input type="checkbox" checked>"#;
    let node = &run_bridge(html)[0].nodes[0];
    assert!(matches!(node.control_init, Some(ControlInit::Toggle { checked: true })));
}

#[test]
fn bridge_extracts_radio_name() {
    let html = r#"<input type="radio" name="grp" checked>"#;
    let node = &run_bridge(html)[0].nodes[0];
    assert!(matches!(node.control_init, Some(ControlInit::Radio { checked: true, name }) if name == "grp"));
}
```

- [ ] **Step 2: 跑测试验证失败**

`cargo test -p loomgui_pkg bridge_extracts` — FAIL（control_init 全是 None）。

- [ ] **Step 3: 实现 bridge 提取（bridge.rs:62-80 Element 分支）**

在 `nodes.push(TemplateNode{...})` 前，按 NodeKind 提取属性填 control_init：
```rust
let control_init = match kind {
    NodeKind::ProgressBar => attr(el, "value").and_then(|v| v.parse().ok()).map(|value| {
        let max = attr(el, "max").and_then(|v| v.parse::<f32>().ok()).unwrap_or(100.0);
        let indeterminate = attr(el, "value").is_none(); // 无 value 视为 indeterminate
        ControlInit::Progress { value, max, indeterminate }
    }),
    NodeKind::Slider => attr(el, "value").and_then(|v| v.parse().ok()).map(|value| {
        let min = attr(el, "min").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let max = attr(el, "max").and_then(|v| v.parse::<f32>().ok()).unwrap_or(100.0);
        let step = attr(el, "step").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
        ControlInit::Slider { value, min, max, step }
    }),
    NodeKind::Toggle => Some(ControlInit::Toggle {
        checked: attr(el, "checked").is_some(),
    }),
    NodeKind::RadioButton => Some(ControlInit::Radio {
        checked: attr(el, "checked").is_some(),
        name: attr(el, "name").unwrap_or_default().to_string(),
    }),
    _ => None,
};
```
（Slider 无 value 属性时返回 None——运行时用默认 0；或给默认 value=min。先 None，Task 9 实例化兜底。）

- [ ] **Step 4: 跑测试验证通过**

`cargo test -p loomgui_pkg` — 全绿。

- [ ] **Step 5: Commit**

```bash
git add crates/packer/pkg/src/bridge.rs crates/packer/pkg/tests/
git commit -m "feat(bridge): extract control HTML attrs into ControlInit"
```

---

## Task 3: core ControlState side table + instantiate 填初始值

**Files:**
- Modify: `crates/core/src/scene/node.rs:350-389`（Scene struct）、`crates/core/src/scene/dynamic.rs:160`（create_node_from_template）
- Test: `crates/core/src/scene/node/tests.rs`

**Interfaces:**
- Consumes: `ControlInit`（Task 1）、Scene struct
- Produces: `Scene.controls: ControlTable`、`ControlState` enum、instantiate 时填表

- [ ] **Step 1: 定义 ControlState + ControlTable（node.rs，Scene struct 旁）**

```rust
/// 控件运行时状态（按 NodeKind 分派）。side table 按 NodeId 索引。
#[derive(Debug, Clone, PartialEq)]
pub enum ControlState {
    Progress { value: f32, max: f32, indeterminate: bool },
    Toggle { checked: bool },
    Radio { checked: bool, name: String },
    Slider { value: f32, min: f32, max: f32, step: f32, dragging: bool },
}

#[derive(Debug, Default)]
pub struct ControlTable(pub slotmap::SecondaryMap<NodeId, ControlState>);

impl ControlTable {
    pub fn get(&self, id: NodeId) -> Option<&ControlState> { self.0.get(id) }
    pub fn ensure(&mut self, id: NodeId, state: ControlState) { self.0.insert(id, state); }
    pub fn remove(&mut self, id: NodeId) { self.0.remove(id); }
}
```

- [ ] **Step 2: Scene 加字段（node.rs:350-389）**

```rust
pub struct Scene { ... 现有字段 ...
    pub controls: ControlTable,  // 控件状态 side table
}
```
Scene 的 `Default`/构造也要初始化 `controls: ControlTable::default()`。

- [ ] **Step 3: instantiate 填初始值（dynamic.rs:160 create_node_from_template）**

建节点后，按 template.control_init 填表 + 触发子节点注入（注入逻辑 Task 4，本 task 先只填表）：
```rust
// dynamic.rs create_node_from_template，建完节点、拿到 node_id 后：
if let Some(init) = &template.control_init {
    let state = match init {
        ControlInit::Progress { value, max, indeterminate } => ControlState::Progress { value:*value, max:*max, indeterminate:*indeterminate },
        ControlInit::Toggle { checked } => ControlState::Toggle { checked:*checked },
        ControlInit::Radio { checked, name } => ControlState::Radio { checked:*checked, name:name.clone() },
        ControlInit::Slider { value, min, max, step } => ControlState::Slider { value:*value, min:*min, max:*max, step:*step, dragging:false },
    };
    scene.controls.ensure(node_id, state);
}
```

- [ ] **Step 4: 写失败测试**

```rust
#[test]
fn instantiate_fills_control_state_from_init() {
    let mut scene = Scene::default();
    let template = TemplateNode { /* ProgressBar, control_init: Progress{70,100,false}, ... */ };
    let id = create_node_from_template(&mut scene, &template, ...);
    let state = scene.controls.get(id).expect("control state");
    assert!(matches!(state, ControlState::Progress { value:70.0, max:100.0, indeterminate:false }));
}
```

- [ ] **Step 5: 跑测试**

`cargo test -p loomgui_core control_state` — PASS；全 workspace 绿。

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/scene/node.rs crates/core/src/scene/dynamic.rs crates/core/src/scene/node/tests.rs
git commit -m "feat(core): ControlState side table, fill from control_init at instantiate"
```

---

## Task 4: core 实例化注入 .loom-* 视觉子节点

**Files:**
- Create: `crates/core/src/scene/control.rs`
- Modify: `crates/core/src/scene/dynamic.rs`（instantiate 后调注入）、`crates/core/src/scene/mod.rs`（pub mod control）
- Test: `crates/core/src/scene/control/tests.rs`（新）

**Interfaces:**
- Consumes: NodeKind、Scene、create_node（运行时建子节点）
- Produces: `inject_control_children(scene, node_id, kind)` —— 控件节点建完后注入视觉子节点

- [ ] **Step 1: 写失败测试（子节点被注入）**

```rust
#[test]
fn progress_injects_fill_child() {
    let mut scene = Scene::default();
    let id = create_progress_node(&mut scene); // helper 建 ProgressBar 节点
    inject_control_children(&mut scene, id, NodeKind::ProgressBar);
    let children: Vec<_> = scene.children(id).collect();
    assert_eq!(children.len(), 1);
    let fill = scene.node(children[0]);
    assert!(fill.classes.iter().any(|c| c == "loom-fill"));
    assert_eq!(fill.kind, NodeKind::Container);
}
// 同理：slider_injects_track_fill_thumb（3 子节点）、toggle_injects_check（1）、radio_injects_check（1）
```

- [ ] **Step 2: 实现 inject_control_children（control.rs）**

```rust
use crate::scene::node::{Node, NodeId, NodeKind, NodeFlags, Scene};
use crate::style::resolved::ResolvedStyle;

const FILL: &str = "loom-fill";
const TRACK: &str = "loom-track";
const THUMB: &str = "loom-thumb";
const CHECK: &str = "loom-check";

fn make_child(class: &str) -> Node {
    let mut n = Node::default_container(); // 现有 helper
    n.classes.push(class.to_string());
    n
}

pub fn inject_control_children(scene: &mut Scene, id: NodeId, kind: NodeKind) {
    let children: Vec<Node> = match kind {
        NodeKind::ProgressBar => vec![make_child(FILL)],
        NodeKind::Slider => vec![
            { let mut t = make_child(TRACK);
              // track 内含 fill —— 但 append 是分层，先建 track 再 append fill 到 track
              t },
            make_child(THUMB),
        ],
        NodeKind::Toggle | NodeKind::RadioButton => vec![make_child(CHECK)],
        _ => return,
    };
    for child in children { scene.append_child(id, child); }
    // Slider 特殊：track 内还要 fill 子节点。在 append track 后，找 track 的 id 再 append fill。
    if kind == NodeKind::Slider {
        let track_id = scene.children(id).next().unwrap();
        scene.append_child(track_id, make_child(FILL));
    }
}
```
（注意：Slider 结构是 `track > fill` + `thumb` 平级。先 append track+thumb，再把 fill append 到 track。）

- [ ] **Step 3: dynamic.rs instantiate 调注入**

create_node_from_template 填完 side table 后：
```rust
if template.control_init.is_some() {
    crate::scene::control::inject_control_children(scene, node_id, template.kind);
}
```

- [ ] **Step 4: 跑测试**

`cargo test -p loomgui_core inject_control` — 4 个子节点测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/scene/control.rs crates/core/src/scene/mod.rs crates/core/src/scene/dynamic.rs crates/core/src/scene/control/tests.rs
git commit -m "feat(core): inject .loom-* visual children for controls"
```

---

## Task 5: 状态→子节点 inline style 绑定（fill width / check display）

**Files:**
- Modify: `crates/core/src/scene/control.rs`
- Test: `crates/core/src/scene/control/tests.rs`

**Interfaces:**
- Consumes: ControlState、Scene、子节点查找（按 class）
- Produces: `sync_control_visuals(scene, node_id)` —— 状态变后同步子节点 inline style

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn progress_fill_width_reflects_value() {
    let mut scene = Scene::default();
    let id = make_progress(&mut scene, /*value*/70.0, /*max*/100.0);
    sync_control_visuals(&mut scene, id);
    let fill = find_child_by_class(&scene, id, "loom-fill");
    let w = scene.inline_style(fill).width; // 读 fill inline width
    assert!((w - 0.7).abs() < 0.001); // 70%
}

#[test]
fn toggle_check_hidden_when_unchecked() {
    let mut scene = Scene::default();
    let id = make_toggle(&mut scene, /*checked*/false);
    sync_control_visuals(&mut scene, id);
    let check = find_child_by_class(&scene, id, "loom-check");
    assert_eq!(scene.inline_style(check).display, Display::None);
}
// toggle_check_shown_when_checked、slider_fill_width + thumb（thumb 位置 Task 6 set_transform 后验）
```

- [ ] **Step 2: 实现 sync_control_visuals（control.rs）**

```rust
pub fn sync_control_visuals(scene: &mut Scene, id: NodeId) {
    let Some(state) = scene.controls.get(id).cloned() else { return };
    match state {
        ControlState::Progress { value, max, .. } => {
            let pct = if max > 0.0 { (value / max).clamp(0.0, 1.0) } else { 0.0 };
            if let Some(fill) = find_child(scene, id, FILL) {
                scene.set_inline_width_pct(fill, pct); // 写 inline style width: 70%
            }
        }
        ControlState::Toggle { checked } | ControlState::Radio { checked, .. } => {
            if let Some(check) = find_child(scene, id, CHECK) {
                scene.set_inline_display(check, if checked { Display::Flex } else { Display::None });
            }
        }
        ControlState::Slider { value, min, max, .. } => {
            let pct = if max > min { ((value - min) / (max - min)).clamp(0.0, 1.0) } else { 0.0 };
            let track = find_child(scene, id, TRACK);
            if let Some(track) = track {
                if let Some(fill) = find_child(scene, track, FILL) {
                    scene.set_inline_width_pct(fill, pct);
                }
            }
            // thumb 位置 = track 末端 = pct；走 transform（Task 6）
        }
    }
}
```
（`set_inline_width_pct` / `set_inline_display` / `find_child` 是 helper——可能需在 Scene 加方法或复用 inline override 层。4a 已有 inline_override 便签层，这里复用它写 width/display。）

- [ ] **Step 3: tick 时序里调 sync_control_visuals**

stage.rs tick（rematch 之后、solve 之前）：每帧对有 control state 的节点 sync 一次（保证状态变 → 视觉更新）。或更省：只在状态变时 sync。先简单每帧 sync 所有控件节点（控件稀疏）。

- [ ] **Step 4: 跑测试**

`cargo test -p loomgui_core sync_control` — PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/scene/control.rs crates/core/src/scene/control/tests.rs crates/core/src/stage.rs
git commit -m "feat(core): bind control state to child inline style (fill width / check display)"
```

---

## Task 6: set_transform 通用化（还债）

**Files:**
- Modify: `crates/core/src/transform.rs`、`crates/core/src/scene/node.rs`（Node 加 transform 字段）、`crates/core/src/stage.rs`（compute_world_transforms 读它）
- Test: `crates/core/src/transform.rs`

**Interfaces:**
- Consumes: NodeTransform（public-api 定义）
- Produces: Node 有 `user_transform` 字段；compute_world_transforms 累计它；set_transform 写它

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn set_transform_offsets_world_without_solve() {
    let mut scene = Scene::default();
    let id = make_div(&mut scene); // rect 初始 (0,0,100,100)
    scene.set_user_transform(id, NodeTransform { translate: [50.0, 0.0], ..Default::default() });
    // 不调 solve，直接 compute_world_transforms
    scene.compute_world_transforms();
    let rect = scene.world_rect(id);
    assert!((rect.x - 50.0).abs() < 0.1); // transform 偏移生效，未触发 solve
}
```

- [ ] **Step 2: Node 加 user_transform 字段（node.rs）**

```rust
pub struct Node { ... 现有字段 ...
    pub user_transform: NodeTransform, // public-api Transform API 的 core 端存储
}
```
Default = identity。compute_world_transforms（stage.rs）累计：`world = parent_world * css_matrix * user_transform.matrix()`。

- [ ] **Step 3: set_user_transform + compute_world_transforms 接它（stage.rs）**

compute_world_transforms 的 DFS 里，原来用 css matrix，现在乘 user_transform：
```rust
let m = css_matrix * node.user_transform.to_matrix();
```

- [ ] **Step 4: 跑测试**

`cargo test -p loomgui_core set_transform` — PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/transform.rs crates/core/src/scene/node.rs crates/core/src/stage.rs
git commit -m "feat(core): NodeTransform user space, applied in compute_world_transforms (no solve)"
```

---

## Task 7: Slider thumb 位置走 transform

**Files:**
- Modify: `crates/core/src/scene/control.rs`
- Test: `crates/core/src/scene/control/tests.rs`

**Interfaces:**
- Consumes: set_user_transform（Task 6）、ControlState::Slider
- Produces: sync_control_visuals 里 Slider 分支设 thumb transform

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn slider_thumb_positioned_by_transform() {
    let mut scene = Scene::default();
    let id = make_slider(&mut scene, /*value*/50.0, /*min*/0.0, /*max*/100.0);
    // 先 solve 一次拿到 track 几何
    scene.solve();
    sync_control_visuals(&mut scene, id);
    let thumb = find_child_by_class(&scene, id, "loom-thumb");
    let tr = scene.node(thumb).user_transform;
    // thumb x = track_width * pct = track_width * 0.5
    assert!(tr.translate[0] > 0.0);
}
```

- [ ] **Step 2: sync_control_visuals Slider 分支加 thumb transform**

```rust
ControlState::Slider { value, min, max, .. } => {
    let pct = ...;
    // fill width（Task 5 已有）
    // thumb translate.x = track_content_width * pct
    if let (Some(track), Some(thumb)) = (find_child(scene,id,TRACK), find_child(scene,id,THUMB)) {
        let track_w = scene.layout_rect(track).w;
        scene.set_user_transform(thumb, NodeTransform { translate: [track_w * pct, 0.0], ..Default::default() });
    }
}
```

- [ ] **Step 3: 跑测试**

`cargo test -p loomgui_core slider_thumb` — PASS。

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/scene/control.rs crates/core/src/scene/control/tests.rs
git commit -m "feat(core): slider thumb positioned via transform (drag-friendly, no solve)"
```

---

## Task 8: FFI 命令（控件状态 get/set + set_transform）

**Files:**
- Modify: `crates/ffi/src/lib.rs`
- Test: `crates/ffi/src/tests.rs`

**Interfaces:**
- Consumes: StageHandle、Scene.controls、Node.user_transform
- Produces: FFI 命令（csbindgen 生 C# 绑定）

- [ ] **Step 1: 写失败测试（FFI 真调）**

```rust
#[test]
fn ffi_set_get_control_value() {
    let stage = make_stage_with_progress(/*value*/70.0);
    unsafe {
        let rc = loomgui_stage_set_control_value(stage.handle, stage.node_id, 90.0);
        assert_eq!(rc, 0);
        let mut out = 0.0f32;
        let rc = loomgui_stage_get_control_value(stage.handle, stage.node_id, &mut out);
        assert_eq!(rc, 0);
        assert!((out - 90.0).abs() < 0.001);
    }
}
// 同理 ffi_set_control_checked、ffi_set_transform（读回 world_rect 验证偏移）
```

- [ ] **Step 2: 加 FFI 命令（lib.rs，仿 set_src/get_node_kind 模式）**

```rust
#[csbindgen]
pub unsafe extern "C" fn loomgui_stage_set_control_value(h: *mut StageHandle, node: u32, value: f32) -> i32 {
    h.stage().and_then(|s| {
        let id = NodeId(node);
        s.with_scene_mut(|sc| {
            if let Some(ControlState::Progress{value:v,max,..}) = sc.controls.get(id).cloned() {
                let clamped = value.max(0.0).min(max);
                sc.controls.ensure(id, ControlState::Progress{value:clamped,max,..});
                Ok(0)
            } else if let Some(ControlState::Slider{min,max,step,..}) = sc.controls.get(id).cloned() {
                let clamped = value.max(min).min(max);
                sc.controls.ensure(id, ControlState::Slider{value:clamped,min,max,step,dragging:false});
                Ok(0)
            } else { Err(()) }
        })
    }).unwrap_or(-1)
}
// get_control_value (out-param)、set/get_control_checked、set/get_control_max/min/step
// set_transform:
#[csbindgen]
pub unsafe extern "C" fn loomgui_stage_set_transform(h: *mut StageHandle, node: u32, tx: f32, ty: f32, sx: f32, sy: f32, rot: f32) -> i32 {
    h.stage().and_then(|s| s.with_scene_mut(|sc| {
        sc.set_user_transform(NodeId(node), NodeTransform{translate:[tx,ty],scale:[sx,sy],rotation:rot,..Default::default()});
        Ok(0)
    })).unwrap_or(-1)
}
```

- [ ] **Step 3: 跑测试 + sync bindings**

`cargo test -p loomgui_ffi_c` — PASS。`cargo run -p xtask -- sync-bindings`。

- [ ] **Step 4: Commit**

```bash
git add crates/ffi/src/lib.rs crates/ffi/src/tests.rs
git commit -m "feat(ffi): control value/checked/transform get/set commands"
```

---

## Task 9: core 交互（Toggle 点击 / Radio 互斥 / Slider 拖拽）

**Files:**
- Modify: `crates/core/src/scene/control.rs`、`crates/core/src/stage.rs`（process 接交互）、`crates/core/src/input.rs`（事件产生）
- Test: `crates/core/src/scene/control/tests.rs`

**Interfaces:**
- Consumes: PointerState.process、hit_test、ControlState
- Produces: pointer down/move 命中控件 → 改 side table + 产生事件

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn toggle_click_flips_checked() {
    let mut scene = Scene::default();
    let id = make_toggle(&mut scene, /*checked*/false);
    process_pointer_down(&mut scene, id); // 模拟点中 toggle
    assert!(matches!(scene.controls.get(id), Some(ControlState::Toggle{checked:true})));
}

#[test]
fn radio_click_mutually_exclusive() {
    let mut scene = Scene::default();
    let a = make_radio(&mut scene, "g", false);
    let b = make_radio(&mut scene, "g", false);
    process_pointer_down(&mut scene, a); // 选 a
    process_pointer_down(&mut scene, b); // 选 b → a 应取消
    assert!(matches!(scene.controls.get(a), Some(ControlState::Radio{checked:false,..})));
    assert!(matches!(scene.controls.get(b), Some(ControlState::Radio{checked:true,..})));
}

#[test]
fn slider_drag_changes_value() {
    let mut scene = Scene::default();
    let id = make_slider(&mut scene, 50.0, 0.0, 100.0);
    let track = find_child(&scene, id, "loom-track");
    let track_rect = scene.layout_rect(track); // 先 solve
    // 在 track 中间按下 + 拖到 75% 处
    process_pointer_down(&mut scene, id); // 命中 thumb/track
    process_pointer_move(&mut scene, track_rect.x + track_rect.w * 0.75);
    let v = match scene.controls.get(id) { Some(ControlState::Slider{value,..}) => value, _ => 0.0 };
    assert!((v - 75.0).abs() < 1.0);
}
```

- [ ] **Step 2: 实现交互（control.rs）**

```rust
pub fn on_pointer_down(scene: &mut Scene, id: NodeId, pos: [f32;2]) {
    if let Some(state) = scene.controls.get(id).cloned() {
        match state {
            ControlState::Toggle { .. } => flip_toggle(scene, id),
            ControlState::Radio { name, .. } => select_radio(scene, id, name),
            ControlState::Slider { min, max, step, .. } => {
                // 设 dragging=true，按 pos 算初始 value
                let v = pos_to_value(scene, id, pos, min, max, step);
                set_slider_value(scene, id, v);
                mark_dragging(scene, id, true);
            }
            _ => {}
        }
    }
}
pub fn on_pointer_move(scene: &mut Scene, id: NodeId, pos: [f32;2]) {
    if matches!(scene.controls.get(id), Some(ControlState::Slider{dragging:true,..})) {
        // 算 value + 更新
    }
}
pub fn on_pointer_up(scene: &mut Scene, id: NodeId) { mark_dragging(scene, id, false); }
```
select_radio：遍历同 name 兄弟，置其它 checked=false + 产生 CheckedChanged。

- [ ] **Step 3: stage.rs process 接 hook**

PointerState.process（input.rs:461）命中后，如果命中节点有 control state，调 control::on_pointer_down。需把"命中节点是不是控件"判断传进去。

- [ ] **Step 4: 跑测试**

`cargo test -p loomgui_core control_on_pointer` — PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/scene/control.rs crates/core/src/stage.rs crates/core/src/input.rs crates/core/src/scene/control/tests.rs
git commit -m "feat(core): control pointer interaction (toggle/radio/slider drag)"
```

---

## Task 10: 控件事件出口（ValueChanged/CheckedChanged/ChangeCommitted）

**Files:**
- Modify: `crates/core/src/input.rs`（EVT_* 常量 22+）、`crates/core/src/ffi/src/lib.rs`（borrow_events 携带）
- Test: `crates/core/src/input.rs`

**Interfaces:**
- Consumes: control 交互（Task 9）
- Produces: EventRecord 含控件事件；borrow_events 出到 C#

- [ ] **Step 1: 定义事件常量（input.rs:73-90）**

```rust
pub const EVT_VALUE_CHANGED: u8 = 22;
pub const EVT_CHECKED_CHANGED: u8 = 23;
pub const EVT_CHANGE_COMMITTED: u8 = 24;
```
EventRecord 扩展（如当前是固定 20B，可能要扩或加新 record 类型）——看 EventRecord 结构，加 target_node + payload（float for value / bool for checked）。

- [ ] **Step 2: 交互时产生事件**

control.rs flip_toggle/select_radio/set_slider_value 内 push EventRecord 到 scene 事件队列。

- [ ] **Step 3: 写测试（事件入队）**

```rust
#[test]
fn toggle_click_emits_checked_changed() {
    let mut scene = Scene::default();
    let id = make_toggle(&mut scene, false);
    on_pointer_down(&mut scene, id, [0.0,0.0]);
    let events = scene.drain_events();
    assert!(events.iter().any(|e| e.evt_type == EVT_CHECKED_CHANGED && e.target == id));
}
```

- [ ] **Step 4: 跑测试 + sync bindings**

`cargo test -p loomgui_core` — PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/input.rs crates/core/src/scene/control.rs crates/core/src/scene/control/tests.rs
git commit -m "feat(core): control events (ValueChanged/CheckedChanged/ChangeCommitted)"
```

---

## Task 11: C# 投影层填壳（ProgressBar/Toggle/Slider/RadioButton + demux）

**Files:**
- Modify: `unity/package/Runtime/Public/LoomGUI.Nodes.cs:1338-1412`、`unity/package/Runtime/Projection/EventDemuxer.cs`、`unity/package/Runtime/LoomGUI.EventType.cs`
- Test: `tests/dotnet/LoomGUI.HeadlessTests/`

**Interfaces:**
- Consumes: FFI（Task 8）、事件 demux
- Produces: 控件 class 填实 + 控件事件 demux 到 ValueChangedEvent/CheckedChangedEvent

- [ ] **Step 1: 填 ProgressBar/Toggle/Slider/RadioButton 壳（Nodes.cs）**

每个 `throw NE()` 的 getter/setter 转发 FFI：
```csharp
// ProgressBar
public float Value { get => Ffi.get_control_value(ctx, id); set => Ffi.set_control_value(ctx, id, value); }
public float Max { get => ...; set => ...; }
public bool IsIndeterminate => /* 读 control_init 或 side table */;

// Toggle
public bool IsChecked { get => Ffi.get_control_checked(ctx,id); set => Ffi.set_control_checked(ctx,id,value); }

// Slider
public float Value { ... } public float Min{...} public float Max{...} public float Step{...}
public event Action<ValueChangedEvent<float>> ValueChanged { add{...} remove{...} }  // 接 demux
```

- [ ] **Step 2: EventType.cs 加控件事件 + EventDemuxer demux**

```csharp
public enum EventType : byte { ..., ValueChanged=22, CheckedChanged=23, ChangeCommitted=24 }
```
EventDemuxer 里按 evt_type 分发到对应 node 的 ValueChanged/CheckedChanged event。

- [ ] **Step 3: Headless 测试**

`tests/dotnet/LoomGUI.HeadlessTests/` 加：
```csharp
[Fact] public void progress_value_roundtrips_via_ffi() { ... }
[Fact] public void toggle_click_raises_checked_changed() { ... }
[Fact] public void slider_drag_raises_value_changed() { ... }
```

- [ ] **Step 4: 跑 Headless 测试 + PublicApi 编译门**

`dotnet test` HeadlessTests — PASS；`dotnet build` PublicApi 编译门 — PASS。

- [ ] **Step 5: Commit**

```bash
git add unity/package/Runtime/Public/LoomGUI.Nodes.cs unity/package/Runtime/Projection/EventDemuxer.cs unity/package/Runtime/LoomGUI.EventType.cs tests/dotnet/LoomGUI.HeadlessTests/
git commit -m "feat(csharp): fill control projection + event demux"
```

---

## Task 12: 围栏"控件必须被 CSS 命中"校验

**Files:**
- Modify: `crates/fence/src/`（新 pipeline pass）、`docs/design/fence.md`
- Test: `crates/fence/tests/`

**Interfaces:**
- Consumes: fence cascade resolve（控件节点是否被规则匹配）
- Produces: 打包期校验 + 教学 diagnostic

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn progress_without_css_errors() {
    let html = r#"<progress value="70" max="100"></progress>"#;
    let diags = run_fence(html);
    assert!(diags.iter().any(|d| d.message.contains("progress") && d.message.contains("CSS")));
}

#[test]
fn progress_with_css_passes() {
    let html = r#"<style>progress{background:#ddd} .loom-fill{background:#4a9}</style><progress value="70"></progress>"#;
    let diags = run_fence(html);
    assert!(diags.is_empty());
}
```

- [ ] **Step 2: 实现校验 pass（fence pipeline）**

在 cascade resolve 后，遍历控件节点（ProgressBar/Slider/Toggle/RadioButton），检查是否有任何规则的选择器匹配它。未匹配 → 推 diagnostic（教学文案：LoomGUI 控件不带默认样式，需为 X 和 .loom-* 提供 CSS）。

- [ ] **Step 3: 同步 fence.md**

在 fence.md 加"控件 CSS 命中校验"章节。

- [ ] **Step 4: 跑测试**

`cargo test -p loomgui_fence control_css` — PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/fence/src/ crates/fence/tests/ docs/design/fence.md
git commit -m "feat(fence): require CSS match for control elements (no UA stylesheet)"
```

---

## Task 13: showcase 控件 CSS + 交互演示 + 重打 pkg

**Files:**
- Modify: `showcase/showcase/character.html,settings.html,form.html,inventory.html,shop.html`
- Test: 人工 Unity PlayMode + dump_page 验证

**Interfaces:**
- Consumes: 全部前序 task

- [ ] **Step 1: 给 showcase 控件配 CSS**

每个用控件的页面加 `<style>`：progress（track + fill）、checkbox/radio（框 + .loom-check 图标）、slider（track + fill + thumb）。

- [ ] **Step 2: 加交互演示**

settings 的 range 滑块拖动改变一个显示值；form 的 checkbox 切换状态；character 的 progress 点击加经验。

- [ ] **Step 3: 重打 pkg + 重编 dll**

```bash
cargo run -p loomgui_pkg -- build showcase
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```

- [ ] **Step 4: dump_page 验证 core 状态**

`cargo run -p loomgui_core --example dump_page -- <pkg>` 验证 fill width / checked / thumb 位置正确。

- [ ] **Step 5: Commit**

```bash
git add showcase/ unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
git commit -m "feat(showcase): control CSS + interactive demos (progress/toggle/slider)"
```

---

## Self-Review 记录

**Spec 覆盖**：§1-2（背景+模型）→ 全局约束；§3（三控件结构）→ Task 4/5/7；§4（数据流）→ Task 1/2/3；§5（FFI+还债）→ Task 6/7/8；§6（交互事件）→ Task 9/10；§7（showcase）→ Task 13；§8（defer）→ 不在 plan；§2.3（围栏校验）→ Task 12。全覆盖。

**类型一致性**：ControlInit（Task1）↔ ControlState（Task3）字段对齐（Progress value/max/indeterminate、Toggle checked、Radio checked+name、Slider value/min/max/step+dragging）；子节点 class 常量 FILL/TRACK/THUMB/CHECK 在 Task4 定义、Task5/7/9 复用一致；EVT_ 常量 Task10 定义、Task11 C# demux 对齐（22/23/24）。

**注意点**（实现时留意，非 plan 漏洞）：
- Task 4 Slider 子结构是 `track>fill` + `thumb` 平级，append 顺序要对（先 track+thumb，再 fill 进 track）
- Task 5 `set_inline_width_pct`/`set_inline_display` 复用 4a 的 inline_override 便签层，不新建机制
- Task 6 NodeTransform 字段名对齐 public-api（translate/scale/rotation/origin）
- Task 8 FFI step 量化（set_slider_value 要 step 量化）
- Task 10 EventRecord 可能要扩字段（加 float payload for value）——看现有 20B 结构是否够
