# LoomGUI

> 跨引擎游戏 UI 框架。标准 HTML/CSS 子集作设计期 DSL，类型化对象树作运行时 API，Rust 核心自绘渲染。
>
> **核心目的**：AI 驱动的界面拼装——HTML 作 DSL，让 AI 既能编辑（文本）又能预测渲染结果（AI 对 HTML/CSS 有强先验）。

[![Rust CI](https://github.com/15wtyuan/LoomGUI/actions/workflows/rust-ci.yml/badge.svg)](https://github.com/15wtyuan/LoomGUI/actions/workflows/rust-ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

## 为什么

- **标准 HTML/CSS**：AI 训练数据海量，读代码能预测渲染。对标 FairyGUI，差异化在 AI 可预测性。
- **类型化对象树**：运行时 API 是 `Container` / `Button` / `Slider`……不是全局句柄。
- **Rust 跨引擎共享**：一份核心，多引擎后端（Unity 首发，Godot 等后续）。
- **围栏验证器**：打包期挡违规语法，设计期即时反馈。

## 当前状态

摸黑阶段结束——骨架链（div + 文字 + 图 + flex + cascade）从 HTML 一路通到 Unity 真机渲染。进入三束加宽：控件、复合能力、视觉特效。

详见 [路线图](docs/roadmap/roadmap.md)。

## 快速开始

```bash
# 核心库
cargo build -p loomgui_core
cargo test  -p loomgui_core

# 打包器：HTML+CSS+资源 → .pkg.bin
cargo build -p loomgui_pkg
cargo run   -p loomgui_pkg -- build <workspace-dir>

# FFI（Rust → C# 绑定）
cargo build -p loomgui_ffi_c
```

Unity 后端：Unity 6.5 打开 `unity/showcase-unity/`，PlayMode 加载 `.pkg.bin`。

## 在其他 Unity 项目中使用

LoomGUI 以 UPM 包发布，通过 git URL 安装。在目标工程的 `Packages/manifest.json` 加一行：

```json
"com.loomgui.unity": "https://github.com/15wtyuan/LoomGUI.git?path=/unity/package#v0.0.1"
```

升级：把 `#v0.0.1` 改成目标 tag，或在 Unity 的 Package Manager 窗口选新版本。

> 当前仅 Windows（native dll 仅含 Windows）。发版流程与多平台计划见 [发布设计](docs/superpowers/specs/2026-08-09-loomgui-release-design.md)。

## 文档

- [主设计](docs/design/main-design.md) — 总体架构与渲染管线
- [围栏](docs/design/fence.md) — HTML/CSS 子集权威清单
- [公共 API](docs/design/public-api.md) — 业务程序员终态契约

## 项目结构

| 目录 | 职责 |
|---|---|
| `crates/core/` | Rust 核心（引擎无关） |
| `crates/packer/pkg/` | 打包器 CLI |
| `crates/ffi/` | C ABI 导出（csbindgen） |
| `crates/fence/` | 围栏验证 |
| `unity/package/` | Unity UPM 包 |
| `unity/showcase-unity/` | Unity demo 工程 |
| `docs/` | 设计文档 |
