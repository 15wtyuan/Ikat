# 代码审查报告：Showcase Driver + FFI 绑定镜像 + Trivial 模块

## 1. LoomShowcaseDriver.cs

**文件**: `loomgui_unity/Assets/Scripts/Demo/LoomShowcaseDriver.cs`（1205 行）

### 总体评价

代码质量良好，结构清晰，注释详尽。"按页 listener 注册表"模式（`_pageListeners` + `ClearPageListeners`）是正确的事件生命周期管理方式。NativeHost 的绑定/解绑、角色 anchor 对齐、虚拟列表的 reuse_key 段隔离等机制实现正确。**可作为参考实现参考。**

以下按问题逐一列出：

---

### 1.1 【建议】Showcase 根视口尺寸硬编码

**文件**: `LoomShowcaseDriver.cs:162`
```csharp
_root = _stage.CreateRoot("div", "width:1080px;height:1920px;background-color:#1a1d2e;flex-direction:column");
```
`1080×1920` 直接硬编码进代码，且 pkg 包名 `"showcase"` 硬编码为常量（`:79`）。demo 场景这样没问题，但若被当作"参考实现"照抄，后续维护者可能忽略这些值需要和自己的 pkg/设计分辨率对齐。

**严重级别**: 低（demo 合理）

---

### 1.2 【建议】RichText link_id 硬编码为 1，依赖 pkg 内部约定

**文件**: `LoomShowcaseDriver.cs:505`
```csharp
_richLinkId = 1;
_stage.EventHandler.AddLinkClickListener(_richLinkId, OnRichLink);
```
注释写道"初始 markup 的 `<a>` 也是 link_id=1"，这是 pkg 包内部内容的假定。如果 page_richtext.html 改了 markup 结构（比如加了别的 `<a>` 标签），link_id 会漂移导致 link 点击不响应。

**修复方向**: 从 Rust 侧 query link_ids 或 pkg 里约定声明。demo 场景可接受。

**严重级别**: 低

---

### 1.3 【建议】VirtualListDriver 不等高 O(n) 线性扫描每帧

**文件**: `LoomShowcaseDriver.cs:1176-1196`
```csharp
int FindFirstVisible(float sy)
{
    float acc = 0f;
    for (int i = 0; i < _itemCount; i++)  // O(n)
    {
        if (acc + _itemSizes[i] > sy) return i;
        acc += _itemSizes[i];
    }
    return (int)_itemCount - 1;
}
```
`FindFirstVisible`、`FindLastVisible`、`SumSizesUpTo` 均做 O(n) 线性扫描。当前 demo 只有 200 项，性能无影响。若作为参考实现被用于万级列表，需要前缀和数组 + 二分查找。

**严重级别**: 低（demo 200 项足够）

---

### 1.4 【注意】VirtualListDriver.FinishMeasure 对 tick 时序有隐式依赖

**文件**: `LoomShowcaseDriver.cs:1086-1097`
```csharp
void FinishMeasure()
{
    var r = _stage.GetNodeLayoutRect(_measureRoot);
    if (r.h > 0)
    {
        _itemSize = r.h;
        ...
    }
}
```
`FinishMeasure` 在 `Update()` 中调用，读取 `layout_rect`。layout 由 `LateUpdate` 中的 `solve` 填充。时序上 `Update` 先于 `LateUpdate`，所以首帧 `r.h` 为 0，必须等下帧重试。这是正确的设计（`_initStep` 状态机保证），但依赖 Unity 的 `Update`/`LateUpdate` 顺序，且注释未说明此时序约束。如果未来有人把 SyncSlots 调到 LateUpdate 之后调用，会立即读到 h>0 跳过一帧等待——虽不影响正确性但逻辑前提变了。

**修复方向**: 在 `FinishMeasure` 注释中说明依赖 Update→LateUpdate 时序。

**严重级别**: 低

---

### 1.5 【确认正确】NativeHost 生命周期管理

**文件**: `LoomShowcaseDriver.cs:206-210, 986-987`
```csharp
// 离开页时 Unbind
if (_nativeBoundNode != uint.MaxValue)
{
    _stage.UnbindNativeHost(_nativeBoundNode);
    _nativeBoundNode = uint.MaxValue;
}

// OnDestroy 时 UnconfigureTransparentMaterials
void OnDestroy()
{
    if (_characterInstance != null)
        NativeHostManager.UnconfigureTransparentMaterials(_characterInstance);
    if (_nativeModelInstance != null)
        NativeHostManager.UnconfigureTransparentMaterials(_nativeModelInstance);
}
```
切页前 Unbind（防残留 wrapper GO），销毁时 UnconfigureTransparentMaterials（释放 clone 的材质）。逻辑正确，符合 CLAUDE.md 的 NativeHost 分层契约。

**严重级别**: 无

---

## 2. FFI 绑定镜像（手补 C# struct）

### 2.1 LoomGUIPointerEvent.cs

**文件**: `loomgui_unity_package/Plugins/LoomGUI/Bindings/LoomGUIPointerEvent.cs`

**Rust 侧** (`loomgui_core/src/input.rs:11-18`):
```rust
#[repr(C)]
pub struct PointerEvent {
    pub kind: PointerKind,  // repr(u8) → 1 byte
    pub button: u8,         // 1 byte
    pub pad: [u8; 2],       // 2 bytes
    pub touch_id: i32,      // 4 bytes, align 4
    pub x: f32,             // 4 bytes, align 4
    pub y: f32,             // 4 bytes, align 4
}
```
**C# 侧** (`LoomGUIPointerEvent.cs:16-24`):
```csharp
[StructLayout(LayoutKind.Sequential)]
internal unsafe struct PointerEvent
{
    public byte kind;        // @0
    public byte button;      // @1
    public byte pad0;        // @2
    public byte pad1;        // @3
    public int touch_id;     // @4 (align 4 ✓)
    public float x;          // @8 (align 4 ✓)
    public float y;          // @12 (align 4 ✓)
}  // 16B total
```

**判定**: 字段类型、顺序、对齐均正确。`i32` 对齐 4 字节 → `touch_id` 在偏移 4 的位置，恰落在 Rust `[u8;2]` 后。`f32` 对齐 4 字节。C# 用两个 `byte` 展开 `[u8;2]` 等价。

**注意**: Rust 侧缺少 `const _: () = { assert!(std::mem::size_of::<PointerEvent>() == 16); };` 的 ABI 尺寸断言（对比 `WheelEvent` 有，`scroll.rs:26-28`）。若未来增减字段，编译器不会提醒 C# 侧同步。

**严重级别**: 低（布局正确，仅缺断言）

---

### 2.2 LoomGUIKeyEvent.cs

**文件**: `loomgui_unity_package/Plugins/LoomGUI/Bindings/LoomGUIKeyEvent.cs`

**Rust 侧** (`loomgui_core/src/input.rs:34-39`):
```rust
#[repr(C)]
pub struct KeyEvent {
    pub key_code: u32,   // 4 bytes, align 4
    pub modifiers: u8,   // 1 byte
    pub is_down: bool,   // 1 byte
    pub pad: [u8; 2],    // 2 bytes
}
```
**C# 侧** (`LoomGUIKeyEvent.cs:15-21`):
```csharp
[StructLayout(LayoutKind.Sequential)]
internal unsafe struct KeyEvent
{
    public uint key_code;   // @0 (4B)
    public byte modifiers;  // @4 (1B)
    public bool is_down;    // @5 (1B)
    public byte pad0;       // @6 (1B)
    public byte pad1;       // @7 (1B)
}  // 8B total
```

**判定**: 字段类型、顺序、对齐均正确。8 字节紧凑。`bool` 在 C# 和 Rust 中都是 1 字节。

**注意**: 同 PointerEvent，Rust 侧缺少尺寸断言。

**严重级别**: 低

---

### 2.3 LoomGUIWheelEvent.cs

**文件**: `loomgui_unity_package/Plugins/LoomGUI/Bindings/LoomGUIWheelEvent.cs`

**Rust 侧** (`loomgui_core/src/scroll.rs:20-25`):
```rust
#[repr(C)]
pub struct WheelEvent {
    pub x: f32,        // 4 bytes
    pub y: f32,        // 4 bytes
    pub delta_x: f32,  // 4 bytes
    pub delta_y: f32,  // 4 bytes
}
const _: () = { assert!(std::mem::size_of::<WheelEvent>() == 16); };  // ← 有断言 ✓
```
**C# 侧** (`LoomGUIWheelEvent.cs:14-21`):
```csharp
[StructLayout(LayoutKind.Sequential)]
internal unsafe struct WheelEvent
{
    public float x;        // @0
    public float y;        // @4
    public float delta_x;  // @8
    public float delta_y;  // @12
}  // 16B total
```

**判定**: 布局完全正确，Rust 侧也有 ABI 尺寸断言防护。全 f32 无 padding 问题。

**严重级别**: 无

---

### 2.4 【建议】PointerEvent / KeyEvent 缺 ABI 尺寸断言

**文件**: `loomgui_core/src/input.rs`（PointerEvent 定义后、KeyEvent 定义后）

`WheelEvent`（`scroll.rs:26-28`）有编译期 `const _: () = { assert!(size_of == ...); };` 断言，但 `PointerEvent` 和 `KeyEvent` 没有。如果未来给这两个 struct 加字段（如 PointerEvent 加 `pressure: f32`），Rust 编译通过但 FFI 布局漂移 → C# 侧未同步 → 静默内存错位（读取到垃圾值），难以排查。

**修复方向**: 在 `input.rs` 的 `PointerEvent` 和 `KeyEvent` 定义后各加 `const _` ABI 断言，分别验证 16 字节和 8 字节。

**严重级别**: 中（防御性缺失，当前布局正确）

---

## 3. Trivial 模块文件确认

### 3.1 `loomgui_core/src/style/mod.rs`（8 行）

纯 re-export：声明 5 个子模块（`cascade` 条件编译在 `parse` feature 下）+ `pub use resolved::LocalTransform`。无额外逻辑。

### 3.2 `loomgui_core/src/parse/mod.rs`（3 行）

纯 re-export：`css`、`dom`、`selector` 三个子模块。无额外逻辑。

### 3.3 `loomgui_core/src/text/mod.rs`（5 行）

纯 re-export：`atlas`、`layout`、`rich` 三个子模块，含一行 doc comment。无额外逻辑。

### 3.4 `loomgui_core/src/scene/mod.rs`（11 行）

纯 re-export：`dynamic`、`node`、`transform` 子模块 + `build_scene`（条件编译在 `parse` feature 下）+ 重导出 `Node`、`NodeId`、`NodeKind`、`Rect`、`Scene`。无额外逻辑。

### 3.5 `loomgui_core/src/layout/mod.rs`（706 行）

**非 trivial**——这是 layout 模块的主实现文件。包含：
- `solve()` 函数（taffy 树构建、测量、回写）
- `MeasureContext` 枚举
- `ImageSizeTable` 类型别名
- 完整的测试套件（7 个测试）

不是纯 re-export，但不在本次审查范围（另一次审查已覆盖）。

---

## 总结

| 类别 | 数量 | 最高严重级别 |
|------|------|-------------|
| FFI 绑定布局错误 | 0 | - |
| 逻辑错误 | 0 | - |
| 防御性缺失 | 2 | 中（ABI 断言缺失） |
| 硬编码/可维护性 | 3 | 低 |
| Trivial 模块确认 | 4/5 确认 trivial | - |

**整体结论**: Showcase driver 代码质量高，值得作为参考实现。FFI 绑定三个 struct 内存布局全部正确（字段类型、顺序、对齐与 Rust `#[repr(C)]` 一致）。建议为 `PointerEvent` 和 `KeyEvent` 补充 Rust 侧 ABI 尺寸断言。
