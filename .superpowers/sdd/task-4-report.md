## Task 4 Report: content_size 注入 + 3 个读/写方法

### Status: DONE

### Commit
`b1d2b12` feat(core): content_size 注入 + get_scroll_pos/get_layout_rect（v1.4-b T4）

### Changes (2 files)

| File | Change |
|------|--------|
| `loomgui_core/src/scroll.rs` | ScrollPaneState +content_size_overridden 字段；refresh_content_sizes 加 override 跳过逻辑（含 tween 补偿）；+build_scroll_stage 助手 + 3 新测试 |
| `loomgui_core/src/stage.rs` | import 补齐 Rect + OverflowMode；+set_content_size / get_scroll_pos / get_node_layout_rect 3 个方法 |

### Test Results

```
cargo test -p loomgui_core
487 passed (lib) + 15 fence + 3 snapshot + 2 v1e_dirty = 507 passed, 0 failed
```

3 新测试：`set_content_size_overrides_refresh`、`get_scroll_pos_reads_state`、`get_node_layout_rect_reads_solved`。无回归。

### 关键实现细节：refresh 跳过逻辑

原 refresh_content_sizes 循环结构：
```
for (nid, kids, viewport) in work {
    ① 遍历 kids AABB 算 content_size
    ② scene.scroll.ensure(nid) → 覆盖 content_size / viewport_size / overlap
    ③ content_size_dirty + tween 补偿
}
```

改动：**在 ① 之前插入 override 判断**，若 `content_size_overridden == true`：
- `ensure` 取 scroll slot
- **只更新** viewport_size = viewport（容器尺寸可能变化）
- **重算** overlap = (content_size - viewport).max(0)（用已注入的 content_size，不是从子节点算的）
- **不覆盖** content_size、不遍历 kids
- **补 tween 补偿**：viewport 变化导致 overlap 缩小 → scroll_pos 可能越界 → clamp + 取消 tween（和原 ③ 同构）
- `continue`（跳过 ①②③）

关键：用 `scene.scroll.get(nid)` 先 immutable borrow 读标记 → `.map().unwrap_or(false)` 立即消费成 `bool` → NLL 释放 borrow → 后续 `ensure` mutable borrow 无冲突。

`content_size_dirty` **不修改**——overridden 容器 content_size 未变，dirty flag 原始语义也不适用（它标记"子 layout_rect 变化导致 content_size 变化"）。

### brief 与实际差异

1. **`build_scroll_stage`** 不存在于 codebase，新建。用 `Stage::new` + `Scene::build` 手搓单 scroll 容器（无子节点），layout_rect (0,0,200,100)。

2. **`get_scroll_pos_reads_state` 需先造 overlap**：brief 原测试直接 `set_scroll_pos(root, 0, 50)` 后 `get`，但初始 overlap=(0,0) → `set_pos` 内部 clamp 到 0。修改为：先 `set_content_size` + `refresh` 造 overlap。

3. **override 分支 tween 补偿**：brief skip 骨架没提 tween 补偿。补齐：viewport 缩小导致 overlap 缩小 → scroll_pos 越界需 clamp + 取消 tween。

4. **import 补齐**：`Rect` 在 `crate::scene::node`，`OverflowMode` 在 `crate::style::resolved`。stage.rs 原来只引 `NodeId, Scene`，补齐两个符号。

### Concerns

- **viewport 归零后首次 refresh**：`set_content_size` 设 `viewport_size = (0,0)`，下次 refresh 才填实际值。在 set 与 refresh 之间若调 `set_scroll_pos`，`set_pos` 的 clamp 用 `self.overlap`（也是 0）→ scroll_pos 始终 0。driver 流程为 set_content_size → tick → 读状态，正常。
- **overridden 后只能通过重新 set_content_size 改**：无 `clear_content_size_override` 方法。若 driver 需从"虚拟列表"切回"普通滚动"，目前无 API。T5/后续可加，或 driver 重建容器。

## Fix: clear_content_size_override

### Commit
`e3921bd` fix(core): add clear_content_size_override API (v1.4-b T4 fix)

### Changes (2 files)

| File | Change |
|------|--------|
| `loomgui_core/src/stage.rs` | +`clear_content_size_override` 方法（node 无效/非 scroll → no-op）；`set_content_size` doc 加中间态说明（viewport/overlap=0,0 至下次 refresh） |
| `loomgui_core/src/scroll.rs` | +`clear_content_size_override_restores_auto` 测试（set 注入→clear 撤销→refresh→断言 overridden=false + content_size 不等于注入值） |

### 实现细节

`clear_content_size_override` 照 `set_content_size` 模式：`scene.scroll.get_mut(node)` 直接清 `content_size_overridden = false`。不 touch content_size/viewport/overlap——这些由下次 `refresh_content_sizes` 重算（子节点 AABB 自动算，overridden=false 走原路径）。

FFI 入口不 panic（坑 102）：node 无效/不在 scroll slot → `get_mut` 返 None → no-op。

### Test Results

```
cargo test -p loomgui_core
488 passed (lib) + 15 fence + 3 snapshot + 2 v1e_dirty = 508 passed, 0 failed
```

新增 1 测试：`clear_content_size_override_restores_auto`。无回归。
