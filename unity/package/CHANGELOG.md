# Changelog

All notable changes to `com.loomgui.unity` will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]
### Added
- **runtime skill 自足**：新增 `loomgui-runtime` 的 `references/api-reference.md`（随 init 落工作区会话根，完整公共 API 查找表——对象层级、控件 role 全表、事件、ListView、动画、样式、异常）。此前 skill 把「完整 API 契约」指回 LoomGUI 源码仓库的 `docs/design/public-api.md`，逼消费者 agent clone 源码翻文档；现以随包 C# 签名为准镜像成离线参考，skill 不再指路仓库。防漂移门加对账（role 宇宙 ↔ fence schema、skill 必须指名 references、禁止回指 repo 文档）。

## [0.0.5] - 2026-08-19
### Added
- **loom CLI**（打包器 CLI 升格，二进制 loom.exe，随 Release 分发 + Editor/Tools 双 exe）：check（零写入校验，--format json 机读诊断）、build（结构化输出）、init（脚手架 + CLI 自拷贝到 .loom/ + 反向配置）、new / list / show / font add / atlas add（workspace 编排——AI 的主编辑路径）、version。
- 诊断 collect-all 修复：跨组件/跨包/注册表/资源（字体缺失、图集溢出、覆盖缺失与冲突）全量收集后统一报告，一次给全（修前首个含 Error 的组件即中断）；失败时 warning 一并携带。
- 退出码契约：0 干净 · 1 Error 级诊断/写命令冲突 · 2 用法/配置/io 错。
- 反向配置 `.loom/config.json`（ui_root + unity_root 双指针，基座）：output_dir 相对 Unity 工程根解析，AI 在会话根一步 loom build 直落 Assets/Bundles。
- 版本同轨：loomgui_pkg crate 版本 == Unity 包版本（release-check 断言），loom version 单一来源。
- **工作区拓扑重构（Tripawd 反馈：skill 困在 ui 目录、AI 会话管不到 Unity）**：会话根 ≠ ui 目录分离形态——skills 与 `.loom/`（loom.exe + config.json，整个入库，团队 clone 即得配套 CLI）落会话根，`loom.workspace.json` 留 ui 目录；`loom init <root> --ui <dir>`（省略 `--ui` = 单目录老形态），config 发现规则统一（会话根 / ui 本体 / ui 直接子目录都可作参数或 cwd）。不再生成 AGENTS.md / CLAUDE.md（入侵性），`--agent` 只决定 skills 目录（.claude/skills / .agents/skills）。
- **agent skills 三件全部重写**（对齐成熟 skill 范式：Figma figma-use / OpenAI figma-implement-design / unity-cli-loop）：`loomgui-editor`（操作手册——Critical Rules 集中开篇、增量工作流含浏览器预览自验步、❌/✅ 反模式、错误表带修法、收尾清单；完整查找表渐进披露到 references/ 三件）；`loomgui-runtime`（**新增**——场景挂载、加载管线钩子、`Get<T>`/事件、`IsPointerOnUI` 门控 3D、NativeHost 内嵌 3D、id 契约双面互指，补上「UI↔3D 桥」）；`loom`（uloop 范式命令手册 + workspace.json/config.json 字段表）。随包 `Editor/Resources/LoomGUI/skill/` 副本删除（新拓扑下 root skills 全覆盖，消除三份漂移面）。
- **GUI 打包器向导双目录**：新建工作区选会话根 + UI 目录（默认 `ui`，允许 `.` 单目录形态）；「打开工作区」接受会话根或 ui 目录（config 发现解析），recent 列表存原始路径。
- `loom font add`：字体已在 `fonts/` 目录时跳过自拷贝直接注册（此前同源同目标拷贝在 Windows 报共享冲突，形似文件被锁）。
- **`<link rel="stylesheet">` 外部 CSS 支持**（Tripawd 反馈）：href 相对所在 HTML 文件（页面与组件同规则）、CSS 内 `url()` 相对 CSS 文件；规则/`@keyframes`/诊断与内联 `<style>` 同待遇，缺文件报 `FenceStylesheetNotFound`（此前静默丢弃）。
- 检查器修复（Tripawd 反馈）：class 规则声明的 `display:block` 此前不被 inline 上下文检查认（报错文案给的修法 (2) 失效），现与 inline style 同待遇；`FenceMixedInlineBlock` 文案不再误称 span 为 block container；组件 `@keyframes` 同名同内容多实例展开静默去重（此前每实例一条告警刷屏）；自定义元素嵌 `<span>` 的报错补教学（slot 属性写在直接子上）。
- 结构检查选择器覆盖对齐（Tripawd 反馈 12–14 批）：display 判定现认静态可判定的单 compound 选择器——class / id / 属性选择器（`[role="tablist"]`、`[data-slot="fill"]`），与控件 CSS 命中检查同覆盖；运行时可变状态属性（aria-checked 等）仍保守不放行。文档：装饰框（背景图 + 前景内容）canonical pattern 写进 skill 与 `FenceMixedInlineBlock` 文案；`switch`/`radio` 无框架槽位（knob 位移用 `[aria-checked]` 状态选择器）写进 skill 与 fence.md。

## [0.0.4] - 2026-08-18
### Added
- Runtime API：`z-index` 层叠、动画 longhand 属性、dropdown 视口定位（pkg 格式 v38）；调度器三件套、`UnloadPackage`、选项 getter、`GetTemplate`（pkg 格式 v37）。
- FFI 全导出统一 panic 边界（catch_unwind guard）；`get_live` 站点标签（函数名格式）+ 释放审计日志常驻，release dll 内「快照后死亡」类 panic 可一行定位。
- 围栏：控件结构 CSS 契约（combobox anchor + popup absolute 定位）；`-webkit-text-security`（disc/circle/square/none）。
- 打包器 GUI：per-project 最近列表 + 移除按钮；窗口标题改短为 "LoomGUI"。
### Fixed
- `TextContent` 每帧重建泄漏 + NodeId 12-bit generation 回卷守卫；`TextContent` 清子后子 wrapper 正确标 disposed（调用方句柄读数抛 `ObjectDisposedException` 而非静默 no-op）。
- 世界空间控件几何 + 自滚动列表虚拟化；文本管线正确性、嵌套滚动命中、dropdown/滚轮手感（浏览器校准基线 + notch 单位直传）。
- pkg.bin 格式版本随 bincode layout 变更强制 bump（v35 旧包全部失效的根因修复，此后 v36→v38 逐版推进）。

## [0.0.3] - 2026-08-16
### Added
- showcase m2-animation #11/#12 程序化动画驱动接线：`Play` / `OnKey` / `OnHook` 锚点 + 动画句柄 Pause/Resume/Stop/Seek 全套演示。
### Changed
- 本版无 runtime 包变更（仅 showcase 演示接线与工程色彩空间设置）。

## [0.0.2] - 2026-08-16
### Added
- 组件系统：Custom Element 打包期展开（组件 registry + slot 接驳 + 完整校验集）、L3 查找作用域硬墙（`Get` / `Query` 不穿透组件 / List item 边界）、`custom_tag` 选择器与 `CustomElement.Tag`。
- 声明式动画：`@keyframes` / `animation`、`Container.Play` 句柄（Pause/Resume/Stop/Seek）、`Container.RestartAnimations` 原位重启。
- 视觉补全：CSS 渐变背景（linear / radial / ellipse + GRADIENT shader 变体）、box-shadow。
- 虚拟列表 slot 模型（parked-but-attached，slot 永驻子树、离场仅标记）。
- 打包器 GUI（Tauri）exe 闭环，Unity 菜单 `LoomGUI > Open Packer` 拉起。
- runtime API 接线：`Node.Touchable`、`Container.ScrollPos`、NumberField 边界 / `Radio.Name` / Slider `IsIndeterminate`。
### Fixed
- 圆角裁切 SDF 像素空间化（rounded overflow clip 视觉扁平）；radial circle 关键字 extents、ellipse 角点 √2 贯穿。
- 文本叶子按可用宽度换行；显式 `min-width` 在被测量的文本叶子上保留。

## [0.0.1] - 2026-08-09
### Added
- 首个可安装 UPM 包。骨架链（div + 文字 + 图 + flex + cascade）从 HTML/CSS 一路通到 Unity 真机渲染。
- Runtime 公共 API 表面（Node/Container/Button/... 类型化投影层）。
- 围栏验证器（标准 HTML/CSS 子集，打包期报错）。
