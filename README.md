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

## 快速开始

```bash
# 全 workspace 构建 + 测试（core / fence / pkg / ffi）
cargo test

# 打包器 CLI：校验一个 HTML+CSS 工作区
cargo run -p loomgui_pkg -- check <workspace-dir>
```

Unity 集成见下节。完整工作流（GUI 打包器、FFI .dll 闭环、发版）文档待补——随 v1.0 公共文档波次补齐。

## 在 Unity 项目中使用

LoomGUI 以 UPM 包发布，通过 git URL 安装（Windows 打包器工具链）。

**推荐**：把[本手册链接](docs/ai-setup.md)交给你的 AI 编码代理——它能全自动完成安装与工作区初始化（改 manifest → 下载 CLI → init → 示例验证 → 等你开一次 Unity），只需回答它两个问题（UI 目录与构建产物目录）。

手动安装：在目标工程的 `Packages/manifest.json` 加一行（tag 从 [Releases](https://github.com/15wtyuan/LoomGUI/releases) 取最新，替换下例的 `v0.0.12`）：

```json
"com.loomgui.unity": "https://github.com/15wtyuan/LoomGUI.git?path=/unity/package#v0.0.12"
```

升级：把 tag 改成目标版本，或在 Unity 的 Package Manager 窗口选新版本；工作区侧用新 exe 跑一次 `loom scaffold` 刷新（详见手册「日常版本升级」）。

版本要求：最低支持 Unity 2021.3 LTS（URP 12.1+）。

## 文档

- [主设计](docs/design/main-design.md) — 总体架构与渲染管线
- [围栏](docs/design/fence.md) — HTML/CSS 子集权威清单
- [公共 API](docs/design/public-api.md) — 业务程序员终态契约
- [C# 投影层](docs/design/projection-layer.md) — 公共 API 的实现机制契约
- [AI 安装手册](docs/ai-setup.md) — 交给 AI 代理的全自动安装与初始化 runbook

## 项目结构

| 目录 | 职责 |
|---|---|
| `crates/core/` | Rust 核心（引擎无关） |
| `crates/packer/pkg/` | 打包器 CLI（loom） |
| `crates/packer/gui/` | 打包器 GUI（Tauri） |
| `crates/ffi/` | C ABI 导出（csbindgen） |
| `crates/fence/` | 围栏验证 |
| `crates/xtask/` | 仓库工具（发版校验、绑定同步） |
| `unity/package/` | Unity UPM 包 |
| `unity/showcase-unity/` | Unity demo 工程 |
| `docs/` | 设计文档 |
