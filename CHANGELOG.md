# Changelog

本文件记录 LoomGUI 所有显著变化。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

项目当前处于 `0.1.x` 阶段（v1 架构走通，功能持续补全中）。

## [Unreleased]

### Added
- 开源治理基线：`LICENSE`（MIT）、`CONTRIBUTING.md`、issue 模板。
- CI 工具链：GitHub Actions（Rust `test` / `fmt` / `clippy` / `build`，Windows 产 `.dll` artifact）。

### Changed
- 仓库结构整顿：Unity 插件/演示拆分（`com.loomgui.unity` UPM 包 + demo 工程引用）。
- Rust 超大文件按职责拆子模块（内嵌测试外提）。

### Removed
- Unity 模板残留（`TutorialInfo/`、`Readme.asset`、`InputSystem_Actions.inputactions`）。

[Unreleased]: https://github.com/15wtyuan/LoomGUI/commits/main
