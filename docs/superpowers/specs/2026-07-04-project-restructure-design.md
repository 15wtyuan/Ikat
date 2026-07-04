# 项目整顿与开源治理 — 设计

> 日期：2026-07-04 · 状态：待审阅 · 类型：伞形 spec（覆盖 4 个子项目）
>
> 决策方式：本 spec 替代独立实施计划。审阅通过后按「执行顺序与验证门」直接开干，每子项目独立 commit。

## 背景（审计结论浓缩）

仓库已公开（`github.com/15wtyuan/LoomGUI`，416 个追踪文件）。从「受欢迎的开源产品」视角，四个独立短板：

| 短板 | 证据 |
|---|---|
| Unity 工程 = 插件 + 演示 + 设计区 + 模板杂物混作一团 | `loomgui_unity/Assets/` 同时装着插件代码 `LoomGUI/`（Runtime/Editor/Tests/Bindings 四个 asmdef 已就位）、演示源 `LoomUI/`、Unity 模板残留（`TutorialInfo/`、`Readme.asset`、`InputSystem_Actions.inputactions` 均入库）。插件无法单独分发。 |
| Rust 几个超大文件 | `input.rs 2449` / `stage.rs 1761` / `ffi lib.rs 1625` / `scroll.rs 1385` / `render/mod.rs 1286` / `blob.rs 1187` / `asset/mod.rs 1075` / `mapping.rs 1000`。core 已部分模块化（parse/style/render/scene 子目录），这几个扁平大文件是拖后腿的。其中大量行数是**内嵌 `mod tests`**——stage.rs 的 1761 行里约 1000 行是测试。 |
| 无 CI / 无发布流水线 | 无 `.github/`。push 不跑测试、不跑 fmt/clippy、PR 无门禁。两机工作流（公司编 .dll、家里验收）全靠人肉。 |
| OSS 治理缺口（公开仓库却无 LICENSE） | 无 LICENSE = 法律上 all-rights-reserved，「开源」名不副实。无 CONTRIBUTING / CHANGELOG / ISSUE_TEMPLATE。README 无徽章。 |

**关于「合并三个 Rust crate」的判断：不合并。** core（纯库 + feature gate）、ffi_c（cdylib C ABI）、pkg（CLI binary）编译目标不同，合并会绑死编译配置、丢掉 feature 隔离（运行时不带 scraper/cssparser 全靠这个边界）。痛的真根因是大文件，不是 crate 数量。

## 目标 / 非目标

**目标：**
- Unity 插件可作为 UPM 包单独分发；demo 工程引用它，改插件即时生效。
- Rust 大文件拆到可读规模（单文件目标 < ~800 行，纯测试除外）。
- push/PR 有 CI 门禁（Rust test + fmt + clippy + build）。
- 仓库具备开源产品的基本治理（LICENSE / CONTRIBUTING / CHANGELOG / README 徽章 / issue 模板）。

**非目标（本轮不做）：**
- Unity 测试进 CI（需 game-ci + license secret，后续单独加）。
- crates.io / Asset Store 发布（仅备好可发布的包结构）。
- 语义化版本自动化 / release 跑 changelog 生成。
- 拆 `.superpowers/sdd/` 过程产物（低优先，留待后续）。
- Rust crate 合并（见上）。

## 总体策略

伞形 spec，4 子项目各自独立 commit。按**风险升序**执行，每块之间设**验证门**：前一块通过才进下一块。这样即使最险的 Unity 拆分炸了，前三块已落袋为安、可单独 bisect/revert。

不另起 writing-plans 仪式——本 spec 的「执行顺序与验证门」一节即计划。

## ① OSS 治理（最低风险，纯新增文件）

**产出：**
- `LICENSE`：MIT。全文 + 版权行 `Copyright (c) 2026 15wtyuan`。
- `CONTRIBUTING.md`：开发命令（指向 README 的 build/test 段）、两机工作流一句、分支/commit 风格、PR 自查清单（`cargo test` 过 + Unity 无红）。
- `CHANGELOG.md`：Keep a Changelog 格式，`## [Unreleased]` 起步，把已交付的 v1 功能列一条概览（不回溯全部历史）。
- `.github/ISSUE_TEMPLATE/bug_report.md` + `feature_request.md`。
- `README.md` 增补：顶部徽章（CI status / license / Unity 6.5）；「快速上手」前补一行 install 概念；「项目结构」表更新（反映 ④ 的包拆分后，本节在 ④ 落地后改）。
- 清 demo 模板垃圾：删 `loomgui_unity/Assets/TutorialInfo/`、`Readme.asset`、`InputSystem_Actions.inputactions`（含 .meta）。确认 demo 不引用再删。

**决策记录：** MIT（Unity 插件圈惯例，匹配参考实现 FairyGUI-unity；Asset Store 友好）。不加 Code of Conduct（小项目、暂无贡献者，YAGNI，后续要加用 Contributor Covenant 2.1）。

## ② CI 工具链（GitHub Actions）

**OS matrix：Windows + Ubuntu**（公开仓库分钟数免费；Ubuntu 跑纯 Rust 的 core/pkg 几乎零成本抓 Unix-only bug；Windows 跑全量并产 .dll 产物）。macOS 暂缓——README 虽提 Mac Mono，但 Mac 字体/渲染在 CI 可能 flaky，待 Mac 成真目标再补。

**两个 workflow：**
- `.github/workflows/rust-ci.yml`（on push/PR）：
  - matrix `[windows-latest, ubuntu-latest]` × `toolchain stable`（暂不锁 msrv，后续定）。
  - steps：`cargo fmt --all -- --check`（门禁）→ `cargo clippy --all-targets -- -D warnings`（门禁；首次跑若历史告警多，先 `-W` 观察、下一轮再 `-D`）→ `cargo test --workspace` → `cargo build -p loomgui_ffi_c --release`（仅 Windows 产 .dll，用 actions/upload-artifact 挂 7 天供家里机取）。
  - 注意：core/pkg 的 snapshot 测试带 `parse` feature（default），workspace 默认测会带上；`--no-default-features` 全量编译另加一步验证 feature gate 不破。
- `.github/workflows/unity-smoke.yml`（on push，**手动/标签触发**，暂不强制）：占位 workflow，后续接 game-ci 跑 Unity PlayMode。本轮只放骨架 + 注释说明 license secret 要点，不启用。

**决策记录：** clippy 首次 `-D warnings` 若历史告警炸就降 `-W`，避免一开 CI 就红；fmt 必须门禁（auto-fix 风格统一最廉价）。

## ③ Rust 大文件拆分（编译器验证，中低风险）

**两层策略，按层推进：**

**层 1（最低风险、最高收益）：内嵌测试外提。** stage.rs 1761 行约 1000 行是 4 个 `mod *_tests`；input/scroll/mapping/node/asset/render 同样塞了大段 `mod tests`。把每个文件的 `mod tests` / `mod *_tests` 外提到同目录 `<name>_tests.rs`（`#[cfg(test)] mod <name>_tests;` 引入）或迁到 `loomgui_core/tests/` 集成测试（视是否需 `pub` 暴露内部而定，优先就近 `_tests.rs`）。文件直接瘦 30-50%，零行为变化，`cargo test` 验。

**层 2（中风险、按职责拆 impl）：** 对瘦身后仍 > ~800 行的文件，按职责拆子模块。逐文件编译器验证。候选拆分（实际边界以 impl 时读代码 + 编译反馈为准）：

| 文件 | 拆分提案 |
|---|---|
| `input.rs` | `input/{mod, types, state, focus}.rs`——types=PointerEvent/KeyEvent/TouchSlot；state=PointerState 大 impl；focus=ancestor/dfs/next_focus 焦点遍历 |
| `scroll.rs` | `scroll/{mod, physics, thumb, wheel}.rs`——physics=ScrollPaneState/advance_all/cubic_out；thumb=*_thumb_rect；wheel=apply_wheel_to_hit |
| `mapping.rs` | `mapping/{mod, parsers, apply, taffy_map}.rs`——parsers=parse_length/color/url/transform/filter/slice/overflow；apply=apply_decl 大 match；taffy_map=justify/align |
| `asset/mod.rs` | `asset/{mod, write, read, reader}.rs`——write=write_package；read=read_package/string_at；reader=Reader struct |
| `render/mod.rs` | `build_render_nodes` 抽到 `render/build.rs`；FrameData/ClipEntry 留 mod.rs |
| `node.rs` | Scene impl + build_scene 若仍大，抽 `scene/build.rs`；否则仅层 1 |

**FFI 两文件（`ffi lib.rs 1625` / `blob.rs 1187`）单独判定：**
- `lib.rs` 是 FFI 大表面（`extern "C"` fn + StageHandle）。csbindgen 从中扫描生成绑定——拆子模块前须**先验证 csbindgen 能否跨模块扫**（读其文档 / 小 spike）。能则按域拆（scene/asset/render/event/input/scroll）；不能或风险高则**只做层 1（测试外提）+ 留 lib.rs 作单表面**。FFI 是契约敏感区，不硬拆。
- `blob.rs` 的 `build_blob` 是单一内聚序列化器，若层 1 后可读则不拆。

**变更门：** 每文件拆完跑 `cargo test -p <crate>` + `cargo build -p loomgui_ffi_c` 全过才进下一个。ffi 拆分若动到生成绑定，**重编 .dll + 重生 LoomGUIBindings.cs**（两机工作流：公司机做）。

## ④ Unity 插件/演示拆分（风险最高，最险一步）

**布局（已选方案 A：同级包）：**
```
LoomGUI/
├─ loomgui_unity_package/          ← UPM 包（可发布）
│   ├─ package.json                com.loomgui.unity 0.1.0 MIT
│   ├─ Runtime/  Editor/  Tests/  Shaders/
│   └─ Plugins/LoomGUI/            .dll + LoomGUIBindings.cs
└─ loomgui_unity/                  ← demo 工程
    ├─ Assets/LoomUI/              showcase + res + atlas + design-systems
    ├─ Assets/Scenes/SampleScene
    ├─ Assets/{Settings,StreamingAssets,Resources}
    └─ Packages/manifest.json      "com.loomgui.unity": "file:../loomgui_unity_package"
```

**核心风险 = `.meta` GUID 保不住。** Unity 靠 .meta 里的 GUID 引用资产；任何 .meta 重新生成（路径变触发 Unity 重 import）→ SampleScene、asmdef 互引、资源引用静默断裂，打开 Unity 才看到满屏红。

**GUID 保全技术：**
1. 关掉 Unity（.dll 不被锁 + 不触发增量 import）。
2. 用 `git mv`（文件系统移动，.meta 随资产一起搬）——GUID 是 .meta 文件内容，文件搬路径不变 GUID。
3. 移完 commit，**再**开 Unity。Unity 见新路径下旧 .meta → 重建索引但保留 GUID，不 regenerate。

**搬运表：**
| 源（`loomgui_unity/Assets/`） | 目的（`loomgui_unity_package/`） | 说明 |
|---|---|---|
| `LoomGUI/Runtime/` | `Runtime/` | 4 asmdef 之一（LoomGUI.Runtime.asmdef）随移 |
| `LoomGUI/Editor/` | `Editor/` | LoomGUI.Editor.asmdef 随移 |
| `LoomGUI/Tests/` | `Tests/` | LoomGUI.Tests.asmdef 随移；test-framework 作 optional dep |
| `LoomGUI/Shaders/` | `Shaders/` | 插件自有 shader 资源 |
| ~~`LoomGUI/Fonts/`~~ | （**不入包**） | 字体是项目设计资源，由 demo `LoomUI/res/fonts/` 供——插件不绑字体（见下「字体策略」） |
| `Plugins/LoomGUI/`（.dll + bindings） | `Plugins/LoomGUI/` | .dll/.meta + LoomGUIBindings.cs/.meta + LoomGUI.Bindings.asmdef 随移 |

**留在 demo（`loomgui_unity/Assets/`）：**
- `LoomUI/`（showcase html + res + atlas + design-systems + `.claude` 编辑 skill）——demo 内容。
- `LoomUI/res/fonts/`——字体是项目设计资源，由 demo 供（DejaVuSans / wqy-microhei / JetBrainsMono / LXGWWenKai / PressStart2P）。插件不绑字体路径。
- `Scenes/SampleScene.unity`——demo 场景。
- `Settings/`（URP 资产）、`StreamingAssets/`（.pkg.bin）、`Resources/`（若有 demo 专属）。

**字体策略（决策 A：硬要求，无系统兜底）：**
- 插件**不自带字体**、不绑字体资源路径——字体是接入方的项目设计资源，由 demo 侧 `Assets/LoomUI/res/fonts/` 提供（DejaVuSans / wqy-microhei / JetBrainsMono / LXGWWenKai / PressStart2P）。
- 运行时契约：Inspector 必须**同时**指定 `_font`（Unity 动态字体）+ `_fontFile`（StreamingAssets 下的 ttf，喂 Rust measure），二者须为**同一份 ttf** 以保证 measure/光栅跨平台一致。
- **未指定 → `EnsureFont()` 发 LogError**（现状代码已是此行为）。不做系统字体兜底——Rust measure 侧是引擎无关纯库，必须有 ttf 字节，OS/Unity 系统字体无法喂给它；两侧字体不一致会重现字距/度量错。
- demo 的 `SampleScene` 已配 `_font`/`_fontFile`，作接入示例。

**特殊处理：`LoomShowcaseDriver.cs`（812 行，Runtime 下）。** 这是 demo 驱动（驱动 showcase 场景），不是插件 API——移出包，放到 demo 的一个 demo-only asmdef（如 `LoomUI/Demo/LoomGUI.Demo.asmdef`）下。它引用 showcase 内容，不属可分发包。

**package.json 骨架：**
```json
{
  "name": "com.loomgui.unity",
  "version": "0.1.0",
  "displayName": "LoomGUI",
  "description": "跨引擎游戏 UI 框架——Rust 核心 + HTML/CSS DSL，Unity 后端",
  "unity": "<以 ProjectVersion.txt 为准，Unity 6.x 形如 6000.0>",
  "license": "MIT",
  "dependencies": {
    "com.unity.render-pipelines.universal": "<对齐 demo 现 lock>",
    "com.unity.inputsystem": "<对齐 demo 现 lock>",
    "com.unity.2d.sprite": "<对齐 demo 现 lock>"
  }
}
```
（版本字段 impl 时按 `loomgui_unity/ProjectSettings/ProjectVersion.txt` + `Packages/packages-lock.json` 填准；不在此处硬编以免漂移。）

**demo manifest.json 加一行：**
```json
"com.loomgui.unity": "file:../loomgui_unity_package"
```

**配套更新：**
- `.gitignore` 白名单核对：`!**/Plugins/**/*.dll` 与 `!**/Plugins/LoomGUI/Bindings/LoomGUIBindings.cs` 仍匹配新路径（`**` 通配），不动。
- `CLAUDE.md` 的 .dll 拷贝路径更新：`loomgui_unity/Assets/Plugins/LoomGUI/` → `loomgui_unity_package/Plugins/LoomGUI/`。
- 两机工作流的 stale .dll 诊断路径同步。

**验证门（最严）：** 移完开 Unity → Console 无红 → SampleScene PlayMode 渲染正常 → asmdef 互引全 resolve。任一红 = 回滚该 commit、排查 GUID。

## 执行顺序与验证门

```
① OSS 治理   →  git 状态干净、LICENSE 在
     ↓
② CI 工具链  →  push 触发、Win+Ubuntu 全绿（clippy 首轮若历史告警多先 -W）
     ↓
③ Rust 拆文件 →  层1 全做；层2 逐文件；每文件 cargo test 全过
     ↓ （若 ffi 拆了→公司机重编 .dll + 重生 bindings + push）
④ Unity UPM  →  git mv 保全 GUID → 开 Unity 无红 → PlayMode 正常
```

每块一个 commit（或一小串），message 前缀分别 `docs(chore)/ci/refactor(rust)/refactor(unity)`。④ 落地后回头更新 README 项目结构表 + CLAUDE.md 的 .dll 路径。

## 风险与回滚

| 风险 | 缓解 |
|---|---|
| ④ GUID 丢、Unity 满屏红 | git mv 保 .meta；关 Unity 再搬；commit 后才开；验证门失败即 revert 该 commit |
| ③ ffi 拆分破坏 csbindgen 生成 | 拆前先 spike 验证 csbindgen 跨模块扫；不行则只做层 1，留 lib.rs 单表面 |
| ② clippy 历史告警炸 CI | 首轮 `-W`，清单清完再升 `-D` |
| ④ SampleScene 引用断 | 场景靠 GUID 引用 LoomGUI 预制/脚本；GUID 不变则引用不断——故 git mv 是关键 |

## 决策记录

| 决策 | 选择 | 理由 |
|---|---|---|
| 合并 crate？ | 否 | 三 crate 编译目标不同，合并绑死配置 + 丢 feature 隔离 |
| LICENSE | MIT | Unity 插件圈惯例，Asset Store 友好，匹配 FairyGUI-unity |
| Unity 包布局 | 同级包（方案 A） | 插件/demo 平级互不套；file: 引用改插件即时生效；可独立打 tag 发布 |
| CI 平台 | Win + Ubuntu | 公开仓库免费；Ubuntu 抓 Unix bug；macOS 待 Mac 成真目标 |
| writing-plans | 跳过 | 用户偏好；本 spec「执行顺序与验证门」即计划 |
| spec/plan 拆分 | 伞形 spec | 4 子项目独立但同属「整顿」，一份 spec 覆盖、独立 commit |
| 字体策略 | 硬要求 `_font`+`_fontFile`，无系统兜底 | Rust measure 需 ttf 字节，OS 字体无法对应；硬要求保证 measure/光栅同源，避免字距错。插件不带字体，demo 供 |
