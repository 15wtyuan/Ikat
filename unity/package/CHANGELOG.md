# Changelog

All notable changes to `com.loomgui.unity` will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.0.6] - 2026-08-20

Tripawd dogfood Field Notes（N 系列）回应批：四个运行时 bug 修复、围栏值域门与
浏览器先验警告族、公共类型改名、工作区生成物刷新通道。

### Fixed
- **投影内容样式失效（N5/N6）**：组件 `<style>` 规则现在真正作用于 slot 投射的
  light 子（文档语义本就如此，运行时未兑现）。根因：投影 span 在页面宇宙被烘
  `rich_text_block` 折叠标志（页面侧分类看不到组件 CSS 的 `display:flex`），
  折叠优先于运行时 display。修复：rematch 应用 `display` 声明且终态为 Flex 时
  翻转布局策略（display 选择 Strategy 的架构不变量兑现）；`set_inline_override`
  同语义。span 带 class 规则 `display:flex` 也在打包期正确解除折叠。
- **flex 列居中容器内无宽文本逐字竖排（N7）**：taffy 某些测量轮次传
  `Definite(0)` 可用宽，首个 0 宽测量经 render 槽「Some 优先」策略钉死成多行
  布局。修复：退化 0 宽约束按无约束处理（浏览器语义：0 宽盒文本横向溢出）。
- **重复 `Node.Play` 静默无效（N8/N11）**：programmatic player 不回收——旧
  Completed+fill-both player 每帧续写末值且永不回收，player 无限累积。修复：
  `Play` 按「同节点+同名」替换旧 player，重复调用 = 确定性从头重播。
- **同节点换名 `Play` 被旧 player 遮蔽（战斗第二回合起动画不播）**：同名回收
  之外，不同名但动了相同通道（如都是 transform）的旧 player 仍每帧续写末值，
  新动画按 slotmap 槽序被静默盖掉。修复：`Play` 接管其所动通道——同名或通道
  重叠的旧 player（不限状态）一律回收；通道不相交（transform + opacity）仍
  共存可组合。
- **滑杆 thumb 偏上（N20）**：作者给 thumb 写定位（负 `top` 居中 / `left` 百分比）
  与控件自身的居中/位移 transform 叠加成双偏移。修复：thumb 定位权归控件——
  运行时逐帧归零其 inset/margin，位移全权由控件按 value 驱动；check 新增
  `FenceSliderThumbPositioned` 警告提示所有权（`left:0; top:0` 锚定与尺寸/外观
  不受影响）。附带修复：inset 四边的 `%` 值此前被运行时静默丢弃（fence 广告的
  `LengthPercentAuto` 语法未兑现），现在按含块百分比正确解析。
- **卡内悬停消失/闪烁（N23）**：C# 事件层曾对 Enter/Leave 统一走 capture→bubble
  祖先链路由，而 core 的 RollOver/RollOut 按悬停链差分逐节点发射（mouseenter/
  mouseleave 语义，本不冒泡）——「后代退链」的 Leave 被误投给祖先订阅，与
  enter/leave 驱动的抬升动画叠加成自激振荡。修复：Enter/Leave 只派发给事件
  目标节点自身，其余事件维持冒泡。
- **InputSystem-only 项目 F8 诊断每帧抛异常（N3）**：`LoomStageDriver.Update`
  的 F8 轮询按 `ENABLE_INPUT_SYSTEM` / `ENABLE_LEGACY_INPUT_MANAGER` 分流。
- `background-size: stretch`（schema 广告的默认值）此前被 core 静默拒（仅认
  `100%`）；`resize` noop 声明此前误报 `FenceBadCssValue`。

### Added
- **ProgressBar.AnimateValue(target, durationSec = 0.4)**：演出缓动糖——fill 宽
  度走布局通道无 CSS 过渡（transition 只支持背景/文字/透明三通道），C# 投影层
  easeOut 插值。`Value` 动画期间读回目标（数据值），直接赋值显式获胜并取消动画；
  重定向从当前显示值平滑转向。
- **围栏值域门（error，双路径统一）**：命名色（`red` 等）与 `transparent`
  （color 之外）、`overflow: clip` 及拼错值、`filter: blur/drop-shadow`、
  `transform: skew/matrix` ——运行时恒无效的浏览器合法值全部打包期报错（此前
  静默吞掉，上线即坏）；`<style>` 规则的 Keyword 值域与行内同门。
- **浏览器先验警告族（warning）**：`display: inline` 语义偏差（按 flex 处理）；
  `transition` 属性域外（含 `all`）；rich-text inline flow 内 span 的死
  width/height；页面侧只可能命中投影内容的类规则（样式墙下恒死代码）。
- **工作区生成物刷新通道**：`loom scaffold` 现为生成物全刷新（三 skill +
  `.loom/` CLI 自拷贝 + `.loom/scaffold.version` 版本戳；config/workspace.json/
  源文件不碰，无 `--agent` 时按在场 agent 目录自动探测）；`loom check` 发现
  版本戳落后出 `StaleScaffold` 警告；GUI 打开工作区时探测并亮「更新工作区」
  按钮（一键 = loom scaffold 子进程）。消费端更新流：UPM 更新包（新双 exe 随
  包落地）→ 跑一次 scaffold 刷新。
- runtime API reference 补齐：值类型工厂（`Length.Px/Pct`）、`LoomColor` 4 参
  构造、transition 支持矩阵、ProgressBar 值域（`[0, Max]`，Max 默认
  aria-valuemax=100）与 AnimateValue、`Image.Src` key 格式与验证手段、
  LoomStageDriver 序列化字段速查（`_designSize` 默认 1080×1920 竖屏警示）。

### Changed
- **公共类型改名（breaking，消 CS0104 歧义）**：`Animation` → `AnimationHandle`、
  `Color` → `LoomColor`、`Vector2` → `LoomVector2`、`Rect` → `LoomRect`、
  `KeyCode` → `LoomKeyCode`（接线层同时 `using LoomGUI; using UnityEngine;` 时
  每文件必撞歧义，N10）。C# 侧机械替换；FFI ABI 不变。
- `box-sizing` 错误引导文案修正为 content-box 事实（此前误称 border-box）；
  Keyword 值错误消息列出合法值域。

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
