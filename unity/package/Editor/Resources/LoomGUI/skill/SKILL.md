---
name: loomgui-editor
description: |
  Generate LoomGUI fence-compliant game UI (HTML+CSS).
  Uses standard HTML/CSS semantics — block/flex layout, 14-tag whitelist.
  No grid, no float, no @media. Spacing via gap, not margin.
  After generating, run loom check / loom build to validate and pack into .pkg.bin for Unity.
triggers:
  - "loomgui ui"
  - "game dashboard"
  - "游戏 UI 面板"
  - "游戏界面"
---

# LoomGUI Editor

生成 LoomGUI 围栏合规的游戏 UI（HTML+CSS），打包成 .pkg.bin 供 Unity 加载。

## 工作流

1. **读围栏规则**：读 `references/fence.md`（标签/CSS/选择器硬约束）+ `references/preview-trust.md`（预览可信清单）。围栏 = 硬约束，围栏外输入打包期报错。

2. **按设计师 prompt 生成 HTML+CSS**：
   - 元素用围栏白名单（14 标签：8 个文档壳 + 6 个运行时标签，详见 fence.md §1）。
   - 布局用 flex + `gap`（子项间距用 gap 不用 margin——Chrome 折叠 margin，LoomGUI 不折叠）。
   - 禁 grid / float / @media / 固定定位（详见 fence.md）。
   - `display:flex` 默认 `flex-direction:row`（标准 CSS）；纵向堆叠写 `flex-direction:column`。
   - 风格（颜色/字号/字体）自由，只要守围栏规则。

3. **生成完跑验证+打包**：
   ```
   loom build <workspace-dir>
   ```
   （loom.exe 在 UI 工作区 `.loom/`，或本包 `Editor/Tools/`。）
   - **非零退出 = 围栏违规**。读 stderr，自纠 HTML/CSS 后重跑。
   - **零退出 = 合规**，.pkg.bin + 图集已产出。

4. **报告**：向设计师报告产出路径（.pkg.bin + atlas.png），说明 Unity 加载方式。

## 注意

- **预览不可信项**：open-design 预览是 Chromium iframe，与 LoomGUI 有分歧。margin 折叠 / 文本换行 / display:grid / @media 别按预览调。详见 references/preview-trust.md。
- **打包器即验证器**：`loom build` 内含围栏验证，违规打包期报错。不需要单独的 lint 步骤。
- **Unity 侧接入**：产物如何在 Unity 场景挂载与加载（含 AB/Addressables 覆写钩子）、UI 与 3D 怎么互通（事件接线、`IsPointerOnUI` 输入门控、NativeHost 内嵌 3D）——读 `references/runtime-integration.md`。
