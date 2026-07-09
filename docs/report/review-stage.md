# Stage 代码审查报告

审查文件：`loomgui_core/src/stage.rs`（862 行）+ 6 个测试文件
审查日期：2026-07-09
严重级别：高🔴 / 中🟡 / 低🟢

---

## 1. tick_and_render 是上帝方法 🔴

**位置**：L716–813

**代码片段**：
```rust
pub fn tick_and_render(&mut self) -> FrameData {
    // ... 10+ 个步骤，~100 行
}
```

**问题**：`tick_and_render` 承担了 10 个独立步骤：tween 推进、focus 消费、pointer process、scroll update、键盘处理、rematch、transition drain、solve、refresh_content、compute_world_transforms、build、rich_fragments 写回。每步各自访问 `scene`、`self` 的不同字段，内部变量 `out`、`dt`、`input`、`wheels`、`keys`、`reqs` 混在一起。

**后果**：
- 无法单独测试中间步骤（如 rematch → transition drain 的交互只能通过端到端测试覆盖）。
- 本地变量 `out` 在 L727 创建，L754 赋值到 `self.last_events`——中间 8 个步骤共享同一 `&mut out`，任何一步引入早返都可能丢事件。
- 新增管线步骤只能在方法体内追加，测试成本线性增长。

**修复方向**：将步骤分组提取为 `Stage` 私有方法：
```rust
fn process_input_phase(&mut self, scene: &mut Scene) -> Vec<EventRecord> { ... }
fn apply_style_phase(&mut self, scene: &mut Scene, dt: f32) { ... }  // rematch + transition
fn layout_phase(&mut self, scene: &mut Scene) { ... }  // solve + refresh + compute
fn writeback_phase(&mut self, scene: &mut Scene, rich_fragments: ...) { ... }
```
每个方法有独立测试。`tick_and_render` 退化为编排器。

---

## 2. Scene struct 字面量三处重复构造 🔴

**位置**：L493–508（`ensure_scene`）、L669–683（`new_for_test`）、L493 以下隐式也有…（实际还有 `load_inline_for_test` 不重复构造但调 `build_scene`）

**代码片段**（L493–508）：
```rust
pub(crate) fn ensure_scene(&mut self) {
    if self.scene.is_none() {
        self.scene = Some(crate::scene::node::Scene {
            roots: vec![],
            nodes: slotmap::SlotMap::with_key(),
            dynamic_rules: Default::default(),
            focused_node: None,
            world_transforms: Vec::new(),
            anim: Default::default(),
            scroll: Default::default(),
            text_layouts: Vec::new(),
            rich_fragments: Vec::new(),
            node_sort_keys: Vec::new(),
            controllers: Default::default(),
            pending_controller_events: Vec::new(),
            pending_transitions: Vec::new(),
        });
        self.prev_node_hashes.clear();
    }
}
```

`new_for_test`（L669–683）有一模一样的 13 字段 struct literal。

**后果**：Scene 加新字段时，必须同步改 `ensure_scene` + `new_for_test`，否则部分路径字段取默认值（`Vec::new()` → 空表），导致"某个路径正常、某个路径空表"的隐性 bug。

**修复方向**：给 `Scene` 实现 `Default`（其所有字段类型都实现了 Default），然后用 `Scene::default()` 替代两处 struct literal。或者至少抽一个 `Scene::new_empty()` 关联函数。

---

## 3. render_json 中的 unwrap 在 FFI 可调用路径上 🔴

**位置**：L816–819

```rust
pub fn render_json(&mut self) -> String {
    let frame = self.tick_and_render();
    serde_json::to_string_pretty(&frame.nodes).unwrap()
}
```

**问题**：`render_json` 是 `pub fn`，意味着 FFI 层可以调用。CLAUDE.md 明确要求"FFI 入口绝不 panic"（坑 102）。`serde_json::to_string_pretty` 只在 OOM 时失败，概率极低，但 `unwrap()` 违反 no-panic 契约。

**修复方向**：返回 `Result<String, String>`，或改为 `unwrap_or_else(|_| "{}".to_string())` 兜底空 JSON。

---

## 4. 缺少 package 卸载 API 🟡

**位置**：L26–27（`packages` 字段定义）、L110–129（`load_package`）

**问题**：`load_package` 只能增/替换包，没有 `unload_package(name) -> Option<Package>` 或 `clear_packages()`。多包共存时如果业务需要切换场景（例如从"背包"界面切到"邮件"界面，卸载背包包释放内存），目前无法做到。

**后果**：
- 内存只增不减（虽然包数据通常是静态的，但长时间运行/多场景切换会累积）。
- `image_sizes` 条目也一样：load 插、替换同名包清旧、但不同名包的旧条目无法主动清。

**修复方向**：加 `pub fn unload_package(&mut self, name: &str)`，同时清除 `packages[name]` + 遍历其 `asset_manifest` 从 `image_sizes` 移除对应的 path 条目。

---

## 5. create_root / create_node 中的 unwrap() 靠前置保证而非防御 🟡

**位置**：L517、L526

```rust
pub fn create_root(&mut self, kind: &str, css: &str) -> Result<NodeId, String> {
    self.ensure_scene();
    let scene = self.scene.as_mut().unwrap();  // L517
    crate::scene::dynamic::create_root(scene, kind, css)
}
```

**问题**：`ensure_scene()` 保证 `self.scene` 是 `Some`，所以 `unwrap()` 实际不会触发。但这是隐式契约——如果将来有人修改 `ensure_scene` 为可失败（比如加个 `Result` 返回），编译器不会提示这里要改。

**修复方向**：
```rust
let scene = self.scene.as_mut().ok_or("scene not initialized")?;
```
用 `?` 把调用栈上的错误返回给 FFI 层，而不是 panic。同理 `create_node` L526、`ensure_scene` 内的 `SlotMap::with_key()` 等。

---

## 6. rich_fragments 写回逻辑冗余 🟡

**位置**：L797–812

```rust
scene.rich_fragments
    .resize_with(scene.nodes.capacity() + 1, || None);   // L801
scene.rich_fragments.fill(None);                          // L806
for (node_id_u32, frags) in &rich_fragments { ... }
```

**问题**：`resize_with` 把容量不足的 slot 填 `None`，然后 `fill(None)` 又把所有 slot（含刚填的）再写一遍 `None`。等效于 `vec.resize(capacity + 1, None)` 一步完成。

**后果**：性能影响可忽略（每帧一次 O(n) 写，n 通常 < 1000），但逻辑冗余增加阅读负担。

**修复方向**：
```rust
scene.rich_fragments.resize(scene.nodes.capacity() + 1, None);
```
一句替代两句。`Option<Vec<RichFragment>>` 实现了 `Clone`。

---

## 7. instantiate 依赖打包器保证 parent_idx 有序，否则 panic 🟡

**位置**：L609、L627–628

```rust
// id_map[模板 idx] = live NodeId（slotmap 分配）。
let mut id_map: Vec<Option<NodeId>> = vec![None; template.nodes.len()];
// ...
if let Some(pidx) = tn.parent_idx {
    let parent = id_map[pidx].expect("parent built before child (parent_idx < i)");
```

**问题**：注释说"parent built before child (parent_idx < i) 由打包器/读保证"——如果 pkg.bin 被篡改或打包器有 bug（parent_idx >= i），这里 `expect` 直接 panic。这是 FFI 可调用路径（`instantiate` 是 `pub fn`）。

**修复方向**：改为 `Result` 错误返回：
```rust
let parent = id_map[pidx].ok_or_else(|| format!(
    "parent_idx {pidx} >= child index {i} in component {component}"
))?;
```

---

## 8. Stage 字段过多，缺少分组 🟡

**位置**：L21–57（Stage struct 定义）

**问题**：Stage 有 25 个 `pub` 字段。其中输入缓冲相关字段（`pending_input`、`pending_keys`、`pending_wheel`、`pending_focus_request`、`pending_dt`）可以组成一个 `InputBuffer` 子结构体；输出相关字段（`last_events`、`pointer_state`、`prev_node_hashes`）可以组成 `FrameState`。

**后果**：`tick_and_render` 内部大量 `std::mem::take(&mut self.pending_*)` 操作，分散在各处。分组后可以一次性 take 整个 buffer，减少对 self 的零散借用。

**修复方向**：不急于重构，但可在下次新增字段时考虑。

---

## 9. 测试中直接访问 TweenManager 内部字段 🟢

**位置**：`tests.rs` L685–692、L761–767 等多处

```rust
s.tweens.tweens.iter().filter(|t| !t.killed && t.node == btn_id ...)
```

**问题**：测试直接访问 `TweenManager.tweens: Vec<Tween>` 内部字段。如果将来 TweenManager 内部数据结构改变（比如从 Vec 改为 HashMap），这些测试编译失败。

**修复方向**：给 `TweenManager` 加测试辅助方法（如 `active_tween_count_for(node, prop) -> usize`），或直接用 `#[cfg(test)]` 限定。

---

## 10. tick_and_render 文档注释与 CLAUDE.md 时序描述不完全一致 🟢

**CLAUDE.md**：
> tick 时序 = 显式依赖拓扑：process(hit 用上帧 world) → rematch_pseudo_classes → solve → refresh_content → compute_world_transforms → build

**stage.rs L702–715**：
> ①tween ②focus_request ③process ④scroll update ⑤process_keys ⑥rematch ⑥.5 transition ⑦solve ⑧refresh ⑨compute ⑩build

**差异**：CLAUDE.md 省略了 tween/focus/scroll/keys/transition 五个步骤。不构成 bug（CLAUDE.md 是概述），但新开发者对着 CLAUDE.md 理解管线的核心不变式（rematch 在 solve/compute 前）时，可能忽略 transition drain 也在 rematch 之后、solve 之前——这个次序同样关键（坑 132 相关）。

**修复方向**：在 CLAUDE.md 的时序描述后加一句"完整步骤见 `stage.rs tick_and_render` 文档注释"，避免两处漂移。

---

## 11. package 池和 scene 耦合：instantiate 要求 scene 先存在 🟢

**位置**：L597–598

```rust
pub fn instantiate(&mut self, pkg: &str, component: &str) -> Result<NodeId, String> {
    let scene = self.scene.as_mut().ok_or("no scene (create_root first)")?;
```

**设计合理性**：这是有意的设计决策——`load_package` 进池不建 scene，`create_root` 建 scene，`instantiate` 从池克隆进 scene。但这意味着不能先 instantiate 再挂到 root——必须先有 scene（即至少有一个 root 存在）。文档已说明，api 会报错，合理。

---

## 12. tick_and_render 中 out 变量生命周期过长 🟢

**位置**：L727–754

```rust
let mut out: Vec<EventRecord> = Vec::new();
// ... 多步 append
self.last_events = out;
```

**问题**：`out` 从 tween update（L731）一直活到 process_keys（L753），横跨 6 个步骤。如果在中间步骤（如 scroll update L747–750）引入 `return` 早返，`out` 里的事件会丢失（不会写入 `self.last_events`）。

**当前安全性**：无早返路径，方法末尾才写 `self.last_events`。但新步骤加入时容易犯错。

**修复方向**：提取 `process_input_phase(&mut self, scene) -> Vec<EventRecord>`，让 `out` 在独立方法内构建，方法返回后立即赋值到 `self.last_events`。

---

## 总结

| # | 问题 | 严重级别 |
|---|------|---------|
| 1 | `tick_and_render` 上帝方法 | 🔴 高 |
| 2 | Scene struct literal 三处重复 | 🔴 高 |
| 3 | `render_json` 中 `unwrap()` | 🔴 高 |
| 4 | 缺少 package 卸载 API | 🟡 中 |
| 5 | `create_root/create_node` 隐式 unwrap | 🟡 中 |
| 6 | `rich_fragments` 写回冗余 | 🟡 中 |
| 7 | `instantiate` parent_idx 越界 panic | 🟡 中 |
| 8 | Stage 字段过多 | 🟡 中 |
| 9 | 测试直访 TweenManager 内部 | 🟢 低 |
| 10 | 文档注释与 CLAUDE.md 时序差异 | 🟢 低 |
| 11 | package/scene 耦合 | 🟢 低 |
| 12 | `out` 变量生命周期过长 | 🟢 低 |

**优先级建议**：#1（拆上帝方法）和 #3（去 unwrap）应最优先处理。#2（Scene::default）改动最小收益最大。#7（parent_idx 校验）一行修复即可消除 panic 风险。
