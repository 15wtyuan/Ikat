# Task 5 报告 — 圆角 SDF 裁剪（CLIPPED_ROUNDED 变体）

## 概述

`overflow:hidden` + `border-radius` 组合时，clip 用圆角矩形 SDF 而非矩形 AABB。
跨 Rust core + FFI blob + C# + Unity shader 四层。Rust 侧 TDD（radii 透传 + blob 52B entry round-trip），
C#/shader 代码审查质量（PlayMode 在家里机验收）。

## 实现路径（8 文件 + 1 测试文件）

### Rust core（3 文件，TDD）

**1. `loomgui_core/src/render/mod.rs` — ClipEntry 加 `radii` 字段**
- `ClipEntry` 加 `pub radii: Option<[(f32, f32); 4]>`。
- `None` = 直角 AABB clip（CLIPPED 变体）；`Some` = 圆角 SDF clip（CLIPPED_ROUNDED 变体）。
- 四角半径对 `(h, v)`，序 [TL, TR, BR, BL]（与 `BorderRadius::as_corners` 同约定）。

**2. `loomgui_core/src/render/batch.rs` — assign_sort_keys 填 radii**
- clipper 节点（`clip_rect.is_some()`）的 `border_radius` 经 `as_corners(own.w, own.h)` 解析。
- 全零 → `None`（直角）；非全零 → `Some`（圆角）。
- 半径按 clipper 自身 box 尺寸解析（不受 scroll/ancestor 交集影响）。

**3. `loomgui_ffi_c/src/blob.rs` — clip 表 entry 20B→52B**
- entry = ctx(4) + rect(16) + radii(32 = 4×(rx,ry)×2×4) = 52B。
- `None` → 全零 32B（C# 据全零判 CLIPPED vs CLIPPED_ROUNDED）。
- `CLIP_ENTRY_SIZE = 52`，`clip_table_len = 4 + count × 52`。
- **不 bump blob version**（v10 保持）：entry 格式是 clip 表段内部布局，per-frame 重建，
  无持久化。C# 侧同步更新 52B stride，无向后兼容问题。

### C#（4 文件 + 1 测试文件，代码审查质量）

**4. `loomgui_unity_package/Runtime/FrameBlob.cs` — ClipRect 加 corner-radius out 参数**
- `ClipRect(ctx, out x, out y, out w, out h, out cornerRadius)`。
- entry stride 20→52。MVP 统一半径：取四角 (rx,ry) 的最小值（`min(minRx, minRy)`）。
- ponytail: 非均匀四角 SDF 留后续——当前取 min 保证四角都不超目标半径（保守，不溢出 clip 边界）。

**5. `loomgui_unity_package/Runtime/ClipMath.cs` — NormalizeCornerRadius**
- design 半径 → shader clipPos 归一化空间：`r_norm = r_design / min(hw, hh)`。
- 取 min half-size 让 SDF 圆角保持圆形（非椭圆），视觉最接近 CSS border-radius。
- 与 ComputeClipBox 同根 transform 重算 half_size（避免 _ClipBox 是 SafeBlank 时除零）。

**6. `loomgui_unity_package/Runtime/MaterialManager.cs` — SetCornerRadius + rounded key**
- `Get` 加 `bool rounded` 参数。`rounded=true` 启 `CLIPPED_ROUNDED`，否则 `CLIPPED`（互斥）。
- `rounded` 进 Key（同 ctx 不同 rounded 各持独立 Material）。
- `SetCornerRadius(ctx, normalizedRadius)` 设 `_CornerRadius` uniform（per-ctx）。
- 新增 `_cornerRadiusByCtx` dict + `Clear()` 同步清空。

**7. `loomgui_unity_package/Runtime/MirrorPool.cs` — UpdateHeader 读 radius 传 mm**
- `ClipRect` 读出 `cornerRadius > 0` → `ClipMath.NormalizeCornerRadius` → `mm.SetCornerRadius` + `rounded=true`。
- `_roundedCtxsThisFrame` HashSet 记录本帧启 CLIPPED_ROUNDED 的 ctx（同 ctx 后续节点复用标志保 material key 一致）。
- `mm.Get(program, tex, maskCtx, !pure, rounded)`。

**8. `loomgui_unity_package/Tests/MaterialManagerTests.cs` — 补 rounded 参数 + 2 新测试**
- 所有 `mm.Get` 调用补 `rounded: false` 参数（签名变更）。
- 新增 `RoundedFlag_SelectsCorrectKeyword` — 验 CLIPPED_ROUNDED vs CLIPPED 互斥。
- 新增 `SetCornerRadius_UpdatesCachedMaterialFloat` — 验 _CornerRadius uniform 刷新。

### Shader（1 文件）

**9. `loomgui_unity_package/Shaders/LoomGUI-Unlit.shader` — CLIPPED_ROUNDED 变体 + SDF**
- Properties 加 `_CornerRadius ("CornerRadius", Float) = 0`。
- CBUFFER 加 `float _CornerRadius;`。
- `#pragma multi_compile _ CLIPPED_ROUNDED`（与 CLIPPED 并列，互斥）。
- vert: `#if defined(CLIPPED) || defined(CLIPPED_ROUNDED)` 算 clipPos（两变体共用）。
- frag: `#ifdef CLIPPED_ROUNDED` 走 SDF，`#elif defined(CLIPPED)` 走原 AABB step。
- SDF: `q = abs(clipPos) - 1.0 + r; sdf = length(max(q,0)) + min(max(q.x,q.y),0) - r; col.a *= smoothstep(1.0, 0.0, sdf)`。
- r 在归一化空间（design_radius / min_half_size），由 ClipMath.NormalizeCornerRadius 算。

## CLIPPED vs CLIPPED_ROUNDED 分支逻辑

两者都是 clip 实现，**互斥不叠加**：
- `cornerRadius == 0` → enable `CLIPPED`（AABB `step(max(abs),1.0)`）
- `cornerRadius > 0` → enable `CLIPPED_ROUNDED`（SDF `smoothstep`）

C# `MaterialManager.Get` 的 `rounded` 参数决定启哪个 keyword。`rounded` 进 Material key，
同 ctx 的直角/圆角各持独立 Material 实例。这避免了在同一 material 上 toggle keyword
（会触发 shader variant 重编译，卡帧）。

## SDF 归一化方案

shader SDF 在 `clipPos` 归一化空间计算（区域内 |x|,|y|<=1）。
`clipPos = worldPos * _ClipBox.zw + _ClipBox.xy`，即 `(worldPos - center) / half_size`。
design 半径 `r_design`（px）须除以 `half_size` 转归一化：`r_norm = r_design / min(hw, hh)`。
取 min half-size 让 SDF 圆角在非方形 rect 下仍保持圆形（CSS border-radius 语义）。

## MVP 统一半径决策 + ponytail

- **MVP**: 取四角 (rx,ry) 的最小值作统一 `r`。非均匀四角 SDF 难（CSG box SDF 需拆各角/边 SDF）。
- **ponytail: 非均匀四角精确 SDF 留后续**。当前取 min 保证四角都不超目标半径——
  小半径角精确，大半径角偏小（保守，不溢出 clip 边界）。CSS `border-radius: 10px 20px 30px 40px`
  的四角不等场景视觉上会有偏差，但 `overflow:hidden + border-radius: 20px`（最常见用例）完全正确。

## TDD RED/GREEN（Rust 侧）

### RED → GREEN

**新增测试（batch.rs）**：
- `clip_node_zero_radius_yields_none_radii` — 全零 border_radius → radii=None
- `clip_node_nonzero_radius_carries_radii` — 非零 border_radius → radii=Some，四角值保真（TL/TR/BR/BL 序）

**新增测试（blob/tests.rs）**：
- `clip_table_radii_round_trip` — radii=Some([(10,12),(20,22),(30,32),(40,42)]) 序列化 52B + 读回保真

**更新测试（blob/tests.rs）**：
- `clip_table_round_trip_with_entries` — entry 20B→52B，clip_table_len 44→108，加 radii=None 断言
- `read_clips` TestView 方法 — 返回 3-tuple `(cid, Rect, Option<radii>)`

### 全部 Rust 测试结果
```
cargo test -p loomgui_core -p loomgui_ffi_c
→ 64 passed; 0 failed（含新增 3 测试）
cargo fmt --all -- --check → clean
cargo clippy -p loomgui_core --all-targets -- -D warnings → clean
cargo clippy -p loomgui_ffi_c --all-targets -- -D warnings → clean
```

## PlayMode 验收清单（家里机）

**前提**：本机已 build .dll + commit；家里机 pull 后 Unity 编译。

### 必验项

1. **圆角裁剪基本功能**：`overflow:hidden` + `border-radius:20px` 的容器，子节点超出圆角区域被裁成圆角（非矩形直角）。子节点内容在圆角外消失，圆角内可见。

2. **直角裁剪零回归**：`overflow:hidden`（无 border-radius）的容器，子节点超出矩形边界被裁成直角（与 v1.7 行为一致）。验 CLIPPED 变体仍正常工作。

3. **无裁剪零回归**：无 `overflow:hidden` 的节点（含带 border-radius 的）不受影响——CLIPPED/CLIPPED_ROUNDED 均不启用。

4. **滚动容器 + 圆角**：`overflow:scroll` + `border-radius` 的容器，滚动时内容被圆角裁剪（scroll 不破坏圆角 clip）。

5. **嵌套 clip**：外层直角 clip + 内层圆角 clip，两层各自正确裁剪（外层 AABB，内层 SDF）。

6. **border-radius 较大**：`border-radius:50%`（圆形）的 `overflow:hidden` 容器，子节点被裁成圆形。

7. **性能无退化**：圆角 clip 场景帧率正常（SDF 在 frag 每像素算，但 clip 区域通常小；多圆角 clip 同屏不卡）。

### 已知限制（非 bug）

- 非均匀四角半径（如 `border-radius: 10px 20px 30px 40px`）取 min 统一——大半径角偏小。
- 嵌套 clip 的交集 rect 若比 clipper 自身 box 小，半径按 design px 传但在更小交集上归一化——视觉上圆角可能偏大（MVP 可接受）。

## 自查

- **blob version 不 bump**：已核实。entry 格式是 clip 表段内部布局，per-frame 重建，C# 同步更新 stride。无持久化 blob，无向后兼容问题。
- **FFI no panic**：blob 读写在 C# 侧越界返 false/0（不崩）；Rust 侧 `build_blob` 无 unwrap（Vec 写入，不会 panic）。
- **注释质量**：无内部坑号；WHY 不 HOW；ponytail 标记非均匀 SDF deferral。
- **CI 门禁**：fmt + clippy clean（core + ffi_c）。
- **.dll 已重编 + 拷贝**：`md5sum` 一致。

## 顾虑

- **PlayMode 未验**：shader SDF + C# 归一化逻辑只能在家用机 PlayMode 验。代码审查对照现有 CLIPPED 模式，无创新路径。
- **非均匀半径 MVP**：`border-radius: 10px 20px` 取 min 会让 20px 角只裁 10px。若验收发现不可接受，需补非均匀 SDF（较重，拆各角 SDF）。
- **Material key 膨胀**：`rounded` 进 key 后，同 ctx 的直角/圆角各持独立 Material。正常场景一个 ctx 只有一种（直角或圆角），不膨胀。仅理论场景（同 ctx 节点一会直角一会圆角）会多建一个 Material——可忽略。
