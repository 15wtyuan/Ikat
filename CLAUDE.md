# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 这是什么

LoomGUI = 跨引擎游戏 UI 框架。HTML/CSS 子集作 DSL，taffy flexbox 布局，自绘渲染。核心目的：**AI 驱动的界面拼装**——HTML 作 DSL，让 AI 既能编辑（文本）又能预测渲染结果（AI 对 HTML/CSS 有强先验）。每条 DSL 决策的首要判据都是"AI 读这段 HTML 能否正确预测渲染出的 UI"。

对标 FairyGUI（参考实现在 `temp/FairyGUI-unity/`，只读）。差异化：HTML/CSS DSL（vs fgui 的 `.fui` 二进制 AI 看不懂）、flexbox（vs 锚点 Relations）、一份 Rust 核心多后端、围栏验证器。

## 构建 / 测试命令

```bash
# 核心（引擎无关纯库，可单测）
cargo build -p loomgui_core
cargo test  -p loomgui_core

# 打包器 CLI（HTML+CSS+资源 → .pkg.bin + 自绘图集 + fonts；二进制名 loom-pkg，复用核心 parse 层）
cargo build -p loomgui_pkg
cargo test  -p loomgui_pkg
# 运行：cargo run -p loomgui_pkg -- build <workspace-dir>    （loom-pkg build <workspace>）

# 独立打包器 GUI（Tauri 桌面应用；出 exe 见下方「GUI 打包器 exe 闭环」段，勿用 cargo build）
cargo tauri dev                  # 开发热重载（需 tauri-cli）
cargo tauri build --no-bundle    # 出 exe（cargo build --release 不 embed 前端 → localhost 白屏 exe）

# FFI（C ABI；csbindgen 在 build.rs 里重新生成 C# 绑定）
cargo build -p loomgui_ffi_c

# 整个 workspace
cargo test
```

**Feature gate（`parse`）**：`scraper`+`cssparser` 是可选的，由 `parse` feature 控制（core/pkg/ffi 默认开）。运行时不带 HTML 解析器。不带 parse 编译全部：
```bash
cargo build --no-default-features --all-targets   # 按 crate，或 workspace 级
```
`snapshot` 集成测试需要 `parse`（`required-features`）。

**跑单个测试 / 围栏门**：
```bash
cargo test -p loomgui_core --test fence_contract   # ← 围栏契约门（见下）
cargo test -p loomgui_core --test snapshot -- <name>
```

**基准测试**：`cargo bench -p loomgui_core`（criterion，`frame_emit`）。

**CI 门禁**（`.github/workflows/rust-ci.yml`，push main / PR 触发）：fmt 严（`cargo fmt --all -- --check`）+ clippy 严（`cargo clippy --all-targets -- -D warnings`）+ Win/Ubuntu matrix test + feature-gate check（`--no-default-features --all-targets`）+ Windows `.dll` artifact（release build）。**push 前本地跑 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`**，否则 CI 红。clippy 各 crate root 有 `#![allow]` 放行可辩护的测试/FFI 模式 lint（`field_reassign_with_default` / `not_unsafe_ptr_arg_deref` / `too_many_arguments` 等，带理由注释），勿误清——新增可辩护模式 lint 在那里加。

### Rust → Unity .dll 闭环（Windows 本机是唯一的编码机）

按记忆/工作流：**任何** Rust 改动后必须重编 + commit `.dll`，否则测不了。

```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll
```
- **拷贝时 Unity 必须关着**（它锁 .dll）。
- **stale .dll 诊断**：PlayMode 全不渲 + Console 干净 → `md5sum target/release/loomgui_ffi_c.dll loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll`；不等 = stale（Rust 改了 blob/ABI，.dll 没换）。
- 入库的 `.dll` + csbindgen 生成的 `LoomGUIBindings.cs` 在 `loomgui_unity_package/Plugins/LoomGUI/`（`**/Plugins/**/*.dll` 和 bindings .cs 是 gitignore 白名单例外；其余 native 产物一律忽略）。

**图集自绘**：v1.8 起图集由打包器自绘（`loom-pkg build` 或 GUI 产 `atlas/*.png`+`atlas/*.atlas.json`），Unity 不再打 `SpriteAtlas`。运行时尺寸由 `atlas.json` + FFI `set_image_sizes` 注入，不再靠 Unity 导入管线。

### GUI 打包器 exe 闭环（loomgui_gui，Tauri 2）

GUI 是 Tauri 2 桌面 app，产物 `loomgui_unity_package/Editor/Tools/loomgui_gui.exe`（Unity `LoomGUI > Open Packer` 拉起）。任何 GUI 改动（Rust `src/` 或前端 `dist/`）后必须重出 exe + 拷贝 + 入库：

```bash
npm install -g @tauri-apps/cli                 # 一次性装 tauri-cli（prebuilt，比 cargo install 快得多）
(cd loomgui_gui && tauri build --no-bundle)    # 出 exe（必须 tauri CLI！--no-bundle 跳 NSIS/MSI installer）
cp target/release/loomgui_gui.exe loomgui_unity_package/Editor/Tools/loomgui_gui.exe
```
- **拷贝时 Unity / 旧 GUI 进程必须关着**（锁 exe，报 `Device or resource busy`）。
- 入库 exe + `.meta`（`Editor/Tools/` 下，gitignore 白名单例外，同 .dll）。
- **路径定位**：`LoomOpenPacker.cs` 用 `PackageInfo.FindForAssetPath("Packages/com.loomgui.unity/package.json").resolvedPath` 取包真实磁盘根——`com.loomgui.unity` 是 manifest.json 里 `file:` 外部本地包，`Packages/com.loomgui.unity` 是虚拟挂载、非真实 FS 路径，`System.IO.File` 探不到。
- **前端手写 dist（无 npm 构建）**，靠 `app.withGlobalTauri:true` 注入 `window.__TAURI__`（tauri.conf.json）；缩略图靠 `app.security.assetProtocol` + Cargo `tauri` feature `protocol-asset`。

## 架构（大局——权威契约读 `docs/design/main-design.md`）

**分层、单向数据流、引擎对象不进核心：**
```
HTML/CSS DSL → 打包器（构建期；复用核心 parse/style）
  → Rust 核心：parse(scraper+cssparser+自写 ~100 行选择器匹配器)
    → style(cascade) → scene(Node 树，代际 NodeId)
    → layout(taffy flexbox solve text)
    → render(Vec<RenderNode>) → stage.tick
  → FFI（csbindgen：Rust ↔ C ABI ↔ C# P/Invoke）
  → Unity 后端（GameObject+MeshRenderer 镜像渲染树；输入采集；资源加载）
```

**关键边界**：
- **Rust 核心**拥有：parse、style、layout、场景图、事件、动画、几何生成、批合、裁剪/顺序。产出 `Vec<RenderNode>` + 命中结果 + 事件。**不持任何引擎对象、不碰 GPU。** 文本渲染核心自绘（ttf-parser outline + ab_glyph 光栅 + etagere 图集），产 UV + atlas，后端降为贴图上传，文本已可参与合批。
- **引擎后端**拥有：输入采集、渲染树→原生镜像、mesh 上传、DrawState 缓存+提交、资源加载代理。**不解析 DSL、不算布局、不生成几何。**
- 核心不知 GameObject/CanvasItem；后端不知 DSL/taffy/几何。

**关键架构不变量**（违反 = 隐 bug）：
- **`<div>` 永远是 flex 容器**（默认 `flex-direction: column`）。无浏览器 block/inline flow——只有 flex item 参与布局。行内混排（一元素内文本+元素+文本）是**编译期报错**，不降级。只有 `div`/`span`/`img`/`button` 标签；围栏外标签报错。
- **transform（x/y/scale/rotation）是渲染/命中层，不进 taffy**——改它不触发 `solve`，只刷新命中几何 + world_matrix。`style_size`/flex 进 taffy → `solve`。位置/缩放动画走 transform 所以廉价。
- **tick 时序 = 显式依赖拓扑**（坑 103 已修）：`process(hit 用上帧 world) → rematch_pseudo_classes → solve → refresh_content → compute_world_transforms → build`。rematch 在 solve/compute **前**——伪类改 taffy_style/transform/colors 三类当帧全生效。hit_test 用上帧 world（1 帧延迟，已认可）；scroll_pos 同帧进 world（compute 在 scroll 后）。
- **所有布局帧末一致**：每帧一次 `solve`（vs fgui 立即推 DisplayObject）。
- **命中几何** = `layout_rect` 经累计（含父链）transform 变换后的 AABB。事件路由本身在**业务侧**（C# `LoomEventHandler`），非核心——核心只做命中 + hover/active diff + 伪类 rematch。
- **坐标系**：核心 = 左上原点、y 向下。核心代码无 `height-y` 翻转。y-flip 是**后端根 Stage 一次性变换**（Unity 根 GO scale (1,-1,1)；Godot flip = 单位矩阵，2D 本就 y 下）。
- **代际 NodeId**：`NodeId(pub u32)`（高 20bit index + 低 12bit gen），FFI/C# 透明不透明句柄；`remove_node` gen++ 让旧句柄自动失效。内部用 `SlotMap<DefaultKey, Node>` 桥接（slotmap 的 64-bit key 装不下 u32，见 `scene/node.rs`）。
- **单一动画时钟**：`TweenManager::update(dt)` 是唯一时钟。Controller/Gear/Transition 都不自驱——全往它提交/kill tween。ScrollPane 物理是**例外**（自维护 tween，绝不用 GTween——content 异步变化时 GTween 的固定 end 会跳变）。
- **虚拟列表 = 层 B'（核心不认识"列表"）**：列表是普通 div（overflow:scroll + position:relative）+ N 个 slot 子节点（position:absolute + reuse_key）。核心只多管 `reuse_key: u32` 字段（传后端复用 GO）+ 3 个 FFI 口子（`set_content_size`/`get_scroll_pos`/`get_node_layout_rect`）。driver 管所有列表逻辑（slot 映射/可见区间/不等高补偿）。**reuse_key 绑稳定槽位 slotIdx（非数据 itemIndex）**——slot 换绑 item 时 NodeId 变但 reuse_key 不变 → MirrorPool 双 dict 复用 GO（坑 109）。**reuse_key 是场景级全局命名空间**——多虚拟列表同屏须用不相交 reuse_key 段，否则两列表同 slotIdx 抢同一 GO、slot 背景互相覆盖（坑 112）。
- **NativeHost 分层**：框架（`NativeHostManager`）只提供**机制**——FFI 按 nodeId 查询（`get_node_world_matrix`/`sort_key`/`visible`，读 `world_transforms`/`node_sort_keys`，**独立于 merge**——blob 是渲染列表被 merge 吞空 div，查询通道不能走 blob，坑 127）、材质 Transparent 配置工具（`ConfigureTransparentMaterials(go)` / `UnconfigureTransparentMaterials(go)` 静态方法，封装坑 129 三件套；caller Instantiate 后 Configure、销毁前 Unconfigure，clone 归 caller——框架不自动碰材质、不接管 GO/材质 ownership）。业务（"角色脚底对齐 slot 底中"等 anchor / 位置 / scale）在 driver——caller 知道"角色"，框架不知。**别把业务塞框架**。Bind 契约 = 场景实例（caller Instantiate，框架不越界实例化，坑 128）。orthographic 3D GO scale z 该小（只增 depth 厚度 near/far clip，不增视觉，坑 130）。
**FFI 契约**：每帧 Rust 产出一个 SOA 公共头 + 扁平 mesh_arena。C# 在 tick 内原子拷贝（拷贝非 pin），Rust 下帧 reset。**变更检测用双 hash**：`header_hash`（表头：world/alpha/sort/mask/color_tint/blend/**reuse_key**，廉价字段）+ `payload_hash`（几何：全量 verts/uvs/colors/glyph/**line.y/height/baseline/width**，**不采样**——采样会漏字段，坑 56/75/76/106）。两 hash 对比上帧 → `ChangeLevel { Skip=0, Header=1, Full=2 }`（`#[repr(u8)]`，blob 一字节列）：Skip=整节点不动；Header=只更 GO transform/材质/MPB，**不重建 mesh**（滑动/transform/opacity 动画走此，兑现"位置动画廉价"）；Full=重建 mesh。**arena 仅 Full 写**（Skip/Header mesh_off/len=0，省带宽）。**`NodePayload` 只剩 Mesh**（v10 text 塌进 mesh_arena，Text 变体已删）——"本帧没变"由 ChangeLevel::Skip 表达（正交轴），不再是 payload 变体。alpha 走 `_Alpha` shader uniform（per-renderer MPB），不烤进顶点色。**`reuse_key`**（v1.4-b）：每节点 u32，0=无复用（后端按 node_id keying），>0=按 reuse_key 复用 GO（虚拟列表 slot：slot 换绑 item 时 NodeId 变但 reuse_key 不变 → 后端 MirrorPool 双 dict 复用 GO 不销毁）。C# 用 `Span<byte>` + `BinaryPrimitives` 读（桌面 Mono 当前使用 `Marshal.PtrToStructure`，移动端 IL2CPP 上线前须换 Span+BinaryPrimitives——IL2CPP struct 对齐坑），**禁跨 FFI 裸指针**。IL2CPP：回调必须 `static` + `[MonoPInvokeCallback]`。**FFI 入口绝不 panic**：cdylib 里 `.expect`/`unwrap` 遇 None 会 non-unwinding abort 拖垮宿主进程（Unity 闪退）——scene=None 等状态优雅早返空帧，别 expect（坑 102，tick_and_render 已修 match None→空 FrameData）。

## 围栏（Fence）——单一真相源

LoomGUI 只支持 HTML/CSS 的**明确子集**，称"围栏"。这是项目漂移高发区。

- **权威真相源 = `loomgui_core/tests/fence_contract.rs`**（可执行契约）。`docs/design/fence.md` 是人类可读副本；**不一致时测试赢**。围栏规则通过独立工作区（standalone workspace directory + `loom.workspace.json`）注入，打包器 `loom-pkg build` 校验。
- **改围栏 = 改 `fence_contract.rs` 测试 + `fence.md`**，不改 `main-design.md` §3（那节只写哲学，避免漂移）。
- **围栏门**：`cargo test -p loomgui_core --test fence_contract`——build .dll 前跑、改 `apply_decl`/`FENCE_TAGS`/选择器后跑。
- 两类围栏外行为（均**测试锁定**，别靠 grep 推断）：围栏外标签 + 行内混排 → **编译期报错**（parse 失败、打包器拒收）。围栏外 CSS 属性（如 `clip-path`、`cursor`）→ **静默忽略**（`apply_decl` 返 `false`）。
  - `position:relative` 教训："grep 无 match" ≠ "不支持"——可能是依赖默认值（taffy `Style::DEFAULT.position = Relative`）。声明支持前先核实依赖默认值 + 补测试。
  - `position:absolute`（v1.4-b 起围栏内）：`absolute`/`relative` 生效（taffy Absolute + inset），`fixed`/`sticky` 仍静默忽略。layout/render/hit 零改（taffy solve 自动）。

## 在本仓库怎么干活

- **实现任何机制前，先对照 FairyGUI 源码 和 RmlUi 源码**（`temp/FairyGUI-unity/ 和 temp/RmlUi/`）。LoomGUI 的渲染/对象模型/批合/事件/动画/资源管线全面借鉴 fgui，核心借鉴RmlUi。先读对应 fgui、RmlUi 文件看它怎么做，再定设计。fgui 是 Built-in RP——URP/shader/材质 API 要适配。
- **设计文档 vs 踩坑**：`docs/design/main-design.md`（设计契约/当前实现真相）、`docs/design/fence.md`（围栏）、`docs/roadmap/roadmap.md`（范围+机制草稿）、`docs/pitfalls.md`（踩坑全库 + 依赖 API 适配，开工前读它查"具体怎么干 + 坑在哪"）。
- **Rust edition 2021**，依赖钉版本：`taffy 0.5`、`ttf-parser 0.20`、`cssparser 0.34`、`scraper 0.19`、`slotmap 1.1`、`csbindgen 1`。snapshot 测试用 `insta`。
- `Cargo.lock` 入库（根级，尽管 `.gitignore` 有通用 `Cargo.lock` 行——它是被追踪的）。
- 设计师工作区是独立磁盘目录（含 `loom.workspace.json`、HTML/CSS 源文件、res 资源、design-systems 组件库）。打包用独立打包器 GUI（Tauri `loomgui_gui`）或 CLI `loom-pkg build <workspace>`。v1.8 起不再使用 `LoomGUI > Settings` 面板（该面板及相关 Unity Editor 脚本已移除，仅保留 `LoomOpenPacker.cs` 菜单项启动 GUI）。运行时引导由 `loom.runtime.json` 统管（声明包/图集/字体），不再依赖 Unity `LoomSettings` ScriptableObject。
- 用户只读中文——问答/选项/总结用中文；代码/commit 照旧英文。
- **代码注释写上线品质**：自包含（不看其他文件就能懂）、精简（说 WHY，不复述代码机制）、**不引用内部编号或暗语**——`坑 120`、`Venkify 法`、`与某某 meta 对齐` 这类项目内指代外人看不懂。坑号只属于 `docs/pitfalls.md`，不进代码。
- **防文档漂移**（遵循 fence_contract 模式——文档的断言必须有测试护着）：
  1. **文档写定性、不写具体数字**。数字（列数、字段数、枚举变体数）只放代码注释或测试里——数字最容易漂移，文档里写"渲染公共字段"而非"20 列"
  2. **关键 claim 加可执行测试**。FFI struct 尺寸：`assert_eq!(size_of::<BlobHeader>(), N)`。NodeKind 变体数：命名字段计数。tick 步骤顺序：读源码验证。写进 fence_contract 或新测试
  3. **改代码后搜 docs/ 里是否引用了改动的 struct/函数/列数**——`git diff` 扫一遍改动的名字，grep docs/ 确认没提到过期的数字或签名。低级但有效

### v1.8+ 独立打包器 + 自绘图集 + 工作区重构

v1.8 起工作流脱离 Unity 编辑器依赖。工作区 = 独立磁盘目录（`loom.workspace.json`），打包器 = 自绘（CLI `loom-pkg build <workspace>` + GUI `loomgui_gui`），图集 = Rust 自绘（etagere shelf pack → `atlas/*.png` + `atlas/*.atlas.json`），运行时引导 = `loom.runtime.json`。pkg.bin 格式 v14→v15（AssetManifest section 移除，尺寸走 atlas.json + FFI `set_image_sizes`）。完整设计见 `docs/superpowers/specs/2026-07-10-standalone-packer-atlas-workspace-design.md`。

移除的 Unity Editor 脚本：`LoomSettingsWindow`、`LoomAtlasSync`、`LoomConfigExporter`、`LoomWorkspaceInitializer`、`PkgManifestReader`、`LoomExePath`、`LoomJsonEscape`、`LoomPackArgs`、`LoomWorkspaceAssetPostprocessor`。移除的 ScriptableObject：`LoomSettings`。仅保留 `LoomOpenPacker.cs`（MenuItem 启动 Tauri GUI）。

## 调试技巧

**dump_*.rs 诊断 example**（pkg.bin 路径，验 core 实际状态而非猜代码）：
- `dump_text` — 文本换行（验 known.width 来源、行数、pen 坐标）
- `dump_img` — 图片尺寸（css.w/h、rect、tex、闭包 `known.w`）
- `dump_scroll` — 滚动（overlap、scroll_pos、content_size）
- `dump_render` — 渲染节点（rect、bg、UV）
- `dump_sw` / `dump_bg` — 节点 base_style（验是否进 pkg）
- `dump_nativehost_slot` — NativeHost FFI 查询（nh-stage 空 div 直查 world_transforms/sort_key，绕 merge blob）
- `dump_controller` — v1.5 Controller（display 显隐 / color 继承 / selected_index / transition）

**跨层特性 PlayMode 报错**（拖不动/晃动/错位）先 example 实测 core 状态（overlap/scroll_pos/content_size）再改，避免盲改物理掩盖 layout 根因。dump 边界/状态取证，别靠"浮点边界/epsilon"症状层猜测。

**SDD per-task review 是代码质量门，不是集成正确性门**：v1.5 Controller 16 task 全 APPROVED + final review APPROVED，PlayMode 仍出 4 bug（坑 131-133：display:none 子树渲染 / runtime color 继承 / transition 逗号多 spec）。CSS 语义集成（display 子树剪枝、继承传播、多 spec 解析）只在 PlayMode 显现——单测验不了。SDD 后必跑 showcase PlayMode 逐项过，别只靠单测绿就 merge。

**SDD long-running worktree 要防 main 漂移**（坑 147）：worktree 串行做多 task 期间，main 可能被别的会话推进（v1.7 PlayMode fixes 等）。merge 回 main 时非快进，两边改同一核心函数签名（如 `build_text_mesh`/`push_text_meshes`）会整段冲突。解法：**反向 merge**（`git merge main` 进 feature 分支，在 feature 分支解冲突——有 feature 上下文，main 不动直到 fast-forward），**合超集签名**（两边参数都收，如 `register_id_map` + `shadow_pairs`），把对方分支的非签名修复逐个移植进 feature 代码路径，用对方分支的测试当合并验收标准（测试绿 = 合并正确）。worktree 开长任务前先确认 main 是否会动。

**subagent 撞 API 限流被 kill 不回滚代码**（坑 148）：subagent-driven SDD 期间 implementer 撞 5 小时上限被 kill 后，先 `git status` + `cargo build` + `cargo test` 核实代码完整度（kill 不回滚已写代码，常是"代码完整但没收尾"），别假设白干或完成。限流背景下的收尾（fmt/clippy/补单测/commit/report）走 controller 工具调用（Bash/Edit 不耗 Agent API，能绕限流），别再派 subagent（会再撞限流）。日志最后一句常提示断点时在做什么。


**偶现/时序 bug**（依赖 Unity 内部事件/帧序，如动态字体 atlas rebuild）光读代码定位不了——加诊断 log 运行时取证（调用栈暴露触发点是破案关键），别静态猜根因反复改（坑 113）。

**改 parse-time 逻辑必重打 pkg**：`Node.base_style` 是打包期 `resolve_styles` 产物（不变）。改 cascade/mapping/parse 只重编 .dll 不够，须 `cargo run -p loomgui_pkg` 重打 pkg（html/css 未变也要）。纯 runtime（render/layout measure/scroll/anim）改 .dll 即可。

csbindgen 不为 `#[repr(C)]` struct 生成 C# stub，须手补 C# 镜像文件；新增/改 FFI struct 须同步镜像（坑 35）。

## API 适配方法论

**plan/草稿的 API 常与 crate 实际不符**——遇编译错按 crate 实际源码（`~/.cargo/registry/src/<crate>-<ver>/src/`）调，**勿硬改依赖版本**。具体 crate 差异见 `docs/pitfalls.md` §3。

**Unity API 同理别信记忆/草稿**（坑 120 的 `Sprite.rect` 语义全程坑人）——查 `Editor/Data/Managed/UnityEditor.xml`（API XML 文档，含准确成员签名）：`grep -oE '<member name="[PMFIT]:[^"]*TypeName[^"]*"' UnityEditor.xml`。别猜属性/方法名。

**FFI 边界 C-like enum 必须 `#[repr(uN)]`**（u8/u16/u32），否则判别 isize 跨平台不稳 + 撑大 struct（坑 34）。永远 `size_of::<T>()` 断言 ABI struct 尺寸，别信草稿。

**Rust FFI 返字符串一律 ptr+len**（不靠 NUL）；C# 侧禁用 NUL-scan 读法（坑 16）。

**移植 fgui 算法**：带数字后缀的变量名（`v2`/`pos2`/`d2`）不能望文生义——须读源码表达式确认是平方还是线性命名残留（fgui `v2` 是 `|v|·scale` 非 `v²`，坑 54）。算法移植按源码逐行 trace 验，勿按文字描述想当然。

## 坑索引

完整踩坑记录见 `docs/pitfalls.md`（坑 1-99+，按编号递增）。新踩坑继续编号递增，写法：症状/根因/解决/教训。
