# Phase1 Spec-P1(taffy 升级 + 真 block)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 LoomGUI 的 taffy 从 0.5 升到 0.12,并启用真 `Display::Block`(取代 flex 伪装 block),让骨架链布局符合标准 CSS block 语义。

**Architecture:** C1 = 版本号升级 + 两类机械替换(LengthPercentage 构造器、对齐常量)+ 2 处 match 改写,编译器驱动(改版本→按编译错替换→test 自验)+ pkg 格式 bump(v20→v21)。C2 = `mapping.rs` display 分支一行 `Display::Flex`→`Display::Block` + 不变量对齐 main-design,TDD(先写 block 验收 test 失败,再改让它过)。验收仿 spec4b headless(不碰 Unity)。

**Tech Stack:** Rust 2021、taffy 0.12、bincode、csbindgen、xtask;C# xUnit HeadlessTests(P/Invoke dll)。

## Global Constraints

(摘自 spec `2026-07-21-phase1-blitz-refactor-design.md` + CLAUDE.md,每个 task 隐含遵守)

- **本机是唯一编码机**:改 Rust 后必 `cargo build -p loomgui_ffi_c --release` + cp dll 到 `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll` + commit dll(Unity 必关着拷)。
- **改 parse-time 逻辑必重打 pkg**:`cargo run -p loomgui_pkg -- build <workspace>`。本 plan bump pkg 格式版本,所有工作区包重打。
- **pkg 格式一刀切**:`MIN_VERSION = MAX_VERSION`(无后向兼容,无迁移器)。
- **围栏手搓 CSS**:不引入 stylo/cssparser。
- **不动成熟资产**:渲染批合(`render/mesh`+`batch`)、滚动物理(`scroll.rs`)、文本自绘(`text/atlas`+`sdf`)、FFI ABI(enum 扩变体不影响 u32 NodeId ABI)。
- **两台机串行**:本机 headless 验为主;Unity 验收推后(当前无 Unity 环境)。
- **push 前**:`cargo fmt --all --check` + `cargo clippy --all-targets -D warnings` + `cargo test -p loomgui_fence`。
- **taffy 升级调研已 de-risk**:high-level API 100% 兼容、Style 字段名 100% 兼容;要改的只是 LengthPercentage 构造器(0.8)、对齐常量(0.11)、2 处 match。详见 spec §5.1。

---

## File Structure

| 文件 | 改动 | 责任 |
|---|---|---|
| `crates/core/Cargo.toml` / `ffi/Cargo.toml` / `fence/Cargo.toml` | taffy 版本号 0.5→0.12 | 依赖升级 |
| `crates/core/src/style/mapping.rs` | LengthPercentage 构造器 + 对齐常量机械替换;Task 5 改 display block 分支 | CSS→taffy 映射 |
| `crates/core/src/style/resolved.rs` | `as_corners` 闘包 match 改写 + 测试构造器替换 | ResolvedStyle(含 taffy_style)+ 圆角 |
| `crates/core/src/style/dynamic.rs` | 测试构造器替换 | 测试 |
| `crates/core/src/layout/mod.rs` | `fn lp` match 改写 | solve 入口 |
| `crates/fence/src/css_resolve.rs` | 对齐常量默认值替换 | 围栏默认样式 |
| `crates/core/src/asset/mod.rs` | `PKG_FORMAT_VERSION` 20→21 | pkg 格式版本 |
| `showcase/spec4b/p1-block-acceptance.html` | 新建 C2 验收页 | block 布局验收基准 |
| `tests/dotnet/LoomGUI.HeadlessTests/BlockLayoutTests.cs` | 新建 C2 验收 test | block 垂直堆叠断言 |

---

### Task 1: taffy 0.5→0.12 升级 + 机械替换(让 build+test 过)

**Files:**
- Modify: `crates/core/Cargo.toml:8`、`crates/ffi/Cargo.toml:12`、`crates/fence/Cargo.toml:8`
- Modify: `crates/core/src/style/{mapping,resolved,dynamic}.rs`、`crates/core/src/layout/mod.rs`、`crates/fence/src/css_resolve.rs`

**Interfaces:**
- Produces: taffy 0.12 编译通过的核心;`taffy::style::LengthPercentage` 构造器走关联函数(`::length/::percent`)、对齐走 SCREAMING_SNAKE 常量(`AlignItems::FLEX_START` 等)。

- [ ] **Step 1: 改 3 个 Cargo.toml 版本号**

`crates/core/Cargo.toml:8`:
```toml
taffy = { version = "0.12", features = ["serde"] }
```
`crates/ffi/Cargo.toml:12` 与 `crates/fence/Cargo.toml:8`:
```toml
taffy = "0.12"
```

- [ ] **Step 2: 跑 build 记录编译错(预期大量错,正常)**

Run: `cargo build -p loomgui_core 2>&1 | tee /tmp/taffy-upgrade-build.log`
Expected: FAIL —— 大量 `LengthPercentage::Length`/`AlignItems::FlexStart` 等"no variant"或"not found"错。这是 Task 1 后续 step 要逐类清零的清单。

- [ ] **Step 3: LengthPercentage / LengthPercentageAuto / Dimension 构造器替换**

按下表全仓替换(集中在 `mapping.rs`、`resolved.rs`、`dynamic.rs` 测试、`layout/mod.rs` 测试,约 40-50 处):

| 0.5(enum 变体) | 0.12(关联函数) |
|---|---|
| `LengthPercentage::Length(x)` | `LengthPercentage::length(x)` |
| `LengthPercentage::Percent(p)` | `LengthPercentage::percent(p)` |
| `LengthPercentageAuto::Length(x)` | `LengthPercentageAuto::length(x)` |
| `LengthPercentageAuto::Percent(p)` | `LengthPercentageAuto::percent(p)` |
| `LengthPercentageAuto::Auto` | `LengthPercentageAuto::auto()` |
| `Dimension::Length(x)` | `Dimension::length(x)` |
| `Dimension::Percent(p)` | `Dimension::percent(p)` |
| `Dimension::Auto` | `Dimension::auto()` |

**不要**替换 `match` 里的解构(`resolved.rs::as_corners`、`layout/mod.rs::lp`)——那是 Step 5 单独处理。

- [ ] **Step 4: 对齐常量替换(AlignItems / AlignContent / JustifyContent / AlignSelf)**

0.11 起这些从 enum 变体改成 struct 关联常量。全仓替换(集中在 `mapping.rs:1138-1157` 的 `parse_justify`/`parse_align`、`fence/src/css_resolve.rs`,约 20 处):

| 0.5(PascalCase 变体) | 0.12(SCREAMING_SNAKE 常量) |
|---|---|
| `::Center` | `::CENTER` |
| `::FlexStart` | `::FLEX_START` |
| `::FlexEnd` | `::FLEX_END` |
| `::Stretch` | `::STRETCH` |
| `::Baseline` | `::BASELINE` |
| `::SpaceBetween` | `::SPACE_BETWEEN` |
| `::SpaceAround` | `::SPACE_AROUND` |
| `::SpaceEvenly` | `::SPACE_EVENLY` |

注:`JustifyContent` 是 `AlignContent` 的 type alias、`AlignSelf` 是 `AlignItems` 的 alias(0.5/0.12 都是),写哪个都行。

- [ ] **Step 5: 2 处 match 改写(LengthPercentage 解构)**

0.12 的 `LengthPercentage` 是 `pub struct(CompactLength)`,内字段私有,不能 `match` 变体。改用 0.12 提供的解构 API。**实现时先查 taffy 0.12 `LengthPercentage` 文档找最简语义方法**(如 `try_length`/`length_or`/`into_raw`+`tag`);下面的 `into_raw()`+`tag()` 是调研得到的候选实现,以编译通过 + 单测绿为准。

`crates/core/src/style/resolved.rs` 的 `as_corners` 闭包(原 114-117):
```rust
// 原(0.5):
let r = |lp: LengthPercentage, side: f32| match lp {
    LengthPercentage::Length(v) => v,
    LengthPercentage::Percent(p) => side * p,
};
```
```rust
// 改(0.12,候选实现,以实际 API 为准):
let r = |lp: LengthPercentage, side: f32| {
    let cl = lp.into_raw();
    match cl.tag() {
        taffy::style::CompactLength::LENGTH_TAG => cl.value(),
        taffy::style::CompactLength::PERCENT_TAG => side * cl.value(),
        _ => 0.0,
    }
};
```

`crates/core/src/layout/mod.rs:52-57` 的 `fn lp`:
```rust
// 原(0.5):
fn lp(v: taffy::style::LengthPercentage) -> f32 {
    match v {
        taffy::style::LengthPercentage::Length(x) => x,
        _ => 0.0,
    }
}
```
```rust
// 改(0.12,候选实现,以实际 API 为准):
fn lp(v: taffy::style::LengthPercentage) -> f32 {
    let cl = v.into_raw();
    match cl.tag() {
        taffy::style::CompactLength::LENGTH_TAG => cl.value(),
        _ => 0.0,
    }
}
```

- [ ] **Step 6: build 通过**

Run: `cargo build -p loomgui_core`
Expected: PASS(0 error)。若仍有错,回到 Step 3-5 按编译错继续替换。

- [ ] **Step 7: 全 workspace build + test 绿**

Run: `cargo build --workspace && cargo test --workspace`
Expected: PASS。重点看 `style::resolved::tests::resolved_style_bincode_roundtrip_preserves_all_fields` 绿(它自验 taffy Style 字段完整性,是 0.12 字段没漏的守卫)。

- [ ] **Step 8: fmt + clippy 清**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings`
Expected: PASS(0 warning)。

- [ ] **Step 9: Commit(代码层升级完成;pkg bump 在 Task 2)**

```bash
git add crates/*/Cargo.toml crates/core/src crates/fence/src
git commit -m "refactor(layout): upgrade taffy 0.5->0.12 (mechanical: LengthPercentage ctors + align constants + 2 match rewrites)

high-level API + Style field names 100% compatible; only LengthPercentage constructors (0.8) + align keywords (0.11) + 2 match sites changed. pkg bump + rebuild in next task.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: pkg 格式 v20→v21 + 全量重打 + dll/exe 重编

**Files:**
- Modify: `crates/core/src/asset/mod.rs:20-22`

**Interfaces:**
- Produces: `PKG_FORMAT_VERSION = 21`(MIN=MAX=21);所有工作区 pkg.bin 重打为 v21。

- [ ] **Step 1: bump pkg 格式版本号**

`crates/core/src/asset/mod.rs:20-22`:
```rust
pub const PKG_FORMAT_VERSION: u32 = 21; // v21: taffy 0.12 (Style 字段数 + LengthPercentage/AlignItems wire 格式变)
pub(crate) const MIN_VERSION: u32 = 21;
pub(crate) const MAX_VERSION: u32 = 21;
```

- [ ] **Step 2: 重编 FFI dll + 拷贝(Unity 必关)**

Run:
```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```
Expected: dll 产出 + 拷贝成功(Unity 关着,dll 没被锁)。

- [ ] **Step 3: 同步 C# 绑定**

Run: `cargo run -p xtask -- sync-bindings`
Expected: `LoomGUIBindings.cs` 同步到 `unity/package/Plugins/LoomGUI/Bindings/`。

- [ ] **Step 4: 重打 showcase 8 页 + 所有工作区包**

Run: `cargo run -p loomgui_pkg -- build showcase`
Expected: exit 0,8 个 v21 pkg.bin 产出(showcase-package-unblock 既有的打包链,现在产 v21)。

- [ ] **Step 5: 重打 HeadlessTests fixture(若 fixture pkg 在 csproj copy)**

检查 `tests/dotnet/LoomGUI.HeadlessTests/*.csproj` 的 `<None CopyToOutputDirectory>` fixture 引用,若 fixture pkg.bin 是预打产物,用新打包器重打该 fixture 并替换。Run HeadlessTests 确认可加载:
Run: `dotnet test tests/dotnet/LoomGUI.HeadlessTests/LoomGUI.HeadlessTests.csproj`
Expected: PASS(现有 spec4a AcceptanceGateTests 9 条绿,证明 v21 pkg 可加载)。

- [ ] **Step 6: 重编 GUI exe + 拷贝(围栏改了,GUI 静态链入 fence)**

Run:
```bash
(cd crates/packer/gui/src-tauri && tauri build --no-bundle)
cp crates/packer/gui/src-tauri/target/release/loomgui_gui.exe unity/package/Editor/Tools/loomgui_gui.exe
```
Expected: exe 重出 + 拷贝。

- [ ] **Step 7: Commit(dll + exe + pkg bump 入库)**

```bash
git add crates/core/src/asset/mod.rs unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/Bindings/ unity/package/Editor/Tools/loomgui_gui.exe
git commit -m "chore(ffi): pkg v20->v21 for taffy 0.12 wire format + rebuild dll/exe

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: C1 验收(全量 test + showcase 打包)

**Files:** 无(验收 task)

- [ ] **Step 1: 全 workspace test 绿**

Run: `cargo test --workspace`
Expected: PASS(含 loomgui_core / loomgui_fence / loomgui_ffi_c / loomgui_pkg 全绿)。

- [ ] **Step 2: showcase 打包 exit 0**

Run: `cargo run -p loomgui_pkg -- build showcase; echo "exit=$?"`
Expected: `exit=0`(8 个 v21 pkg.bin)。

- [ ] **Step 3: fmt/clippy 门**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings`
Expected: PASS。

- [ ] **Step 4: C1 完成标记**

C1(taffy 升级)到此完成——build/test/打包全绿,dll/exe/pkg 入库。无新 commit(验收 task)。

---

### Task 4: C2 验收 HTML + HeadlessTests(failing test)

**Files:**
- Create: `showcase/spec4b/p1-block-acceptance.html`
- Create: `tests/dotnet/LoomGUI.HeadlessTests/BlockLayoutTests.cs`

**Interfaces:**
- Consumes: `StageHarness.Create/Destroy`、`UIContext.LoadPackage/Instantiate`、`Container.Geometry.LayoutRect`(均 spec4a 既有)。
- Produces: 一个 failing test,证明当前"伪 block"(block→flex row)下,显式 `display:block` 的 div 子元素**水平**排列(非标准 block 的垂直堆叠)。

- [ ] **Step 1: 写 C2 验收 HTML**

`showcase/spec4b/p1-block-acceptance.html`:
```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>P1 Block Acceptance</title>
  <style>
    .root { width: 400px; height: 300px; background-color: #111; }
    /* 显式 display:block 的容器,两个固定尺寸子 div。
       标准 CSS block:子 div 垂直堆叠(c2.y > c1.y + c1.h)。
       伪 block(flex row):子 div 水平排列(c2.y == c1.y)。 */
    .stack { display: block; }
    .item { width: 100px; height: 40px; background-color: #e94560; }
  </style>
</head>
<body>
  <div class="root">
    <div class="stack" id="stack">
      <div class="item" id="c1"></div>
      <div class="item" id="c2"></div>
    </div>
  </div>
</body>
</html>
```

- [ ] **Step 2: 把验收 HTML 打成 fixture pkg**

按 spec4a 既有 fixture 机制(参考 `tests/dotnet/LoomGUI.HeadlessTests/` 现有 `test.pkg.bin` 的产出方式:workspace 内 html → `loom-pkg build` → pkg.bin → csproj `<None CopyToOutputDirectory>` 到 fixtures/)。把 `p1-block-acceptance.html` 打成 `p1-block.pkg.bin` 并配 csproj copy。

- [ ] **Step 3: 写 failing HeadlessTest**

`tests/dotnet/LoomGUI.HeadlessTests/BlockLayoutTests.cs`(仿 `AcceptanceGateTests.cs` 的 InstantiateFixture 模式):
```csharp
using LoomGUI.Bindings;
using Xunit;

namespace LoomGUI.HeadlessTests
{
    public unsafe class BlockLayoutTests
    {
        [Fact]
        public void BlockChildrenStackVertically()
        {
            var (stage, ctx) = StageHarness.Create();
            try
            {
                StageHandle* h = (StageHandle*)stage.ToPointer();
                // 加载 p1-block.pkg.bin(仿 AcceptanceGateTests.InstantiateFixture)
                Container root = InstantiateBlockFixture(h, ctx);
                Container stack = root.Get<Container>("stack");
                Container c1 = stack.Get<Container>("c1");
                Container c2 = stack.Get<Container>("c2");

                Native.loomgui_stage_tick(h, 0.016f);

                // 标准 block:c2 在 c1 下方(c2.y >= c1.y + c1.h - 1px 容差)
                float c1Bottom = c1.Geometry.LayoutRect.Y + c1.Geometry.LayoutRect.Height;
                Assert.True(c2.Geometry.LayoutRect.Y >= c1Bottom - 1.0f,
                    $"block: c2.y ({c2.Geometry.LayoutRect.Y}) should be >= c1 bottom ({c1Bottom}); " +
                    $"got pseudo-block flex-row stacking");
            }
            finally { StageHarness.Destroy(stage); }
        }

        // InstantiateBlockFixture:仿 AcceptanceGateTests.InstantiateFixture,
        // 加载 fixtures/p1-block.pkg.bin,Instantiate("p1-block-acceptance")。
        // (复制 InstantiateFixture 主体,改 fixture 文件名 + template 名)
        private static Container InstantiateBlockFixture(StageHandle* h, UIContext ctx) { /* ... */ }
    }
}
```

- [ ] **Step 4: 跑 test 确认 fail**

Run: `dotnet test tests/dotnet/LoomGUI.HeadlessTests --filter BlockChildrenStackVertically`
Expected: FAIL —— c2.y < c1Bottom(伪 block = flex row,c2 与 c1 水平排列,y 相同)。这正是 C2 要修的。

- [ ] **Step 5: Commit(failing test 入库)**

```bash
git add showcase/spec4b/p1-block-acceptance.html tests/dotnet/LoomGUI.HeadlessTests/BlockLayoutTests.cs
git commit -m "test(block): add failing block vertical-stack acceptance (pseudo-block flex-row)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: 启用真 block(mapping.rs display 分支 + 不变量)

**Files:**
- Modify: `crates/core/src/style/mapping.rs:672-676`

**Interfaces:**
- Consumes: Task 4 的 failing test。
- Produces: `display:block` → `taffy::Display::Block`(标准 CSS 块流);Task 4 test 转 pass。

- [ ] **Step 1: 改 display block 分支**

`crates/core/src/style/mapping.rs:672-676`,原:
```rust
"block" => {
    // block：taffy 仍 Flex（守铁律），仅旁路字段标记。
    ts.display = taffy::Display::Flex;
    style.display_mode = DisplayMode::Block;
}
```
改为:
```rust
"block" => {
    // 真 block:taffy 0.12 block_layout,标准 CSS 块流(垂直堆叠)。
    ts.display = taffy::Display::Block;
    style.display_mode = DisplayMode::Block;
}
```

- [ ] **Step 2: 跑 Task 4 test 确认 pass**

Run: `dotnet test tests/dotnet/LoomGUI.HeadlessTests --filter BlockChildrenStackVertically`
Expected: PASS —— c2.y >= c1Bottom(block 垂直堆叠)。

- [ ] **Step 3: 确认 fence 默认 display 对齐 main-design §3.1**

main-design §3.1 规定 `div/header/nav/p/ul/ol/li/option` 默认 `display:block`。查 `crates/fence/src/css_resolve.rs` 给这些元素的默认 display:若 fence 给 div 默认 flex(旧范式残留"div 永远 flex column"),改为默认 block(或在 mapping/fence 默认值处对齐)。grep `css_resolve.rs` 的 display/flex_direction 默认值,核实每个围栏 block 元素默认走 block。

- [ ] **Step 4: 全 workspace test 绿(回归)**

Run: `cargo test --workspace && dotnet test tests/dotnet/LoomGUI.HeadlessTests`
Expected: PASS。注意:现有 spec4a `AcceptanceGateTests` 用的是显式 `display:flex`(.container{display:flex;flex-direction:column}),不受 C2 影响;若有 test 依赖"裸 div = flex"会红,按"裸 div 应 block"修正该 test(那是旧范式假设)。

- [ ] **Step 5: 重编 dll(运行时行为变)+ 重打受影响 pkg**

Run:
```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
```
(C2 是纯 runtime 改,不动 pkg 格式,无需 bump 版本;但 display 默认若改了 fence 打包期默认值,需重打 pkg:`cargo run -p loomgui_pkg -- build showcase`。)

- [ ] **Step 6: 更新不变量文档(CLAUDE.md + main-design)**

`CLAUDE.md` 旧范式条目「`<div>` 永远是 flex 容器」/「`display:block` 映射到 `taffy::Display::Flex`」标注为已消除(C2 完成)。核实 `docs/design/main-design.md` §3.1 / §11 的 block 布局描述与实现一致(若 main-design 仍说"deferred",更新为"已实现")。

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/style/mapping.rs crates/fence/src/css_resolve.rs CLAUDE.md docs/design/main-design.md unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll
git commit -m "feat(layout): enable real CSS block layout (Display::Block) replacing flex pseudo-block

div defaults to block (standard CSS); display:flex still available. flex->block changes all block div layout (vertical stack). CLAUDE.md/main-design invariants updated.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: C2 终验(rect-diff block 对齐浏览器)

**Files:** 无(验收 task)

- [ ] **Step 1: 用 rect-diff 跑 p1-block-acceptance.html 对比**

用 `showcase/scripts/rect-diff/` 既有设施(Playwright headless Chrome DOM rect vs LoomGUI rect),对 `p1-block-acceptance.html` 跑 rect diff。
Run: 按 `showcase/scripts/rect-diff/` 既有脚本/README 跑(参考 `snapshot-2026-07-21.md` 的用法)。
Expected: LoomGUI 的 c1/c2 rect 与 Chrome 的 rect 对齐(y 坐标一致 = block 垂直堆叠对齐)。

- [ ] **Step 2: 浏览器人工对比(无 Unity,退而求其次)**

浏览器打开 `p1-block-acceptance.html`,确认两个 item 垂直堆叠(标准 block)。LoomGUI headless dump(spec4b_dump 或 dump_*.rs)喂同一 pkg,确认 rect 与浏览器一致。

- [ ] **Step 3: C2 完成标记**

C2(真 block)到此完成——block 垂直堆叠对齐浏览器 + 不变量文档更新。P1(C1+C2)整体完成。

---

## Self-Review

**Spec coverage:**
- C1(taffy 升级)= Task 1-3 ✅
- C2(真 block)= Task 4-6 ✅
- 验收(仿 spec4b headless)= Task 3/4/6 ✅
- pkg bump = Task 2 ✅
- 不变量更新 = Task 5 Step 6 ✅

**Placeholder scan:** Task 4 Step 2(fixture 打包)引用 spec4a 既有机制(项目既有,非 plan 内 task);Task 4 Step 3 `InstantiateBlockFixture` 主体标"仿 InstantiateFixture"(给了指向 + 改动点,实现者照现有 30 行 helper 复制改文件名);Task 1 Step 5 的 0.12 match 解构给候选实现 + 标"以实际 API 为准"(机械替换靠编译错驱动,合理)。无 "TBD/TODO/适当处理"。

**Type consistency:** `PKG_FORMAT_VERSION` v20→v21(Task 2)+ `Display::Block`(Task 5)+ `BlockChildrenStackVertically`(Task 4/5 跨 task 名一致)✅。

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-21-phase1-p1-taffy-block.md`. Two execution options:

1. **Subagent-Driven (recommended)** — 每个 task 派 fresh subagent + 两阶段 review,迭代快。
2. **Inline Execution** — 本 session 内按 executing-plans 批量执行 + checkpoint。

选哪个?
