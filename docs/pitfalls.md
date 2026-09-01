# Ikat 踩坑记录

> 只收**可复用规则**：依赖/平台不讲理的事实、跨层动态契约——看代码看不出来的才配进这里。
> 新坑按主题归位、不编号；bug 编年史不记（代码 + git history 是载体）。
> 历史 231 条全文见 [archive/pitfalls-2026-08.md](archive/pitfalls-2026-08.md)（v0.x–v1.x 纪元，只读）。

## 1. 依赖 API 适配

> plan/草稿写的 API 常与 crate 实际不符——遇编译错按 crate 实际源码调（`~/.cargo/registry/src/<crate>-<ver>/src/`），勿硬改依赖版本。

### taffy 0.12（core/src/layout）
- **trait 对象模式，无 `MeasureFunc` 枚举**：`TaffyTree<NodeContext>` + `new_leaf_with_context(style, ctx)` 存 owned 测量上下文，单个 `compute_layout_with_measure(root, avail, FnMut)` 闭包按 `Option<&mut NodeContext>` 分派 Text/Image 测量。
- 测量闭包是 `FnMut` 非 `'static`——闭包内借 `&FontTable` 合法，不需要 Arc。
- **`LengthPercentage` 是 tagged pointer**（`pub struct(CompactLength)`，内字段私有无法 match 变体）——解构用 `into_raw()` + tag 位判 Length 分支。
- `Style` 无 `order` 字段——flex order 排序做不了，渲染按 DOM 顺序。
- **`overflow ≠ Visible` 必须显式设置**：flex 自动 min-size=0 只对非 Visible 生效——不设则容器被 content min-content 撑开，scroll overlap=0 失效。
- **auto-min 是裸 min-content，不实现 CSS 的 specified-size suggestion**：空/无内容容器 min-content=0 → 溢出的 flex 行会把**显式定尺寸的兄弟**也按比例挤扁（76px 顶栏被压成 50px；浏览器以声明尺寸为地板）。近似修法（core layout build 已做）：`size` 为 Length 且 `min_size` 为 Auto 时把 Length 复制进 min——作者显式 min 声明永远赢。
- `serde` feature 可整体序列化 `Style`（pkg 格式依赖它）；bincode 编码随 taffy/bincode 版本走——升级依赖必 bump `PKG_FORMAT_VERSION`（见 §2）。
- **行为怪癖**：① span 显式 flex + padding + 文字子 → flex 容器不做文本测量，宽度退化为 padding 值；② 测量不能只看 `known_dimensions`，须结合 `available_space`（否则定宽容器内文本不换行）；③ 某些 sizing 轮次传 `Definite(0)`——首个 0 宽测量若被当最终结果钉死，文字会竖排。
- **Block 流不实现 gap**（flex 才读 `row_gap`）——block ul 的 spacer 间无 gap，可见区计算盲扣会让 spacer 偏矮、滚动条失真；flex 容器的 gap 必须计入可见区累积位，漏计 = 视口顶部空白。
- **增量 API 事实**（持久树复用必备）：`set_node_context` 存在（换 ctx + 标脏）；`set_children` 自带从旧父摘挂 + `mark_dirty`；`remove` 只摘自身、**子节点留孤儿滞留树内**（删子树须逐节点 remove）；`mark_dirty` 递归上溯祖先（已脏早退）；`Style`/节点上下文可 `PartialEq` 值比较短路 set；`children()` 返回 `Vec` clone（比较别怕贵）；compute 对干净子树跳过（布局缓存按节点粒度）。
- **MinContent 约束下子项 min-size 被转成 `Definite(min)` 可用空间，wrap 容器据此真换行且几何泄漏进最终布局**（#82）：滚动容器祖先会触发子树 min-content 测量；flexbox 在 MinContent 约束下测子项时，子项有 min-size 就把可用空间换成 `Definite(min)`——列方向 wrap 容器的主轴行收集在 Definite 下会真换行（MaxContent 才永不换），且该测量相位的换行几何会写进当帧最终布局（子项横排一帧后复原；容器高度仍正常，因 `determine_container_main_size` 取 `max(最长行, available)`）。触发组合 = **列方向 wrap + 自带 min-height + 滚动容器祖先**（Blink 无此泄漏）。页面规避：列容器别带 wrap（含类规则残留）；core 修法在 taffy 内部，见 issue #82。

### ttf-parser 0.20（core/src/text）
- kerning 在 `face.tables().kern.subtables` 遍历（取 horizontal 非状态机子表），`.glyphs_kerning(GlyphId, GlyphId) -> Option<i16>`。
- `glyph_hor_advance(GlyphId) -> Option<u16>`（无旧名 `glyph_advance_width`）。
- `.ttc`（TrueType Collection）`Face::parse` 第二参 = collection index，index 0 未必是目标 face。

### slotmap 1.1（core/src/scene）
- `new_key_type!` 生成的 Key 是 `KeyData { idx: u32, version: NonZeroU32 }` **64bit**，且字段私有不能自定位型——NodeId 保持手写 `pub struct NodeId(pub u64)`（位型 idx:32+gen:24+tag:8，tag 字节归渲染合成 id 命名空间）+ `from_key/to_key` 桥接，勿改用 new_key_type!。
- idx 从 1 起（0 是 sentinel slot）；version 恒奇 = occupied；`capacity()` 是总槽位且 remove 不缩——并行数组按 `capacity()+1` 分配才不越界。

### csbindgen 1（双扫描点：ffi/build.rs + xtask/src/bindings.rs）
- **两处独立 csbindgen 扫描清单互为镜像**——`input_extern_file` 只显式列文件，FFI 函数挪进新模块必须两处同步补，漏一处绑定**静默缺函数**（编译全绿，运行时 EntryNotFound 才炸）。
- 默认生成 `internal` 类型——跨程序集访问须 `[assembly: InternalsVisibleTo]`。
- 类型映射：opaque `*mut T` → `T*`（类型化指针非 IntPtr）；`csharp_use_function_pointer(false)` 切 Mono 模式。
- C# `fixed (T* p = &localVar)` 非法（CS0213 already fixed）——`fixed` 只 pin 托管对象（数组/string），局部变量直接取址。
- **FFI enum 出口用 return-code + out-param，勿用 0 当「不存在」哨兵**——首变体判别值 = 0（如 NodeKind::Container），会与合法值相撞、无法区分。

### Unity Input System 1.19（IkatInputCollector.cs）
- 双路径 `#if ENABLE_INPUT_SYSTEM`（`Mouse.current...` 新 API）/ else 旧 `UnityEngine.Input`；asmdef 引用名是 `Unity.InputSystem`（非 `UnityEngine.InputSystemModule`）。

### image 0.25（packer）
- PNG 编码到内存用 `RgbaImage::write_to(&mut Cursor<Vec<u8>>, ImageFormat::Png)`（无 `save_buffer_to_memory` 这个 API）。

### unicode-linebreak 0.1（core/src/text）
- `linebreaks() -> impl Iterator`（非 Vec）；枚举名 `BreakOpportunity`；返回 **byte offset**（非 char index）；在空白**后**断 → 行首无多余空格。
- UAX#14 的 LB13/LB14 类规则已天然覆盖 CJK 避头尾（闭标点前/开括号后不断）——软断行路径的行首禁则白送；只有**自造断点**的路径（`overflow-wrap: break-word` 逐字拆、rich token 贪心路）需要手工禁则调整。别在软断行上重复实现禁则。

### cargo / crates.io 网络（Windows 本机）
- 在线 cargo 命令（fetch/test/clippy）半刷新索引时可能**把 Cargo.lock 写到新版本再下载失败**——此后 `--offline` 也挂（lock 指向缓存里没有的版本，报「failed to download X / attempting to make an HTTP request」）。恢复序：① `git diff Cargo.lock` 有漂移先 `git checkout Cargo.lock`（提交态通常全缓存）；② 仍缺的 crate 用 `cargo update -p <crate> --precise <缓存已有版本> --offline` 降回（须满足语义版本约束）；③ 无缓存同系版本的从镜像直下进 `~/.cargo/registry/cache/index.crates.io-*/`——USTC `https://mirrors.ustc.edu.cn/crates.io/crates/<name>/<name>-<ver>.crate` 实测可用，static.crates.io / rsproxy 常被掐断。诊断用 bash 对账：lock 里 name|version 逐个查 cache 目录（别用 MSYS python，glob 不展开 `~`）。

### Tauri 2（packer/gui 前端）
- `onDragDropEvent` 的 `position` 是物理像素，须除 `devicePixelRatio` 才是 CSS 逻辑坐标；payload 字段名 `type`/`paths`/`position`。

### 本机 shell（Git Bash on Windows）
- heredoc 喂 python 会**吃掉一层反斜杠**：想在替换串里产出源码字面量 `'\n'`（python 里写 `\\n`），实际落到 python 是真换行——批量编辑含转义序列的 Rust/JS 源码必坏且难察觉（实锤：把换行器测试注释改出断行、断言匹配静默失配）。含反斜杠字面量的文本编辑用 Edit 工具；纯结构化替换（无反斜杠）才走 heredoc python。
- **全仓批量替换的文件集必须把字体族纳入二进制黑名单**：`.bin/.png/.dll/.exe` 之外还有 `.ttf/.ttf.bytes/.ttc/.otf`——字体被当文本替换后字节错位，症状是测试大面积 `NoHeadTable` / `need font` panic，且只炸加载该字体的那部分用例（DejaVu 全绿仍有 wqy-microhei.ttc 在烂）。提交前 `git checkout -- <file>` 可零损失回滚，验收 = 全量 cargo test 归零才算二进制无害（更名批 #91 实锤：14 个 ttf + ttc 受损靠测试才现形）。另：`xargs` 默认按空白拆路径，带空格路径会伪报 can't read——统一 `tr '\n' '\0' | xargs -0`。
- 另外 GNU sed 在部分 `-E` 场景对 `\1/\2` 反向引用替换静默失效（不报错、输出原样）：带捕获组引用的替换跑完必须 grep 抽验命中率，不确定就用字面量枚举替换单条验证。
- **Windows 上 spawn npm 全局 CLI 须 `.cmd` 回落**：npm 全局装的是 `tauri.cmd` 之类的 shim，`std::process::Command::new("tauri")` 走 CreateProcess 只自动补 `.exe` 不补 `.cmd` → spawn 失败；裸名失败后用 `tauri.cmd` 重试。bash 里裸名能跑（bash 自己解析 shim）会骗过手工测试——程序化 spawn 必须两个名字都试（xtask gui.rs 实装）。

## 2. 跨层闭环规则

### pkg 格式 bump 代价链
改任何进 `.pkg.bin` 的序列化布局（ResolvedStyle、ControlInit、bincode 结构）→ **必 bump `PKG_FORMAT_VERSION`**（含 MIN/MAX + mod.rs 顶部 changelog 注释）。bump 的代价链（v42 全程实录）：① `core/asset/tests.rs` 有 4 处钉死版本号的断言要同步升；② 重打 14 个 fixtures（13 个 `tests/dotnet/.../fixtures/*.workspace` + showcase 直出 `Assets/Bundles`）并拷回 `*.pkg.bin`——已机械化进 `xtask reout`（重打 + 拷回 + 清构建现场），MIN/MAX 同拍另有护栏测试 `min_version_tracks_current` 当场拦（v48 实证：漏拍 MIN 会让旧包漏过版本门、以 Bincode 结构错配炸成无指引的 malformed，0.0.16 CI 红一天才定位）；③ `packer/pkg/tests/schema_lock.rs` 用失败信息里的新哈希更新 `LOCKED_HASH`；④ golden 事件流 `IKATGUI_UPDATE_GOLDEN=1 cargo test -p ikat_ffi_c --lib golden` 再生成；⑤ C# `GoldenEventsAndAbiLayoutTests` 的 `REC` 常量 + 尺寸断言同步；⑥ 重编 .dll + 重出双 exe。漏一环就版本错配（stale pkg / loader rc=-1 / 「tag for enum is not valid」），且常在离改动最远的 consumer 测试才炸——文本 merge 干净 + cargo 全绿 ≠ C# 测试绿。flags 字节加位**布局不变**（字节宽不变、旧包位恒 0）可免 bump 走语义增量（v47 disabled 位实装）。

### 长跑进程锁构建产物
- **`ikat preview` server 持锁 `target/debug/ikat.exe`**：进程活着时任何 debug 构建/测试都「failed to remove file 拒绝访问 (os error 5)」——多会话共享工作树时常见且难归因（错误像编译失败）。解法：跑测试用私有 `CARGO_TARGET_DIR=<临时目录>` 隔离；release 产物不受影响（锁的是 debug 路径）。别杀别人的 server。

### 机制设计/删除前置检查
- **设计渲染合成机制前先读对端 shader 能力**：曾设计整套合成 RenderNode 机制，被一次 shader 阅读推翻——对端早已做 source-over 合成。core program 编号 ↔ Unity shader 能力是跨层闭环，先核对两端现状再设计。
- **删「看似单用途」机制前先 grep 共用者**：曾视作单用途的 flag 实为两条路径共用（div box-shadow 与文字效果层），grep 取证救过一次静默错渲染。
- **fence 委托 + core 解析永真 = 围栏静默放行破损**：fence 零自有校验、委托 core `apply_decl` 的 CSS 属性，core 解析失败必须返 false，否则 `FenceBadCssValue` 链路整体失效——「fence 放行 + core 渲染坏」可同时成立、打包不报错。
- **core 反向调宿主服务须启动期注册函数指针对**：core 是 cdylib，不能 extern 调宿主符号（链接期不可解析 + C# 给不出 linkable C 符号）——剪贴板/原生弹窗/系统字体查询都走注册回调模式；内存契约：get 缓冲区宿主持有（活到下次 get），core 立即拷贝、不跨分配器 free。
- **快照测试锁 glyph 度量必须钉仓库内字体**：不同 OS 默认字体（Win arial vs Linux DejaVu）度量漂移、Linux CI 无 arial——fixtures 字体入库是前提，不是优化。

### 本地绿 ≠ CI 绿（clippy 双盲区）
本地 clippy 全绿挡不住 CI 红：① CI 用 `dtolnay/rust-toolchain@stable` 滚动，可比本地 stable 新一档（1.97→1.98 实锤新 lint `chunks_exact_to_as_chunks`）；② clippy 增量缓存跳过未变更 crate——久未 push 的积压 commit 里潜伏的 lint 只在 push 后暴露。push 被 CI bot 镜像 commit 挡直推时先 `git pull --rebase`。

### 预览模拟脚本的级联序（经典 script vs ESM）
旧内联 `<script>`（解析中执行）往 head `appendChild` 的 `<link>`/`<style>` 天然落在页面 `<style>` **之前**；换成 ESM（defer 语义，解析完执行）后同样代码落在**最后**——同名规则级联胜负翻转。预览 base.css 注入必须显式 `insertBefore(head.firstChild)` 复原「polyfill 先、页面样式后」的旧序（showcase/preview/main.js 实锚）。

## 3. Unity 平台特性

- **非纯平移节点的 renderer.bounds ≠ 真实视觉 AABB**：rotate/scale 走 `_ObjM` shader 矩阵、GO 只带平移分量 → Unity 剔除/拾取看到的 bounds = GO 平移 × 未旋转 mesh。任何新消费 renderer.bounds 的功能（剔除/遮挡/视口判定）都须过 `MirrorPool.CompensateMeshBoundsForLinear`（bounds 置线性矩阵 × 顶点 AABB）；否则旋转节点滚动/移动中被错误剔除（#66 实锤）。
- **`git tag` 输出按字典序**：`v0.0.10` 排在 `v0.0.5` 之前——`git tag | tail` 会漏最新版本号、误判「未发过版」。查 tag 存在性用 `git tag | grep <精确版本>` 或 `git ls-remote --tags origin | grep`。

- **EditMode 禁 `Object.Destroy`**（须 `DestroyImmediate`）；Mesh 是独立 Object，GO 销毁不连带——`[ExecuteAlways]` 路径须显式销毁防泄漏。
- **`Cursor.SetCursor` 纹理硬要求**：RGBA32 / 可读 / 无 mip 链 / 标准光标尺寸——`Apply(_, true)`（makeNoLongerReadable）与 4×4 之类非标准尺寸都被 Windows 硬件光标拒收（仅告警 "Invalid texture used for cursor"，不崩）。程序化生成光标纹理用 `Apply(false, false)` + 32×32。
- **`.meta` 须入库**，且 Unity 关着时不生成（新增 .cs 要启动 Unity 才产 .meta）——提代码漏 .meta，别人打开工程全断链。
- **`Resources.Load` 不搜 `Editor/Resources/`**（那是 `AssetDatabase.LoadAssetAtPath` 专用，后者要含扩展名全路径）；`.md`/`.html` 在 Unity 里是 DefaultAsset 非 TextAsset。
- **ScriptableObject 禁 `new`** → `CreateInstance<T>()`（`new` 绕过原生对象追踪，IL2CPP 静默失败或产损坏资产）。
- **shader keyword 须 `multi_compile` 非 `shader_feature`**——未启用的 variant 会被 strip，clip 类功能静默失效且构建期不可见。
- **ShaderLab Properties 无 Matrix 类型**；MPB 只覆盖 `UnityPerMaterial` CBUFFER 内字段——per-renderer uniform 必须进 CBUFFER 才能被 MPB 覆盖。
- **PlayMode 首帧 `Time.unscaledDeltaTime` 可达秒级**（加载延迟）——tween/动画别在 Start 自动播（瞬间 complete 写末值）。
- **UPM 包内代码引用包资源**用 `Packages/<name>/...` 路径，非 `Assets/...`。
- **单通道纹理（R8）存非颜色数据**：采样只采 `.r`（D3D 下单通道 GBA 缺省 (0,0,1)，错采 `.a` 恒 1——SDF 文本全画成实心方块）；上传必须 `linear: true`（否则按 sRGB 采样，SDF 距离场全空、字体消失）。
- **根 y-flip 使 winding 反转**——UI shader 必须 `Cull Off`，漏了 = 背面剔除把整个 UI 吃掉。
- **Domain Reload 保护**：关闭 Domain Reload 时 C# static 活过 Play、native 句柄已释放 → 野指针 crash。`SubsystemRegistration` hook 必须调 shutdown；将来引入全局 native 态（global texture/font registry）在此自动清。
- **读渲染 blob 用定长列 + `BitConverter` 直读**——不用 `Marshal.PtrToStructure` 走 marshal 对齐假设；Unity Mono 缺新 .NET API（如 `BitConverter.SingleToUInt32Bits`），用版本无关等价写法。
- **Material 缓存键不含 shader keyword**——新 keyword 组合必须有独立 key 来源（新 program 号或新 key flag 维度），蹭已有 program/键会命中同一 Material 实例 → keyword 冲突静默错渲染。
- **fgui 的 mesh 合并实靠 Unity Dynamic Batching**（隐式、与 SRP Batcher 互斥——URP 下不可控）；SRP Batcher 只降 CPU 不降 draw call。要真 N→1 必须自己合并 mesh。
- **csproj `<Link>` 引用带 UnityEngine/native 依赖的生产源进纯 net10.0 headless 项目编译失败**——`<Link>` 只拷文件不带依赖链；headless 测试用物理拷贝源文件。
- **C# `using` alias 解不了父命名空间同名类型遮蔽**：子命名空间内的类型名必先命中父级同名类型（如 `Ikat.Editor.EventType` 撞 `Ikat.EventType`），只能全限定名，alias 无用。
- **Unity 新版把弃用 API 升级为编译 error（CS0619）**：`Scene.handle` 的 int 隐式转换（6.2+）、`FindObjectsOfType`（2023.1+ 弃用）、`FlareLayer` 类型（6000.5+ 挪出程序集）都会在对应版本直接编不过，而其「替代品」在旧版（如 2021 showcase 工程）又不存在——跨版本写法：按 `Scene` 本身做字典键（IEquatable）、`UNITY_2023_1_OR_NEWER` 门控 API 选择、按类型名字符串摘组件（`GetComponent(typeName)`）。升级 Unity 或写引擎兼容层时 grep 这批符号。
- **纹理 `Apply(makeNoLongerReadable:true)` 与原地重上传互斥**：页式纹理（字形图集/atlas 页）走「同尺寸脏页 SetPixels 重传」路径，首次上传即弃 CPU 拷贝会让后续每次脏重传抛 "not readable" 且脏位永不清（帧循环中断 = UI 冻结）。须保持可读（`Apply(false, false)`）——光标纹理条目的 `Apply(_, true)` 是一次性消费场景，页纹理不是。
- **URP 的 `CameraClearFlags.Depth` 连颜色一起清**（URP 17 起 Base 相机语义）：叠在宿主 3D 相机之上的 UI 相机用它会把 3D 场景整屏抹掉。clearFlags 须按管线分流：有更深打底相机时 Built-in 用 Depth、SRP 用 Nothing；无打底（纯 UI 首相机）用 SolidColor（不清色会读到未初始化缓冲）。
- **序列化字段的默认值只对「没保存过该字段」的场景生效**：字段是场景保存之后新加的，旧场景反序列化吃字段初始化器；而改初始化器不影响任何已保存过该字段值的场景——两个方向都可能咬人（`_useSharedHost` 后加默认 false，老场景静默不吃共享宿主）。新序列化字段上线时审一遍默认值语义 + 老场景兼容。

## 4. 动态契约
- **CSS 多声明分割必须括号感知**：`animation`/`transition` 等逗号多声明与函数参数逗号共用语法——`split(',')` 会把 `cubic-bezier(.3,0,.7,1)` 的参数切成独立声明，静默错位成默认值（解析不报错、行为不对）。统一走 `mapping::split_top_level_commas`（渐变/rgba 同款）；新收函数形属性（带参数的 CSS 函数值）先过它再分段。

- **dirty hash 的「全量」是动态契约**：每给 RenderNode/Line 加视觉字段，必同步检 payload/header hash 是否覆盖新字段——漏一个 = 静默 stale（不崩、只是不更新）。历史上反复漏过（uvs / 圆角顶点 / line-height / reuse_key / baseline）。
- **纯平移不进 payload 顶点（位置编码在矩阵、顶点是局部系）**：任何「按 payload 哈希判变更」的新路径（批合并定级/缓存复用/脏检测）都抓不住纯平移变化（滚动/Transform 位移）——必须把矩阵平移轴单独入键。merged 批的教训：成员局部 hash 拼合 + 恒 IDENTITY 矩阵 = 滚动拖拽整批冻结在旧位置。
- **高频运行时 setter 必须同值幂等**：逐帧无条件 `Set(true)` 的调用形态（如世界锚点每帧确认可见）× 非幂等 setter（每次写都 bump 失效版本）= 全缓存逐帧 miss 的性能暗坑，且不崩不报。运行时态 setter 入口一律同值短路。
- **查询缓存别缓存 miss**（除非确定源不变）——运行时资源可能后到，缓存 miss 会永久遮蔽后到的正确值。
- **坐标空间劈叉**：`pos` 是世界坐标、`layout_rect` 是页面内容坐标，祖先滚动下两者劈叉——调试命中/滚动偏移先分清在哪个空间。
- **keepalive 保留粒度必须对齐后端 GO 持有粒度**：MirrorPool 是扁平池（slot 根按 reuse_key、叶子按 node_id 独立持有）——core 只发 slot 根 keepalive 保不住叶子 GO，stale 销毁→reactivate 重建→churn 复发；keepalive 须发整子树超集。改 blob 契约或池模型任一侧都要重新对齐粒度。
- **跨树 id 解析必须作用域化**：每新增一种作用域形态（组件实例/List item），全局 `find_by_id_attr` 首匹配就会串实例（组件多实例全部命中第一个）——解析须向上找最近 LOOKUP_SCOPE 根在其子树内做（`find_node_by_id_in_own_scope`）。
- **`remove_node` 联动清理是动态契约**：删节点须同步清全部持久附属表（anim/scroll/controls/roles/lists/text_contents/image_srcs…）——新增持久附属表必须同步加清理，漏一个 = 悬空引用/残留状态。
- **@keyframes 的 transform 只收 px（TRS 像素模型）——百分比静默不动**：`translateX(-100%)` 等百分比形 parse_transform_trs 走 parse_px 直接 None，fence 不校验 keyframe 值域 → 打包零报错、player 照常 Playing 但每帧 props 全 None，视觉静止。写动画用 px（自身尺寸百分比语义需 layout 参与，框架级支持另立票）。
- **ABI 位型/字段宽度变更的静默错解码**：位掩码/移位常量（`& 0x6000_0000`、`>> 24`、`0xFFFFFFFF` 哨兵）在位型拓宽后**编译全过但语义死掉**——必须 grep 全部位常量逐个对新位型表重审，不能只跟编译器走；C# 侧同理，csbindgen 对**函数签名引用到**的 repr(C) struct 会生成含字段布局的 C# stub（v43 IkatTweenSpec 实测），但签名外独立序列化的结构（事件 SOA 偏移、`NativeEventBuffer` 手写偏移）仍无生成物——手写镜像的宽度/布局无编译期保护，字段变宽必须人工重排，且每 struct 配 Rust 侧 `size_of` 常量断言 + C# `Marshal.SizeOf` 对照。另防装箱断言陷阱：`Assert.Equal(42u, ulong值)` 经 object 装箱恒 false 但编译过。
