# LoomGUI 打包器 GUI 编辑器 — Tab 布局重构

**日期**: 2026-07-11
**状态**: 设计已获用户认可，待写实现计划

## 背景

打包器 GUI（`loomgui_gui`，Tauri 2 桌面 app）当前编辑界面一屏堆叠 workspace / packages / atlases / fonts / build-log 五个 section，易用性差：

- 拖拽失效（`onDragDropEvent` handler 逻辑问题；API 本身已由 `withGlobalTauri` 经 `bundle.global.js` 注入，确认可用）
- 建包 / 加图集 / 加字体都靠 prompt 弹窗手动输入路径，不人性化（目录选择器已在前序改动中接入新建/打开工作区）
- build log 常驻占主区空间

## 目标

把编辑界面重构为 **tab 布局**，统一"拖入即建 + 重选目录 + 删除"交互模式，日志改弹窗，补一个"初始化工作区"入口。保持单文件前端，YAGNI。

## 设计

### 1. 整体布局

```
┌──────────────────────────────────────────────┐
│ [通用][包][图集][字体]      日志  打包  ◀返回 │  顶栏
├──────────────────────────────────────────────┤
│                                              │
│  当前 tab 内容                                │  主区
│                                              │
└──────────────────────────────────────────────┘
```

- 顶栏：tab 切换 + 【日志】【打包】按钮 + 返回（回启动屏）
- 主区：只显示当前 tab 的内容
- 去掉原来一屏堆叠的所有 section

### 2. 通用 tab

配置 `output_dir`（用户口语称"工作区目录" = build 产物输出到哪，相对工作区根，如 `../loomgui_unity/Assets/Bundles`）+ 初始化工作区。

- **输出目录**：
  - 文本框显示当前值
  - 支持拖目录进来 → `relativize` 成相对路径更新
  - 右侧【打开】按钮 → 弹原生目录选择器（`plugin:dialog|open`）重选，确定后更新 output_dir
- **初始化工作区**按钮：
  - 调后端 `init_workspace(path)`
  - 把 `CLAUDE.md` + `.claude/skills/loomgui-editor/SKILL.md`（从 `templates/`，`include_str!`）覆盖拷进工作区
  - **不碰** `loom.workspace.json` 和 showcase / res 等源文件
  - 用于补齐或更新框架脚手架

### 3. 包 / 图集 / 字体 tab（统一交互模式）

三个 tab 共用一套交互：

- **列表**：每项一张卡片，列表区本身也是 dropzone
- **拖入即建**：拖目录 / 文件到列表区自动建项，名称从拖入项推导：
  - 包 / 图集 = 拖入目录的 basename
  - 字体 = 拖入文件名去扩展名
  - 用户事后可改名称
  - **删掉所有手动输入弹窗**（建包的包名 prompt、加图集 / 字体的 prompt）
- **每个目录 / 文件字段**：拖入追加 + 右侧【打开】按钮（弹选择器重选，放删除按钮**左边**）
- **删除**按钮：删该项

各 tab 特有字段：
- **包**：源目录（多个，拖入 / 重选）+ html（自动扫，保留"手动指定 / 恢复自动扫"）
- **图集**：资源目录（多个，拖入 / 重选）+ default / standalone 勾选 + max_size + padding
- **字体**：file（拖入字体文件 / 重选）+ default（单选）+ fallback（勾选）

### 4. 日志

- 不常驻主区
- 顶栏【日志】按钮 → 弹 modal 显示 build log
- 打包后日志写入 modal；关闭 modal 收起
- 打包进行中 modal 显示"构建中..."

### 5. 拖拽修复

`onDragDropEvent` handler 修复（API 已注入，问题在 handler 逻辑）：

- `over` / `enter`：用 `document.elementFromPoint(pos.x/dpr, pos.y/dpr)` 命中 dropzone，加 `drop-active` 高亮
- `drop`：`relativize(拖入绝对路径)` → 按 dropzone 类型建项或追加目录 / 文件
- `leave`：清高亮

重点查 `position` 坐标的 devicePixelRatio 缩放、payload 字段名（`type` / `paths` / `position`）。

### 6. 后端改动

新增命令 `init_workspace`（`commands.rs`），复用 `create_workspace` 的脚手架拷贝段（`include_str!` templates），但**不写** workspace.json：

```rust
#[tauri::command]
pub fn init_workspace(path: String) -> Result<(), String> {
    // 拷 CLAUDE.md + .claude/skills/loomgui-editor/SKILL.md 到 path（覆盖）
    // 不碰 workspace.json / 源文件
}
```

`main.rs` 的 `generate_handler!` 注册 `init_workspace`。

目录"重选"全部前端调 `plugin:dialog|open`（dialog plugin 已装 + `capabilities/default.json` 已授权 `dialog:default`），不加后端命令。

## 涉及文件

- `loomgui_gui/src/commands.rs` — 新增 `init_workspace`
- `loomgui_gui/src/main.rs` — 注册 `init_workspace`
- `loomgui_gui/dist/index.html` — tab 布局 + 日志 modal DOM
- `loomgui_gui/dist/editor.js` — tab 切换 + 各 section 重构 + 拖拽修复 + 打开按钮 + 日志 modal
- `loomgui_gui/dist/style.css` — tab / dropzone / modal 样式

## 验收

- tab 切换正常，主区只显示当前 tab
- 通用 tab：输出目录可拖入 / 重选；初始化工作区拷脚手架（不动 workspace.json）
- 包 / 图集 / 字体 tab：拖入建项、字段拖入 / 重选、删除，全程无手动输入弹窗
- 拖拽工作（从系统拖目录 / 文件进 GUI）
- 日志按钮弹 modal，打包后显示日志
- 完整打包流程：配置 → 打包 → 产物落 output_dir（pkg.bin + atlas + fonts + runtime.json）
- release exe 无控制台黑窗（回归确认）

## 不做（YAGNI）

- 不拆前端模块（保持单文件 editor.js）
- 不加"在文件管理器中打开"（两类打开按钮都是重选）
- 不加 workspace.json 的重置 / 清空（初始化只补脚手架）
