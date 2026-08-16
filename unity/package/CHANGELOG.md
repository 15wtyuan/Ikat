# Changelog

All notable changes to `com.loomgui.unity` will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
