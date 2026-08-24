# border 单边 longhand + border_width 减法重构 设计

> **定位**：补围栏外静默丢弃的 `border-top/right/bottom/left` 单边属性，并顺手清掉阻碍它的历史遗留字段 `ResolvedStyle.border_width`。
> **起因**：showcase `page_image.html` 的 §3/§3.8/§3.9 标题分割线（`.sec-h { border-bottom:1px solid #3a3f55 }`）在 Unity 上不渲染。根因有二——① mapping 无单边 longhand arm，`border-bottom` 走默认分支静默忽略；② 即使解析了，render 读的 `border_width` 单值字段会把四边独立宽度压扁成 top 单值。
> **方法论**：三份并行 subagent 调研背书——影响面审计（A）、render 数据流验证（B）、参考实现 + 几何方案（C，对照 FairyGUI + RmlUi）。

---

## 0. 目标 + 非目标

**目标**：
- 支持 `border-top/right/bottom/left` 单边 longhand（`<width> <style>? <color>?`，style 围栏外忽略，同 `border` 简写语义）。
- 单边 + 不等宽 `border-width` 四值都正确渲染：每边独立宽度，画非均匀边框环。
- **删掉** `ResolvedStyle.border_width: f32` 单值字段——它是 `ts.border` 的有损投影，是单边 border 渲染不出来的直接根因之一。render 改直读 `ts.border`（单一真相源）。
- 零新 `ResolvedStyle` 字段、零 layout 改动、零 FFI 改动。
- 兑现 CLAUDE.md 首要判据：AI 写 `border-bottom` 预期分割线 → Unity 实际渲染分割线。

**非目标**（YAGNI）：
- **每边独立颜色**：`border-bottom:red` 的 red 仍存进单值 `border_color`（四边共享）。单边场景（其余边 width=0）视觉完全正确；唯一做不到的是同元素四边异色（`border:2px solid red; border-bottom:2px solid blue`）——`border_color` 被后写者覆盖，文档注明围栏外。showcase 无此场景。
- **圆角 + 非均匀边框**：当前 `border_ring` 遇 `border-radius` 直角退化（圆角留待 SDF task）。本次直角四边先做对，圆角路径解耦。
- **border + background-image 共存**：现有限制（`render/mod.rs` `if !has_image`）保留——彩色边框环走 program=0，与 bg-image program=2/4 不兼容，共存需独立 draw call，另 task。
- `border-style`（dashed/dotted）：简写仍只取 width + color，style 丢弃（围栏外，既有行为）。

---

## 1. 方案选择

### 1.1 大方向：减法（删字段）而非加法（加机制）

**采用**：删 `border_width`，render 回归读 `ts.border: Rect<LengthPercentage>`（四边独立）。border 围栏内只支持 px，故 `ts.border` 只可能是 `Length(px)`，render resolve 是 trivial（render 已有 `resolve_lp` helper，Text/RichText arm 早就在用它读 `ts.border` 画文本盒——Container arm 跟上是既有模式的一致性延伸）。

**否决**：
- 加 `node.resolved_border` 字段 + layout 回写 `taffy Layout.border`——给 layout 开后门、把 layout 中间产物复制进 node，是加法；render 本就拥有推导 border 几何的全部输入（`ts.border` + `layout_rect` + `border_color`），不需要喂。
- 加 `ResolvedStyle.border_widths: BorderWidths` 四值字段（C 调研初始建议）——把单值冗余换成四值冗余，仍是 `ts.border` + 镜像两份，没消除根本问题。

### 1.2 几何：环拓扑不变（路线 1），非重写 per-side strip（路线 2）

**采用**：现有 8 顶点环拓扑**已天然支持非均匀宽度**。环的每边是 `outer[i]→outer[i+1]→inner[i+1]→inner[i]` 的梯形带；非均匀时这个梯形自然变成"左端 left 宽、右端 right 宽"的非对称带。**角区域不需要额外几何**——角是邻接梯形的重叠区，由梯形形状隐式正确表达。仅 3 处改：内轮廓公式四边独立、per-axis 钳制、per-edge 跳零宽边。

**否决**：重写成 fgui 式 5-quad per-side strip（路线 2）——改动更大，且环模型的"角是邻接带重叠区"比 fgui 的"角归 left/right 条带"更简洁。

### 1.3 钳制：per-axis 比例缩（CSS 浏览器语义）

`left+right > rw` → 两边同乘 `rw/(left+right)`（top/bottom 同理）。保证内 rect 永不负尺寸，且与 html 预览一致（兑现 AI 预测判据）。否决"不钳"（RmlUi 式重叠/反转）与"per-side ≤ 半轴"（破坏合法语义）。

### 1.4 颜色：四边共享单色

见 §0 非目标。`border_color` 单值不变。

---

## 2. CSS 子集

```
border-top    : <width> <style>? <color>?    /* 仅作用于 top 边 */
border-right  : <width> <style>? <color>?    /* right */
border-bottom : <width> <style>? <color>?    /* bottom */
border-left   : <width> <style>? <color>?    /* left */
  <width> = <px>          /* 非 px 整条落 false（围栏外静默忽略），同 border 简写 */
  <style> = solid|dashed|dotted|...   /* 围栏外忽略，不报错 */
  <color> = #rrggbb|#rgb|rgba(...)     /* 同 border-color 解析 */
```

| 写法 | 语义 |
|---|---|
| `border-bottom:1px solid #3a3f55` | bottom 宽 1、色 #3a3f55；其余三边不动 |
| `border-top:4px solid #e0e0e0` | top 宽 4；其余不动（showcase §3.1 top demo） |
| `border:2px solid blue` 后接 `border-bottom:1px solid red` | CSS：四边 2px blue、bottom 1px red。**单色共享局限**：`border_color` 被 red 覆盖 → 三边也显 red（CSS 应为 blue），见 §0 非目标 |
| `border-bottom:1px solid red` 后接 `border:2px solid blue` | CSS 简写重置四边：最终四边全 2px blue（bottom 的 1px red 被简写覆盖）。单色共享下 `border_color`=blue，与 CSS 一致 |

**与既有 arm 的叠加**：单边 arm 只设 `ts.border.<side>` 对应一边 + `border_color`，不动其他三边。多次单边声明累积（`border-top:2px; border-bottom:3px` → 上下各设，左右默认 0）。

**围栏同步**：`fence_contract.rs` 加单边 longhand 解析测试；`docs/design/fence.md` 补 `border-top/right/bottom/left` 条目（围栏内）。

---

## 3. 数据流（减法）

```
mapping 填 ts.border.<side>: LengthPercentage::Length(px)
  （border 简写四边同值 / border-width 四值独立 / 新增单边 longhand 单边）
  → layout solve（taffy 读 ts.border 算盒模型，零改）
  → render：resolve_lp 读 ts.border 四边 → BorderWidths
  → border_ring(rect, radii, BorderWidths, border_color) 画非均匀环
```

- **单一真相源**：`ts.border` 同时供 layout 盒模型 + render 几何。
- **render 复用既有 `resolve_lp`**（`render/mod.rs`，Text arm 先例）——不暴露 layout 的私有 `lp()`，零迁移成本。
- **删 `ResolvedStyle.border_width`**：render 不再读它；mapping 不再赋值。

---

## 4. 解析（`loomgui_core/src/style/mapping.rs`）

### 4.1 新增 4 个单边 longhand arm

复用 `border` 简写（~line 471）的 `<width> <style>? <color>?` 解析逻辑——抽一个共享 helper 解析"单边声明"，返回 `(width: f32, color: Option<[f32;4]>)`。简写 arm 与 4 个单边 arm 都调它：

```rust
"border-top"    => apply_border_side(value, Side::Top,    ts, style),
"border-right"  => apply_border_side(value, Side::Right,  ts, style),
"border-bottom" => apply_border_side(value, Side::Bottom, ts, style),
"border-left"   => apply_border_side(value, Side::Left,   ts, style),
```

`apply_border_side` 语义：解析出 `w` 与可选 `c`，设 `ts.border.<side> = Length(w)`，有 `c` 则 `style.border_color = Some(c)`，返回 `true`。**不动其他三边**。width 解析失败（非 px）→ 返回 `false`（整条静默忽略，与简写一致）。

### 4.2 删 `border_width` 赋值（2 处）

- `border` 简写 arm：删 `style.border_width = w`（保留 `ts.border = Rect{...}` 四边同值）。
- `border-width` arm：删 `style.border_width = t`（保留 `ts.border` 四边独立 `parse_four`）。

**顺带修既有 bug**：`border-width:1px 2px 3px 4px` 此前进 taffy border box 四边独立（layout 正确），但 `border_width = t` 只存 top → render 只画 top 宽均匀环。删 `border_width`、render 读 `ts.border` 后，四值不等宽自动正确渲染。

---

## 5. 几何（`loomgui_core/src/render/border.rs`）

### 5.1 `BorderWidths` 参数类型（border.rs 局部，不进 ResolvedStyle）

```rust
/// border_ring 的四边宽度参数（命名防 parse_four 的 [t,r,b,l] 索引错位）。
/// 仅作函数参数，不序列化、不进 ResolvedStyle。
pub struct BorderWidths { pub top: f32, pub right: f32, pub bottom: f32, pub left: f32 }
```

### 5.2 `border_ring` 签名 + 拓扑（3 处改，顶点拓扑不变）

签名 `width: f32` → `widths: BorderWidths`。8 顶点环拓扑保留，改 3 处：

**① 内轮廓 4 角按四边独立**（标准 CSS box model，与 RmlUi `ComputeBorderMetrics` 一致）：
```
inner_TL = (x + left,        y + top)
inner_TR = (x + rw - right,  y + top)
inner_BR = (x + rw - right,  y + rh - bottom)
inner_BL = (x + left,        y + rh - bottom)
```
外轮廓仍是 rect 4 角不变。每边梯形带三角化序 `[oi, oni, ini, oi, ini, ii]` 不变。

**② per-axis 比例钳制**（防内轮廓交叉）：
```rust
let xsum = l + r;
if xsum > rw && xsum > 0.0 { let s = rw / xsum; l *= s; r *= s; }
let ysum = t + b;
if ysum > rh && ysum > 0.0 { let s = rh / ysum; t *= s; b *= s; }
```
保证 `l+r ≤ rw`、`t+b ≤ rh`，内 rect 永不负尺寸。

**③ per-edge 跳零宽边**：发射前 `if widths[i] > 0.0` 才发该边 2 三角。
- **顶点固定 8 个**（仍发射全部 outer+inner 4 角）——简化 base offset（verts 索引恒 0..7），且零宽邻边的内角仍被相邻非零边引用（如 left=0 但 top/bottom≠0 时，inner_TL/BL 仍被 top/bottom strip 用），定义正确非浪费。
- 索引数：N 边 >0 → `6×N`（N∈0..4）。全 0 → 0 索引，caller `if !br.3.is_empty()` 已会跳过拼接。
- 唯一"浪费"：仅 1 边 >0 时 4 个未引用顶点（64 字节），可接受（与 arena 重带宽哲学不抵的替代方案——per-edge 动态顶点——base offset 复杂化收益仅 64 字节，不值）。

**早返**：`rect.w<=0 || rect.h<=0` 或**四边全 ≤0** → 空四表（caller `if !br.3.is_empty()` 跳拼接，无 8 顶点浪费）。至少一边 >0 时才进入环生成——此时"顶点固定 8 个"成立。激活门（任一边 >0）在 caller，`border_ring` 内不重复判。

### 5.3 winding 风险（实现时验证）

非均匀时角梯形可能凹（left≠right 使 top strip 梯形非对称）。须验证两三角朝向（CCW）一致，否则背面剔除导致缺角。加测试断言两三角有符号面积同号。现 `[oi, oni, ini, oi, ini, ii]` 在对称环下正确，非对称下重新验证。

---

## 6. 共存语义

### 6.1 border-color 单色共享

单边 arm 的 color 进 `border_color`（四边共享）。单边场景（其余边 width=0 → 不画）视觉完全正确。同元素四边异色不支持（§0 非目标）。

### 6.2 border + background-image

保留 `if !has_image` 门（`render/mod.rs`）：彩色边框环 program=0（白 1×1 纹理 × 顶点色）与 bg-image program=2/4（采样纹理 × 顶点色）同 Mesh payload 不兼容。单边 border 不改变此约束。共存需独立 RenderNode（独立 draw call），另 task。

### 6.3 border-image-slice（九宫格）互斥

`border-image-slice` 走 bg-image 路径（依赖 `background-image`，即 `has_image=true`），而彩色边框环门是 `!has_image`——天然互斥，无叠加路径。单边 border 不需为它加排他逻辑。

### 6.4 border-radius（圆角）

`border_ring` 遇 radii 直角退化（`let _ = radii;`，圆角留 SDF task）。四边非均匀对直角退化路径零影响。未来圆角 SDF + 非均匀需处理：per-corner per-axis 椭圆内半径、内半径≤0 塌缩、per-corner radius 沿边 clamp——解耦推进，直角四边是其子集基础，不浪费。

---

## 7. FFI / Unity 后端（零改）

- **payload_hash 全覆盖**（`render/dirty.rs`）：border 几何（verts/uvs/colors/indices）进 `mesh_arena`，hash 全量采样。改 border_ring 四边几何 → verts/indices 变 → `payload_hash` 变 → `ChangeLevel::Full` 自动触发。**FFI struct / blob / C# 绑定 / .dll ABI 零改**。
- **`border_width` 不进 FFI**：它从来是核心内部 `ResolvedStyle` 字段，不穿越 ABI（border 几何在核心内拼进 mesh）。删它不触 csbindgen、不动 `LoomGUIBindings.cs`。
- **双 hash 正交**：border 几何变只动 `payload_hash`（Full），不影响 `header_hash`（位置/alpha/sort 等）。
- **唯一动作**：重编 `loomgui_ffi_c.dll`（核心改了）+ 重打 pkg（bincode 布局变，见 §8）。

---

## 8. 包格式（破坏性变更）

删 `ResolvedStyle.border_width` 改变 bincode 序列化布局（坑 74 教训：bincode 字段顺序无模式，删字段旧 pkg 反序列化错位）。必须 bump 版本 + 拒绝旧 pkg：

- `PKG_FORMAT_VERSION: 15 → 16`（`asset/mod.rs`，注释更新：`删 ResolvedStyle.border_width 字段`）
- `MIN_VERSION = 16`、`MAX_VERSION = 16`
- `read_rejects_unsupported_version` 测试基线升 v16（TooNew=17, TooOld=15）
- **重打所有 pkg**（含 showcase）——旧 v15 pkg 被拒绝，必须用新打包器重产。

---

## 9. Showcase 验收（家里机 PlayMode）

showcase `page_image.html` 逐项过（无需改 HTML/CSS——它们早写了 `border-bottom`/`border-top`，现在能渲染了）：

- **R1**：§3/§3.8/§3.9 标题（`.sec-h`）下各有一条 `#3a3f55` 分割线（`border-bottom:1px solid`）。
- **R2**：`.header` 下分割线（`border-bottom:2px solid #3a3f55`）。
- **R3**：§3.1 border demo 的 `border-top:4px solid #e0e0e0` 只显示顶边（其余三边无）。
- **R4**：§3.1 其余 border demo（`border:2px solid` 等四边简写）仍正常渲染四边环——零回归。
- **R5**：`border-width:1px 2px 3px 4px` 类不等宽（若有）四边各自宽度。

**前提**：公司机重编 .dll + 重打 pkg（PKG_FORMAT 16）+ commit；家里机 pull 后 PlayMode 验（双机工作流）。

---

## 10. 测试

### 10.1 mapping（`style/mapping.rs` + tests）
- `border-bottom:1px solid #3a3f55` → `ts.border.bottom == Length(1.0)` + `border_color == #3a3f55`，其余三边不动。
- 四个单边 arm 各自只设对应边。
- 单边 color 缺省（`border-bottom:1px`）→ 不碰 `border_color`。
- 非 px width（`border-bottom:1em`）→ `false`（静默忽略）。
- `border-width:1px 2px 3px 4px` → `ts.border` 四边 `[1,2,3,4]`（实证已 parse，删 border_width 后渲染也正确）。
- 既有 `border_width` 断言（mapping/tests 多处）改断言 `ts.border.top` 等。

### 10.2 border_ring（`render/border.rs`）
- **回归**：`BorderWidths::all(5.0)` = 旧均匀环（8 顶点、24 索引、内角 (5,5)/(95,5)/(95,45)/(5,45)）。
- **单边**：`bottom=1, 其余=0` → 只 6 索引（底边 2 三角），8 顶点（4 未引用）。
- **非对称四边**：`top=2,right=3,bottom=4,left=5` → 4 内角落点正确。
- **钳制**：`left=80,right=40,rw=100` → scale=100/120，left≈66.67/right≈33.33。
- **单边超尺寸**：`left=200,rw=100` → scale=0.5，inner.x=x+100=x+rw（塌右缘）。
- **全 0**：0 索引（caller 跳拼接）。
- **winding**：非对称角梯形两三角有符号面积同号（朝向一致）。
- **退化 rect**（w=0）：空输出不 panic。

### 10.3 fence_contract（权威真相源）
- 加 `border-top/right/bottom/left` 到支持属性表（实证解析）。
- 既有 `border_width` 断言（~line 121）改 `ts.border`。

### 10.4 render 集成（render/tests.rs）
- 既有 `border_width = 4.0` 测试改设 `ts.border = Rect::length(4.0)`，断言不变（四边等宽几何同）。
- 端到端：`border-bottom:1px solid red` 元素 → mesh 含底边红三角、无其他三边。

### 10.5 dirty（payload_hash）
- 改 border 几何 → `payload_hash` 变 → Full（§7，实证 dirty.rs 全量采样）。

---

## 11. 已知限制 / 后续

- **同元素四边异色不支持**（§0/§6.1）：`border_color` 单色共享。罕见场景，文档注明围栏外。
- **圆角 + 非均匀边框**（§6.4）：直角退化，圆角 SDF 时再处理 per-corner per-axis 椭圆内半径。
- **border + bg-image 共存**（§6.2）：`!has_image` 限制保留，共存需独立 draw call。
- **`border-style` dashed/dotted**：仍忽略（围栏外，既有行为）。
- **每边独立色**：若将来需要，`border_color` 提升为 `Option<Rect<[f32;4]>>`，render 按边上色——本次不为它预留。

---

## 12. 涉及文件

| 文件 | 改动 |
|---|---|
| `loomgui_core/src/style/mapping.rs` | 加 4 单边 longhand arm + 共享 `apply_border_side` helper；删 border 简写/border-width arm 的 `border_width` 赋值 |
| `loomgui_core/src/style/resolved.rs` | 删 `border_width` 字段 + default + roundtrip 测试 |
| `loomgui_core/src/render/border.rs` | `BorderWidths` 类型；`border_ring` 签名四边 + 内轮廓公式 + per-axis 钳制 + per-edge 跳零宽；补单测 |
| `loomgui_core/src/render/mod.rs` | 删 `border_width` 读取；`resolve_lp` 读 `ts.border` 四边构造 `BorderWidths`；激活门改"任一边>0" |
| `loomgui_core/src/style/mapping/tests.rs` | `border_width` 断言改 `ts.border` |
| `loomgui_core/src/render/tests.rs` | `border_width` 设值改 `ts.border` |
| `loomgui_core/tests/fence_contract.rs` | 加单边 longhand 支持 + 改 `border_width` 断言 |
| `loomgui_core/src/asset/mod.rs` | `PKG_FORMAT_VERSION` 15→16 + MIN/MAX=16 + read_rejects 测试 |
| `docs/design/fence.md` | 补 `border-top/right/bottom/left` 条目 |
| `loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll` | 重建（核心改了） |
| showcase pkg.bin | 重打（PKG_FORMAT 16） |

**零改**：FFI blob / csbindgen / C# 绑定 / Unity 后端 / layout / scene node / dirty.rs（hash 已含 verts）/ shader / MaterialManager。

---

## 13. 参考对照

| 点 | FairyGUI | RmlUi | LoomGUI 本次 |
|---|---|---|---|
| 非均匀/单边 border | ❌ 不支持（单 lineWidth + lineColor，搜 `borderTop/...` 零命中） | ✅ 完整 per-side（四边独立 width + color） | ✅ 四边独立 width，单色共享 |
| 几何拓扑 | 5-quad per-side strip（角归 left/right） | corner-list + edge-fill（8 顶点环，锐角单色时与 LoomGUI 同形） | ✅ 8 顶点环（角是邻接带重叠区，不需额外几何） |
| 内轮廓公式 | N/A（均匀） | `outer + (left,top)`，`size - (l+r, t+b)` | ✅ 同 RmlUi（标准 CSS box model） |
| 零宽边 | 无（全画或全不画） | 跳过该边 FillEdge | ✅ 跳过该边 2 三角（顶点固定 8） |
| width 钳制 | 无（oversize 静默翻转） | 不钳（信任 layout） | 🔀 per-axis 比例缩（CSS 浏览器语义，html 预览对齐） |
| 参考路径 | `temp/FairyGUI-unity/.../Mesh/RectMesh.cs:40-78` | `Source/Core/GeometryBackgroundBorder.cpp`（upstream，本地 `temp/RmlUi/` 未 checkout） | — |

**关键洞察（C 调研）**：fgui 的 5-quad 与 LoomGUI 的 8 顶点环是两种拓扑；环模型的"角是邻接梯形重叠区"比 fgui 的"角归 left/right 条带"更适合非均匀扩展（角不需单独几何）。RmlUi 的 per-side 完整实现给方案背书（标准 CSS box model + 零宽边跳过模式一致）。
