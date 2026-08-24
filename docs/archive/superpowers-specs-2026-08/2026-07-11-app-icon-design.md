# LoomGUI 图标设计

> 2026-07-11。用 LogoLoom MCP 生成，替换 Tauri GUI 默认图标（原 `icon.ico` 仅 70 字节，疑似占位）。

## 概念

`<◆>` —— 尖括号 + 中央菱形。

- 尖括号 `<` `>`：HTML/CSS DSL（项目核心差异化——AI 可预测的标签式 UI）
- 中央菱形 ◆：替代斜线 `/`，既是"标签自闭合标记"，又呼应 Loom（织布机）的织造结，也是一个 UI 像素点缀

## 配色

沿用 GUI 前端 `loomgui_gui/dist/style.css` 既有品牌色，不引入新色：

- mark 主色：`#7c5cfc`
- 菱形点缀（提亮紫）：`#9d86ff`
- app icon 底：`#1e1e2e`（深底，搭 GUI 暗色窗口）
- 浅场景底（README / 白底）：`#ffffff`

## 几何（512×512 viewBox）

```
<rect>  0 0 512 512, rx=112, fill=底色
<path>  M 202 188 L 106 256 L 202 324   stroke=#7c5cfc width=32   (<)
<path>  M 310 188 L 406 256 L 310 324   stroke=#7c5cfc width=32   (>)
<path>  M 256 226 L 286 256 L 256 286 L 226 256 Z   fill=#9d86ff   (◆ 60×60)
```

尖括号缝隙 82px，与菱形间留白 8px（防笔画重叠）。

## 迭代教训

- v1 菱形 40px + 尖括号缝隙 20px + stroke 36：stroke 外扩 18 致两个尖括号笔画重叠，整体糊成"嵌套菱形"。
- 解法：缝隙拉开（20→82px）+ 菱形放大（40→60）+ stroke 收细（36→32），菱形与笔画间留 8px 白。

## 产出与接入

- 源 SVG / PNG 全套：`target/logo-preview-dark/`（深底）、`target/logo-preview/`（白底变体）
- GUI app icon：`loomgui_gui/icons/icon.ico`（6 尺寸 16/32/48/64/128/256，PNG-encoded，深底）+ `icon.png`（512）
- 闭环：`tauri build --no-bundle` 重出 exe（icon 嵌入 exe 资源）→ cp `loomgui_unity_package/Editor/Tools/loomgui_gui.exe`（cp 时 Unity 必须关）
