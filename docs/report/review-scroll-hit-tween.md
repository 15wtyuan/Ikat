# Scroll / Hit / Tween 模块深度代码审查

> 审查文件：`scroll.rs` (684L) / `tests.rs` (1070L)、`hit.rs` (403L)、`tween.rs` (619L)
> 日期：2026-07-09

---

## 1. Per-Axis 代码重复（scroll.rs）

### 1.1 严重：所有物理方法大量重复 if/else 分轴

**精确位置**：

| 方法 | 行号 | 重复模式 |
|------|------|----------|
| `drag_follow` | 135–170 | `if ax == 0 { .0 } else { .1 }` 每字段 2 次 |
| `begin_inertia` | 192–255 | 同上，每轴 4 个字段 × 2 个写点 |
| `begin_bounce` | 261–296 | 同上 |
| `advance` | 308–401 | 同上，且两处（推进行 336–361 + bounce 行 381–392） |
| `apply_wheel` | 406–435 | 同上 |
| `set_pos` (animated) | 448–471 | 同上 |

**当前结构**：
```rust
for ax in 0..2u8 {
    let cur = if ax == 0 { self.scroll_pos.0 } else { self.scroll_pos.1 };
    let ov  = if ax == 0 { self.overlap.0 }     else { self.overlap.1 };
    // ... 计算 ...
    if ax == 0 { self.scroll_pos.0 = np; } else { self.scroll_pos.1 = np; }
}
```

每个方法中这类分支出现 8–15 次，总重复行数约 **250 行**，占 scroll.rs 主逻辑 40%+。

**问题分析**：`ScrollPaneState` 的各字段成对出现 (`scroll_pos: (f32,f32)`, `overlap: (f32,f32)`, …)，但使用元组而非 `[f32; 2]`，导致无法用索引访问，必须按轴分支。

另外 `tweening: u8` 是整个轴共享的状态，但在代码中并不需要分轴。begin_bounce (line 297) 在 for 循环外设 `self.tweening = 2`，而 begin_inertia (line 256) 和 advance (line 399) 的条件判断也在这里——一旦某轴触发了 bounce，会误标整个 tweening=2，可能影响同帧另一轴仍在进行的惯性曲线。

**修复方向**：将 `ScrollPaneState` 的 per-axis 字段改为 `[f32; 2]`，引入 `AXIS_X = 0, AXIS_Y = 1` 常量。循环体内直接索引访问。这可以把每个方法从 ~50 行缩到 ~15 行，消除分支噪音让物理逻辑一目了然。

**严重级别**：⚠️ 中（不影响正确性，但显著降低维护性，且增加了 per-axis 状态不一致的风险）

---

## 2. 滚动物理

### 2.1 惯性衰减常量合理性

```rust
// scroll.rs:36
pub const DECELERATION_RATE: f64 = 0.967;
```

每帧 (1/60 s) 速度衰减为 96.7%。1 秒后 `0.967^60 ≈ 0.133`（86.7% 衰减）。手感测试通过，但对低帧率场景（30 fps，`0.967^30 ≈ 0.365`）衰减速度减半——帧率敏感。当前无 delta 补偿，可能在某些设备上惯性时长翻倍。

**严重级别**：🟡 低（当前 60 fps 锁定，但若支持变帧率需补偿）

### 2.2 `begin_inertia`：边界检查顺序正确但缺注释

```rust
// scroll.rs:209-227
let over_lo = start < 0.0;
let over_hi = ov > 0.0 && start > ov;
if over_lo || over_hi {
    // bounce tween ...
    continue;  // ← 跳过本轴后续 inertia 逻辑
}
```

越界松手 → 直接 bounce，不 inertia。逻辑正确。但 `over_hi` 的条件 `ov > 0.0 && start > ov` 隐含：当 `ov <= 0`（content ≤ viewport）时不触发 bounce——因为越界方向无法判断（`start > ov = 0 = start > 0`，但 `ov = 0` 被短路）。这是对的——overflow ≤ 0 时无需回弹。

**但有一种情况未覆盖**：`ov > 0 && start < 0` 且 velocity 很大朝 `ov` 方向（向下）。此时 start 在 0 上方越界，应 bounce 到 0。当前代码 `over_lo = start < 0.0` 已覆盖。没问题。

**严重级别**：✅ 无问题

### 2.3 `advance` 运行时越界截断的阈值选择

```rust
// scroll.rs:371-379
let bounce = if (pos < -BOUNCE_THRESHOLD && cc < 0.0) || (pos < 0.0 && cc == 0.0) {
    Some((0.0_f32, 0.0 - pos))
} else if ov > 0.0
    && ((pos > ov + BOUNCE_THRESHOLD && cc > 0.0) || (pos > ov && cc == 0.0))
{
    Some((ov, ov - pos))
} else { None };
```

- `BOUNCE_THRESHOLD = 20.0`：inertia 过冲 > 20 px 才截断，小过冲 (< 20 px) 不做弹性回弹——依赖 tween 自然完成后的 `clamp`（line 397–398）。这是合理的：小过冲不必额外回弹。
- `cc == 0.0` 分支（已完成但仍在越界）：这是兜底保护——正常情况 tween 完成时应在边界，但若因精度或外部修改导致越界，仍回弹。

**但存在隐患**：`cc == 0.0 && pos > ov` 触发回弹时，`new_change = ov - pos` 是负值（向边界走）。回弹 tween 以 cubic_out 执行，看起来是平滑的。正确。

**严重级别**：✅ 无问题

### 2.4 `refresh_content_sizes`：overridden 容器越界检查遗漏非 tween 情况

```rust
// scroll.rs:571-583
if st.tweening != 0 {
    let out_of_range = st.scroll_pos.0 < 0.0
        || st.scroll_pos.0 > new_overlap.0
        || st.scroll_pos.1 < 0.0
        || st.scroll_pos.1 > new_overlap.1;
    if out_of_range {
        st.scroll_pos.0 = st.scroll_pos.0.clamp(0.0, new_overlap.0);
        st.scroll_pos.1 = st.scroll_pos.1.clamp(0.0, new_overlap.1);
        st.tweening = 0;
    }
}
```

当 `tweening == 0`（无动画，可能正在拖动中或已静止）且 viewport 缩小导致 overlap 缩小时，scroll_pos 可能越界但不会被 clamp。这会导致 scroll_pos 超出新的 `[0, overlap]` 范围——虽然后续 `set_pos` 或 `apply_wheel` 调用会 clamp，但在窗口期（上屏渲染、scrollbar thumb 位置）可能短暂错误。

**修复方向**：去掉 `tweening != 0` 前置条件，任何越界都应 clamp。`tweening = 0` 时取消 tween 是 no-op（已是 0），不影响。

**严重级别**：🟡 低（短暂越界，下一帧操作会修正）

### 2.5 `drag_follow`：速度平滑系数与 dt 耦合

```rust
// scroll.rs:130-133
let smoothing = (dt * VELOCITY_SMOOTH).clamp(0.0, 1.0);
self.velocity.0 += (delta.0 / dt - self.velocity.0) * smoothing;
```

`VELOCITY_SMOOTH = 10.0` 在 60 fps (`dt = 0.016`) 下 `smoothing = 0.16`，约 6 帧稳定。变帧率时行为漂移。这与惯性衰减（2.1）同类问题。

**严重级别**：🟡 低

---

## 3. `hit.rs` 命中测试

### 3.1 `effective_draw_order`：每帧每命中分配排序

```rust
// hit.rs:16-21
fn effective_draw_order(scene: &Scene, parent: NodeId) -> Vec<NodeId> {
    let mut kids: Vec<NodeId> = scene.get(parent).expect("live node").children.clone();
    kids.reverse();
    kids.sort_by_key(|&c| -scene.get(c).expect("live node").style.order);
    kids
}
```

每次 `hit_subtree` 递归到一个有孩子的节点，就会 `clone` 一整份 children Vec + 排序。对深树（深度 D、每层 N 个子），每次 hit_test 产生 D 次分配 + O(N log N) 排序。

"order" 在同一次 tick 内是稳定的（rematch 后不变）。但 `rematch_pseudo_classes` → `solve` → `compute_world_transforms` → `build` 之后，hit_test 多次调用时，draw_order 不变。

**是否应该缓存**：当前设计是命中发生在布局完成后的稳定帧内，典型 UI 树深度 ≤ 10、每节点子数 ≤ 20，分配成本可忽略。若后续出现虚拟列表大子树或高频 hit（如 drag 每像素），可考虑在 Scene 添加 `cached_draw_order: HashMap<NodeId, Vec<NodeId>>`，在 `tick` 末尾失效。当前不需要。

**严重级别**：✅ 无需修改（当前规模）

### 3.2 嵌套 clip 门控的正确性

```rust
// hit.rs:67-71
if let Some(clip) = node.clip_rect {
    if !point_in_rect(point, clip) {
        return None;
    }
}
```

**clip_rect 坐标空间分析**：`clip_rect` 由 layout `write_back`（`layout/mod.rs:359-361`）填入，使用的是累积 parent_origin 的绝对坐标。`hit_subtree` 入口时 `point` 是设计坐标（= 世界坐标），不做变换。所以 clip 检查是**世界空间点 vs 世界空间 rect**——正确满足嵌套 clip 的交集语义：

- 父级 clip 拒绝 → 子 clip 不查，整个子树 None
- 父级 clip 通过 → 子级 clip 独立查自己 rect → 交集 = 父 ∩ 子

**但存在一个假设**：`clip_rect` 在渲染/命中阶段始终与世界空间对齐。当前 layout 阶段设置的 clip_rect 是绝对坐标；渲染 batch 阶段（`batch.rs:154-161`）用 `clip_rect` 做 mask_context 交集。如果渲染阶段调整了 clip_rect（如 scroll 偏移补偿），命中阶段取到的可能是过期的 rect。

**实际情况**：当前架构里 clip_rect 在 layout solve 后不再修改（scroll 偏移在 render node 层处理，不修改 node 的 clip_rect）。因此命中阶段使用的 clip_rect 是 layout 时的值，与渲染一致。✅ 正确。

**严重级别**：✅ 无问题

### 3.3 命中测试：world_transforms 未对齐时的兜底

```rust
// hit.rs:85-88
let wm = match scene.world_transforms.get(id.index()) {
    Some(wm) => *wm,
    None => return None,
};
```

注释已写明"1 帧延迟语义"：新增节点本帧 world_transforms 未算时返回 None。这在 tick 时序内是一致的（hit_test 在 compute_world_transforms 之后调用）。✅ 设计合理。

**严重级别**：✅ 无问题

### 3.4 `hit_scrollbar_grip` 遍历所有节点

```rust
// hit.rs:25-40
pub fn hit_scrollbar_grip(scene: &Scene, point: (f32, f32)) -> Option<(NodeId, u8)> {
    for (_key, n) in &scene.nodes {
        // check v_thumb_rect / h_thumb_rect
    }
}
```

O(N) 遍历所有节点找 thumb 命中。对于非 scroll 容器的节点，`v_thumb_rect` / `h_thumb_rect` 返回 None（因为 `scene.scroll.get(id)` 无 state），所以只对有效 scroll 容器做实际 rect 检查。每次 hit_test 都做一次——对多容器场景（如虚拟列表 + sidebar），这是若干次空查。

**优化方向**：维护一个 `active_scroll_containers: Vec<NodeId>` 列表，在 `refresh_content_sizes` 时更新。这样 hit_test 只迭代有实际滚动状态的容器。

**严重级别**：🟡 低（当前 scroll 容器数小，可 defer）

---

## 4. `tween.rs` 动画

### 4.1 `TweenProp` 变体：6 个，添加新属性需改 4 处

| 位置 | 行号 | 需改动 |
|------|------|--------|
| 枚举定义 | 10–17 | 加变体 |
| `try_from` | 22–31 | 加 match arm |
| `prop_value_size` | 36–41 | 返回分量数 |
| `apply` | 296–303 | lerp 后写 anim 通道 |

另外 FFI/C# enum 需同步对齐，C# bindings 需更新。

6 个变体时这种分散还可接受，但若扩展到 15+ 个属性，apply 函数的 match 会很长。当前设计中 `transform` 通道被 Translate/Scale/Rotation 三个属性共享（都写 `a.transform`），这意味着同一节点不能同时做 translate + scale tween——后写的会覆盖先写的。这是 GTween 的传统 behavior（单 transform 通道），不是 bug。

**严重级别**：🟡 低（当前规模可接受）

### 4.2 `TweenManager::update` 线性扫描

```rust
// tween.rs:247
for t in &mut self.tweens {
    if t.killed { continue; }
    // ... update logic
}
self.tweens.retain(|t| !t.killed);
```

O(N) 扫描所有 tween，典型场景（几十个 tween/帧）完全够用。无需优化。

**严重级别**：✅ 无需修改

### 4.3 killed tween 清理：无泄漏

- `kill` / `kill_node` 只标 `killed = true`（line 235, 200）
- `update` 末尾 `retain(|t| !t.killed)` 移除（line 280）
- `clear` 直接清空整个 Vec（line 193）
- 悬空 NodeId 兜底：update 中 `scene.get(t.node).is_none()` 标 killed（line 254–257）

**但有微妙情况**：`kill` + 同帧不调 `update` + 调 `clear` → 无泄漏（clear 全清）。`kill` + 同帧不调 `update` + 不调任何东西 → killed 标记在 Vec 里，不参与迭代也不泄漏。✅ 无泄漏。

**严重级别**：✅ 无问题

### 4.4 `apply` 中 transform 通道的覆盖写

```rust
// tween.rs:298-301
TweenProp::Translate => a.transform = Some(transform::from_translate(lerp(0), lerp(1))),
TweenProp::Scale     => a.transform = Some(transform::from_scale(lerp(0), lerp(1))),
TweenProp::Rotation  => a.transform = Some(transform::from_rotate(lerp(0))),
```

三个属性共享 `NodeAnim::transform` 字段，后 update 的 tween 覆盖先前的。这是 GTween 兼容 design（不支持单节点同时两个 transform 动画），但需注意：若对同一个 node 提交了 `Translate` tween 后又提交 `Scale` tween，`Translate` 会被覆盖（不是组合）。由 Controller 的 transition 管理系统保证不会同时提交冲突 tween。

**严重级别**：🟢 信息（已有设计约束保证）

---

## 5. 注释质量

### 5.1 优质的注释

- `scroll.rs:180-184`：`begin_inertia` 三步分支的文档说明了越界→bounce / 界内→inertia / 界内低速→停的优先级
- `scroll.rs:229`：明确标注 `v2 = |v|*scale = 线性 |v|（非 v²）`，避免误解（fgui 命名残留，坑 54）
- `scroll.rs:362-364`：`advance` 中运行时越界截断的注释解释了"弹性过冲回弹"
- `hit.rs:1-3`：顶部 module doc 概括了整体策略
- `tween.rs:1-3`：module doc 说明了 replace-override 语义

### 5.2 可改进的注释

- `scroll.rs:240`：`let dur = ((60.0f64 / v2_eff as f64).log(DECELERATION_RATE).abs() / 60.0) as f32;` 公式无推导向量，仅注释"经验公式"
- `scroll.rs:242`：`let change = v_eff * dur * INERTIA_DIST_COEFF;` 中 `0.4` 的选择无依据
- `scroll.rs:160-163`：PULL_RATIO 打折逻辑 `min(位移*PULL_RATIO, vp*PULL_RATIO)` 为什么有两层 min 没解释——实际是 `min(越界量*0.5, vp*0.5)`，cap 在 `vp*0.5` 防止大越界穿透太多

**严重级别**：🟡 低

---

## 总结

| 类别 | 发现 | 级别 |
|------|------|------|
| 代码重复 | per-axis if/else 覆盖 6 个方法、~250 行 | ⚠️ 中 |
| 正确性 | `refresh_content_sizes` overridden 容器 clamp 遗漏 tweening==0 情况 | 🟡 低 |
| 性能 | `effective_draw_order` 每帧分配（当前规模 OK） | ✅ |
| 性能 | `hit_scrollbar_grip` O(N) 遍历（可 defer） | 🟡 低 |
| 脆性 | 帧率相关常量（DECELERATION_RATE、VELOCITY_SMOOTH）无 dt 补偿 | 🟡 低 |
| 注释 | 经验公式缺推导 | 🟡 低 |
| 正确性 | tween killed 清理无泄漏 | ✅ |
| 正确性 | 嵌套 clip_rect 门控正确 | ✅ |
| 正确性 | 滚动物理边界条件正确 | ✅ |

**优先级最高的修复**：per-axis 重复代码重构。这不仅能删 ~200 行，更直接预防因分轴 if/else 不一致导致的潜在 bug（如某轴分支改了但另一轴忘记改）。
