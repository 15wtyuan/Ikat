# Spec-4b P2：多引擎后端分层 + LoomStage 退役 + deferred ①② Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** LoomStage 旧命令式门面退役，引擎后端编排按多引擎接缝重构为 `LoomHost`（引擎无关驱动核心）+ `LoomBackend`（抽象契约）+ `UnityLoomBackend`（Unity 实现），Driver 瘦身为生命周期宿主；顺手修 deferred ①② 两个 core bug。

**Architecture:** 引擎无关层（Public/ + Projection/ + 新 Host/）+ Unity 特定后端（UnityLoomBackend）。LoomHost 持 stage handle + UIContext + LoomBackend + EventDemuxer，跑每帧驱动序（CollectInput → tick → borrow_frame→SyncFrame → borrow_events→Pump）。borrow_frame FFI 在 LoomHost，backend 只消费 blob。NativeHost 不进通用契约（UnityLoomBackend 额外方法）。headless 不动（继续直驱 tick，4a 模式）。

**Tech Stack:** C#（Unity Runtime，csbindgen FFI）+ Rust core（deferred ①② 修复）。

**对照 spec：** `docs/superpowers/specs/2026-07-18-spec4b-unity-acceptance-and-backend-retirement-design.md` §3 / §6.1 deferred ①②。

**前置：** P1 plan（清理）已完成——LoomStage 仅剩后端编排方法（业务 API 已删），LoomEventHandler 已删，pkg v19，dll/bindings synced。

## Global Constraints

- LoomHost/LoomBackend **零 `using UnityEngine`**（放 `Runtime/Host/`，引擎无关层）。编译门断言。
- UnityLoomBackend/LoomStageDriver 在 Unity 特定层（可 using UnityEngine）。
- 改 Rust FFI（deferred ②）后重编 dll + sync bindings + 拷贝（Unity 关着）。
- **保留复用**：MirrorPool/MaterialManager/NativeHostManager/SpriteResolver/InputCollector 零改，只从 LoomStage 搬到 UnityLoomBackend。
- 用户只读中文；代码/commit 英文。
- 每个 task 末尾 fmt/clippy（Rust）/ 编译（C#）清。

---

## Task 1: 建 LoomBackend 抽象基类（引擎无关）

**Files:**
- Create: `unity/package/Runtime/Host/LoomBackend.cs`

**Interfaces:**
- Produces: `LoomBackend` 抽象基类（CollectInput/SyncFrame）—— Task 2 UnityLoomBackend 实现，Task 3 LoomHost 持有。

> spec §3.2 签名精化：`SyncFrame` 含 `stage`（SyncFontAtlas 拉 atlas FFI + NativeHostManager.Sync 都用 stage），不止 frame blob。

- [ ] **Step 1: 建目录 + 文件**

`unity/package/Runtime/Host/LoomBackend.cs`：
```csharp
namespace LoomGUI
{
    /// <summary>
    /// 引擎后端契约（main-design §17：消费 RenderNode + 输入注入 + 资源）。
    /// 引擎无关抽象——Unity 实现 <see cref="UnityLoomBackend"/>；Godot-C# 未来实现 GodotLoomBackend。
    /// 零 UnityEngine（放 Runtime/Host/）。<see cref="LoomHost"/> 持具体实现并驱动每帧序。
    /// </summary>
    public abstract class LoomBackend
    {
        /// <summary>
        /// 采集引擎输入（Unity: InputCollector）+ 调 set_input FFI。
        /// set_input FFI 引擎中立（数据是 PointerEvent struct），backend 采集后直接调，不破坏引擎无关性。
        /// </summary>
        public abstract void CollectInput(IntPtr stage);

        /// <summary>
        /// 消费 borrow_frame 的 blob 做镜像渲染——不调 borrow FFI（<see cref="LoomHost"/> 已 borrow，
        /// 把 stage + ptr + len 传进来）。Unity: MirrorPool.Sync + SyncFontAtlas + NativeHostManager.Sync。
        /// </summary>
        public abstract void SyncFrame(IntPtr stage, IntPtr framePtr, int frameLen);
    }
}
```

- [ ] **Step 2: 加 asmdef 引用确认**

确认 `Runtime/Host/` 下的代码能被 LoomGUI asmdef 编译（Host/ 是文件夹不是独立 asmdef，随 LoomGUI.Runtime asmdef 编译）。若项目用 asmdef 按文件夹划分，确认 Host/ 归 LoomGUI.Runtime。

- [ ] **Step 3: 编译验证**

Run: `cd tests/dotnet && dotnet build`（或 Unity 对应编译）
Expected: 编译通过（抽象类无实现，暂无 caller）。

- [ ] **Step 4: 零 UnityEngine 断言**

Run: `grep -c "using UnityEngine" unity/package/Runtime/Host/LoomBackend.cs`
Expected: `0`。

- [ ] **Step 5: Commit**

```bash
git add unity/package/Runtime/Host/LoomBackend.cs
git commit -m "feat(host): add LoomBackend abstract contract (engine-agnostic backend, main-design §17)"
```

---

## Task 2: 建 UnityLoomBackend（搬后端组件 + 实现契约）

**Files:**
- Create: `unity/package/Runtime/UnityLoomBackend.cs`
- Reference: `unity/package/Runtime/MirrorPool.cs` / `MaterialManager.cs` / `NativeHostManager.cs` / `SpriteResolver.cs` / `LoomInputCollector.cs`（零改复用）
- Reference: `unity/package/Runtime/LoomStage.cs:194-284`（Tick + SyncFontAtlas，搬家源）

**Interfaces:**
- Consumes: Task 1 `LoomBackend`。
- Produces: `UnityLoomBackend`（CollectInput/SyncFrame + SyncFontAtlas + NativeHost 绑定）—— Task 3 LoomHost 持有。

- [ ] **Step 1: 建 UnityLoomBackend，搬后端组件 + Tick/SyncFontAtlas 逻辑**

`unity/package/Runtime/UnityLoomBackend.cs`：
```csharp
using System;
using System.Runtime.InteropServices;
using UnityEngine;
using LoomGUI.Bindings;

namespace LoomGUI
{
    /// <summary>
    /// Unity 引擎后端实现：持 MirrorPool/MaterialManager/NativeHostManager/SpriteResolver/InputCollector
    /// （零改复用，从 LoomStage 搬来）。LoomHost 通过 LoomBackend 契约驱动。
    /// NativeHost（GameObject 绑定 3D 模型）是 Unity 专属，不进 LoomBackend 通用契约，作额外方法。
    /// </summary>
    public sealed unsafe class UnityLoomBackend : LoomBackend
    {
        readonly MirrorPool _pool = new();
        MaterialManager _mm;
        readonly NativeHostManager _nhm = new();
        public SpriteResolver _sprites;   // SpriteResolver 给 LoomHost InitSprites 资源注册用
        LoomInputCollector _inputCollector;
        Transform _renderRoot;
        byte[] _frameBuf;                 // 借自 LoomStage 的 frame buffer（ArrayPool 复用）

        public UnityLoomBackend(MaterialManager mm) { _mm = mm; }

        /// <summary>Driver Awake 注入：渲染根（MirrorPool/NativeHost 镜像 GO 挂此 root）+ 输入采集器。</summary>
        public void SetRuntimeRoot(Transform root, LoomInputCollector input)
        {
            _renderRoot = root;
            _inputCollector = input;
        }

        public NativeHostManager NativeHost => _nhm;

        // ── LoomBackend 契约 ──

        public override void CollectInput(IntPtr stage)
        {
            if (stage == IntPtr.Zero || _inputCollector == null) return;
            // LoomInputCollector 内部调 set_input/set_key_input/set_wheel_input FFI（引擎中立）。
            // designSize/useSafeArea 由 Driver 经 input collector 配置（保留旧 Driver 传参语义）。
            _inputCollector.Collect(stage, _inputCollector.DesignSize, _inputCollector.UseSafeArea);
            _inputCollector.CollectKeys(stage);
            LoomInputCollector.CollectWheelStagePtr(stage, _inputCollector);
        }

        public override void SyncFrame(IntPtr stage, IntPtr framePtr, int frameLen)
        {
            if (framePtr == IntPtr.Zero || frameLen <= 0 || _renderRoot == null) return;
            StageHandle* h = (StageHandle*)stage.ToPointer();

            // frame buffer（ArrayPool 复用，搬自 LoomStage.Tick）
            if (_frameBuf == null || _frameBuf.Length < frameLen)
            {
                if (_frameBuf != null) ArrayPool<byte>.Shared.Return(_frameBuf);
                _frameBuf = ArrayPool<byte>.Shared.Rent(frameLen);
            }
            Marshal.Copy(framePtr, _frameBuf, 0, frameLen);
            var blob = new FrameBlob(_frameBuf);

            SyncFontAtlas(h);
            _pool.Sync(blob, _renderRoot, _mm, _sprites, Texture2D.whiteTexture);
            _nhm.Sync(h);
        }

        // ── SyncFontAtlas（搬自 LoomStage.SyncFontAtlas，零改）──
        // ... 完整搬 LoomStage.cs:241-284 的 SyncFontAtlas（font atlas dirty pages 上传 + SpriteResolver 注册）
        //   注：原方法用 _stage（StageHandle*）+ _sprites，这里改 h + _sprites。逻辑不变。
        //   实现时整段拷贝 LoomStage.cs:241-284，把 _stage 换成 h。
    }
}
```

⚠️ `CollectWheel` 原签名是 `LoomInputCollector.CollectWheel(LoomStage stage)`（静态，读 stage.DesignSize/UseSafeArea）。搬去 backend 后改 `CollectWheelStagePtr(IntPtr stagePtr, LoomInputCollector ctx)`（用 ctx 读配置）——Task 6 Step 1 改 LoomInputCollector.CollectWheel 签名时做（LoomStage 退役 + Driver 切 LoomHost 那个 task）。

- [ ] **Step 2: 搬 SyncFontAtlas 完整逻辑**

从 `LoomStage.cs:241-284` 整段拷贝 SyncFontAtlas 到 UnityLoomBackend，把 `_stage`（StageHandle*）参数化（SyncFrame 已有 `h`），`_sprites` 用 backend 字段。逻辑零改。

- [ ] **Step 3: 编译验证**

Run: Unity 编译 / `dotnet build`
Expected: 编译通过（UnityLoomBackend 无 caller，但实现完整）。

- [ ] **Step 4: Commit**

```bash
git add unity/package/Runtime/UnityLoomBackend.cs
git commit -m "feat(unity): add UnityLoomBackend — move MirrorPool/MaterialManager/NativeHostManager/SpriteResolver/InputCollector from LoomStage"
```

---

## Task 3: 建 LoomHost（引擎无关驱动核心）

**Files:**
- Create: `unity/package/Runtime/Host/LoomHost.cs`
- Reference: `unity/package/Runtime/LoomStage.cs:194-230`（Tick 驱动序，搬家源）+ `Projection/EventDemuxer.cs`（4a）

**Interfaces:**
- Consumes: Task 1 LoomBackend + Task 2 UnityLoomBackend（运行时注入）+ EventDemuxer（4a）+ UIContext（4a `internal UIContext(IntPtr stage)`）。
- Produces: `LoomHost`（Step 驱动 + Context 暴露 + 资源 FFI + Dispose）—— Task 5 Driver 持有。

- [ ] **Step 1: 建 LoomHost**

`unity/package/Runtime/Host/LoomHost.cs`：
```csharp
using System;
using LoomGUI.Bindings;

namespace LoomGUI
{
    /// <summary>
    /// 引擎无关 stage 宿主 + 每帧驱动核心（零 UnityEngine，放 Runtime/Host/）。
    /// 持 stage handle + UIContext + LoomBackend + EventDemuxer。Driver 持本类，每帧调 <see cref="Step"/>。
    /// 资源 FFI（register_font/set_image_sizes）引擎中立，放此。borrow_frame FFI 在此（backend 只消费 blob）。
    /// Unity + Godot-C# 共享（Godot 写 GodotLoomBackend 注入）。
    /// </summary>
    public sealed unsafe class LoomHost : IDisposable
    {
        StageHandle* _stage;
        readonly UIContext _ctx;
        readonly LoomBackend _backend;
        readonly EventDemuxer _demuxer;
        public bool IsDisposed { get; private set; }

        public LoomHost(float designW, float designH, LoomBackend backend)
        {
            _stage = Native.loomgui_stage_new(designW, designH);
            if (_stage == null)
                throw new InvalidOperationException($"loomgui_stage_new({designW},{designH}) returned null");
            _ctx = new UIContext((IntPtr)_stage);
            _backend = backend;
            _demuxer = new EventDemuxer(_ctx);
        }

        public UIContext Context => _ctx;
        internal StageHandle* StagePtr => _stage;
        public LoomBackend Backend => _backend;

        /// <summary>每帧驱动序（main-design §16，严格时序）。</summary>
        public void Step(float dt)
        {
            if (_stage == null) return;
            // 1. 输入采集 → set_input（backend 调引擎中立 FFI）
            _backend.CollectInput((IntPtr)_stage);
            // 2. flush（4a 即时过桥 seam——setter 立即 set_inline_override；升级攒批时此处加帧末 Flush 在 tick 前）
            // 3. tick
            Native.loomgui_stage_tick(_stage, dt);
            // 4. borrow_frame → backend.SyncFrame（backend 不调 borrow FFI，只消费 blob）
            nuint lenRaw = 0;
            byte* ptr = Native.loomgui_stage_borrow_frame(_stage, &lenRaw);
            _backend.SyncFrame((IntPtr)_stage, (IntPtr)ptr, (int)lenRaw);
            // 5. borrow_events → typed On<T> 路由（EventDemuxer 4a）
            nuint evLen = 0;
            byte* evPtr = Native.loomgui_stage_borrow_events(_stage, &evLen);
            _demuxer.Pump((IntPtr)evPtr, (int)evLen);
        }

        // ── 资源 FFI（引擎中立，byte[]/描述过桥）──

        public void RegisterFont(string family, byte[] bytes, bool isDefault)
        {
            // 搬自 LoomStage.RegisterFont（LoomStage.cs:102-116），_stage 换字段。
            // ... 实现：byte[] family/bytes fixed → loomgui_stage_register_font FFI
        }

        public void SetFallbackFamilies(System.Collections.Generic.IEnumerable<string> families)
        {
            // 搬自 LoomStage.SetFallbackFamilies（LoomStage.cs:118-126）
        }

        public void SetImageSizes(string[] paths, uint[] ws, uint[] hs)
        {
            // 搬自 LoomStage.SetImageSizes（LoomStage.cs:147-178）
        }

        public void InitSprites(System.Collections.Generic.List<AtlasManifest> atlases, Func<string, Texture2D> loadPage)
        {
            // ⚠️ Texture2D 是 Unity 类型——此方法不该在引擎无关 LoomHost。
            // 移到 UnityLoomBackend.InitSprites（Unity 特定资源）。LoomHost 不暴露此方法。
            throw new NotSupportedException("InitSprites is Unity-specific; call UnityLoomBackend directly");
        }

        public void Dispose()
        {
            if (_stage != null)
            {
                Native.loomgui_stage_free(_stage);
                _stage = null;
            }
            IsDisposed = true;
        }
    }
}
```

⚠️ **InitSprites 是 Unity 特定**（Texture2D）——不能在引擎无关 LoomHost。移到 UnityLoomBackend.InitSprites（Task 2 补，或本 task 注明 LoomHost 不暴露）。LoomHost 只暴露引擎中立的资源 FFI（RegisterFont byte[]/SetImageSizes 元数据）。SpriteResolver 注册（Unity Texture2D）归 UnityLoomBackend。

- [ ] **Step 2: 搬资源 FFI 实现**

从 LoomStage.cs:102-178 搬 RegisterFont/SetFallbackFamilies/SetImageSizes 到 LoomHost（`_stage` 换字段）。InitSprites 不搬（Unity 特定，归 UnityLoomBackend，Task 2 Step 2 补一个 `UnityLoomBackend.InitSprites` 方法，搬 LoomStage.cs:137-146）。

- [ ] **Step 3: 补 UnityLoomBackend.InitSprites**

Task 2 的 UnityLoomBackend 加 `public void InitSprites(List<AtlasManifest> atlases, Func<string, Texture2D> loadPage)`（搬自 LoomStage.cs:137-146，初始化 SpriteResolver）。

- [ ] **Step 4: 零 UnityEngine 断言**

Run: `grep -c "using UnityEngine" unity/package/Runtime/Host/LoomHost.cs`
Expected: `0`（LoomHost 零 UnityEngine）。InitSprites 因 Texture2D 移到 UnityLoomBackend 正确。

- [ ] **Step 5: 编译验证**

Run: Unity 编译 / dotnet build
Expected: 通过（LoomHost + UnityLoomBackend 完整，但仍无 caller——Driver Task 5 接）。

- [ ] **Step 6: Commit**

```bash
git add unity/package/Runtime/Host/LoomHost.cs unity/package/Runtime/UnityLoomBackend.cs
git commit -m "feat(host): add LoomHost (engine-agnostic stage driver) + UnityLoomBackend.InitSprites"
```

---

## Task 4: deferred ① remove_child 直系子校验修复（Rust core）

**Files:**
- Modify: `crates/core/src/scene/dynamic.rs:231-238`
- Test: `crates/core/src/scene/dynamic.rs`（加非直系子 remove 测试）

**Interfaces:** 无（core 内部 bug 修复）。

> bug：原 `remove_child` 不校验 child 是否 parent 的直系子——`retain(|&c| c != child)` 对非直系子无效（child 不在 parent.children），但仍执行 `child.parent = None`，误断 child 与其真实 parent 的关系。4a 加了 C# GetChildIndex 守卫（public-api 侧），本 task 修 core 根因。

- [ ] **Step 1: 写失败测试（非直系子 remove 应报错，不误清 parent）**

`crates/core/src/scene/dynamic.rs` 测试模块加：
```rust
#[test]
fn remove_child_rejects_non_direct_child() {
    let mut scene = Scene::default();
    let root = scene.create_root(NodeKind::Container, "").unwrap();
    let a = scene.create_node(NodeKind::Container, "").unwrap();
    let b = scene.create_node(NodeKind::Container, "").unwrap();
    append_child(&mut scene, root, a).unwrap();
    append_child(&mut scene, root, b).unwrap();
    // b 的 parent 是 root，不是 a → remove_child(a, b) 应 Err，且 b.parent 仍是 root。
    let err = remove_child(&mut scene, a, b);
    assert!(err.is_err(), "remove_child on non-direct child must error");
    assert_eq!(scene.get(b).unwrap().parent, Some(root), "b.parent must stay root");
    assert!(scene.get(a).unwrap().children.contains(&b) == false, "a.children unchanged");
    assert!(scene.get(root).unwrap().children.contains(&b), "root.children still has b");
}
```

- [ ] **Step 2: 跑测试确认 fail**

Run: `cargo test -p loomgui_core remove_child_rejects_non_direct_child`
Expected: FAIL（原实现 retain 无效但仍清 child.parent，且不报错——`assert!(err.is_err())` 失败）。

- [ ] **Step 3: 修 remove_child（加直系子校验）**

改 `crates/core/src/scene/dynamic.rs:231`：
```rust
pub fn remove_child(scene: &mut Scene, parent: NodeId, child: NodeId) -> Result<(), String> {
    // 直系子校验：child 的真实 parent 必须是传入的 parent。
    // 原实现无校验，retain 对非直系子无效但仍把 child.parent 清成 None → 误断真实父子关系。
    let actual_parent = scene.get(child).and_then(|c| c.parent);
    if actual_parent != Some(parent) {
        return Err("remove_child: child is not a direct child of parent".into());
    }
    let p = scene.get_mut(parent).ok_or("parent not live")?;
    p.children.retain(|&c| c != child);
    scene.get_mut(child).unwrap().parent = None;
    Ok(())
}
```

- [ ] **Step 4: 跑测试确认 pass + 全 core 测不回归**

Run: `cargo test -p loomgui_core`
Expected: 绿（新测试 pass + 原 remove_child 测试不回归——直系子 remove 仍正常）。

- [ ] **Step 5: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/scene/dynamic.rs
git commit -m "fix(core): remove_child validates direct-child (was silently clearing non-direct child's parent)"
```

---

## Task 5: deferred ② FFI null css check 修复（Rust FFI）

**Files:**
- Modify: `crates/ffi/src/lib.rs` — `loomgui_stage_create_node` + `loomgui_stage_create_root` + `loomgui_stage_set_text`（凡接 `css: *const u8` 的 FFI）
- Test: `crates/ffi/src/tests.rs` 或 `abi_tests.rs`

**Interfaces:** 无（FFI 防御性 null check）。

> bug：FFI 接 `css: *const u8, css_len: usize`，caller 传 null（C# `null` 或默认）时 `slice::from_raw_parts(null, 0)` 是 UB（null 指针不能 from_raw_parts，即使 len=0）。

- [ ] **Step 1: grep 所有 css:*const u8 FFI**

Run: `grep -n "css: \*const u8\|css_len: usize" crates/ffi/src/lib.rs`
Expected: 命中 create_node / create_root / set_text / set_inline_override（?）等。记录全部。

- [ ] **Step 2: 写失败测试（null css 不 UB，返空串语义）**

`crates/ffi/src/tests.rs` 加（以 create_node 为例）：
```rust
#[test]
fn create_node_null_css_does_not_ub() {
    let mut stage = test_stage();
    let root = loomgui_stage_create_root(stage_handle(&mut stage), c"div".as_ptr(), 3, std::ptr::null(), 0);
    assert_ne!(root, 0xFFFF_FFFF, "create_root with null css must succeed (empty css)");
    // 不 UB（nullptr from_raw_parts 是 UB，本测试跑过即证明 null 被守卫）
}
```

- [ ] **Step 3: 跑测试确认 fail（或 UB crash）**

Run: `cargo test -p loomgui_ffi_c create_node_null_css`
Expected: FAIL 或 crash（null from_raw_parts UB）。

- [ ] **Step 4: 加 null check 到所有 css FFI**

每个接 `css: *const u8` 的 FFI，把 css 转 str 的逻辑改为 null-safe 模式：
```rust
let css_str: &str = if css.is_null() || css_len == 0 {
    ""
} else {
    match std::str::from_utf8(unsafe { std::slice::from_raw_parts(css, css_len) }) {
        Ok(s) => s,
        Err(_) => return /* 该 FFI 的错误码 */,
    }
};
```
应用到 create_node / create_root / set_text / set_inline_override / 等所有命中 FFI。

- [ ] **Step 5: 跑测试确认 pass + 全 FFI 测不回归**

Run: `cargo test -p loomgui_ffi_c && cargo test -p loomgui_core`
Expected: 绿。

- [ ] **Step 6: fmt + clippy**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`

- [ ] **Step 7: Commit**

```bash
git add crates/ffi/src/lib.rs
git commit -m "fix(ffi): null-safe css pointer in create_node/create_root/set_text (was from_raw_parts(null,0) UB)"
```

---

## Task 6: LoomStage 退役 + LoomStageDriver 切 LoomHost

> 核心搬家已完成（Task 2/3）。本 task 把 Driver 从 `LoomStage` 切到 `LoomHost + UnityLoomBackend`，然后删 LoomStage.cs。

**Files:**
- Modify: `unity/package/Runtime/LoomStageDriver.cs` — Awake 建 LoomHost+UnityLoomBackend，Update 调 host.Step，资源经 host/backend
- Delete: `unity/package/Runtime/LoomStage.cs`（整文件）
- Modify: `unity/package/Runtime/LoomInputCollector.cs` — `CollectWheel` 签名改接 IntPtr（不再依赖 LoomStage）

**Interfaces:**
- Consumes: Task 2 UnityLoomBackend + Task 3 LoomHost。
- Produces: LoomStage 类消失；Driver 持 LoomHost，业务从 `driver.Context`（UIContext）接入。

- [ ] **Step 1: 改 LoomInputCollector.CollectWheel 接 IntPtr**

`LoomInputCollector.cs` 的 `CollectWheel(LoomStage stage)` 改为 `CollectWheel(IntPtr stagePtr, LoomInputCollector ctx)`（用 ctx 读 DesignSize/UseSafeArea）。caller（UnityLoomBackend.CollectInput + LoomStageDriver）跟改。

- [ ] **Step 2: 改 LoomStageDriver.Awake 建 LoomHost + UnityLoomBackend**

`LoomStageDriver.cs:164` 的 `_stage = new LoomStage(_designSize)` 改为：
```csharp
var backend = new UnityLoomBackend(_mm);  // _mm = MaterialManager（shader 加载保留）
_host = new LoomHost(_designSize.x, _designSize.y, backend);
backend.SetRuntimeRoot(transform, _inputCollector);
backend.NativeHost.SetRoot(transform);  // NativeHostManager 根注入（原 _stage.SetNativeHostRoot）
```
字段 `_stage`（LoomStage）→ `_host`（LoomHost）。暴露 `public UIContext Context => _host.Context;`（替代旧 `Stage`）。

- [ ] **Step 3: 改 Awake 资源加载走 LoomHost + UnityLoomBackend**

Awake 的 bootstrap（LoomStageDriver.cs:171-232）：
- `_stage.LoadPackage` → `_host.Context.LoadPackage`（UIContext API，4a 已有）
- `_stage.SetImageSizes` → `_host.SetImageSizes`
- `_stage.InitSprites` → `backend.InitSprites`（Unity 特定）
- `_stage.RegisterFont` / `SetFallbackFamilies` → `_host.RegisterFont` / `SetFallbackFamilies`

- [ ] **Step 4: 改 LoomStageDriver.LateUpdate 调 host.Step**

`LoomStageDriver.cs:286-308` LateUpdate：输入采集移入 `host.Step`（backend.CollectInput），tick+borrow+events 在 host.Step。改为：
```csharp
void LateUpdate()
{
    if (_host == null) return;
    // resize 检测（保留）
    if (Screen.width != _lastScreenW || Screen.height != _lastScreenH) { ... ConfigureTransforms(); }
    _host.Step(Time.unscaledDeltaTime);
}
```
（输入采集不再在 Driver 直调 InputCollector——host.Step 内 backend.CollectInput 做。）

- [ ] **Step 5: 改 OnDestroy Dispose**

`_stage.Dispose()` → `_host.Dispose()`。

- [ ] **Step 6: 删 LoomStage.cs**

Run: `git rm unity/package/Runtime/LoomStage.cs unity/package/Runtime/LoomStage.cs.meta`
（确认无残留引用：`grep -rn "LoomStage\b" unity/package/Runtime/ --include="*.cs"` 应空，除可能的注释）

- [ ] **Step 7: 编译验证**

Run: Unity 编译 / dotnet build
Expected: 编译通过（LoomStage 消失，Driver 经 LoomHost）。

- [ ] **Step 8: Headless 测不回归**

Run: `cd tests/dotnet && dotnet test`
Expected: 300 HeadlessTests 绿（headless 直驱 stage FFI，不经 LoomStage/LoomHost——LoomStage 删不影响）。

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "refactor(unity): retire LoomStage — Driver uses LoomHost+UnityLoomBackend, drop LoomStage.cs"
```

---

## Task 7: 【Rust gate】重编 dll + sync bindings（deferred ② 改了 FFI 防御，但签名没变——确认同步）

> deferred ② 改了 FFI 内部（null check），但 FFI 签名（参数/返回）没变 → bindings 不需重新生成。但 core 改了（deferred ① remove_child）→ dll 要重编（core 内嵌）。确认 dll/md5 同步。

**Files:**
- Regenerate: `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`

- [ ] **Step 1: 确认 Unity 关着**

- [ ] **Step 2: 重编 dll**

Run: `cargo build -p loomgui_ffi_c --release && cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`

- [ ] **Step 3: dll md5 一致**

Run: `md5sum target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`
Expected: 相等。

- [ ] **Step 4: 全 Rust 测**

Run: `cargo test`
Expected: 全绿。

- [ ] **Step 5: Commit**

```bash
git add unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
git commit -m "chore(ffi): rebuild dll (core remove_child fix + ffi null-css guard)"
```

---

## Task 8: 【P2 gate】全量验证

- [ ] **Step 1: Rust 全测 + fmt + clippy + feature-gate**

Run: `cargo test && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo clippy --all-targets --no-default-features -D warnings`
Expected: 全绿/清。

- [ ] **Step 2: C# Headless + PublicApi 编译门**

Run: `cd tests/dotnet && dotnet test && dotnet build LoomGUI.PublicApi`
Expected: 300 HeadlessTests 绿 + PublicApi 编译。

- [ ] **Step 3: 零 UnityEngine 断言（引擎无关层）**

Run: `grep -L "using UnityEngine" unity/package/Runtime/Host/LoomHost.cs unity/package/Runtime/Host/LoomBackend.cs && echo "Host/ clean"`
Expected: 两个文件都不含 using UnityEngine。

- [ ] **Step 4: LoomStage 残留扫零**

Run: `grep -rn "LoomStage" unity/package/Runtime/ --include="*.cs" | grep -v "//\|///"`
Expected: 空（LoomStage 类已删，无引用）。

- [ ] **Step 5: dll md5 synced**

Run: `md5sum target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`
Expected: 相等。

- [ ] **Step 6: deferred ①② 验证**

Run: `cargo test -p loomgui_core remove_child_rejects_non_direct_child && cargo test -p loomgui_ffi_c null_css`
Expected: 两测试 pass。

- [ ] **Step 7: P2 完成 commit**

```bash
git add -A && git commit -m "test: P2 gate green — LoomHost layering + LoomStage retired + deferred ①② fixed" --allow-empty
```

---

## P2 完成标准

- ✅ LoomHost（引擎无关）+ LoomBackend（契约）+ UnityLoomBackend（Unity 实现）就位，零 UnityEngine 断言过
- ✅ LoomStage 类删除，Driver 经 LoomHost + ctx.LoadPackage
- ✅ deferred ① remove_child 直系子校验 + ② FFI null css check
- ✅ cargo test + headless + PublicApi 全绿，dll synced
- ✅ MirrorPool/MaterialManager/NativeHostManager/SpriteResolver/InputCollector 零改复用

**下一棒**：P3（Unity 真机验收：手写最小页 + pkg v19 + PlayMode 4 条 done 判据）——待写 P3 plan。
