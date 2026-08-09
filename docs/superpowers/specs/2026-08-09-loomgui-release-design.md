# LoomGUI 发布设计（git URL 分发）

**日期**：2026-08-09
**状态**：待实现

## 背景与动机

LoomGUI 已完成"摸黑"阶段，骨架链（div + 文字 + 图 + flex + cascade）从 HTML/CSS 一路通到 Unity 真机渲染。即将进入"用真实游戏验证"阶段：在**另一个独立仓库**里用 LoomGUI 开发游戏。

本设计解决唯一问题：**如何把 LoomGUI 框架（`com.loomgui.unity` UPM 包）发布出去，供其他 Unity 项目（含那个游戏项目）消费使用。**

`unity/package/` 已是一个合规的 UPM 包，且 `showcase-unity` 工程已用 `"file:../../package"` 本地引用消费它。本设计补齐的是「分发路径 + 版本管理 + 发布流程 + 质量门」。

## 范围

**在范围内**：LoomGUI 框架的公网开源发布（Windows-only native）。

**不在范围内**：
- 游戏项目的发布——另一仓库，独立处理。
- 多平台 native（macOS/Linux/移动端）——Windows-only 起步，后续视情况扩展。
- OpenUPM registry 接入——等框架稳定（约 v0.5+）再评估。
- CI 全自动编 dll（方案 B）——当前用手动编 + CI 验证，理由见下。

## 关键约束

- **git URL 分发的硬约束**：消费方拉 `#v0.0.1` 拿到的是 tag 对应 commit 的快照，**dll 必须在该 commit 内**。这决定 CI 只能「验证 + 出 Release」，不能后置编 dll。
- **Windows-only**：仓库当前只有 `loomgui_ffi_c.dll`（Windows）。声明只支持 Windows，不阻塞起步。
- **公网开源**：走 GitHub 公开仓库的 git URL。

## 方案选择：git URL 直装

消费方在目标 Unity 工程的 `Packages/manifest.json` 加一行：
```json
"com.loomgui.unity": "https://github.com/15wtyuan/LoomGUI.git?path=/unity/package#v0.0.1"
```

**为什么选 git URL 而非 OpenUPM / 自建 registry：**
- 零基础设施，与现状（`file:` 本地引用）架构一致，只换 URL。
- `com.loomgui.unity` 包名已合规，将来升级 OpenUPM 无需改名。
- dll 直接 commit 入库，git clone 零配置（OpenUPM 不支持 Git LFS，这也是选直接 commit 的理由）。

**OpenUPM（备选，不在本期）**：框架稳定后接入，提供版本浏览与社区曝光，包名无需改。

## 产物与版本

### 产物
- 发布产物 = `unity/package/` 整个目录（Runtime / Editor / Plugins 含 dll / Shaders / Tests + package.json + CHANGELOG.md）。
- `showcase-unity` 留在仓库当 demo，**不是发布产物**；消费方用 `?path=/unity/package` 只拉 package 目录。

### 版本与 tag
- 版本号**唯一真相源**：`unity/package/package.json` 的 `"version"`。
- **首版 `0.0.1`**（雏形语义，非 `0.1.0`）。
- SemVer `0.0.x` 阶段：全为"不稳定，随时 breaking"；到 `0.1.0` 再谈稳定化。
- git tag 命名：`v<version>`（如 `v0.0.1`）。
- CI 校验 tag 名 == package.json version，防漂移。

## Release 流程（手动编 dll + CI 验证出 Release）

### 本地（发版人照做）
1. Rust 改动 → `cargo build -p loomgui_ffi_c --release` → cp dll 到 `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`（现有流程，见 AGENTS.md）。
2. 有 FFI 改动 → `cargo run -p xtask -- sync-bindings`（现有流程）。
3. 本地 `showcase-unity` 进 PlayMode 跑一遍（人工跨层验收门——单测验不了 CSS 语义集成）。
4. `cargo run -p xtask -- release-check`（新增）——见下。
5. 更新 `unity/package/CHANGELOG.md`（写 `[<version>]` 段落）。
6. bump `package.json` 的 `version`。
7. `git commit` → `git tag v<version>` → `git push` → `git push --tags`。

### CI（`.github/workflows/release.yml`，tag `v*` 触发）
1. 校验 tag 名 == package.json version（防漂移）。
2. `cargo test --workspace`（再确认绿）。
3. `cargo run -p xtask -- release-check`。
4. 从 CHANGELOG.md 提取该 version 段落。
5. 创建 GitHub Release，正文 = CHANGELOG 段落。

> CI **不编 dll**——dll 已在本地编好并 commit 进 tag 的 commit。理由见"git URL 硬约束"。若 CI 跑挂，git URL 仍可拉到带 dll 的包，CI 起的是"质量门 + Release 页面"作用，而非发版闸门。

## 改动清单

### 1. `unity/package/package.json`
- `version`: `0.1.0` → `0.0.1`
- 新增 `author`、`keywords`、`changelogUrl`、`documentationUrl`、`licensesUrl`：
```jsonc
"author": { "name": "15wtyuan", "url": "https://github.com/15wtyuan" },
"keywords": ["ui", "html", "css", "game-ui", "rust"],
"changelogUrl": "https://github.com/15wtyuan/LoomGUI/blob/main/unity/package/CHANGELOG.md",
"documentationUrl": "https://github.com/15wtyuan/LoomGUI/blob/main/docs/design/main-design.md",
"licensesUrl": "https://github.com/15wtyuan/LoomGUI/blob/main/LICENSE"
```
> `changelogUrl` / `documentationUrl` 用 GitHub 绝对地址（包内相对路径在消费端 UPM UI 不一定解析）。

### 2. `unity/package/CHANGELOG.md`（新增）
Keep a Changelog 格式。首版段落：
```markdown
# Changelog

All notable changes to `com.loomgui.unity` will be documented here.
Format: [Keep a Changelog](https://keepachangelog.com/). Adheres to [SemVer](https://semver.org/).

## [Unreleased]

## [0.0.1] - 2026-08-09
### Added
- 首个可安装 UPM 包。骨架链（div + 文字 + 图 + flex + cascade）从 HTML/CSS 一路通到 Unity 真机渲染。
- Runtime 公共 API 表面（Node/Container/Button/... 类型化投影层）。
- 围栏验证器（标准 HTML/CSS 子集，打包期报错）。
```

### 3. `crates/xtask/`：新增 `release-check` 子命令
`cargo run -p xtask -- release-check`，校验清单：
1. `package.json` 存在、可解析、必填字段齐全（name/version/unity/displayName）。
2. `version` 符合 SemVer。
3. dll 存在于 `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll`。
4. **dll 是否存在**：校验 `unity/package/Plugins/LoomGUI/loomgui_ffi_c.dll` 存在。不校验字节 staleness（Rust release 构建非确定性，不可靠）。
5. 三 asmdef 齐全（LoomGUI.Runtime / LoomGUI.Editor / LoomGUI.Bindings）。
6. `CHANGELOG.md` 存在且含当前 version 段落。
7. 退出码：全部通过 0，任一失败非 0（CI 据此判绿红）。

### 4. `.github/workflows/release.yml`（新增）
tag `v*` 触发；`runs-on: windows-latest`；`permissions: contents: write`（创建 Release 需要）。步骤如上 CI 节。创建 Release 用 `softprops/action-gh-release`；CHANGELOG 段落提取用 awk（截取 `## [<version>]` 到下一个 `## [` 之间）。

### 5. 文档
- 仓库根 `README.md` 加「发布/安装」小节，给出消费方 manifest.json 模板与升级方式。

## 消费方安装

在目标 Unity 工程的 `Packages/manifest.json` 加：
```json
"com.loomgui.unity": "https://github.com/15wtyuan/LoomGUI.git?path=/unity/package#v0.0.1"
```
升级：改 tag 号，或在 Unity Package Manager UI 选新版本。

## 验收标准
1. 打 `v0.0.1` tag push 后，CI 绿，GitHub Releases 出现 `v0.0.1` 且正文 = CHANGELOG `[0.0.1]` 段落。
2. 在一个干净的 Unity 工程里，manifest.json 加上述 git URL 行，能成功导入 `com.loomgui.unity`，dll 加载正常，Editor 菜单 `LoomGUI > Open Packer` 可见。
3. `release-check` 在入库 dll 缺失时报错退出（**存在性检查**，不校验字节 staleness——Rust release 构建字节非确定性，比较不可靠）；dll 存在则通过。

## 不在本期范围（备忘）
- OpenUPM registry 接入（v0.5+ 稳定后）。
- CI 方案 B（CI 自动编 dll + 自动打 tag）——当手动编 dll 成为负担时升级。
- 多平台 native（macOS / Linux / 移动端）。
- Git LFS（dll 直接 commit；膨胀后再评估，注意 OpenUPM 不支持 LFS）。
