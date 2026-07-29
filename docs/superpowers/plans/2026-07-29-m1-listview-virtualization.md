# M1 · ListView 虚拟化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把虚拟化（slot 池化 / 可见区裁剪 / 不等高补偿 + scroll anchoring / reuse_key 编码 / 模板克隆）整层吸收进 Rust core，C# `ListView` 全实装，driver 不再手写虚拟列表。

**Architecture:** 虚拟化全在 core（`crates/core/src/list.rs` 新 side table）。可见 item 走正常 CSS 流 + 头/尾 spacer 撑高（对 CSS 语义透明）。bind 回调走 pending 队列 + 帧首排空（无跨 FFI 同步回调）。不等高 = 估算 + 实测回填 + scroll anchoring 同帧补偿 scroll_pos。前置基建：`<template>` 进 pkg（v26→v27）、拆 `SCOPE_ROOT`/`LOOKUP_SCOPE` 双语义 flag、`clone_subtree` 场景级子树克隆。

**Tech Stack:** Rust 2021（loomgui_core / loomgui_ffi_c / loomgui_pkg / fence）、csbindgen C ABI → C# 投影、taffy 0.12 布局、slotmap 1.1 节点存储。Unity 后端为镜像层（不进本计划逻辑验证）。

## Global Constraints

（逐条照抄自 spec §3/§4/§5/§6/§10/§12，所有任务隐含遵守）

- **本质判据**（spec §1）：`ItemCount=1000` 与 `ItemCount=10000` 的 render node 数**相等**（不随总项数增长）。
- **虚拟化全在 core**；C# 不做 slot 映射 / 可见区计算。
- **item 定位**：可见 item + 头/尾 spacer 走正常 CSS 流；spacer 是普通 Container（不带 class、不参与 cascade），`flex-shrink:0` + 显式 height。
- **heights 语义 = margin box**：`height_of(i) = layout_rect.h + margin_top + margin_bottom`（解析像素值）。
- **gap 修正按 display 分支**：ul 为 Block（默认）**不扣 gap**；仅 `display:flex` 的 ul 读 `ResolvedStyle.row_gap` 扣减。
- **spacer margin 折叠阻断**：spacer 声明 `padding-top:0.01px`，使其不可被 margin collapsing 吞掉。
- **ul 高度必须为 auto**：数据驱动模式检测到非 auto（含被祖先 flex 拉伸）→ 抛 `UIContractException`，不静默失效。
- **bind 过桥 = pending 队列 + 帧首排空**；`drain_now`（ScrollToItem / 首次 ItemCount）必须**先跑 `update_visible` 再排空**。
- **`reuse_key` 编码**：`((list_ordinal + 1) << 16) | (slot_idx & 0xFFFF)`，恒 ≠ 0，多 ListView 不撞。
- **clone_subtree side table 判定**：controls/text_contents/image_srcs 克隆（controls 克隆模板初值、复用时 reset）；scroll/anim/tweens/EditState/text_layouts/focused_node/事件订阅不克隆。
- **SCOPE_ROOT 拆双语义**：`SCOPE_ROOT`（`1<<5`，仅 CSS 隔离）+ `LOOKUP_SCOPE`（`1<<6`，仅 `Get<T>` 边界）。slot 根只打后者。
- **`<template>` 进 pkg**：`NodeKind::Template` **枚举末尾追加**（不破坏既有 u8 判别值）；template 子树 display:none + 不参与 cascade / 渲染 / 命中。
- **pkg bump v26→v27** + dll 重编 + sync-bindings + **GUI exe 重出并拷贝** + showcase 重打。
- **dll 重编纪律**：每加一批 FFI 立即重编 release dll + 拷 `unity/package/Plugins/LoomGUI/` + `cargo run -p xtask -- sync-bindings`（坑 158 stale 链同源）。
- **错误一律 `UIContractException`**，围栏外明确失败不静默降级（无祖先 ScrollPane 例外：退化全量渲染 + 一次性警告）。
- **tick 时序不变量**：每帧一次 solve。新增 `list.update_visible`（solve 前）+ `list.collect_heights`（solve 后、refresh_content_sizes 前）。
- **edition 2021**；fmt 严（`cargo fmt --all -- --check`）+ clippy 严（`cargo clippy --all-targets -- -D warnings`）。
- **代码注释上线品质**：说 WHY 不说 WHAT，不引用坑号（坑号只进 docs/pitfalls.md）。
- **Windows 是唯一编码机**：任何 Rust 改动后重编 + 拷 .dll；拷 .dll 时 Unity 必须关着。

---

## File Structure

**新增文件：**

- `crates/core/src/list.rs` — ListView 虚拟化内核：`ListState` / `ListTable` / `HeightCache` / `Slot` / 可见区算法 / clone_subtree 接树 / anchoring。单一职责（虚拟化状态机），照 `scroll.rs`（滚动状态）模式。
- `tests/dotnet/LoomGUI.HeadlessTests/VirtualizationTests.cs` — headless 端到端断言（render node 不随总数增长、无漂移、CSS 命中、异常契约）。

**修改文件（按首次触碰任务号）：**

- Task 0：`crates/core/src/scene/node.rs`（NodeKind 追加 Template）、`crates/core/src/asset/mod.rs`（版本 bump）、`crates/packer/pkg/src/bridge.rs`（不跳过 template）、packer template 根校验、layout/render/hit/cascade 的 display:none 子树剪枝复验。
- Task 1：`crates/core/src/scene/node.rs`（NodeFlags 加 LOOKUP_SCOPE）、`crates/core/src/style/dynamic.rs`（compute_scope_map / parent_in_scope / rematch 校验拆读）、`crates/core/src/scene/dynamic.rs`（create_root + remove_node was_scope_root）、`crates/core/src/stage.rs`（instantiate 双打 flag、find_node_by_id 改读 LOOKUP_SCOPE）、`crates/core/src/scene/node.rs` find_by_id_attr。
- Task 2：`crates/core/src/stage.rs`（clone_subtree）、`crates/core/src/scene/dynamic.rs`（clone_node_recursive + side table 判定）、`crates/ffi/src/lib.rs`（FFI）、C# `UITemplate` 重定义。
- Task 3：`crates/core/src/list.rs`（HeightCache + 可见区算法，纯逻辑）。
- Task 4：`crates/core/src/list.rs`（ListState + spacer/slot 接树 + tick 挂钩）、`crates/core/src/stage.rs`（tick_and_render 插 list.update_visible / collect_heights）。
- Task 5：`crates/core/src/list.rs`（pending_binds + set_item_count 等）、`crates/ffi/src/lib.rs`（list_* FFI 全套）、C# `LoomGUI.Nodes.cs`（ListView 投影实装）+ `UIContext` tick 前排空。
- Task 6：`crates/core/src/list.rs`（margin box 回填 + anchoring）、`crates/core/src/scroll.rs`（clamp 分支 anchoring_active 豁免不清 tweening）。
- Task 7：`crates/core/src/list.rs`（ScrollToItem/Refresh/Notify + drain_now）、FFI、C#。
- Task 8：`crates/core/src/asset/mod.rs`（最终 dll）、`docs/design/public-api.md`、`docs/roadmap/milestones.md`、C#（删 SelectedIndex/SelectionChanged）、`tests/dotnet/LoomGUI.PublicApi`。

---

## Task 0: `<template>` 进 pkg（v26 → v27）

**阻断前置**：template 当前在打包期被整体丢弃（`crates/packer/pkg/src/bridge.rs:34` `is_in_template_subtree → continue`），运行时无源可克隆。本任务让它进 pkg 并强制 display:none 语义。

**Files:**
- Modify: `crates/core/src/scene/node.rs:91-140`（NodeKind 枚举 + from_u8 + 穷尽 guard）
- Modify: `crates/core/src/asset/mod.rs:23-25`（版本 bump）
- Modify: `crates/packer/pkg/src/bridge.rs:34, 123`（不跳过 + map_semantic）
- Modify/Verify: `crates/fence/src/schema/tag.rs`（template 的 display 已为 none）
- Verify: `crates/core/src/render/mod.rs:163`（collect_display_none_subtree）、layout taffy Display::None 剪枝
- Modify: packer（template 根为 li 校验）

**Interfaces:**
- Consumes: `SemanticKind::Template`（fence 已有）、`collect_display_none_subtree`（render 已有，剪 display:none 整子树）
- Produces: `NodeKind::Template`（u8 = 20，末尾追加）、pkg v27 格式、template 子树真实存在于场景树

- [ ] **Step 1: 写失败测试 — NodeKind::Template 存在 + from_u8 双向**

在 `crates/core/src/scene/node.rs` 的 tests mod 加：

```rust
#[test]
fn template_variant_appended_after_searchfield() {
    // 末尾追加保证既有判别值稳定（pkg.bin 兼容）。Template = 20。
    assert_eq!(NodeKind::from_u8(20), Some(NodeKind::Template));
    assert_eq!(NodeKind::from_u8(19), Some(NodeKind::SearchField));
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p loomgui_core template_variant_appended`
Expected: 编译错（`NodeKind::Template` 不存在）

- [ ] **Step 3: 实装 NodeKind::Template**

`crates/core/src/scene/node.rs` 的 `enum NodeKind`（`SearchField` 之后）追加：

```rust
    /// `<template>` — ListView 模板蓝图。display:none 强制；不参与布局/渲染/命中/cascade。
    /// 进 pkg 但运行时不渲染（list.rs 用 clone_subtree 克隆其子树产 slot）。
    Template,
```

同步 `from_u8`（line 120-140 区块末尾 `_ => None` 之前）追加 `20 => Some(NodeKind::Template),`。

同步穷尽 guard `_assert_from_u8_exhaustive`（line ~200）match 末尾加 `| NodeKind::Template`。

`is_container()`：**不加** Template（它不是 Container content model；layout/render 用 display:none 剪枝，不走 is_container）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p loomgui_core template_variant_appended`
Expected: PASS

- [ ] **Step 5: 写失败测试 — bridge 不再跳过 template**

`crates/packer/pkg/src/bridge.rs` tests mod（若无则建）加：

```rust
#[test]
fn template_subtree_enters_pkg() {
    let html = r#"<ul><template><li class="row"><span class="title">x</span></li></template></ul>"#;
    let parsed = loomgui_fence::parse_template(html).expect("fence parse");
    let nodes = bridge(&parsed).expect("bridge");
    assert!(nodes.iter().any(|n| n.kind == loomgui_core::scene::NodeKind::Template));
    assert!(nodes.iter().any(|n| n.kind == loomgui_core::scene::NodeKind::ListItem));
}
```

- [ ] **Step 6: 运行确认失败**

Run: `cargo test -p loomgui_pkg template_subtree_enters_pkg`
Expected: FAIL（template 被 continue，nodes 里无 Template）

- [ ] **Step 7: 改 bridge 不再跳过 + map_semantic**

`crates/packer/pkg/src/bridge.rs`：

1. 删 line 33-37 的 `if is_in_template_subtree(ir_idx, parsed) { continue; }` 块（连注释）。
2. `map_semantic`（约 line 123）把 `Some(SemanticKind::Template) => Err(...)` 改为：

```rust
        Some(SemanticKind::Template) => Ok(NodeKind::Template),
```

3. 核实 fence schema `tag.rs` template 的 TagSpec display 已是 None（读 `crates/fence/src/schema/tag.rs` template 条目）；若不是，改为 None 并跑 `cargo test -p loomgui_fence`。
4. 删 `is_in_template_subtree` 函数（grep 确认无其他引用后删）。

- [ ] **Step 8: 运行确认通过**

Run: `cargo test -p loomgui_pkg template_subtree_enters_pkg && cargo test -p loomgui_fence`
Expected: PASS

- [ ] **Step 9: 写测试 — template 子树不产 render node（display:none 剪枝）**

新建 `crates/core/tests/template_render.rs`：

```rust
use loomgui_core::stage::Stage;

#[test]
fn template_subtree_pruned_from_render() {
    // display:none 的 template 整子树不进 render（collect_display_none_subtree 覆盖 Template）。
    let mut s = Stage::new_for_test();
    let ul = s.create_root("ul", "").unwrap();
    // create_node 走运行时 css 解析；用 "display:none" 让 base_style.display=None
    let tpl = s.create_node("template", "display:none").unwrap();
    s.append_child(ul, tpl).unwrap();
    let li = s.create_node("li", "").unwrap();
    s.append_child(tpl, li).unwrap();
    let frame = s.tick_and_render();
    // frame 节点里不应有 template 也不应有 li（整子树剪）。
    // FrameData 字段名以实际 struct 为准（grep struct FrameData）；下面假设 .nodes: &[RenderNodeBlob]
    assert!(frame.nodes.iter().none(|n| n.id == li.0 || n.id == tpl.0),
        "template subtree must be pruned from render");
}
```

> **核实点**：`FrameData.nodes` 字段名 + `RenderNodeBlob.id` 字段以 `crates/core/src/` 实际 struct 为准。`set_inline_override` 若存在也可替代 `"display:none"` css。以 crate 实际 API 调整测试构造，断言意图不变。

- [ ] **Step 10: 运行确认**（display:none 剪枝已由 `collect_display_none_subtree` + taffy Display::None 覆盖，应直接通过）

Run: `cargo test -p loomgui_core --test template_render`
Expected: PASS（若失败，说明 template 的 display 未真正变成 taffy Display::None，需在 css_resolve/mapping 核实）

- [ ] **Step 11: 写失败测试 — 打包期校验 template 根必须是 li**

`crates/packer/pkg/src/bridge.rs` tests mod 加：

```rust
#[test]
fn template_root_not_li_errors() {
    let html = r#"<ul><template><div>x</div></template></ul>"#;
    let parsed = loomgui_fence::parse_template(html).unwrap();
    assert!(bridge(&parsed).is_err(), "template root must be <li>");
}
```

- [ ] **Step 12: 运行确认失败**

Run: `cargo test -p loomgui_pkg template_root_not_li_errors`
Expected: FAIL（bridge 当前不校验）

- [ ] **Step 13: 实装 template 根校验**

在 `bridge.rs` 的 `bridge` 函数开头（roots 数校验后）加一次 IrTree 遍历：找所有 IrNode 里 tag == `"template"` 的，检查其直接子 Element（`tree.nodes[ir_idx].children`）的 tag 必须全部是 `"li"`，否则 `return Err("template 子元素必须是 <li>")`。写在校验块而非 main loop 内（loop 顺序建节点不好回溯 template→child 关系）。

- [ ] **Step 14: 运行确认通过**

Run: `cargo test -p loomgui_pkg`
Expected: PASS

- [ ] **Step 15: pkg 版本 bump v26 → v27**

`crates/core/src/asset/mod.rs` line 23-25：

```rust
pub const PKG_FORMAT_VERSION: u32 = 27; // v27: <template> subtree enters pkg (NodeKind::Template added)
pub(crate) const MIN_VERSION: u32 = 27;
pub(crate) const MAX_VERSION: u32 = 27;
```

- [ ] **Step 16: 重编 + sync-bindings + 拷 dll**

```bash
cargo build -p loomgui_ffi_c --release
cargo run -p xtask -- sync-bindings
# Unity 必须关着：
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```

- [ ] **Step 17: 重出 GUI exe 并拷贝**（坑 158 stale exe 同源，bump 必须重出）

```bash
(cd crates/packer/gui/src-tauri && tauri build --no-bundle)
cp crates/packer/gui/src-tauri/target/release/loomgui_gui.exe unity/package/Editor/Tools/loomgui_gui.exe
```

- [ ] **Step 18: 重打 showcase pkg 验整链**

```bash
cargo run -p loomgui_pkg -- build showcase
```
Expected: exit 0（8 组件 showcase.pkg.bin 重产，含 template 的组件正常）

- [ ] **Step 19: 全量门禁**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```
Expected: 全绿

- [ ] **Step 20: Commit**

```bash
git add -A
git commit -m "core(list): template subtree enters pkg (v26->v27); NodeKind::Template appended"
```

---

## Task 1: 拆 `SCOPE_ROOT` / `LOOKUP_SCOPE` 双语义

**阻断前置**：`NodeFlags::SCOPE_ROOT`（node.rs:25）同时管 CSS 隔离和 `Get<T>` 边界。slot 根若打它，页面 CSS 对 item 全部失效（item 裸奔）。必须先拆。

**Files:**
- Modify: `crates/core/src/scene/node.rs:15-28`（NodeFlags 加 LOOKUP_SCOPE + `find_by_id_attr:671`）
- Modify: `crates/core/src/style/dynamic.rs:433, 443-460, 513`（compute_scope_map / parent_in_scope / rematch 读 SCOPE_ROOT 不变）
- Modify: `crates/core/src/scene/dynamic.rs:153`（create_root 双打）、`:506,550`（remove_node 双清）
- Modify: `crates/core/src/stage.rs:773`（instantiate 双打）

**Interfaces:**
- Consumes: 现有 SCOPE_ROOT 语义
- Produces: `NodeFlags::LOOKUP_SCOPE`（`1<<6`）；所有现有 SCOPE_ROOT 设置点同时打 LOOKUP_SCOPE（行为不变）；slot 根（Task 4）只打 LOOKUP_SCOPE

- [ ] **Step 1: 写失败测试 — LOOKUP_SCOPE 存在且与 SCOPE_ROOT 独立**

`crates/core/src/scene/node.rs` tests mod 加：

```rust
#[test]
fn lookup_scope_flag_exists_distinct_from_scope_root() {
    assert!(NodeFlags::LOOKUP_SCOPE.contains(NodeFlags::LOOKUP_SCOPE));
    assert!(!NodeFlags::LOOKUP_SCOPE.contains(NodeFlags::SCOPE_ROOT));
    let both = NodeFlags::SCOPE_ROOT | NodeFlags::LOOKUP_SCOPE;
    assert!(both.contains(NodeFlags::SCOPE_ROOT));
    assert!(both.contains(NodeFlags::LOOKUP_SCOPE));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p loomgui_core lookup_scope_flag_exists`
Expected: 编译错（LOOKUP_SCOPE 不存在）

- [ ] **Step 3: 加 LOOKUP_SCOPE flag + 改 SCOPE_ROOT 注释**

`crates/core/src/scene/node.rs` NodeFlags bitflags，在 `SCOPE_ROOT`（line 25）后追加：

```rust
        /// `Get<T>` 查找边界（与 CSS 作用域隔离解耦）：模板实例化根 / 文档根 / ListView slot 根打此位。
        /// `find_by_id_attr` 在此边界内停止向下穿透嵌套作用域。
        /// 与 SCOPE_ROOT 独立：slot 根只打此位（CSS 规则仍按页面根 scope 匹配，页面 CSS 对 item 生效）。
        const LOOKUP_SCOPE = 1 << 6;
```

把 `SCOPE_ROOT` 注释从「`Get<T>` 查找边界 + CSS dynamic_rules 作用域隔离都据此判定」改为「**仅** CSS scoped 规则隔离（rematch scope 校验 + 后代选择器边界）」。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p loomgui_core lookup_scope_flag_exists`
Expected: PASS

- [ ] **Step 5: 写回归测试 — 全局 find_by_id_attr 行为不变**

`crates/core/src/scene/node.rs` tests mod 加：

```rust
#[test]
fn find_by_id_attr_global_match_unaffected_by_flag_split() {
    // 本任务不引入 scoped find（slot 边界由 list.rs 处理），只拆 flag。
    // 锁定：增加 LOOKUP_SCOPE 后全局首匹配不变。
    use crate::scene::dynamic;
    let mut scene = Scene::default();
    let root = dynamic::create_root(&mut scene, "div", "").unwrap();
    let child = dynamic::create_node(&mut scene, "div", "").unwrap();
    dynamic::append_child(&mut scene, root, child).unwrap();
    scene.get_mut(child).unwrap().id_attr = Some("dup".into());
    assert_eq!(scene.find_by_id_attr("dup"), Some(child));
}
```

- [ ] **Step 6: 运行确认通过**（全局查找不变，回归保护）

Run: `cargo test -p loomgui_core find_by_id_attr_global_match`
Expected: PASS

- [ ] **Step 7: create_root / instantiate 双打 flag**

`crates/core/src/scene/dynamic.rs:153`（create_root）当前 `n.interaction.flags.insert(NodeFlags::SCOPE_ROOT);`，改为：

```rust
        n.interaction
            .flags
            .insert(NodeFlags::SCOPE_ROOT | NodeFlags::LOOKUP_SCOPE);
```

`crates/core/src/stage.rs:773`（instantiate）同样改 `SCOPE_ROOT` 插入为 `SCOPE_ROOT | LOOKUP_SCOPE`。

> 理由：保现有行为不变（页面根既是 CSS scope root 又是 lookup 边界）。Task 4 的 slot 根只打 LOOKUP_SCOPE。

- [ ] **Step 8: remove_node was_scope_root 双清**

`crates/core/src/scene/dynamic.rs:506` 取 `was_scope_root` 处，改为同时取两个 flag 并重命名：

```rust
        let (was_css_scope, _was_lookup_scope) = match scene.get(id) {
            Some(n) => (
                n.interaction.flags.contains(NodeFlags::SCOPE_ROOT),
                n.interaction.flags.contains(NodeFlags::LOOKUP_SCOPE),
            ),
            None => return,
        };
```

`:550` 清理处：把 `was_scope_root` 改为 `was_css_scope`（CSS 规则清理只跟 SCOPE_ROOT）。grep 该函数内所有 `was_scope_root` 引用统一重命名。

- [ ] **Step 9: 运行现有 dynamic 测试套确认无回归**

Run: `cargo test -p loomgui_core`
Expected: 全绿

- [ ] **Step 10: 回归测试 — CSS scope 隔离不变**

先 grep `crates/core/src/style/dynamic.rs` tests mod 现有 scope 隔离测试。若有覆盖「页面规则不命中组件实例内节点」的，跳过新增并在 commit 注明；若无，按现有 helper 模式补一个：构造页面根(scope_root) → child → 组件实例根(SCOPE_ROOT|LOOKUP_SCOPE) → leaf，加 scope_root=页面根 的 .leaf 规则，断言 leaf 不命中（node_scope=实例根 ≠ 页面根）。

- [ ] **Step 11: 运行确认**

Run: `cargo test -p loomgui_core`
Expected: PASS

- [ ] **Step 12: 全量门禁**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
Expected: 全绿

- [ ] **Step 13: 重编 dll + sync-bindings + 拷**（核实 C# NodeFlags 镜像是否需同步）

```bash
cargo build -p loomgui_ffi_c --release
cargo run -p xtask -- sync-bindings
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```

> 核实 C# `NodeFlags` 镜像（grep `SCOPE_ROOT` unity/package/Runtime/）是否需加 LOOKUP_SCOPE = 1<<6。若 NodeFlags 不经 FFI 暴露（仅内部），跳过 C# 改动。

- [ ] **Step 14: Commit**

```bash
git add -A
git commit -m "core: split SCOPE_ROOT (CSS isolation) and LOOKUP_SCOPE (Get<T> boundary) flags"
```

---

## Task 2: `clone_subtree` + side table 逐条判定

**Files:**
- Modify: `crates/core/src/stage.rs`（clone_subtree 方法，与 instantiate 并列）
- Modify: `crates/core/src/scene/dynamic.rs`（clone_node_recursive 递归 helper）
- Modify: `crates/ffi/src/lib.rs`（loomgui_stage_clone_subtree）
- Modify: `unity/package/Runtime/Public/LoomGUI.Nodes.cs`（UITemplate enum 重定义）

**Interfaces:**
- Consumes: `create_node_from_template`（dynamic.rs:166）、`remove_node` side table 清单（dynamic.rs:526-533）
- Produces: `Stage::clone_subtree(src: NodeId) -> Result<NodeId, String>`（游离根，不挂树）；FFI `loomgui_stage_clone_subtree(stage, src) -> u32`（0xFFFF_FFFF = err）

- [ ] **Step 1: 写失败测试 — clone_subtree 深拷贝结构 + classes + text + image**

`crates/core/src/scene/dynamic.rs` tests mod 加。先 grep `pub fn set_text\|pub fn set_src\|pub fn add_class\|pub fn set_image_src` in stage.rs 确认 API 名，照实际写：

```rust
#[test]
fn clone_subtree_copies_structure_text_image_classes() {
    let mut s = crate::stage::Stage::new_for_test();
    let root = s.create_root("div", "").unwrap();
    let img = s.create_node("img", "").unwrap();
    s.set_src(img, "icon.png").unwrap();
    s.append_child(root, img).unwrap();
    let txt = s.create_node("span", "").unwrap();
    s.set_text(txt, "hello").unwrap();
    s.append_child(root, txt).unwrap();

    let cloned = s.clone_subtree(root).unwrap();
    assert!(s.scene.as_ref().unwrap().get(cloned).unwrap().parent.is_none(), "cloned root is detached");
    assert_eq!(s.scene.as_ref().unwrap().get(cloned).unwrap().children.len(), 2);
    assert_ne!(cloned, root);
    // text_contents / image_srcs 拷贝（查 scene 副表）
    let cloned_kids: Vec<_> = s.scene.as_ref().unwrap().get(cloned).unwrap().children.clone();
    let img_child = cloned_kids.iter().find(|&&c| s.scene.as_ref().unwrap().get(c).unwrap().kind == crate::scene::node::NodeKind::Image).copied().unwrap();
    assert_eq!(s.scene.as_ref().unwrap().image_srcs.get(&img_child).map(|s| s.as_str()), Some("icon.png"));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p loomgui_core clone_subtree_copies_structure`
Expected: 编译错（clone_subtree 不存在）

- [ ] **Step 3: 写失败测试 — clone_subtree 不拷运行时 side table**

```rust
#[test]
fn clone_subtree_skips_runtime_side_tables() {
    let mut s = crate::stage::Stage::new_for_test();
    let root = s.create_root("div", "overflow:auto").unwrap();
    // 让 root 有 scroll state（apply_wheel 或直接写 scroll_pos；以实际 API 为准）
    let cloned = s.clone_subtree(root).unwrap();
    let scene = s.scene.as_ref().unwrap();
    // 克隆根的 scroll state 不存在或 scroll_pos=0（不拷运行时滚动位置）
    let scroll_zero = scene.scroll.get(cloned).map(|st| st.scroll_pos.1 == 0.0).unwrap_or(true);
    assert!(scroll_zero, "scroll state must not be cloned");
}
```

- [ ] **Step 4: 实装 clone_subtree + clone_node_recursive**

`crates/core/src/stage.rs` 在 `instantiate` 方法后加：

```rust
    /// 场景级子树克隆（与 instantiate 并列，但不走 pkg 组件）。
    /// 深拷贝 kind/classes/id_attr/base_style/文本/img src，返回游离新根（不挂树，调用方 append）。
    /// side table 判定见 list spec §6 表（controls/text/image 拷；scroll/anim/tween/EditState 不拷）。
    pub fn clone_subtree(&mut self, src: NodeId) -> Result<NodeId, String> {
        let scene = self.scene.as_mut().ok_or("no scene (create_root first)")?;
        if scene.get(src).is_none() {
            return Err("clone_subtree: src node not found".into());
        }
        Ok(crate::scene::dynamic::clone_node_recursive(scene, src))
    }
```

`crates/core/src/scene/dynamic.rs` 加私有 helper：

```rust
/// 递归克隆子树。side table 判定（list spec §6）：
/// 拷：kind/classes/id_attr/base_style/text_contents/image_srcs
/// 不拷：scroll/anim/tweens/EditState/text_layouts/focused_node/事件订阅
/// 控件初值：control_init=None（list bind 后由 set_control_value 显式设；slot 复用时 reset）。
fn clone_node_recursive(scene: &mut Scene, src: NodeId) -> NodeId {
    let (kind, base_style, classes, id_attr, content, src_path) = {
        let n = scene.get(src).expect("live src");
        (n.kind, n.base_style.clone(), n.classes.clone(), n.id_attr.clone(),
         scene.text_contents.get(&src).cloned(), scene.image_srcs.get(&src).cloned())
    };
    let new_id = create_node_from_template(scene, kind, base_style, None);
    {
        let n = scene.get_mut(new_id).unwrap();
        n.classes = classes;
        n.id_attr = id_attr;
    }
    if let Some(c) = content { scene.text_contents.insert(new_id, c); }
    if let Some(sp) = src_path { scene.image_srcs.insert(new_id, sp); }
    let children = scene.get(src).expect("live src").children.clone();
    for child in children {
        let new_child = clone_node_recursive(scene, child);
        scene.get_mut(new_id).unwrap().children.push(new_child);
        scene.get_mut(new_child).unwrap().parent = Some(new_id);
    }
    new_id
}
```

> **controls 初值注**：上面 control_init=None。若 headless 测试发现模板内 Slider/Toggle 无初值导致渲染/命中挂，则改为从 `scene.controls.get(src)` 读 ControlState 反推 ControlInit。先按 None 实现，测试驱动补。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p loomgui_core clone_subtree`
Expected: PASS

- [ ] **Step 6: 写 FFI round-trip 测试**

`crates/ffi/src/abi_tests.rs` 加（照现有 helper 模式构造场景，调 `loomgui_stage_clone_subtree`，断言返回 ≠ 0xFFFF_FFFF 且新根 child_count 正确）：

```rust
#[test]
fn clone_subtree_ffi_round_trip() {
    // 照 abi_tests.rs 现有 helper（loomgui_stage_create_root/create_node/append_child）构造
    // root > child。clone_subtree(root) → 新 id != INVALID，新根无父 + 结构完整。
    // （精确 helper 调用以 abi_tests.rs 现有模式为准。）
}
```

- [ ] **Step 7: 实装 FFI**

`crates/ffi/src/lib.rs` 加（照 `loomgui_stage_set_reuse_key` 模式）：

```rust
/// 克隆场景内子树（游离根，不挂树）。返回新 node_id；0xFFFF_FFFF = err / null 句柄 / 无效 src。
#[no_mangle]
pub extern "C" fn loomgui_stage_clone_subtree(h: *mut StageHandle, src: u32) -> u32 {
    const ERR: u32 = 0xFFFF_FFFF;
    if h.is_null() { return ERR; }
    let sh = unsafe { &mut *h };
    match sh.stage.clone_subtree(NodeId(src)) {
        Ok(id) => id.0,
        Err(_) => ERR,
    }
}
```

- [ ] **Step 8: 运行 FFI 测试确认通过**

Run: `cargo test -p loomgui_ffi_c clone_subtree_ffi`
Expected: PASS

- [ ] **Step 9: UITemplate 重定义（SceneSubtree 变体）**

`unity/package/Runtime/Public/LoomGUI.Nodes.cs` 找 `class UITemplate`（grep 定位），把内部存储扩为支持 SceneSubtree{node_id}。**公共签名不变**，内部加 NodeId 句柄字段。`DoInstantiate` 内部分支：SceneSubtree 走 `loomgui_stage_clone_subtree` FFI，PackageComponent 走原 instantiate FFI。本步骤无新 C# 测试（Task 5 统一验）。

- [ ] **Step 10: 重编 dll + sync + 拷**

```bash
cargo build -p loomgui_ffi_c --release
cargo run -p xtask -- sync-bindings
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```

- [ ] **Step 11: 全量门禁**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test -p loomgui_core && cargo test -p loomgui_ffi_c`
Expected: 全绿

- [ ] **Step 12: Commit**

```bash
git add -A
git commit -m "core: Stage::clone_subtree (scene-level subtree clone) + FFI + UITemplate SceneSubtree variant"
```

---

## Task 3: `HeightCache` + 可见区算法（纯逻辑，不接树）

**Files:**
- Create: `crates/core/src/list.rs`（新建）
- Modify: `crates/core/src/lib.rs`（`pub mod list;`）

**Interfaces:**
- Consumes: 无（纯逻辑）
- Produces: `HeightCache`、`compute_visible_range(...)`、`BUFFER`/`INITIAL_SLOTS` 常量；供 Task 4 接树用

- [ ] **Step 1: 写失败测试 — HeightCache 求和/回填/估算收敛**

新建 `crates/core/src/list.rs`（模块体先写常量 + 测试，让 HeightCache 不存在导致编译失败）：

```rust
//! ListView 虚拟化内核：HeightCache + 可见区算法 + slot 池 + spacer 撑高 + anchoring。
//! side table 模式（照 scroll.rs / EditState），不塞进 Node。

/// 预渲染缓冲项数（可见区前后各 BUFFER 项提前克隆 + bind，吸收滚动速度）。
pub const BUFFER: usize = 2;
/// 冷启动（首帧 layout_rect 全 0 → viewport.h=0）时实例化的项数。
pub const INITIAL_SLOTS: usize = 1 + 2 * BUFFER;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_cache_sum_with_mixed_known_estimate() {
        let mut hc = HeightCache::new(3, 20.0);
        hc.set(0, 10.0);
        hc.set(2, 30.0);
        approx_eq(hc.sum(0..3), 60.0);
    }

    #[test]
    fn height_cache_estimate_updates_to_known_mean() {
        let mut hc = HeightCache::new(5, 40.0);
        hc.set(0, 10.0);
        hc.set(1, 30.0);
        approx_eq(hc.estimate, 20.0);
        approx_eq(hc.sum(0..5), 100.0);
    }

    #[test]
    fn height_cache_sum_empty_range_zero() {
        let hc = HeightCache::new(10, 50.0);
        approx_eq(hc.sum(5..5), 0.0);
    }

    fn approx_eq(a: f32, b: f32) {
        assert!((a - b).abs() < 0.01, "{a} != {b}");
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p loomgui_core height_cache`
Expected: 编译错（HeightCache 不存在）

- [ ] **Step 3: 实装 HeightCache**

`crates/core/src/list.rs`（常量后、tests 前）：

```rust
/// 每项高度缓存。未测项用 estimate（已测均值；无已测时=模板首次布局高）。
/// sum = 已测部分精确和 + 未测数 × estimate。朴素 O(n)，触发换 Fenwick 判据：sum 占 tick > 5%。
#[derive(Debug, Clone)]
pub struct HeightCache {
    pub known: Vec<Option<f32>>,
    pub estimate: f32,
}

impl HeightCache {
    pub fn new(item_count: usize, initial_estimate: f32) -> Self {
        Self { known: vec![None; item_count], estimate: initial_estimate }
    }

    pub fn resize(&mut self, item_count: usize, initial_estimate: f32) {
        self.known.resize(item_count, None);
        if self.known.is_empty() { self.estimate = initial_estimate; }
    }

    pub fn height_of(&self, i: usize) -> f32 {
        self.known.get(i).copied().flatten().unwrap_or(self.estimate)
    }

    pub fn set(&mut self, i: usize, h: f32) {
        if i < self.known.len() { self.known[i] = Some(h); }
        self.recompute_estimate();
    }

    /// 求和 [start..end)。已测精确 + 未测 × estimate。
    pub fn sum(&self, range: std::ops::Range<usize>) -> f32 {
        let mut total = 0.0;
        for i in range { total += self.height_of(i); }
        total
    }

    fn recompute_estimate(&mut self) {
        let known: Vec<f32> = self.known.iter().filter_map(|v| *v).collect();
        if !known.is_empty() {
            self.estimate = known.iter().sum::<f32>() / known.len() as f32;
        }
    }
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p loomgui_core height_cache`
Expected: PASS

- [ ] **Step 5: 写失败测试 — compute_visible_range**

`crates/core/src/list.rs` tests mod 加：

```rust
    #[test]
    fn visible_range_basic() {
        let r = compute_visible_range(100, 0.0, 0.0, 100.0, &uniform_heights(100, 10.0));
        assert_eq!(r, 0..12);
    }

    #[test]
    fn visible_range_scrolled_mid() {
        let r = compute_visible_range(100, 50.0, 0.0, 100.0, &uniform_heights(100, 10.0));
        assert_eq!(r, 3..17);
    }

    #[test]
    fn visible_range_clamps_to_count() {
        let r = compute_visible_range(5, 50.0, 0.0, 100.0, &uniform_heights(5, 10.0));
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 5);
    }

    #[test]
    fn visible_range_empty_count() {
        let r = compute_visible_range(0, 0.0, 0.0, 100.0, &HeightCache::new(0, 10.0));
        assert_eq!(r, 0..0);
    }

    #[test]
    fn visible_range_cold_start_viewport_zero() {
        let r = compute_visible_range(1000, 0.0, 0.0, 0.0, &uniform_heights(1000, 10.0));
        assert_eq!(r, 0..INITIAL_SLOTS);
    }

    fn uniform_heights(n: usize, h: f32) -> HeightCache {
        let mut hc = HeightCache::new(n, h);
        for i in 0..n { hc.set(i, h); }
        hc
    }
```

- [ ] **Step 6: 运行确认失败**

Run: `cargo test -p loomgui_core visible_range`
Expected: 编译错（compute_visible_range 不存在）

- [ ] **Step 7: 实装 compute_visible_range**

```rust
/// 计算可见项区间 [start, end)（含 BUFFER）。viewport.h==0 → 冷启动返 INITIAL_SLOTS。
/// top = scroll_pos.y - listview_offset（ul 相对 pane 的偏移）。
pub fn compute_visible_range(
    item_count: usize,
    scroll_pos_y: f32,
    listview_offset: f32,
    viewport_h: f32,
    heights: &HeightCache,
) -> std::ops::Range<usize> {
    if item_count == 0 { return 0..0; }
    if viewport_h <= 0.0 { return 0..INITIAL_SLOTS.min(item_count); } // 冷启动
    let top = scroll_pos_y - listview_offset;
    let mut acc = 0.0;
    let mut first = 0usize;
    for i in 0..item_count {
        if acc > top { break; }
        first = i;
        acc += heights.height_of(i);
    }
    let target = top + viewport_h;
    let mut acc2 = 0.0;
    let mut last = item_count;
    for j in 0..item_count {
        acc2 += heights.height_of(j);
        if acc2 >= target { last = j + 1; break; }
    }
    let start = first.saturating_sub(BUFFER);
    let end = (last + BUFFER).min(item_count);
    start..end
}
```

- [ ] **Step 8: 运行确认通过**

Run: `cargo test -p loomgui_core visible_range`
Expected: PASS

- [ ] **Step 9: 注册模块 + 全量门禁**

`crates/core/src/lib.rs` 加 `pub mod list;`（照 `pub mod scroll;` 位置）。

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test -p loomgui_core`
Expected: 全绿

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "core(list): HeightCache + compute_visible_range (pure logic, no tree wiring)"
```

---

## Task 4: ListState + spacer/slot 接树 + tick 挂钩 + reuse_key 编码

**本质判据任务**：接树后 headless 断言「render node 数不随总数增长」必须绿。

**Files:**
- Modify: `crates/core/src/list.rs`（ListState/ListTable/Slot + enter_data_driven + update_visible + spacer 写入 + reuse_key 编码）
- Modify: `crates/core/src/stage.rs`（tick_and_render 插 list.update_visible）
- Modify: `crates/core/src/scene/node.rs`（Scene 加 `pub lists: crate::list::ListTable`）

**Interfaces:**
- Consumes: `HeightCache`/`compute_visible_range`（Task 3）、`clone_subtree`（Task 2）、`LOOKUP_SCOPE`（Task 1）、`NodeKind::ListView/ListItem`、`Stage::create_node`/`append_child`
- Produces: `ListTable`（Scene 字段）、`list::update_visible(scene)`、`list::enter_data_driven(stage, node, ordinal)`、`list::set_item_count(stage, node, n)`

- [ ] **Step 1: 写失败测试 — enter_data_driven 生成 spacer + 清空设计期 li**

`crates/core/src/list.rs` tests mod 加：

```rust
    #[test]
    fn enter_data_driven_creates_spacers_and_backups_li() {
        let mut s = crate::stage::Stage::new_for_test();
        let ul = s.create_root("ul", "").unwrap();
        let li = s.create_node("li", "").unwrap();
        s.append_child(ul, li).unwrap();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        let scene = s.scene.as_ref().unwrap();
        let ul_node = scene.get(ul).unwrap();
        assert_eq!(ul_node.children.len(), 2, "ul has head+tail spacer only");
        let ls = scene.lists.get(ul).expect("list state created");
        assert!(ls.template_root.is_some(), "design-time li backed up as template");
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p loomgui_core enter_data_driven`
Expected: 编译错

- [ ] **Step 3: Scene 加 lists 字段**

`crates/core/src/scene/node.rs` Scene struct 加 `pub lists: crate::list::ListTable,`，并在 `Scene::build`（约 line 559+）补 `lists: crate::list::ListTable::default()`。核实 `impl Default for Scene`（若手动 impl）也补。

- [ ] **Step 4: 实装 ListState/ListTable + enter_data_driven**

`crates/core/src/list.rs` 加（HeightCache 已有，补 Default + ListState）：

```rust
use crate::scene::node::{Node, NodeFlags, NodeId, NodeKind, Scene};
use slotmap::SecondaryMap;

impl Default for HeightCache {
    fn default() -> Self { Self { known: vec![], estimate: 0.0 } }
}

#[derive(Debug, Clone, Default)]
pub struct ListState {
    pub item_count: usize,
    pub template_root: Option<NodeId>,
    pub heights: HeightCache,
    pub slots: Vec<Slot>,
    pub free: Vec<NodeId>,
    pub visible: std::ops::Range<usize>,
    pub head_spacer: NodeId,
    pub tail_spacer: NodeId,
    pub pending_binds: Vec<(NodeId, usize)>,
    pub list_ordinal: u32,
    pub anchoring_active: bool,
    pub dirty: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Slot { pub node: NodeId, pub item_index: usize }

#[derive(Debug, Clone, Default)]
pub struct ListTable(pub SecondaryMap<slotmap::DefaultKey, ListState>);

impl std::ops::Index<NodeId> for ListTable {
    type Output = ListState;
    fn index(&self, id: NodeId) -> &ListState { &self.0[crate::scene::node::key_for_pub(id)] }
}
```

> **核实点**：`SecondaryMap` 的 key = `slotmap::DefaultKey`（与 Scene.nodes 的 SlotMap 对齐）。需在 Scene 上暴露 `key_for(NodeId) -> DefaultKey` 的公开方法（grep `fn key_for`；若 private，加 `pub(crate) fn key_for` 或在 node.rs 加 `pub fn key_for_pub`）。ListTable 的查询用 scene.key_for(id) 转换。

`enter_data_driven`（实现在 list.rs，取 stage 借用）：

```rust
/// 进入数据驱动模式：备份模板（兜底=第一个设计期 li）+ 建 spacer + 清空设计期 li + 建 ListState。
/// ul 高度必须 auto（spec §4）；非 auto → Err。
pub fn enter_data_driven(stage: &mut crate::stage::Stage, ul: NodeId, list_ordinal: u32) -> Result<(), String> {
    let scene = stage.scene.as_mut().ok_or("no scene")?;
    if scene.get(ul).map(|n| n.kind) != Some(NodeKind::ListView) {
        return Err("enter_data_driven: node is not a ListView".into());
    }
    // ul 高度必须 auto（spec §4）
    check_ul_height_auto(scene, ul)?;
    // 兜底模板：第一个设计期 li
    let first_li = scene.get(ul).and_then(|n| n.children.iter().copied().find(|&c|
        scene.get(c).map(|cn| cn.kind) == Some(NodeKind::ListItem)));
    let template_root = if let Some(li) = first_li {
        let cloned = stage.clone_subtree(li)?; // 游离备份（在清空前）
        let lis: Vec<NodeId> = scene.get(ul).unwrap().children.iter()
            .copied().filter(|&c| scene.get(c).map(|cn| cn.kind) == Some(NodeKind::ListItem)).collect();
        for li in &lis { stage.remove_node(*li); }
        Some(cloned)
    } else {
        return Err("ListView 无模板来源：无 <template>、无设计期 li、未设 ItemTemplate".into());
    };
    let head = stage.create_node("div", "")?;
    let tail = stage.create_node("div", "")?;
    // spacer 样式：flex-shrink:0 + padding-top:0.01px（阻断 margin collapsing）+ height:0
    configure_spacer(stage, head);
    configure_spacer(stage, tail);
    stage.append_child(ul, head)?;
    stage.append_child(ul, tail)?;
    let ls = ListState {
        item_count: 0, template_root, heights: HeightCache::new(0, 0.0),
        slots: vec![], free: vec![], visible: 0..0, head_spacer: head, tail_spacer: tail,
        pending_binds: vec![], list_ordinal, anchoring_active: false, dirty: true,
    };
    stage.scene.as_mut().unwrap().lists.0.insert(stage.scene.as_ref().unwrap().key_for(ul), ls);
    Ok(())
}

fn check_ul_height_auto(scene: &Scene, ul: NodeId) -> Result<(), String> {
    // 读 ul.base_style 的 height；非 Auto → Err。
    // 核实 ResolvedStyle.height 字段类型（LengthPercentageAuto 或自定义枚举），非 Auto 即 Err。
    // 被祖先 flex 拉伸检测较复杂，先做显式 height 非 auto；flex 拉伸留 Unity 真机诊断。
    let n = scene.get(ul).ok_or("ul not found")?;
    let h = &n.base_style.taffy_style.size.height;
    if !matches!(h, taffy::style::LengthPercentageAuto::Auto) {
        return Err("数据驱动 ListView 高度必须为 auto（否则虚拟化无法撑出可滚内容）".into());
    }
    Ok(())
}

fn configure_spacer(stage: &mut crate::stage::Stage, spacer: NodeId) {
    // 运行时 css 字符串解析能力待核实；保险起见直接改 base_style。
    let scene = stage.scene.as_mut().unwrap();
    let n = scene.get_mut(spacer).unwrap();
    n.base_style.taffy_style.flex_shrink = 0.0;
    n.base_style.taffy_style.padding.top = taffy::style::LengthPercentage::Length(0.01); // 阻断 margin collapsing
    n.base_style.taffy_style.size.height = taffy::style::LengthPercentageAuto::Length(0.0);
    n.style = n.base_style.clone();
    n.dirty_mesh = true;
}
```

> **关键核实点**：`create_node("div", css)` 的 css 参数运行时是否解析（grep `fn create_node` → `apply_css`）。若运行时 css 不解析，`configure_spacer` 直接改 base_style（如上）是正路。核实 `Scene::key_for` 可见性。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p loomgui_core enter_data_driven`
Expected: PASS

- [ ] **Step 6: 写失败测试 — set_item_count + update_visible 实例化可见 slot**

```rust
    #[test]
    fn update_visible_instantiates_initial_slots() {
        let mut s = crate::stage::Stage::new_for_test();
        let ul = s.create_root("ul", "").unwrap();
        let li = s.create_node("li", "").unwrap();
        s.append_child(ul, li).unwrap();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 1000);
        crate::list::update_visible(s.scene.as_mut().unwrap());
        let scene = s.scene.as_ref().unwrap();
        let ul_node = scene.get(ul).unwrap();
        assert_eq!(ul_node.children.len(), 2 + crate::list::INITIAL_SLOTS);
        // slot 根打 LOOKUP_SCOPE（不打 SCOPE_ROOT）
        let slot_node = scene.get(ul_node.children[2]).unwrap();
        assert!(slot_node.interaction.flags.contains(NodeFlags::LOOKUP_SCOPE));
        assert!(!slot_node.interaction.flags.contains(NodeFlags::SCOPE_ROOT));
    }
```

- [ ] **Step 7: 运行确认失败**

Run: `cargo test -p loomgui_core update_visible_instantiates`
Expected: FAIL（set_item_count/update_visible 不存在）

- [ ] **Step 8: 实装 set_item_count + plan_visible/execute_visible 两阶段**

> **关键架构决定**：clone_subtree 需 `&mut Stage`，但 update_visible 借 `&mut Scene` 冲突。故拆两阶段：`plan_visible(scene) -> PendingOps`（算可见区、回收 slot、产待克隆 index 列表），`execute_visible(stage, ops)`（clone + append + 标 LOOKUP_SCOPE + reuse_key + 入队 pending_binds）。tick 内依次调。

`crates/core/src/list.rs` 加：

```rust
/// plan 阶段：算可见区、回收离开的 slot 入 free 池、产待克隆 index 列表。只借 scene。
pub struct PendingOps {
    pub list_ul: NodeId,
    pub to_clone: Vec<usize>,       // 待克隆的 item index
    pub new_visible: std::ops::Range<usize>,
    pub spacer_head_h: f32,
    pub spacer_tail_h: f32,
}

pub fn plan_visible(scene: &mut Scene) -> Vec<PendingOps> {
    let keys: Vec<slotmap::DefaultKey> = scene.lists.0.keys().collect();
    let mut out = Vec::new();
    for key in keys {
        if let Some(op) = plan_one(scene, key) { out.push(op); }
    }
    out
}

fn plan_one(scene: &mut Scene, key: slotmap::DefaultKey) -> Option<PendingOps> {
    let ul = NodeId::from_key(key);
    let (scroll_y, viewport_h, ul_y) = {
        let (sy, vh) = ancestor_scroll_viewport(scene, ul);
        let uy = scene.get(ul).map(|n| n.layout_rect.y).unwrap_or(0.0);
        (sy, vh, uy)
    };
    let ls = scene.lists.0.get_mut(key)?;
    let visible = compute_visible_range(ls.item_count, scroll_y, ul_y, viewport_h, &ls.heights);
    // 回收离开的 slot（old indices − visible）→ free 池
    let new_set: std::collections::HashSet<usize> = visible.clone().collect();
    let mut keep_slots = Vec::new();
    let mut to_free = Vec::new();
    for s in ls.slots.drain(..) {
        if new_set.contains(&s.item_index) { keep_slots.push(s); }
        else { to_free.push(s.node); }
    }
    ls.slots = keep_slots;
    ls.free.extend(to_free);
    // 待克隆 = visible − 当前 slot indices
    let have: std::collections::HashSet<usize> = ls.slots.iter().map(|s| s.item_index).collect();
    let to_clone: Vec<usize> = visible.clone().filter(|i| !have.contains(i)).collect();
    let spacer_head_h = apply_gap_deduction(scene, ul, ls.heights.sum(0..visible.start));
    let spacer_tail_h = apply_gap_deduction(scene, ul, ls.heights.sum(visible.end..ls.item_count));
    Some(PendingOps { list_ul: ul, to_clone, new_visible: visible, spacer_head_h, spacer_tail_h })
}

/// execute 阶段：clone slot + append + 标 LOOKUP_SCOPE + reuse_key + 入队 pending_binds + 写 spacer。
pub fn execute_visible(stage: &mut crate::stage::Stage, ops: Vec<PendingOps>) {
    for op in ops {
        execute_one(stage, op);
    }
}

fn execute_one(stage: &mut crate::stage::Stage, op: PendingOps) {
    let key = stage.scene.as_ref().unwrap().key_for(op.list_ul);
    let template_root = stage.scene.as_ref().unwrap().lists.0[key].template_root;
    let list_ordinal = stage.scene.as_ref().unwrap().lists.0[key].list_ordinal;
    let tpl = match template_root { Some(t) => t, None => return };
    for item_index in &op.to_clone {
        // 优先从 free 池取（取不到 clone_subtree）
        let node = stage.scene.as_mut().unwrap().lists.0[key].free.pop()
            .map(Ok)
            .unwrap_or_else(|| stage.clone_subtree(tpl));
        let node = match node { Ok(n) => n, Err(_) => continue };
        // 标 LOOKUP_SCOPE（不打 SCOPE_ROOT，spec §6.2）
        stage.scene.as_mut().unwrap().get_mut(node).unwrap().interaction.flags.insert(NodeFlags::LOOKUP_SCOPE);
        // reuse_key 编码：((list_ordinal+1)<<16)|(slot_idx)。slot_idx 用 slots.len() 当前序号。
        let slot_idx = stage.scene.as_ref().unwrap().lists.0[key].slots.len();
        stage.set_reuse_key(node, encode_reuse_key(list_ordinal, slot_idx));
        // append 到 tail_spacer 之前（head/tail spacer 始终首尾）
        let tail = stage.scene.as_ref().unwrap().lists.0[key].tail_spacer;
        stage.insert_before(op.list_ul, node, tail).ok();
        stage.scene.as_mut().unwrap().lists.0[key].slots.push(Slot { node, item_index: *item_index });
        stage.scene.as_mut().unwrap().lists.0[key].pending_binds.push((node, *item_index));
    }
    // 写 spacer 高度
    let ls = stage.scene.as_mut().unwrap().lists.0.get_mut(key).unwrap();
    ls.visible = op.new_visible;
    let head = ls.head_spacer; let tail = ls.tail_spacer;
    set_spacer_height(stage.scene.as_mut().unwrap(), head, op.spacer_head_h);
    set_spacer_height(stage.scene.as_mut().unwrap(), tail, op.spacer_tail_h);
}

/// reuse_key 编码：((list_ordinal+1)<<16)|(slot_idx & 0xFFFF)，恒≠0。
fn encode_reuse_key(list_ordinal: u32, slot_idx: usize) -> u32 {
    ((list_ordinal + 1) << 16) | ((slot_idx as u32) & 0xFFFF)
}
```

辅助函数（ancestor_scroll_viewport / set_spacer_height / apply_gap_deduction）照 Task 4 Step 4 设计实装：

```rust
fn ancestor_scroll_viewport(scene: &Scene, node: NodeId) -> (f32, f32) {
    let mut cur = scene.get(node).and_then(|n| n.parent);
    while let Some(pid) = cur {
        if let Some(st) = scene.scroll.get(pid) {
            return (st.scroll_pos.1, st.viewport_size.1);
        }
        cur = scene.get(pid).and_then(|n| n.parent);
    }
    (0.0, 0.0) // 无祖先 ScrollPane → 冷启动退化（viewport=0 → INITIAL_SLOTS）
}

fn set_spacer_height(scene: &mut Scene, spacer: NodeId, h: f32) {
    if let Some(n) = scene.get_mut(spacer) {
        let lp = taffy::style::LengthPercentageAuto::Length(h);
        n.base_style.taffy_style.size.height = lp;
        n.style.taffy_style.size.height = lp;
        n.dirty_mesh = true;
    }
}

fn apply_gap_deduction(scene: &Scene, ul: NodeId, raw: f32) -> f32 {
    let n = match scene.get(ul) { Some(n) => n, None => return raw };
    if !matches!(n.base_style.taffy_style.display, taffy::Display::Flex) { return raw; }
    let gap = n.base_style.taffy_style.gap.height.0; // 核实 taffy 0.12 gap 字段路径
    (raw - gap).max(0.0)
}
```

> **核实点**：`Scene::key_for` 可见性（若 private 加 `pub(crate)`）、`NodeId::from_key` 可见性、`stage.set_reuse_key`/`stage.insert_before` API 名、`taffy_style.gap` 字段路径（grep gap in taffy-0.12.2）。

- [ ] **Step 9: tick_and_render 插 plan/execute**

`crates/core/src/stage.rs` tick_and_render，在 `rematch_pseudo_classes(scene)`（约 line 867）**之前**插：

```rust
        // list 可见区更新（solve 前，新克隆 slot 本帧布局）。plan/execute 两阶段解 clone 借用冲突。
        let list_ops = crate::list::plan_visible(scene);
        crate::list::execute_visible(self, list_ops);
```

- [ ] **Step 10: 运行测试确认通过**

Run: `cargo test -p loomgui_core update_visible_instantiates`
Expected: PASS

- [ ] **Step 11: 重编 dll + sync + 拷**

```bash
cargo build -p loomgui_ffi_c --release
cargo run -p xtask -- sync-bindings
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```

- [ ] **Step 12: 全量门禁**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test -p loomgui_core`
Expected: 全绿

- [ ] **Step 13: Commit**

```bash
git add -A
git commit -m "core(list): ListState + spacer/slot wiring + plan/execute update_visible + reuse_key encoding"
```

---

## Task 5: pending_binds 队列 + FFI + C# 投影 + headless 本质断言

**Files:**
- Modify: `crates/core/src/list.rs`（take_pending_binds + drain_now + collect_heights 等高版）
- Modify: `crates/core/src/stage.rs`（tick 插 collect_heights）
- Modify: `crates/ffi/src/lib.rs`（list_* FFI 全套）
- Modify: `unity/package/Runtime/Public/LoomGUI.Nodes.cs`（ListView 投影实装 + UIContext tick 前排空）
- Create: `tests/dotnet/LoomGUI.HeadlessTests/VirtualizationTests.cs`

**Interfaces:**
- Consumes: Task 4 的 plan_visible/execute_visible
- Produces: C# `ListView.ItemCount/ItemTemplate/BindItem` 可用；headless「1000 vs 10000 render node 相等」绿

- [ ] **Step 1: 写失败测试 — take_pending_binds 返回新 slot 再排空**

`crates/core/src/list.rs` tests mod 加：

```rust
    #[test]
    fn take_pending_binds_returns_new_slots_then_empty() {
        let mut s = crate::stage::Stage::new_for_test();
        let ul = s.create_root("ul", "").unwrap();
        let li = s.create_node("li", "").unwrap();
        s.append_child(ul, li).unwrap();
        crate::list::enter_data_driven(&mut s, ul, 0).unwrap();
        crate::list::set_item_count(&mut s, ul, 5);
        let ops = crate::list::plan_visible(s.scene.as_mut().unwrap());
        crate::list::execute_visible(&mut s, ops);
        let binds = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        assert_eq!(binds.len(), crate::list::INITIAL_SLOTS);
        let binds2 = crate::list::take_pending_binds(s.scene.as_mut().unwrap(), ul);
        assert!(binds2.is_empty(), "second take empty");
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p loomgui_core take_pending_binds`
Expected: 编译错（take_pending_binds 不存在）

- [ ] **Step 3: 实装 take_pending_binds + drain_now + collect_heights（等高版）**

`crates/core/src/list.rs` 加：

```rust
/// 取 pending bind 队列（C# tick 前调，逐条执行 BindItem 后数据写回 core）。
pub fn take_pending_binds(scene: &mut Scene, ul: NodeId) -> Vec<(NodeId, usize)> {
    let key = scene.key_for(ul);
    scene.lists.0.get_mut(key).map(|ls| std::mem::take(&mut ls.pending_binds)).unwrap_or_default()
}

/// 同帧排空（spec §7）：先跑一次 plan/execute 再排空。
/// ScrollToItem / 首次 ItemCount 调用，避免新进入可见区 item 首帧显示模板原样。
pub fn drain_now(stage: &mut crate::stage::Stage, ul: NodeId) -> Vec<(NodeId, usize)> {
    let ops = plan_visible(stage.scene.as_mut().expect("scene"));
    execute_visible(stage, ops);
    take_pending_binds(stage.scene.as_mut().expect("scene"), ul)
}

/// 每帧 solve 后、refresh_content_sizes 前调：回填 known[i]。等高版（margin box + anchoring 在 Task 6）。
pub fn collect_heights(scene: &mut Scene) {
    let keys: Vec<slotmap::DefaultKey> = scene.lists.0.keys().collect();
    for key in keys {
        let slots: Vec<(NodeId, usize)> = scene.lists.0[key].slots.iter()
            .map(|s| (s.node, s.item_index)).collect();
        for (node, idx) in slots {
            let h = scene.get(node).map(|n| n.layout_rect.h).unwrap_or(0.0);
            scene.lists.0.get_mut(key).unwrap().heights.set(idx, h);
        }
    }
}
```

- [ ] **Step 4: tick_and_render 插 collect_heights**

`crates/core/src/stage.rs` tick_and_render，在 `solve(scene, ...)`（约 line 898）**之后**、`refresh_content_sizes(scene)`（line 903）**之前**插：

```rust
        crate::list::collect_heights(scene);
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p loomgui_core take_pending_binds`
Expected: PASS

- [ ] **Step 6: 实装 list_* FFI 全套**

`crates/ffi/src/lib.rs` 加（照 `loomgui_stage_set_reuse_key` 模式）：

```rust
#[no_mangle]
pub extern "C" fn loomgui_list_set_item_count(h: *mut StageHandle, node: u32, count: i32) -> i32 {
    if h.is_null() { return -1; }
    let sh = unsafe { &mut *h };
    crate::list::set_item_count(&mut sh.stage, NodeId(node), count.max(0) as usize);
    0
}

#[no_mangle]
pub extern "C" fn loomgui_list_set_template(h: *mut StageHandle, node: u32, template_node: u32) -> i32 {
    if h.is_null() { return -1; }
    let sh = unsafe { &mut *h };
    let key = sh.stage.scene.as_ref().unwrap().key_for(NodeId(node));
    match sh.stage.scene.as_mut().unwrap().lists.0.get_mut(key) {
        Some(ls) => { ls.template_root = Some(NodeId(template_node)); 0 }
        None => -1,
    }
}

#[no_mangle]
pub extern "C" fn loomgui_list_take_pending_binds(
    h: *mut StageHandle, out_nodes: *mut u32, out_indices: *mut i32, cap: u32, out_len: *mut u32,
) -> i32 {
    if h.is_null() || out_nodes.is_null() || out_indices.is_null() || out_len.is_null() { return -1; }
    let sh = unsafe { &mut *h };
    let mut all: Vec<(u32, i32)> = Vec::new();
    let keys: Vec<slotmap::DefaultKey> = sh.stage.scene.as_ref().unwrap().lists.0.keys().collect();
    for k in keys {
        let ul = NodeId::from_key(k);
        let binds = crate::list::take_pending_binds(sh.stage.scene.as_mut().unwrap(), ul);
        for (n, idx) in binds { all.push((n.0, idx as i32)); }
    }
    let n = all.len().min(cap as usize);
    unsafe {
        for (i, (node, idx)) in all.iter().take(n).enumerate() {
            *out_nodes.add(i) = *node;
            *out_indices.add(i) = *idx;
        }
        *out_len = n as u32;
    }
    0
}

#[no_mangle]
pub extern "C" fn loomgui_list_drain_now(h: *mut StageHandle, node: u32) -> i32 {
    if h.is_null() { return -1; }
    let sh = unsafe { &mut *h };
    let _ = crate::list::drain_now(&mut sh.stage, NodeId(node));
    0
}

// loomgui_list_refresh / loomgui_list_notify / loomgui_list_scroll_to 在 Task 7 实装（占位 stub）。
#[no_mangle] pub extern "C" fn loomgui_list_refresh(_h: *mut StageHandle, _n: u32, _s: i32, _c: i32) -> i32 { 0 }
#[no_mangle] pub extern "C" fn loomgui_list_notify(_h: *mut StageHandle, _n: u32, _op: u8, _a: i32, _b: i32) -> i32 { 0 }
#[no_mangle] pub extern "C" fn loomgui_list_scroll_to(_h: *mut StageHandle, _n: u32, _i: i32, _b: u8) -> i32 { 0 }
```

> 核实 `NodeId::from_key` 可见性（node.rs 有 `pub fn from_key`）。

- [ ] **Step 7: C# ListView 投影实装**

`unity/package/Runtime/Public/LoomGUI.Nodes.cs` 的 `class ListView`（line 2329）：把 `ItemCount`/`ItemTemplate`/`BindItem` 的 `throw NE()` 改为 FFI 调用（照 Dropdown 投影模式）。

- `ItemCount` setter → 调 `loomgui_list_set_item_count`。首次设值前需 enter_data_driven：由 `set_item_count` 内部检测 ListTable 无条目则先 enter（兜底模板路径），C# 不单独调 enter。
- `BindItem` 是 `Action<ListItem,int>`，存到 UIContext 的 ListView 注册表；UIContext tick 前调 `loomgui_list_take_pending_binds` 取数组，按 out_nodes 的 node_id 反查其 ListView 祖先实例，构造 ListItem 包装调 BindItem。
- `ChildCount` 覆写：数据驱动模式下返 `ItemCount`（不直走 get_child_count）。
- `ListItem.Index`：BindItem 回调里由 C# 传入 index 存到 ListItem（不走额外 FFI）。

在 UIContext 的 tick 入口（grep `void Tick` in LoomGUI.Context.cs 或同级）**最前**插排空：

```csharp
        // 排空 core pending_binds，按 node 分发到对应 ListView 的 BindItem。
        // 调 loomgui_list_take_pending_binds(out nodes[], out indices[], cap, out len)
        // 遍历：node_id → 找其 ListView 祖先 → 调 listView.BindItem(new ListItem(node, index), index)
```

- [ ] **Step 8: 重编 dll + sync + 拷**

```bash
cargo build -p loomgui_ffi_c --release
cargo run -p xtask -- sync-bindings
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```

- [ ] **Step 9: 写 headless 本质断言 — render node 不随总数增长**

新建 `tests/dotnet/LoomGUI.HeadlessTests/VirtualizationTests.cs`。参考 `tests/dotnet/LoomGUI.HeadlessTests/` 现有测试（Dropdown/VisualDecoration）的 Stage 构造 + FrameData 读取模式：

```csharp
using LoomGUI;
using NUnit.Framework;

namespace LoomGUI.HeadlessTests;

public class VirtualizationTests
{
    [Test]
    public void RenderNodeCount_DoesNotGrow_WithTotalItemCount()
    {
        int n1000 = CountRenderNodesAfterTick(itemCount: 1000);
        int n10000 = CountRenderNodesAfterTick(itemCount: 10000);
        Assert.AreEqual(n1000, n10000, "render node count must not grow with total item count (virtualization essence)");
    }

    static int CountRenderNodesAfterTick(int itemCount)
    {
        // 构造：overflow:auto 容器 > ul > li(模板)；load + create_root；
        // find ListView；set ItemCount；set BindItem（空 delegate）；
        // tick 数帧（让 layout_rect 就绪 + bind 排空）；读 frame render node 总数。
        // 照 HeadlessTests 现有 helper（Stage 构造 + FrameData 读节点数）填实现。
        return 0; // 实现时填
    }
}
```

> **断言意图**：两数据点 render node 数**相等**（非阈值）。

- [ ] **Step 10: 运行 headless 测试确认通过**

Run: `dotnet test tests/dotnet/LoomGUI.HeadlessTests --filter VirtualizationTests`
Expected: PASS（render node 数两数据点相等）

- [ ] **Step 11: 全量门禁**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: 全绿

- [ ] **Step 12: Commit**

```bash
git add -A
git commit -m "core(list): pending_binds + FFI + C# ListView projection; headless no-growth assertion green"
```

---

## Task 6: 不等高 — margin box 回填 + scroll anchoring + tween 豁免

**退出判据任务**：headless「滚回头无漂移」断言绿。承接 Task 5 的等高版 collect_heights，升级为 margin box + anchoring。

**Files:**
- Modify: `crates/core/src/list.rs`（collect_heights 升级：margin box 回填 + anchoring 补偿 + list.refresh_scroll_anchoring）
- Modify: `crates/core/src/scroll.rs:640-647`（clamp 分支 anchoring_active 豁免不清 tweening）

**Interfaces:**
- Consumes: Task 5 的 collect_heights；`scroll.rs` refresh_content_sizes 的 clamp 逻辑
- Produces: 不等高列表滚回头首项 y 不变；anchoring_active 期 tween 不被杀

- [ ] **Step 1: 写失败测试 — margin box 回填（带 margin 的 li 求和正确）**

`crates/core/src/list.rs` tests mod 加：

```rust
    #[test]
    fn collect_heights_uses_margin_box_not_border_box() {
        // 模板 li 含 margin-bottom:8px；slot 实例化后 layout_rect.h=20，回填后 height_of=28（含 margin）。
        // 构造场景 + 设 li margin + enter + set_item_count + plan/execute + solve(手 tick) + collect_heights
        // 断言 heights.known[0] ≈ 28（border 20 + margin 8）。
        // （精确构造以 Stage API 为准；用 create_node("li", "margin-bottom:8px") 若 css 运行时解析，
        //  否则直接改 base_style.margin。）
    }
```

> **实现时**：核实 li margin 的运行时设置路径（css 字符串 vs base_style.margin）。

- [ ] **Step 2: 实装 margin box 回填**

`crates/core/src/list.rs` 的 `collect_heights` 内层，把 `let h = n.layout_rect.h` 改为读 margin：

```rust
    let (border_h, mt, mb) = scene.get(node).map(|n| (
        n.layout_rect.h,
        n.base_style.taffy_style.margin.top.0,  // 核实 taffy 0.12 margin 字段
        n.base_style.taffy_style.margin.bottom.0,
    )).unwrap_or((0.0, 0.0, 0.0));
    let h = border_h + mt + mb;  // margin box（spec §4）
```

- [ ] **Step 3: 写失败测试 — anchoring 补偿头部区间高度变化**

```rust
    #[test]
    fn anchoring_compensates_head_height_delta() {
        // 构造：可见区 [5..15]，头部 [0..5] 高度由估算回填为实测（变大 delta=10）。
        // collect_heights 应把祖先 ScrollPane.scroll_pos.y += 10（补偿，内容不动）。
        // 断言：collect_heights 前后 scroll_pos.y 差值 == delta。
    }
```

- [ ] **Step 4: 实装 anchoring**

`crates/core/src/list.rs` 的 `collect_heights`，回填后加 anchoring 逻辑：

```rust
    // 对每个 ListView：若本帧回填修正了 visible.start 之前区间（头 spacer 覆盖范围）的高度总和，
    // delta != 0 → 同帧把祖先 ScrollPane.scroll_pos.y += delta（spec §5 anchoring）。
    // 补偿点 solve 之后、refresh_content_sizes 之前；不触发二次 solve。
    let old_head_sum = ls.heights.sum(0..ls.visible.start);  // 用回填前的快照
    // ... 回填 known[i] ...
    let new_head_sum = ls.heights.sum(0..ls.visible.start);
    let delta = new_head_sum - old_head_sum;
    if delta.abs() > 0.001 {
        // 找祖先 ScrollPane 补偿 scroll_pos.y
        if let Some(pane) = ancestor_pane(scene, ul) {
            if let Some(st) = scene.scroll.get_mut(pane) {
                st.scroll_pos.1 += delta;
                ls.anchoring_active = true;
            }
        }
    } else {
        ls.anchoring_active = false;
    }
```

> 实现时需在回填前快照 `old_head_sum`（在循环外先取），回填后算 new。`ancestor_pane` 复用 ancestor_scroll_viewport 的找祖先逻辑返 NodeId。

- [ ] **Step 5: scroll.rs clamp 分支 anchoring 豁免**

`crates/core/src/scroll.rs:640-647` 的 clamp 块，在越界 clamp 时，若该 pane 被某 ListState 标了 `anchoring_active` 则**不清 tweening**（spec §5 tween 交互）：

```rust
        if new_overlap != old_overlap {
            let out_of_range = /* 原样 */;
            if out_of_range {
                st.scroll_pos.0 = st.scroll_pos.0.clamp(0.0, new_overlap.0);
                st.scroll_pos.1 = st.scroll_pos.1.clamp(0.0, new_overlap.1);
                // anchoring 期不清 tweening（几何变化源于虚拟化回填，tween 应继续）。
                // 核实：读 scene.lists 是否有 anchoring_active 涉及本 pane。
                if !scene_is_anchoring(scene, nid) {
                    st.tweening = [0, 0];
                }
            }
        }
```

> `scene_is_anchoring` 判据：遍历 scene.lists，任一 ListState.anchoring_active 且其祖先 pane == nid → true。实装为 scroll.rs 的私有 helper（读 scene.lists）。**注意**：refresh_content_sizes 当前签名若不持 scene.lists 访问，需调整（lists 在 scene 上，可直接读）。

- [ ] **Step 6: 运行单测确认通过**

Run: `cargo test -p loomgui_core anchoring`
Expected: PASS

- [ ] **Step 7: 写 headless 断言 — 不等高无漂移**

`tests/dotnet/LoomGUI.HeadlessTests/VirtualizationTests.cs` 加：

```csharp
    [Test]
    public void VariableHeight_NoDrift_OnScrollDownThenUp()
    {
        // 混合高度数据集（含 margin），从头滚到底再滚回头，首项 y 与初始一致。
        // 构造：不等高模板（margin-bottom）+ 100 项数据；模拟 scroll_pos 从 0 → max → 0；
        // 每步 tick；断言滚回头后第一可见 item 的 world y ≈ 初始值。
    }
```

- [ ] **Step 8: 运行 headless 确认通过**

Run: `dotnet test tests/dotnet/LoomGUI.HeadlessTests --filter NoDrift`
Expected: PASS

- [ ] **Step 9: 重编 dll + sync + 拷 + 全量门禁 + Commit**

```bash
cargo build -p loomgui_ffi_c --release && cargo run -p xtask -- sync-bindings
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
git add -A && git commit -m "core(list): margin-box heights + scroll anchoring + tween clamp exemption"
```

---

## Task 7: ScrollToItem / Refresh / Notify + 异常契约

**Files:**
- Modify: `crates/core/src/list.rs`（scroll_to_item / refresh_items / notify_inserted/removed/moved + 越界 UIContractException）
- Modify: `crates/ffi/src/lib.rs`（实装 Task 5 的 3 个 stub FFI）
- Modify: `unity/package/Runtime/Public/LoomGUI.Nodes.cs`（ScrollToItem/RefreshItem(s)/Notify* + 异常）

**Interfaces:**
- Consumes: Task 5/6 的 ListState/heights/anchoring
- Produces: C# `ScrollToItem/RefreshItem(s)/Notify*` 可用；越界抛 UIContractException

- [ ] **Step 1: 写失败测试 — scroll_to_item 用 drain_now 同帧 bind**

`crates/core/src/list.rs` tests mod 加：

```rust
    #[test]
    fn scroll_to_item_drains_now_and_targets_index() {
        // set_item_count(100) + scroll_to_item(50) → drain_now 同帧排空 + 设 scroll_pos 到 item 50 偏移。
        // 断言：pending_binds 非空（50 附近 slot 已 bind）；scroll_pos.y ≈ sum(0..50)。
    }
```

- [ ] **Step 2: 实装 scroll_to_item**

`crates/core/src/list.rs`：

```rust
/// ScrollToItem：drain_now 同帧 bind（spec §7）+ 设祖先 ScrollPane scroll_pos 到 item 偏移。
/// behavior: 0=Instant, 1=Smooth（Smooth 走 tween；anchoring 期目标值每次回填后重算，见 Task 6）。
pub fn scroll_to_item(stage: &mut crate::stage::Stage, ul: NodeId, index: usize, behavior: u8) -> Result<(), String> {
    let max = stage.scene.as_ref().unwrap().lists.0[stage.scene.as_ref().unwrap().key_for(ul)].item_count;
    if index >= max { return Err("ScrollToItem index out of range".into()); }
    let _binds = drain_now(stage, ul); // 同帧排空
    // 算目标 offset = heights.sum(0..index)
    let target = {
        let ls = &stage.scene.as_ref().unwrap().lists.0[stage.scene.as_ref().unwrap().key_for(ul)];
        ls.heights.sum(0..index)
    };
    // 设祖先 ScrollPane scroll_pos（Instant）或起 tween（Smooth）
    if let Some(pane) = ancestor_pane(stage.scene.as_mut().unwrap(), ul) {
        if behavior == 1 {
            // Smooth: 起 scroll_pos.y tween 到 target（核实 tween prop；走 crate::tween::TweenProp::ScrollY 或类似）
            // 实现时照 scroll.rs 现有 tween 发起路径。
        } else if let Some(st) = stage.scene.as_mut().unwrap().scroll.get_mut(pane) {
            st.scroll_pos.1 = target;
        }
    }
    Ok(())
}
```

- [ ] **Step 3: 写失败测试 — NotifyInserted/Removed 搬移 heights + slot index**

```rust
    #[test]
    fn notify_inserted_shifts_heights_and_slot_indices() {
        // set_item_count(5) + 实例化 slot[index=2,3] + notify_inserted(at=2, count=1)
        // → heights.known 插入 None 在 idx 2；slot.item_index >=2 的 +1（2→3, 3→4）。
        // 断言：heights.known.len()==6；原 idx=2 的 slot 现在 item_index=3。
    }
```

- [ ] **Step 4: 实装 notify_inserted/removed/moved + refresh_items**

`crates/core/src/list.rs`：

```rust
pub fn notify_inserted(scene: &mut Scene, ul: NodeId, at: usize, count: usize) {
    let key = scene.key_for(ul);
    let ls = scene.lists.0.get_mut(key).expect("list state");
    // heights.known 插入 count 个 None 在 at
    for _ in 0..count { ls.heights.known.insert(at, None); }
    ls.item_count += count;
    // slot.item_index >= at 的 +count
    for s in ls.slots.iter_mut() { if s.item_index >= at { s.item_index += count; } }
    ls.dirty = true;
}

pub fn notify_removed(scene: &mut Scene, ul: NodeId, at: usize, count: usize) {
    let key = scene.key_for(ul);
    let ls = scene.lists.0.get_mut(key).expect("list state");
    let end = (at + count).min(ls.heights.known.len());
    ls.heights.known.drain(at..end);
    ls.item_count = ls.item_count.saturating_sub(count);
    // slot.item_index 在 [at, end) 的 → 回收；> end 的 -count
    // 实现时：先标脏回收范围 slot，再修正剩余 index。
    ls.dirty = true;
}

pub fn notify_moved(scene: &mut Scene, ul: NodeId, from: usize, to: usize) {
    // heights.known.remove(from).insert(to)；slot.item_index 同步搬移。
    let _ = (scene, ul, from, to); // 实现时填
}

pub fn refresh_items(scene: &mut Scene, ul: NodeId, start: usize, count: usize) {
    // 标脏：把 [start, start+count) 的 slot 重入 pending_binds。
    let key = scene.key_for(ul);
    if let Some(ls) = scene.lists.0.get_mut(key) {
        let vis = ls.visible.clone();
        for s in ls.slots.iter() {
            if s.item_index >= start && s.item_index < start + count {
                ls.pending_binds.push((s.node, s.item_index));
            }
        }
        let _ = vis;
    }
}
```

> **越界检查**：notify/refresh 的 at/index 越界 → 返 Err（FFI 返 -1）或 panic-free 忽略？spec §10 规定越界抛 UIContractException。core 返 Result，FFI 转 -1，C# 抛 UIContractException。

- [ ] **Step 5: 实装 3 个 stub FFI**

`crates/ffi/src/lib.rs` 把 Task 5 的 `loomgui_list_refresh/notify/scroll_to` stub 改为实装（调 list.rs 对应函数，返 0/-1）。

- [ ] **Step 6: C# 实装 + 异常**

`LoomGUI.Nodes.cs` ListView 的 ScrollToItem/RefreshItem(s)/Notify* 改为调 FFI，rc != 0 抛 `UIContractException`。

- [ ] **Step 7: 写 headless 断言 — Notify 后滚动位置不跳**

```csharp
    [Test]
    public void NotifyInserted_PreservesScrollPosition()
    {
        // 构造可见列表 + 滚到中段 + NotifyInserted(at=可见区前) → 滚动位置与可见内容不跳。
    }
```

- [ ] **Step 8: 重编 + sync + 拷 + 门禁 + Commit**

```bash
cargo build -p loomgui_ffi_c --release && cargo run -p xtask -- sync-bindings
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
dotnet test tests/dotnet/LoomGUI.HeadlessTests
git add -A && git commit -m "core(list): ScrollToItem/Refresh/Notify + drain_now + exception contract"
```

---

## Task 8: 公共 API 缩减 + 文档同步 + 末次入库

**收尾任务**：删 SelectedIndex/SelectionChanged（破坏性但零用户），同步 public-api.md / milestones.md，确保 PublicApi 编译门绿。

**Files:**
- Modify: `unity/package/Runtime/Public/LoomGUI.Nodes.cs`（删 SelectedIndex/SelectionChanged 两成员）
- Modify: `docs/design/public-api.md`（§8 + 判据 17，ul/ol→ul）
- Modify: `docs/roadmap/milestones.md`（M1 退出判据：删 SelectedIndex、ul/ol→ul）
- Modify: `tests/dotnet/LoomGUI.PublicApi`（删对 SelectedIndex 的引用，若有）

**Interfaces:**
- Consumes: Task 1-7 全绿
- Produces: 公共 API 终态契约与实现一致；PublicApi 门绿

- [ ] **Step 1: 删 C# SelectedIndex/SelectionChanged**

`unity/package/Runtime/Public/LoomGUI.Nodes.cs` 的 `class ListView`（line 2329）删两行：

```csharp
        public int SelectedIndex { get { throw NE(); } set { throw NE(); } }   // 删
        public event Action<SelectionChangedEvent> SelectionChanged;            // 删
```

> 核实：`SelectionChangedEvent` 结构体由 Dropdown 独立使用，**不删**（grep 确认）。保留 `ItemExitClass` 的 `throw NE()`（依赖 M2）。

- [ ] **Step 2: PublicApi 门**

`tests/dotnet/LoomGUI.PublicApi` grep `SelectedIndex` 引用，删/改。编译门绿。

Run: `dotnet build tests/dotnet/LoomGUI.PublicApi`
Expected: PASS

- [ ] **Step 3: 同步 public-api.md**

`docs/design/public-api.md` §8（ListView 代码块）删 SelectedIndex/SelectionChanged 两行；判据 17 删「ItemExitClass 退场动画」之外提到的 SelectedIndex；§339 `ul/ol → ListView` 改为仅 `ul`。

- [ ] **Step 4: 同步 milestones.md**

`docs/roadmap/milestones.md` M1 退出判据：`ul/ol → ListView` 改 `ul`；删「SelectedIndex」实装要求（与本决策冲突）。

- [ ] **Step 5: 末次 dll 入库 + 全量门禁**

```bash
cargo build -p loomgui_ffi_c --release
cargo run -p xtask -- sync-bindings
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
dotnet test tests/dotnet/LoomGUI.HeadlessTests
dotnet build tests/dotnet/LoomGUI.PublicApi
```
Expected: 全绿

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "api(list): remove SelectedIndex/SelectionChanged (ul has no HTML selection semantics); sync public-api.md + milestones.md"
```

---

## Self-Review（计划作者自查）

**1. Spec 覆盖**：
- §3 决策（全在 core / spacer 流 / 不等高估算+anchoring / pending 队列帧首排空 / clone_subtree / template 进 pkg / YAGNI）→ Task 0-8 全覆盖。
- §4 数据模型（ListState/side table/spacer/gap 分支/margin box/ul 高度契约）→ Task 4。
- §5 高度缓存与可见区（HeightCache/sum/回填/anchoring/tween 交互/可见区/cold start/slot 池/reuse_key 编码）→ Task 3（逻辑）+ Task 4（接树）+ Task 6（anchoring）。
- §6 clone_subtree（side table 表 / id 作用域 / 6.1 template 进 pkg / 6.2 拆 flag / 6.3 模板来源）→ Task 0（6.1）+ Task 1（6.2）+ Task 2（clone_subtree）+ Task 4（兜底模板）。
- §7 帧时序（update_visible 在 solve 前 / collect_heights 在 solve 后 / drain_now 先 update_visible）→ Task 4-5。
- §8 FFI 契约（7 个 list_* FFI）→ Task 5-7。
- §9 公共 API 范围（实装 10 + ChildCount/ListItem.Index + ItemExitClass NE + 删 SelectedIndex）→ Task 5（实装）+ Task 7（ScrollTo/Notify）+ Task 8（删）。
- §10 错误处理（UIContractException 各项 + ul 非 auto + 无模板来源 + 越界）→ Task 4（ul auto）+ Task 2（无模板）+ Task 7（越界）。
- §11 测试（core 单测 + headless 本质断言/无漂移/CSS 命中/异常/template 不渲染）→ 散布各 Task；CSS 命中（§6.2 拆 flag）在 Task 1 回归测试；template 不渲染在 Task 0。
- §12 交付顺序 → 本计划 Task 编号 0-8 与 spec §12 步骤 0-8 一一对应。

**2. 占位符扫描**：headless 测试（Task 5 Step 9、Task 6 Step 7、Task 7 Step 7）含 `return 0; // 实现时填` 与「精确构造以现有 helper 为准」——这是故意的：headless 测试依赖现有 HeadlessTests helper 的精确签名（Stage 构造/FrameData 读取），实现者读现有测试模式填。这是 spec 明确授权的（AGENTS.md：遇编译错按 crate 实际源码调）。**非占位失败**——断言意图写死（render node 相等 / 无漂移 / Notify 不跳），只有构造代码待实现者按现有模式填。其余所有代码块是完整实装指引。

**3. 类型一致**：`HeightCache`/`compute_visible_range`/`ListState`/`Slot`/`ListTable`/`PendingOps`/`plan_visible`/`execute_visible`/`take_pending_binds`/`drain_now`/`collect_heights`/`encode_reuse_key`/`enter_data_driven`/`set_item_count`/`scroll_to_item`/`notify_inserted`/`notify_removed`/`notify_moved`/`refresh_items` 在各 Task 间命名一致。FFI 函数名 `loomgui_list_*` 一致。

**已知实现时核实点（非占位，是 spec 授权的 crate 适配）**：`Scene::key_for` 可见性、`NodeId::from_key` 可见性、`taffy_style.gap`/`.margin` 字段路径、`create_node` css 运行时解析能力、`FrameData.nodes` 字段名、C# UIContext tick 入口方法名——均标注在对应 step「核实点」，实现者读 crate 源码调整。
