# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 这是什么

LoomGUI = 跨引擎游戏 UI 框架。**Rust 核心（引擎无关纯库）+ 引擎后端（Unity 首发）**，HTML/CSS 子集作 DSL，taffy flexbox 布局，自绘渲染。核心目的：**AI 驱动的界面拼装**——HTML 作 DSL，让 AI 既能编辑（文本）又能预测渲染结果（AI 对 HTML/CSS 有强先验）。每条 DSL 决策的首要判据都是"AI 读这段 HTML 能否正确预测渲染出的 UI"。

对标 FairyGUI（参考实现在 `temp/FairyGUI-unity/`，只读）。差异化：HTML/CSS DSL（vs fgui 的 `.fui` 二进制 AI 看不懂）、flexbox（vs 锚点 Relations）、一份 Rust 核心多后端、围栏验证器。

## 构建 / 测试命令

```bash
# 核心（引擎无关纯库，可单测）
cargo build -p loomgui_core
cargo test  -p loomgui_core

# 打包器 CLI（HTML+CSS+资源 → .pkg.bin + 图集；复用核心 parse 层）
cargo build -p loomgui_pkg
cargo test  -p loomgui_pkg

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
cargo test -p loomgui_core fence_contract   # ← 围栏契约门（见下）
cargo test -p loomgui_core --test snapshot -- <name>
```

**基准测试**：`cargo bench -p loomgui_core`（criterion，`frame_emit`）。

**CI 门禁**（`.github/workflows/rust-ci.yml`，push main / PR 触发）：fmt 严（`cargo fmt --all -- --check`）+ clippy 严（`cargo clippy --all-targets -- -D warnings`）+ Win/Ubuntu matrix test + feature-gate check（`--no-default-features --all-targets`）+ Windows `.dll` artifact（release build，7 天供家里机取）。**push 前本地跑 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`**，否则 CI 红。clippy 各 crate root 有 `#![allow]` 放行可辩护的测试/FFI 模式 lint（`field_reassign_with_default` / `not_unsafe_ptr_arg_deref` / `too_many_arguments` 等，带理由注释），勿误清——新增可辩护模式 lint 在那里加。

### Rust → Unity .dll 闭环（Windows 本机是唯一的编码机）

按记忆/工作流：本机负责 build `.dll` + commit + push；家里机只做 Unity PlayMode 验收。**任何** Rust 改动后必须重编 + commit `.dll`，否则家里机测不了。

```bash
cargo build -p loomgui_ffi_c --release
cp target/release/loomgui_ffi_c.dll loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll
```
- **拷贝时 Unity 必须关着**（它锁 .dll）。
- **stale .dll 诊断**：PlayMode 全不渲 + Console 干净 → `md5sum target/release/loomgui_ffi_c.dll loomgui_unity_package/Plugins/LoomGUI/loomgui_ffi_c.dll`；不等 = stale（Rust 改了 blob/ABI，.dll 没换）。
- **改 FFI 签名后 push 前自查 dll 导出**：公司机本地缓存的旧 dll + gitignored bindings 会让公司机"能跑"却 commit 了旧 dll（坑 100）——改 FFI 后 `nm/findstr` 查新符号在 dll 里再 push，否则家里机干净 pull 编译炸。
- 入库的 `.dll` + csbindgen 生成的 `LoomGUIBindings.cs` 在 `loomgui_unity_package/Plugins/LoomGUI/`（`**/Plugins/**/*.dll` 和 bindings .cs 是 gitignore 白名单例外；其余 native 产物一律忽略）。
- Unity 6.5，URP。打开 `loomgui_unity/`，PlayMode 从 `StreamingAssets/` 加载 `.pkg.bin`。

## 架构（大局——权威契约读 `docs/design/main-design.md`）

Workspace 成员：`loomgui_core`、`loomgui_pkg`、`loomgui_ffi_c`（+ `loomgui_unity` Unity 工程）。

**分层、单向数据流、引擎对象不进核心：**
```
HTML/CSS DSL → 打包器（构建期；复用核心 parse/style）
  → Rust 核心：parse(scraper+cssparser+自写 ~100 行选择器匹配器)
    → style(cascade) → scene(Node 树，代际 NodeId)
    → text(ttf-parser 测量 → TextLayout) → layout(taffy flexbox solve)
    → render(Vec<RenderNode>) → stage.tick
  → FFI（csbindgen：Rust ↔ C ABI ↔ C# P/Invoke）
  → Unity 后端（GameObject+MeshRenderer 镜像渲染树；输入采集；资源加载）
```

**关键边界**（别跨越）：
- **Rust 核心**拥有：parse、style、layout、场景图、事件、动画、几何生成、批合、裁剪/顺序。产出 `Vec<RenderNode>` + 命中结果 + 事件。**不持任何引擎对象、不碰 GPU。** 非文本几何在核心生成；**文本 mesh 是例外**——核心只产 `TextLayout`，后端光栅化（动态字形 UV 只有引擎字体 API 才有）。
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
- **虚拟列表 = 层 B'（核心不认识"列表"）**（v1.4-b）：列表是普通 div（overflow:scroll + position:relative）+ N 个 slot 子节点（position:absolute + reuse_key）。核心只多管 `reuse_key: u32` 字段（传后端复用 GO）+ 3 个 FFI 口子（`set_content_size`/`get_scroll_pos`/`get_node_layout_rect`）。driver 管所有列表逻辑（slot 映射/可见区间/不等高补偿）。**reuse_key 绑稳定槽位 slotIdx（非数据 itemIndex）**——slot 换绑 item 时 NodeId 变但 reuse_key 不变 → MirrorPool 双 dict 复用 GO（坑 109）。**reuse_key 是场景级全局命名空间**——多虚拟列表同屏须用不相交 reuse_key 段，否则两列表同 slotIdx 抢同一 GO、slot 背景互相覆盖（坑 112）。不暴露 `<l-list>` 标签（围栏只有 div/span/img/button）。
- **NativeHost 分层**（v1.4-c）：框架（`NativeHostManager`）只提供**机制**——FFI 按 nodeId 查询（`get_node_world_matrix`/`sort_key`/`visible`，读 `world_transforms`/`node_sort_keys`，**独立于 merge**——blob 是渲染列表被 merge 吞空 div，查询通道不能走 blob，坑 127）、渲染状态配置（`renderQueue`/`ZWrite`/URP `_Surface`+`_SURFACE_TYPE_TRANSPARENT` keyword，坑 129）。业务（"角色脚底对齐 slot 底中"等 anchor / 位置 / scale）在 driver——caller 知道"角色"，框架不知。**别把业务塞框架**。Bind 契约 = 场景实例（caller Instantiate，框架不越界实例化，坑 128）。orthographic 3D GO scale z 该小（只增 depth 厚度 near/far clip，不增视觉，坑 130）。

**FFI 契约**（§13.3）：每帧 Rust 产出一个 SOA 公共头（渲染节点公共字段，当前 22 列，含 `change_level` + `reuse_key`）+ 按类型分区的扁平 arena（mesh_arena、text_arena）。C# 在 tick 内原子拷贝（拷贝非 pin），Rust 下帧 reset。**变更检测用双 hash**：`header_hash`（表头：world/alpha/sort/mask/color_tint/blend/**reuse_key**，廉价字段）+ `payload_hash`（几何：全量 verts/uvs/colors/glyph/**line.y/height/baseline/width**，**不采样**——采样会漏字段，坑 56/75/76/106）。两 hash 对比上帧 → `ChangeLevel { Skip=0, Header=1, Full=2 }`（`#[repr(u8)]`，blob 一字节列）：Skip=整节点不动；Header=只更 GO transform/材质/MPB，**不重建 mesh**（滑动/transform/opacity 动画走此，兑现"位置动画廉价"）；Full=重建 mesh。**arena 仅 Full 写**（Skip/Header mesh_off/len=0，省带宽）。**`NodePayload` 只剩 Mesh/Text**——"本帧没变"由 ChangeLevel::Skip 表达（正交轴），不再是 payload 变体。alpha 走 `_Alpha` shader uniform（per-renderer MPB），不烤进顶点色。**`reuse_key`**（v1.4-b）：每节点 u32，0=无复用（后端按 node_id keying），>0=按 reuse_key 复用 GO（虚拟列表 slot：slot 换绑 item 时 NodeId 变但 reuse_key 不变 → 后端 MirrorPool 双 dict 复用 GO 不销毁）。C# 用 `Span<byte>` + `BinaryPrimitives` 读——**禁 `Marshal.PtrToStructure`**（IL2CPP struct 对齐坑），**禁跨 FFI 裸指针**。IL2CPP：回调必须 `static` + `[MonoPInvokeCallback]`。**FFI 入口绝不 panic**：cdylib 里 `.expect`/`unwrap` 遇 None 会 non-unwinding abort 拖垮宿主进程（Unity 闪退）——scene=None 等状态优雅早返空帧，别 expect（坑 102，tick_and_render 已修 match None→空 FrameData）。

## 围栏（Fence）——单一真相源

LoomGUI 只支持 HTML/CSS 的**明确子集**，称"围栏"。这是项目漂移高发区。

- **权威真相源 = `loomgui_core/tests/fence_contract.rs`**（可执行契约）。`docs/design/fence.md` 是人类可读副本；**不一致时测试赢**。编辑器用 `loomgui_unity/Assets/LoomUI/` 工作区 + `LoomGUI > Settings` 面板注入围栏规则。
- **改围栏 = 改 `fence_contract.rs` 测试 + `fence.md`**，不改 `main-design.md` §3（那节只写哲学，避免漂移）。
- **围栏门**：`cargo test -p loomgui_core fence_contract`——build .dll 前跑、改 `apply_decl`/`FENCE_TAGS`/选择器后跑。
- 两类围栏外行为（均**测试锁定**，别靠 grep 推断）：围栏外标签 + 行内混排 → **编译期报错**（parse 失败、打包器拒收）。围栏外 CSS 属性（如 `clip-path`、`cursor`）→ **静默忽略**（`apply_decl` 返 `false`）。
  - `position:relative` 教训："grep 无 match" ≠ "不支持"——可能是依赖默认值（taffy `Style::DEFAULT.position = Relative`）。声明支持前先核实依赖默认值 + 补测试。
  - `position:absolute`（v1.4-b 起围栏内）：`absolute`/`relative` 生效（taffy Absolute + inset），`fixed`/`sticky` 仍静默忽略。layout/render/hit 零改（taffy solve 自动）。

## 在本仓库怎么干活

- **实现任何机制前，先对照 FairyGUI 源码**（`temp/FairyGUI-unity/`）。LoomGUI 的渲染/对象模型/批合/事件/动画/资源管线全面借鉴 fgui。先读对应 fgui 文件看它怎么做，再定设计。fgui 是 Built-in RP——URP/shader/材质 API 要适配。
- **设计文档 vs 踩坑**：`docs/design/main-design.md`（设计契约/当前实现真相）、`docs/design/fence.md`（围栏）、`docs/roadmap/roadmap.md`（范围+机制草稿）、`docs/pitfalls.md`（踩坑全库 + 依赖 API 适配，开工前读它查"具体怎么干 + 坑在哪"）、`docs/superpowers/specs|plans/`（历史 per-feature 记录）。
- **Rust edition 2021**，依赖钉版本：`taffy 0.5`、`ttf-parser 0.20`、`cssparser 0.34`、`scraper 0.19`、`slotmap 1.1`、`csbindgen 1`。snapshot 测试用 `insta`。
- `Cargo.lock` 入库（根级，尽管 `.gitignore` 有通用 `Cargo.lock` 行——它是被追踪的）。
- 设计师工作区在 `loomgui_unity/Assets/LoomUI/`（showcase 打包源 + res 资源 + design-systems 组件库）。编辑器工作流用 `LoomGUI > Settings` 面板配置 + 初始化。
- 用户只读中文——问答/选项/总结用中文；代码/commit 照旧英文。
- **代码注释写上线品质**：自包含（不看其他文件就能懂）、精简（说 WHY，不复述代码机制）、**不引用内部编号或暗语**——`坑 120`、`Venkify 法`、`与某某 meta 对齐` 这类项目内指代外人看不懂。坑号只属于 `docs/pitfalls.md`，不进代码。

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

**易重踩的高频坑**：
- 坑 10 stale .dll、坑 66 改 parse-time 必重打 pkg、坑 41/43 跨 crate 签名变更漏改
- 坑 34 `#[repr(u8)]`、坑 35 csbindgen struct 手补镜像、坑 39 borrow_events out_len 是 count 非字节
- 坑 57 围栏外标签硬挡/属性静默死 CSS、坑 94 l-container 假自定义元素
- 坑 102 cdylib FFI 入口 panic 拖垮宿主、坑 103 tick 时序 rematch 在 compute/solve 后致伪类改 transform/布局属性丢（**已修**，tick 重排）、坑 106 payload_hash 位置-bake 致位置变误 Full（双 re-base 中间漏）
- 坑 56/75/76/105 dirty hash 采样漏字段（**已根治**，双 hash 全量）、坑 107 剥离 uniform 漏改旧烘焙路径致双乘
- 坑 54 fgui v2 非 v²、坑 79 shader tex×vcol 非 CSS 合成
- 坑 113 动态字体 atlas rebuild 漏刷 text（偶现上下颠倒，光读代码定位不了，必运行时 log 取证）、坑 114 blob pen_y 多加行高（单行掩盖，plan 公式照抄）、坑 119 CJK 缺字 advance 不匹配（Rust .notdef 0.6em vs Unity fallback 1em → 字距重叠；多字符类字距问题先确认症状落 CJK/Latin/数字哪类，别假设）
- 坑 131-133 CSS 语义集成 bug 逃过单测（display:none 子树不剪 / runtime color 继承缺失 / transition 逗号多 spec 被吞）——只在 PlayMode 显现，SDD 后必跑 showcase 验收
- 坑 122 生成器输出路径要随产物归属变（csbindgen build.rs 插件移包后路径未改 → 每次 build 重生 straggler）、坑 123 `git add <name>` 不含同名 `<name>.rs`（测试外提漏 commit 主文件，add 后必验暂存集）、坑 124 UPM 包代码引用包资源用 `Packages/<name>/` 非 `Assets/`
