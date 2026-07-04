# LoomGUI

> 跨引擎游戏 UI 框架。Rust 核心（引擎无关）+ 多引擎后端（Unity 首发），HTML/CSS 子集作 DSL，taffy flexbox 布局，自绘渲染。
>
> **核心目的**：AI 驱动的界面拼装——HTML 作 DSL 让 AI 既能编辑（文本）又能预测渲染结果（AI 对 HTML/CSS 有强先验）。

[![Rust CI](https://github.com/15wtyuan/LoomGUI/actions/workflows/rust-ci.yml/badge.svg)](https://github.com/15wtyuan/LoomGUI/actions/workflows/rust-ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Unity](https://img.shields.io/badge/Unity-6.5%20URP-black.svg)](https://unity.com)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org)

## 为什么（对标 FairyGUI 的差异化）

- **AI 可预测性**：HTML/CSS-DSL，AI 能读写 + 预测渲染（vs fgui `.fui` 二进制 AI 看不懂）
- **flexbox 布局**：流式 / 响应式 / 内在尺寸（超 fgui 锚点式 Relations）
- **Rust 跨引擎共享核心**：一份核心，多后端（Unity/Godot，vs fgui 各引擎独立 SDK）
- **围栏验证器**：打包期挡违规 CSS，AI 的第一道反馈

## 当前状态

v1 架构走通 + 桌面可演示（Win/Mac Mono）。已交付：渲染/文本/事件/布局/滚动/打包器/FFI/动态树（代际 NodeId + 命令式 API）/ColorFilter/九宫格/圆角/background-image。

距上线 = v1.x 功能（列表/富文本/Controller/TextInput）+ 编辑器工作流（v other）+ v2 平台（移动/IL2CPP/Godot）。详见 [路线图](docs/roadmap/roadmap.md)。

## 安装

当前开发中（`0.1.x`，尚未发 UPM 包 / crates.io），直接 clone：

```bash
git clone https://github.com/15wtyuan/LoomGUI.git
```

后续将提供 `com.loomgui.unity` UPM 包。构建/运行见下方「快速上手」。

## 快速上手

```bash
# 核心（引擎无关纯库，可单测）
cargo build -p loomgui_core
cargo test  -p loomgui_core

# 打包器（HTML+CSS+资源 → .pkg.bin）
cargo build -p loomgui_pkg

# FFI（C ABI，csbindgen 生成 C# 绑定）
cargo build -p loomgui_ffi_c
```

Unity 后端：用 Unity 6.5 打开 `loomgui_unity/`，PlayMode 加载 `.pkg.bin` 渲染。

示例见 `loomgui_unity/Assets/LoomUI/showcase/`（showcase 打包源）。

## 文档

- [文档总览](docs/README.md)
- [主设计](docs/design/main-design.md) · [围栏权威](docs/design/fence.md) · [路线图](docs/roadmap/roadmap.md)

## 项目结构

| 目录 | 职责 |
|---|---|
| `loomgui_core/` | Rust 核心（解析/样式/布局/场景图/渲染状态/事件/动画/文本，引擎无关纯库） |
| `loomgui_pkg/` | 打包器 CLI（HTML+CSS+资源 → `.pkg.bin`，复用 core 的 parse 层） |
| `loomgui_ffi_c/` | C ABI 导出（csbindgen，Rust ↔ C# P/Invoke） |
| `loomgui_unity/` | Unity 6.5 URP 后端（GameObject 镜像 + DrawState 缓存 + 输入采集） |
| `docs/` | 设计 / 路线 / 文档 |

核心可编译为 WASM（给编辑器）和 C ABI（给引擎），同一份代码。参考实现：FairyGUI-unity（`temp/FairyGUI-unity/`，渲染/对象模型/动画的原理参考）。
