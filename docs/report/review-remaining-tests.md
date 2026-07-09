# 独立测试文件深度代码审查报告

## 目录

1. [dotnet 测试（`tests/dotnet/`）](#1-dotnet-测试)
2. [Rust 集成测试（`loomgui_core/tests/`）](#2-rust-集成测试)
3. [Rust 内联测试模块](#3-rust-内联测试模块)
4. [跨文件问题汇总](#4-跨文件问题汇总)
5. [硬编码 assert 值统计](#5-硬编码-assert-值统计)

---

## 1. dotnet 测试

### 1.1 CoordMath.cs / CoordMathTests.cs——与 Rust transform.rs 一致性

**审查结论：两套数学正交，不直接对应。**

- `CoordMath.cs` 实现的是**屏幕坐标→设计坐标**的逆映射（viewport fitting + y-flip + safe-area），属于 Unity 后端的坐标变换层。
- Rust `transform.rs` 实现的是 2D 仿射矩阵运算（translate/rotate/scale/mul/inverse），是核心层的基础设施。
- 它们的关系是**上下层**: transform.rs 产出的 world_matrix 经 CoordMath 的逆映射回到设计坐标系。

**发现 1 — 公式正确性已验证（无 bug）**  
`tests/dotnet/CoordMath.cs:14-36`  
```csharp
float sf = Math.Min(aw / rw, ah / rh);
float offX = ax + (aw - rw * sf) * 0.5f;
float offYTop = ay + ah;
float dx = (screenX - offX) / sf;
float dy = (offYTop - screenY) / sf;
```
注释引用的 `LoomStageDriver.ConfigureTransforms` 前向公式：
- `sf = min(areaW/rootW, areaH/rootH)` — shrink-to-fit 缩放因子
- `offX` — 水平居中偏移
- `offYTop = areaY + areaH` — 屏幕顶部坐标（Unity 原点在左下）
- `dy = (offYTop - screenY) / sf` — y-flip 逆映射（对应根 GO `scale (1,-1,1)` 翻转）
与 Rust 注释的"核心 = 左上原点、y 向下"坐标系一致。✓

**级别：INFO**

**发现 2 — Round-trip 测试覆盖屏幕中心，但缺少边缘/角落**  
`tests/dotnet/CoordMathTests.cs:99-114`  
```csharp
var (dx, dy) = CoordMath.ScreenToDesign(
    screenX: 960f, screenY: 540f,  // 中心点
    ...
);
Assert.Equal(540f, dx, 1);
Assert.Equal(960f, dy, 1);
```
仅验证设计中心 (540, 960) ↔ 屏幕中心 (960, 540) 的双向映射。缺少屏幕四角、safe-area 边界场景的 round-trip 验证。

**级别：LOW — 补 safe-area 角落 round-trip 用例。**

**发现 3 — 零尺寸防御缺少 NaN 测试**  
`tests/dotnet/CoordMathTests.cs:62-75`  
```csharp
// 全 0 输入退回 sf=1，屏幕左上→design(100, 方向翻转后 dy≈-199)
Assert.True(float.IsFinite(dx));
Assert.True(float.IsFinite(dy));
```
只断言 `IsFinite`，没有验证具体返回值。`dy` 的预期值未计算确认（注释 "≈-199" 是猜测口气）。

**级别：LOW — 明确断言零尺寸下的期望返回值。**

---

### 1.2 EventRouter.cs——与 Rust hit/input 的关系 & 逻辑同步

**审查结论：不是对应物，是同一份 C# 业务逻辑的副本。**

根据 CLAUDE.md（"事件路由本身在业务侧（C# LoomEventHandler），非核心"）：
- Rust 核心做命中检测 + hover/active diff + 伪类 rematch，**不做事件路由**。
- `tests/dotnet/EventRouter.cs` 是事件路由算法（capture + bubble + stop + touchCapture），是生产代码 `loomgui_unity_package/Runtime/LoomEventHandler.cs` 的独立副本。
- `tests/dotnet/Stubs/` 提供桩类型让测试脱离 Unity 运行。

**发现 4 — EventRouter.cs 是生产代码的分离副本，存在漂移风险**  
`tests/dotnet/EventRouter.cs:1-83` vs `loomgui_unity_package/Runtime/LoomEventHandler.cs:72-103`
- 两处代码实现了相同的事件路由逻辑（capture 阶段反向遍历链、bubble 阶段正向、stop propagation / stop immediate / touch capture）。
- 如果生产代码修改路由逻辑（如支持更多事件类型、修改 phase 语义），测试副本不会自动同步。
- 注释（第 6 行）说明 `LoomEventHandler 委托给本类的静态方法跑路由`，暗示实际部署时代码已内聚到生产文件。但测试仍用独立副本。

**级别：MEDIUM — 建议确认 `tests/dotnet/EventRouter.cs` 是否仍与生产 `LoomEventHandler` 逻辑一致；或改为直接测试生产代码。**

**发现 5 — NO_PARENT 哨兵值 0xFFFF_FFFF 与 NodeId::INVALID 一致**  
`tests/dotnet/EventRouter.cs:11`
```csharp
public const uint NO_PARENT = 0xFFFF_FFFF;
```
Rust `node/tests.rs:13`：
```rust
assert!(!NodeId::INVALID.is_valid(), "0xFFFF_FFFF = INVALID");
```
两处哨兵值一致。✓

**发现 6 — Stubs 不够模拟真实 Unity 行为**  
`tests/dotnet/Stubs/UnityEngine.cs:1-23`
- 只提供 `Vector2`, `Color`, `Mathf.Abs` 三个最小类型。
- Unity 真实 `Color` 有隐式转换、运算符重载、属性（`red`, `green` 等）；桩只有 struct 字段。
- Unity 真实 `Mathf` 有 `PI`, `Sin`, `Cos`, `Clamp`, `Lerp` 等方法；桩只有 `Abs`。
- 当下测试不依赖这些 API，所以测试能通过。但如果新测试涉及更多 Unity 类型，编译将失败。

**级别：LOW — 按需扩展桩。当前够用。**

**发现 7 — LoomEventTypes.cs 桩缺少 InputEvent / Touch 等高级类型**  
`tests/dotnet/Stubs/LoomEventTypes.cs:1-52`
- 只定义了 `EventType` 枚举、`Phase` 枚举、`EventCallback` 委托、`EventContext` 类及其对象池。
- 缺少 `InputEvent`, `TouchInfo`, `KeyEvent` 等真实事件系统的结构体。
- `EventType` 枚举（第 7-15 行）跳号：`LongPress=9` 后直接 `KeyDown=12`，中间跳过了 10、11。需确认与生产定义一致。

**级别：LOW — 跳号需核实。**

---

### 1.3 FrameBlobTests.cs——blob 格式 v10 一致性

**发现 8 — elemSizes 数组与 Rust columns 一致（已验证）**  
`tests/dotnet/FrameBlobTests.cs:15`
```csharp
int[] elemSizes = { 4, 4, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 1, 4, 4, 4, 1, 80, 1, 4 };
```
Rust `blob.rs:21-42` columns:
```
node_id:4, parent_id:4, visible:1, alpha:4, sort_key:4,
mask_context:4, m_a:4, m_b:4, m_c:4, m_d:4, m_tx:4, m_ty:4,
payload_kind:1, mesh_off:4, mesh_len:4,
path_idx:4, program:1, color_matrix:80, change_level:1, reuse_key:4
```
20 列，顺序一致。header offset: `3*4 + 20*4 + 6*4 = 116`，与 C# `FrameBlob.cs` 头长 116 一致。✓

**级别：INFO**

**发现 9 — V10Header helper 缺少 columnData 长度验证**  
`tests/dotnet/FrameBlobTests.cs:36-44`
```csharp
int expected = elemSizes[c] * nodeCount;
var data = columnData[c];
if (data != null)
    b.AddRange(data);
```
如果 `columnData[c]` 的长度不等于 `expected`，数据会被错误写入，导致 ColumnAccessors test 可能读取到错误偏移。实际测试都传入了正确长度（单节点 × elemSize），没有触发，但 helper 缺少防御。

**级别：LOW — 加 `Debug.Assert(data.Length >= expected)` 防御。**

**发现 10 — color_matrix 列 80 字节（[f32;20]），但 C# 测试中初始化为全零**  
`tests/dotnet/FrameBlobTests.cs:110`
```csharp
// cols[17] = 80 zero bytes  // color_matrix
```
Rust 端对所有非 ColorFilter 节点填零矩阵。C# 端 frame 里 `color_matrix: [0.0; 20]`。测试断言时没有读回此列验证全零。这不是 bug，但缺覆盖。

**级别：LOW — 补一个 color_matrix 非零 round-trip 用例（对应 Rust `blob_color_matrix_column_round_trips`）。**

**发现 11 — Path 表 path_idx 是 1-based，但 C# 测试 ReadPath 调用 index=1 和 2**  
`tests/dotnet/FrameBlobTests.cs:161-174`
```csharp
pathTable.AddRange(U32(2));  // path_count = 2
// ...
Assert.Equal("res/icon.png", blob.ReadPath(1)); // 1-based ✓
Assert.Equal("a/b", blob.ReadPath(2));          // 1-based ✓
```
与 Rust `intern_path` 的 1-based 索引（0=纯色无图）一致。✓

---

### 1.4 PkgManifestReaderTests.cs

**发现 12 — 测试用内联字节构造，与生产 PkgManifestReader 解析逻辑对齐**  
`tests/dotnet/PkgManifestReaderTests.cs:61-102`
- 手动构造 pkg.bin 二进制（header + string table + component table + asset manifest entries）。
- 这实际上是对 `PkgManifestReader.ReadAssetManifest` 格式的**白盒测试**——测试知道内部格式。如果 pkg 格式升级，测试也需同步更新。
- 验证了 path/w/h 的 round-trip。✓

**级别：INFO**

**发现 13 — 缺少 manifest entry 带 0 宽高 / 空路径的边界测试**  
测试仅在 `WithEntries` 用例中覆盖了正常数据。没有宽/高为 0、路径为空字符串、路径含特殊字符（中文/空格）等边界。

**级别：LOW — 补边界用例。**

---

## 2. Rust 集成测试

### 2.1 snapshot.rs——snap 文件状态

**发现 14 — 3 个 snap 文件均存在，与测试函数一一对应**  
```
snapshot__simple_panel.snap      (870 行)
snapshot__cascade_inheritance.snap
snapshot__image_with_texture.snap
```
三个 snap 文件均在 `loomgui_core/tests/snapshots/` 目录下，对应 `snapshot_simple_panel`, `snapshot_cascade_inheritance`, `snapshot_image_with_texture`。

**级别：INFO**

**发现 15 — snap 文件含字体相关的浮点值，换字体/改渲染代码会导致 snap 漂移**  
`loomgui_core/tests/snapshots/snapshot__simple_panel.snap:219`  
```json
"verts": [[-1.046875, 16.1875], [10.953125, 16.1875], ...]
```
这些值来自 DejaVuSans.ttf 的特定 glyph 度量。如果字体文件更新、或 text layout 算法改变，snap 中的浮点数会变化，需 `INSTA_UPDATE=always` 重新接受。这是 insta snapshot 的正常使用模式，不是问题。

**级别：INFO**

**发现 16 — snap 文件含 slotmap NodeId 绝对值（如 4097），变更 slotmap 实现会导致漂移**  
`loomgui_core/tests/snapshots/snapshot__simple_panel.snap:7`  
```json
"node_id": 4097
```
`NodeId` 由 slotmap 分配，值取决于内部 free-list 状态。如果 slotmap 升级或测试顺序改变，这些值会变化。不过 insta snapshot 会标记为不匹配，提醒开发者审查。

**级别：LOW — 如果频繁因 NodeId 漂移而需更新 snap，考虑在 `render_json()` 中输出相对 id（如用 id 属性或索引替代）。**

---

### 2.2 node_sort_keys.rs

**发现 17 — 验证了 NativeHost 查询的关键不变量**  
`loomgui_core/tests/node_sort_keys.rs:39-81` 和 `83-112`
- 两个测试覆盖了空 div（no bg, merge 后吞 RenderNode entry）的 sort key 保留行为，以及 sort key 与 RenderNode.sort_key 的对齐。
- 注释引用了 NativeHost FFI 查询的设计意图。✓

**级别：INFO**

**发现 18 — 只测试两个兄弟节点，缺少深层嵌套、多子树场景**  
`loomgui_core/tests/node_sort_keys.rs:45`
```html
<div><div id="nh-stage"></div><div id="nh-effect" style="background-color:#000"></div></div>
```
并未覆盖以下场景：
- 三层以上嵌套
- 两个独立子树（两个 root node，如 Scene 多根）
- 虚拟列表 slot 场景（reuse_key>0 节点）

**级别：LOW — 如果 sort key 逻辑变复杂，补更多场景。**

---

### 2.3 v1e_dirty.rs

**发现 19 — 只测了 Skip 和 Full，缺 Header change level 测试**  
`loomgui_core/tests/v1e_dirty.rs:36-58`
```rust
fn stage_static_frame_produces_skip() {
    // 首帧全 Full，第二帧静态 → Skip
    assert!(f2.nodes.iter().any(|n| n.change_level == ChangeLevel::Skip));
}
```
`ChangeLevel::Header`（仅 header 变、不重建 mesh）没有被测试触发。Header 是 transform/opacity/tint 动画的优化路径，缺少覆盖意味着如果未来代码破坏 Header 的 hash 计算，不会被这里发现。

**级别：MEDIUM — 补一个触发表头变更（如只改 transform/alpha）产生 Header 的测试。**

**发现 20 — font_path() 返回无用的 `usize`**  
`loomgui_core/tests/v1e_dirty.rs:12-19`
```rust
fn font_path() -> (String, usize) {
    let p = format!(...);
    let n = p.len();
    (p, n)
}
```
返回值的 `usize` 在所有调用处被解构为 `_fplen`（未使用）。简化为只返回 `String`。

**级别：LOW — 清理死代码。**

---

### 2.4 stage_getters.rs

**发现 21 — 覆盖了完整的状态空间（有效节点 / 无效 NodeId / scene=None）**  
`loomgui_core/tests/stage_getters.rs:114-151`
- `get_node_invalid_returns_none`: 验证 `NodeId::INVALID` → 三个 getter 全部返回 None/false。
- `get_node_no_scene_returns_none`: 验证 scene=None → 早返不 panic（引用坑 102）。
- 每条断言都有错误消息。✓

**级别：INFO**

**发现 22 — 缺少 get_node_world_matrix 返回 [a,b,c,d,tx,ty] 6 元素的断言**  
`loomgui_core/tests/stage_getters.rs:54-63`
```rust
let wm = stage.get_node_world_matrix(n).expect("...");
assert!((wm[4] - 100.0).abs() < 1e-3, ...);
assert!((wm[5] - 200.0).abs() < 1e-3, ...);
```
只验证了 tx, ty 分量。未验证 a, b, c, d 分量（translate 时应为 [1,0,0,1]）。虽然当前实现正确，但缺少防御性断言。

**级别：LOW — 加 `wm[0]==1, wm[1]==0, wm[2]==0, wm[3]==1` 断言。**

---

## 3. Rust 内联测试模块

### 3.1 style/mapping/tests.rs——CSS 属性覆盖

**发现 23 — fence_contract.rs 声明的支持属性有 30 个，mapping 测试只覆盖约 17 个**  
fence_contract.rs `supported_layout_props_return_true` + `supported_visual_props_return_true` 属性列表：

| 属性 | fence test | mapping test | 映射测试断言粒度 |
|------|-----------|-------------|-----------------|
| display | ✓（true/false） | ✗ | — |
| flex-direction | ✓ | ✗ | — |
| flex-wrap | ✓ | ✗ | — |
| gap | ✓ | ✗ | — |
| justify-content | ✓ | ✗ | — |
| align-items | ✓ | ✗ | — |
| width | ✓ | ✓ | `apply_decl` returns true（非字段级） |
| padding | ✓ | ✗ | — |
| margin | ✓ | ✗ | — |
| aspect-ratio | ✓ | ✗ | — |
| order | ✓ | ✓ | 字段级（`s.order == 2`） |
| background-color | ✓ | ✓ | 字段级 |
| background-image | ✓ | ✓ | 字段级 |
| background-size | ✓ | ✓ | 字段级 |
| border-radius | ✓ | ✓ | 字段级 |
| opacity | ✓ | ✗ | — |
| overflow | ✓ | ✓ | 字段级 |
| color | ✓ | ✗ | — |
| font-size | ✓ | ✗ | — |
| font-weight | ✓ | ✗ | — |
| text-align | ✓ | ✗ | — |
| white-space | ✓ | ✗ | — |
| transform | ✓ | ✓ | 字段级 |
| pointer-events | ✓ | ✓ | 字段级 |
| filter | ✓ | ✓ | 字段级 |
| border-image-slice | ✓ | ✓ | 字段级 |
| transition | ✓ | ✓ | 字段级 |
| top/right/bottom/left | ✓ | ✗ | — |
| position | ✓ | ✗ | — |

**mapping 测试缺少：display, flex-direction, flex-wrap, gap, justify-content, align-items, padding, margin, aspect-ratio, opacity, color, font-size, font-weight, text-align, white-space, position, inset 字段级断言。**
这些属性仅在 fence_contract.rs 中验证了 `apply_decl` 返回 true/false，但没有验证写入 `ResolvedStyle` 的**字段值**是否正确。

**级别：MEDIUM — 部分属性（padding 四值展开、margin 简写、flex-direction 映射到 taffy）的字段级正确性没有测试覆盖，靠集成测试间接验证。**

**发现 24 — 过渡属性 parse_transition_multiple_comma_specs 回归测试覆盖了坑 133**  
`loomgui_core/src/style/mapping/tests.rs:489-497`
```rust
let ts = parse_transition("background-color 0.3s ease-out, color 0.3s ease-out");
assert_eq!(ts.len(), 2, "逗号分隔两个 spec");
```
直接回归了坑 133（transition 逗号多 spec 被吞）。✓

**发现 25 — filter concat 顺序回归测试覆盖了 CSS vs fgui 顺序问题**  
`loomgui_core/src/style/mapping/tests.rs:428-455`
```rust
let correct = color_filter::concat(&hue, &sat); // CSS: H × S
let reversed = color_filter::concat(&sat, &hue); // 错误顺序
```
验证了 filter 多函数串联时 "新值在左" 的矩阵乘法顺序（与 CSS/fgui 一致）。✓

---

### 3.2 scene/node/tests.rs

**发现 26 — ControllerChangedEvent ABI 尺寸测试重复**  
`loomgui_core/src/scene/node/tests.rs:558-566` 和 `615-618`
```rust
fn controller_changed_event_abi_size() {  // 行 558
    assert_eq!(std::mem::size_of::<ControllerChangedEvent>(), 12);
}
fn controller_changed_event_size() {      // 行 616
    assert_eq!(std::mem::size_of::<ControllerChangedEvent>(), 12);
}
```
两个测试做完全相同的事。删除一个。

**级别：LOW — 删除重复。**

**发现 27 — AnimTable::clear_prop 测试使用 macro_rules! 绕过 is_empty 过滤**  
`loomgui_core/src/scene/node/tests.rs:516-520`
```rust
macro_rules! raw {
    () => { t.0.get(&id).expect("条目存在（clear_prop 不 remove）") };
}
```
这段注释说 "macro：每次展开独立借用，避免闭包持借冲突 clear_prop 的 &mut"。macro 用在这里略怪异——每个展开点都产生一次 `t.0.get(&id)` 调用。用普通函数或内联写法也可。不构成 bug，但影响可读性。

**级别：LOW — 考虑改为 inline `t.0.get(&id).unwrap()` 替代 macro。**

**发现 28 — NodeId index/gen 测试仅验证编码/解码，未测 FFI 透传**  
`loomgui_core/src/scene/node/tests.rs:3-9`
```rust
fn node_id_index_and_gen_decode() {
    let id = NodeId((5 << 12) | 7);
    assert_eq!(id.index(), 5);
    assert_eq!(id.gen(), 7);
}
```
验证了 20bit index + 12bit gen 位布局。但未测试 u32 透传到 C# 后能否正确回传（U32 → FFI → C# int → FFI → U32）。此测试在 C# 侧（FrameBlobTests 中 column accessor 验证了 node_id u32 round-trip）间接覆盖。

**级别：INFO**

**发现 29 — Scene::build 条目是 8 元组，多处测试用冗长构造**  
`loomgui_core/src/scene/node/tests.rs:19-28` 和多个其他测试
```rust
let entries: Vec<(
    Option<usize>, NodeKind, ResolvedStyle, Vec<String>, Option<String>,
    bool, Option<i32>, Option<String>,
)> = vec![(None, NodeKind::Container, ResolvedStyle::default(), Vec::new(), None, false, None, None), ...];
```
每个测试都手动构造 8 元组，代码重复严重。如果 Scene::build 签名未来再变，需修改所有测试。建议封装 builder 函数。

**级别：LOW — 封装测试 helper（如 `entry_container(parent, id, draggable, tabindex, controller)`）。**

---

### 3.3 scene/node/parse_tests.rs

**发现 30 — 覆盖了 parse_html → build_scene 的完整路径**  
- 验证了 tag→NodeKind 映射（div→Container, span/裸文本→Text, button→Button, img→Image）
- 验证了 css cascade 效果（color 继承、font-size 继承、height 不继承）
- 验证了 draggable/tabindex/data-controller 属性解析
- 覆盖了 overflow hidden → clip_rect slot 生成

**级别：INFO**

**发现 31 — 缺 display:none 子树在 build_scene 中的行为验证**  
`parse_tests.rs` 没有测试 `display:none` 样式下子节点是否在 scene 中被标记为不渲染。此行为在 `stage_getters.rs:91-111` 中通过 `get_node_visible` 间接覆盖，但不是 parse 层的直接测试。

**级别：LOW**

---

### 3.4 blob/tests.rs

**发现 32 — TestView 注释中列索引标注过时**  
`loomgui_ffi_c/src/blob/tests.rs:345-351`
```rust
// col_off 索引：0=node_id ... 19=reuse_key (v9)
// v10：删 text_off/text_len，22→20 列。
struct TestView<'a> {
```
注释提到 "22→20 列"（v10 的实际改动），但行 345-349 的快速索引表仍然是 v10 的实际布局。**列号正确，注释的版本号存在微小的不一致**：
- 行 349: `15=path_idx (v7)` — 实际在 v10 中仍是第 15 列
- 行 350: `18=change_level (v8)` — 实际在 v10 中仍是第 18 列
- 行 351: `19=reuse_key (v9)` — 实际在 v10 中仍是第 19 列
这些 "(vX)" 标注表示"此列在 vX 版本中引入"，而非"此列当前在 vX 版本的位置"。没有 bug，但容易误会。

**级别：LOW**

**发现 33 — world_matrix_roundtrip 注释称 VERSION=9**  
`loomgui_ffi_c/src/blob/tests.rs:772` 和 `line 738`
```rust
/// VERSION=9（v9：加 reuse_key 列），blob len > 100。
```
实际当前版本是 VERSION=10。编译时断言是 `VERSION=10`（行 774），但注释未同步更新。

**级别：LOW — 更新注释。**

**发现 34 — reuse_key 测试注释称"第 22 列"**  
`loomgui_ffi_c/src/blob/tests.rs:877`
```rust
/// v9：reuse_key 列（第 22 列）round-trip。
```
v10 删了 text_off/text_len 列后，reuse_key 从第 21 列变为第 19 列（0-indexed），或第 20 列（1-indexed），不再是 22。注释过时。

**级别：LOW — 修正为"第 19 列（0-indexed）"。**

**发现 35 — blob 测试覆盖了 v10 关键变更**  
- ✓ text_arena 已删除 → clip_table 紧跟 mesh_arena
- ✓ 20 列结构验证
- ✓ mesh verts re-base 到本地坐标
- ✓ alpha 不烤进顶点色
- ✓ merged mesh blob 保持绝对坐标
- ✓ change_level 列 round-trip
- ✓ reuse_key 列 round-trip
- ✓ path 表 1-based 索引
- ✓ clip 表 round-trip
- ✓ color_matrix 列 round-trip
- ✓ program 列 round-trip

**级别：INFO — 覆盖质量高。**

---

## 4. 跨文件问题汇总

### 4.1 load_html_css 重复定义（4 处）

| 文件 | 行号 |
|------|------|
| `loomgui_core/tests/snapshot.rs` | 38-48 |
| `loomgui_core/tests/node_sort_keys.rs` | 25-35 |
| `loomgui_core/tests/v1e_dirty.rs` | 22-32 |
| `loomgui_core/tests/stage_getters.rs` | 24-34 |

四个文件定义**完全相同的** `load_html_css` 函数（和 `font_path` 辅助函数）。如果 API 变更，需改四处。

**级别：MEDIUM — 提取到共享 `tests/common/mod.rs`。**

### 4.2 font_path 重复定义（4 处）

同上，`font_path()` / `test_font_path()` 在各测试文件中重复定义。

**级别：MEDIUM — 一并提取。**

---

## 5. 硬编码 assert 值统计

| 文件 | 硬编码 assert 值数 | 类别 |
|------|-------------------|------|
| FrameBlobTests.cs | ~45 | blob 格式常量、坐标值、哈希值 |
| CoordMathTests.cs | ~20 | 坐标、缩放因子、屏幕尺寸 |
| EventRouterTests.cs | ~12 | node id 字面量、期望链长度 |
| PkgManifestReaderTests.cs | ~10 | pkg 二进制字面值 |
| blob/tests.rs | ~60 | 顶点坐标、矩阵分量、arena offset |
| style/mapping/tests.rs | ~40 | CSS 解析结果、矩阵分量、颜色值 |
| scene/node/tests.rs | ~20 | NodeId 位编码、ABI 尺寸、selected_index |
| scene/node/parse_tests.rs | ~15 | node count、CSS 属性值 |
| snapshot.rs | 0（由 snap 文件接管） | — |
| node_sort_keys.rs | ~5 | sort key 范围 |
| v1e_dirty.rs | ~4 | ChangeLevel 枚举值 |
| stage_getters.rs | ~8 | transform 坐标、sort key 排序 |

**大部分硬编码值是合理的**（如验证 `parse_color("#ff0000")` 返回 `[1.0,0,0,1.0]`，无需常量化）。真正需要常量化的是跨文件共享的魔法数字：

- blob header 长度 `116`（FrameBlobTests.cs:16 和 blob/tests.rs 隐含在 col_off 计算中）
- 列偏移量（多处隐式）
- NodeId 位宽度 `20`/`12`（node/tests.rs:6）

**级别：LOW — blob header 116 和 NodeId 位宽可考虑提取常量。**

---

## 发现严重级别分布

| 级别 | 数量 | 说明 |
|------|------|------|
| MEDIUM | 5 | EventRouter 与生产代码同步风险、Header change level 缺测、mapping 属性字段级覆盖不足、load_html_css 重复、font_path 重复 |
| LOW | 14 | 清理注释、补充边界测试、清理死代码/重复、加防御断言 |
| INFO | 16 | 确认正确、覆盖良好、无需行动 |
