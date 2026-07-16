# Spec-3 ②：IrTree→TemplateNode 桥 + 打包编排

> **路线位置**：roadmap §2 ②（Ir→TemplateNode 桥 + 打包编排）。Spec-1（阶段 S cascade spike）
> ✅、Spec-2（① core 类型化重构）✅ 均已完成。本 spec 把断点接上——让真 HTML 第一次
> 端到端走进 core，推到**终点线 1**（headless 端到端 smoke）。
>
> **范围**：pkg 格式升 v18（kind_tag 全 23 变体 + 删 RichText 死字段）／IrTree→TemplateNode
> 桥／packer 接 fence 打包编排／base_style 灌入 + 接 inherited_set inline bake（修坑 161）／
> 终点线 1 smoke 门。
>
> **不做**：③ cascade 收尾（static bake / 选择器校验 / 全量集成测试）、控件行为（控件束）、
> rich text runs（复合束）、controller 逻辑（旧范式退役）、攒批回写 / set_transform（第一个
> 高频控件）。详见 §10 Defer 表。
>
> **依赖契约**：fence `ParsedTemplate`（pipeline.rs）／core `TemplateNode`（asset/mod.rs，
> Spec-2 已加 content/src）／`Stage::instantiate`（stage.rs）／cascade 引擎 `rematch_pseudo_classes`
> （dynamic.rs，spike 已验证完整）。本 spec 不改这些契约的形状，只新增"桥 + 编排"把它们串通。

---

## 1. 现状与断点

三方代码核实（fence / core / packer），真实现状：

| 层 | 状态 |
|---|---|
| fence `parse_template` → `ParsedTemplate{tree, styles, dynamic_rules, referenced_sprites}` | ✅ 成熟（6 阶段流水线，79 测试），**停在 IrTree，无任何往 core 节点树的转换** |
| core `TemplateNode`（含 Spec-2 content/src）+ `Scene::build`（10-tuple）+ side table | ✅ 就绪，缺数据来源 |
| core `write_package`/`read_package` | ⚠️ **kind_tag 只序列化 4 种 NodeKind**（Container/Button/Image/TextNode），其余 19 个 write 时 fallback `KIND_CONTAINER`、read 时 `BadKind`（asset/mod.rs 写映射 ~189、读映射 ~397）。fe81e76 已在 wildcard arm 加 `debug_assert!` 提醒 |
| packer `build()` | ⚠️ d8fe705 删了整条 HTML→pkg.bin 编排链（`pack`/`scene_to_template`/`resolve_img_src`/...），**当前完全不产 pkg.bin**，packages 恒空；atlas 自绘 + font copy 管线存活 |
| **断点** | **IrTree ↔ TemplateNode 之间没有桥**；packer 不依赖 fence |

三个被埋的坑（fe81e76 标记，本 spec 一并修）：
1. **kind_tag 4 种塌缩**：② 桥翻译的 Slider/Toggle 进包再出就变 Container——roadmap §3.3「假绿」的具体来源。
2. **RichText 死字段**：`rich_runs_arena`（恒空）+ 每节点 `rich_off`（4 字节 `NULL_RICH_OFF`）是 RichText 退役后的死字段，fe81e76 defer 到 pkg 格式清理。
3. **inherited_set bake 未接线（坑 161）**：fence `css_resolve` 应用 inline style 时不设 `inherited_set` → 恒 0 → 运行时 `propagate_inherited` 把子的 inline 继承声明覆盖成父值（`<div style="color:red"><span style="color:blue">x` → span 渲染红）。`<style>` class 规则的继承不受影响（rematch set bit）。

---

## 2. 数据流总览

```
showcase/*.html
  │
  ▼  fence::parse_template(html, file)
ParsedTemplate {
  tree: IrTree,                          // arena: IrNode{Element{tag,attrs,semantic}, Text(s), Comment, Doctype}
  styles: Vec<ResolvedStyle>,            // Stage4: 每元素 inline resolve（不含 class 级联）
  dynamic_rules: Vec<DynamicRule>,       // Stage4.5: <style> 的 class/tag/id/后代/伪类规则
  referenced_sprites: Vec<String>,       // img src + bg-image url（已归一化）
}
  │
  ▼  【B 桥 — 在 packer】DFS IrTree → Vec<TemplateNode>
每 Element:  读 el.semantic → NodeKind（Spec-2 §1.2 total 映射）
            抽 class/id/tabindex/data_controller/src → TemplateNode 字段
            base_style = styles[ir_idx]  + inherited_set inline bake（修坑 161，§6）
每 Text:     → TextNode，content = 扁平化文本
Comment/Doctype: 跳过    Template: 不进实例化
  │
  ▼  【C 编排 — packer::build】
组 ComponentTemplate{ nodes, dynamic_rules, controllers:[] }
  → core::write_package(PackageInput) → output/ui/<name>.pkg.bin   (v18)
  → referenced_sprites 累积 → 回接 atlas validate（assign_and_validate 复活）
  → loom.runtime.json packages 填实际包名
  │
  ▼  【E smoke — core】
load_package → instantiate(pkg, component) → tick_and_render
  → 断言 rect / 继承 / class 命中 / display:none 剪枝 / 无静默语义丢失
```

---

## 3. A'：pkg 格式升 v18（kind_tag 扩容 + 删 RichText 死字段）

### 3.1 为什么一次 v18 做两件事

kind_tag 扩容改写读写映射 = pkg 格式变化 = 必须 bump 版本。fe81e76 又把"删 rich_runs_arena/rich_off 死字段"defer 到"专门 pkg 格式清理 v17→v18"。二者都触发 v18，**合并一次 bump 做掉**，避免二次升版本 + 二次重打所有包。

### 3.2 kind_tag：语义常量 → NodeKind 判别值

当前 5 个语义常量（`KIND_CONTAINER=0/BUTTON=1/IMAGE=2/TEXT=3/RICHTEXT=4`退役）只覆盖 4 种 NodeKind，19 个 fallback。

**方案**：kind_tag 直接用 NodeKind 的 `u8` 判别值，全 23 变体保真。

- `NodeKind` 加 `#[repr(u8)]`（当前无 repr；23 变体 ∈ 0..22，u8 足够），让 `kind as u8` 等于判别值且稳定。
- **write**（asset/mod.rs `write_package`）：删 `KIND_*` 常量与 4-arm match + wildcard，直接 `kind_tag: u8 = tn.kind as u8`。fe81e76 的 wildcard `debug_assert!` 随之消失（不再有 fallback）。
- **read**（asset/mod.rs `read_package`）：删 4-tag 识别，改为 `NodeKind::from_u8(byte)`——match 0..22 → 对应变体，其余返 `PkgError::BadKind`。
- `NodeKind` 新增 `pub fn from_u8(b: u8) -> Option<NodeKind>`（match，不用 unsafe transmute）。
- **变体只追加不插中间**：保持现有判别值稳定，新变体追加到 enum 末尾（MIN=MAX 模式下加变体本就要 bump，但追加语义上更安全）。

### 3.3 删 RichText 死字段

- 删 `rich_runs_arena`（write 中恒空 `Vec::new()`，asset/mod.rs ~154）+ NodeBlock 的 `rich_off: u32` 字段（每节点 4 字节，恒 `NULL_RICH_OFF`）。
- 读写双端同步删除；`NULL_RICH_OFF` 常量一并删。
- 命中 fe81e76 的 `TODO(pkg-format-cleanup)`，清除后删该 TODO 注释。

### 3.4 版本与稳定性门

- `PKG_FORMAT_VERSION` 17→18，`MIN_VERSION == MAX_VERSION == 18`（弃 v17，无迁移器，个人项目不兼容）。
- 修 mod.rs 顶部 doc 注释（仍写 "version=16"，已双重过时）。
- 稳定性测试（扩展 Spec-2 的 bincode 门）：
  - **NodeKind 全 23 变体 pkg roundtrip 保真**：`write_package(Slider)` → `read_package` → 断言 `kind == Slider`（不塌 Container）—— 这条直接锁住坑 1 不复发。
  - NodeKind `as u8` / `from_u8` roundtrip 全变体。
  - TemplateNode（含各 kind + content/src）pkg roundtrip。
  - 断言 pkg.bin 中**无 rich_runs 段**（布局/尺寸回归基线）。

---

## 4. B：IrTree→TemplateNode 桥（② 核心，新写）

### 4.1 放 packer，不放 fence

fence 定位是纯解析器（memory `cascade-in-core-fence-parses`），产物 `ParsedTemplate` 停在 IrTree+styles+rules+sprites。桥把 IrTree 翻译成 core 打包结构 `TemplateNode`，属打包起点。放 packer 保持 fence 纯净——fence 已破例单向产 core 选择器类型（`ParsedSelector`/`DynamicRule`），不应再产 core 节点树类型。**packer 加 `loomgui_fence` 依赖**（当前未依赖）。

> spike 的 throwaway mini-bridge（`crates/fence/tests/cascade_spike.rs::bridge`）只命中 2 映射、丢 styles、硬编码字段、扁平化文本——本 spec 是它的生产级替代，逻辑放 packer。

### 4.2 映射规则（Spec-2 §1.2 表落地）

桥读 `IrElement.semantic: Option<SemanticKind>`（fence Stage 6 annotate 已填）做 total 映射，**不再像 spike 那样 match tag 字符串**。22 个进 IrTree 的 SemanticKind → NodeKind，加 TextNode（来自 `IrNodeKind::Text`），共 23。三个差异点显式处理：

| 来源 | 处理 |
|---|---|
| `IrNodeKind::Text(String)` | → `NodeKind::TextNode`，content 进 `TemplateNode.content` |
| `SemanticKind::InputDispatch` | 不出现（annotate 期已分派成 TextField/NumberField/Slider/Toggle/RadioButton） |
| `SemanticKind::Template` | IrTree 保留但**不产 TemplateNode**（`<template>` display:none，实例化时另处理，② 不实现投影） |
| 其余 21 个 SemanticKind | 按 Spec-2 §1.2 表 1:1 映射（Container/TextBlock/.../CustomElement/Canvas/Slot） |

**total + 非静默**：桥对每个 `Option<SemanticKind>` 显式映射；`None`（未识别标签）打包期报 Diagnostic（fence 围栏门应已挡，桥做防御性兜底）。加测试断言"showcase 8 页无元素静默丢失语义标签"（roadmap §3.3）。

### 4.3 属性抽取（当前全是原始 HTML 属性，无任何抽取）

`IrElement.attributes: Vec<IrAttribute>` 线性扫描，按 schema 分类表 dispatch 到 TemplateNode 字段：

| HTML 属性 | TemplateNode 字段 | 抽取方式 |
|---|---|---|
| `class` | `classes: Vec<String>` | `split_whitespace` 收集 |
| `id` | `id_attr: Option<String>` | 直接 |
| `tabindex` | `tabindex: Option<i32>` | `parse::<i32>()`，失败报 Diagnostic |
| `data-controller` | `data_controller: Option<String>` | 直接（保数据；② 不建 registry，见 §10） |
| `img src` | `src: Option<String>` | 仅 Image kind 填（fence extract_sprites 已归一化路径） |
| 元素直接 Text 子节点 | `content: Option<String>` | 仅 TextBlock/TextElement/Label/Button/Link/Container 等含文本的，扁平化拼接 |

### 4.4 parent_idx 局部化

IrTree 是 arena（全局 `IrNodeId(usize)`），TemplateNode 用组件内局部 `parent_idx: Option<usize>`。桥 DFS 时维护 `ir_idx → template_idx` 映射；Text/Comment/Doctype/Template 节点不占 template_idx（跳过），故映射非 1:1。产出的 `Vec<TemplateNode>` 满足 **parent 先于子**（`pidx < i`），否则 `Stage::instantiate` 返 Err（已有校验）。

### 4.5 文本 inline 扁平化（② 不做 runs）

元素的所有直接 `IrNodeKind::Text` 子节点拼成一个 `content: String`。inline 嵌套（`<p>Hi <strong>x</strong></p>`）暂扁平化（丢 `<strong>` 结构），**rich text runs 留复合束**（§10）。骨架链 showcase 文本多为纯文本，够用。

### 4.6 多组件

showcase 一个 workspace 含多 package，每 package 含多 HTML 组件（含 `components/*.html`）。桥函数按单组件：`fn bridge(parsed: &ParsedTemplate) -> (Vec<TemplateNode>, Vec<ControllerEntry>)`，遍历 `parsed.tree.roots`（fence 对一个 HTML 文件产一个 IrTree，roots 为顶层元素；showcase 组件 HTML 通常 `<body>` 下单根，如 form.html 的 `<div class="root">`）。每 HTML 文件 = 一个 `ComponentTemplate`（文件名去扩展 = 组件名），多组件组装在 §5 编排层。

---

## 5. C：packer 接 fence 打包编排

### 5.1 重建 packages 循环（d8fe705 删掉的）

packer `build()` 当前只做 atlas + font + runtime.json，packages 恒空。加回 packages 段：

```
for pkg in &ws.packages:                       // 每个 package
  html_files = resolve_html_list(workspace_root, pkg)
  components = []
  all_refs = []
  for html in html_files:                      // 每个组件 HTML
    parsed = loomgui_fence::parse_template(&html_src, html)
    (nodes, controllers) = bridge(&parsed)     // §4
    base_style_bake(&parsed, &mut nodes)       // §6（接 styles + inherited_set）
    components.push((component_name, nodes, DynamicRuleTable{rules: parsed.dynamic_rules}, controllers))
    all_refs.extend(parsed.referenced_sprites)
  bytes = loomgui_core::asset::write_package(&PackageInput{components})
  write output/ui/<pkg-name>.pkg.bin
  accumulate all_refs → BuildReport
```

- packer `Cargo.toml` 加 `loomgui_fence = { path = "../../fence" }`。
- 旧 `resolve_img_src` 不重建——fence `extract_sprites` 已直接返回归一化的 `referenced_sprites`。
- `loom.runtime.json` 的 `packages` 字段填实际产出的包名（当前恒空）。

### 5.2 referenced_sprites 回接 atlas validate

`atlas/validate.rs::assign_and_validate`（引用图交叉验证）当前是死代码（d8fe705 删了 build 里的调用）。② 把 `all_refs` 累积进 `BuildReport`，调 `assign_and_validate` 验证"所有 HTML 引用的 sprite 都在 atlas 里"——缺失即报错（不静默降级）。命中 fe81e76 后 build.rs 的 `// kept for future cross-validation (R3)` 注释清除。

---

## 6. D'：base_style 灌入 + inherited_set inline bake（修坑 161）

### 6.1 base_style = fence styles[ir_idx]

fence Stage 4 `resolve_inline_styles_with_diags`（css_resolve.rs）产每元素 inline resolve 结果（套 schema display 默认 + inline `style=""`，**不含 class 级联**）。桥把它填进 `TemplateNode.style`（= `base_style`）。spike 丢弃了这个（硬编码 default），② 接上。

### 6.2 全 dynamic cascade（spike 做法精确版）

`<style>` 的 class/tag/id/后代/伪类规则全进 `dynamic_rules`（Stage 4.5 已产），运行时 `rematch_pseudo_classes` 每帧从 `base_style` 重起叠加（spike 已证能跑通 class 命中 + 继承）。即 ② 完全复刻 spike cascade 数据流，数据源换成 fence 真实产物。

### 6.3 修坑 161：inherited_set inline bake

**问题**：fence `css_resolve` 调 `apply_decl` 应用 inline 声明时**不设 `inherited_set`** → 恒 0 → 运行时 `propagate_inherited` 把所有继承属性判 unset → 父值覆盖子的 inline 声明（color/font-size/...）。

**修法**：`css_resolve` 的 `apply_decl(&mut styles[idx], prop, value)` 成功返回后，查 `CssPropSpec.inherited`（fence schema css.rs 已有此 flag）；为 true 则 set `styles[idx].inherited_set` 对应 bit。

**统一双源**（坑 161 点名）：当前"哪些属性可继承"有两份来源——
- fence schema `CssPropSpec.inherited: bool`（css.rs）
- core `inherited_bit(prop) -> Option<u16>` + `INH_*` 常量（dynamic.rs:85-105，private）

二者需核对一致并合并为**单一真相源**：把 core 的 `inherited_bit`（或 `INH_*` → prop 名映射）pub 出来供 fence 调，或把 bit 编进 fence schema（`CssPropSpec` 加 `inh_bit: Option<u16>`）。实现期定具体形态；关键是消除"fence 标 inherited 但 core 无对应 bit"或反之一类的不一致。

**回归测试**（锁坑 161 不复发）：`<div style="color:red"><span style="color:blue">x</span></div>` → smoke 断言 span 渲染 **blue**（inline 声明不被父覆盖）；class 规则继承同时验。

### 6.4 与 ③ 的边界

② 只 bake **inline 声明**的 inherited_set（修坑 161，让 inline 继承正确）。**全量 static bake**（把 class 静态命中部分也 bake 进 base_style，减少每帧 rematch 量）推 ③——② 用全 dynamic 已能让 smoke 绿，优化推后。

---

## 7. E：终点线 1 smoke 门

### 7.1 主门（锁范式，手搓最小 HTML）

位置：core 集成测试（headless，本机验，不依赖家里机 Unity）。手搓最小 HTML 覆盖骨架链：

```html
<style>
  .wrap { display:flex; flex-direction:column; width:200px; }
  .hide { display:none; }
  .hot:hover { color:red; }
</style>
<div class="wrap">
  <p class="page" style="font-size:20px">hello</p>     <!-- 继承 + class 命中 -->
  <span class="hide">invisible</span>                   <!-- display:none 剪枝 -->
  <img src="icon.png">                                   <!-- Image kind 保真 -->
  <button class="hot">btn</button>                       <!-- Button + 伪类规则 -->
</div>
```

全链：HTML 字符串 → `parse_template` → 桥 → `write_package`（内存 bytes）→ `load_package` → `instantiate` → `tick_and_render`。断言 5 项（roadmap §3.3，rect 对 ≠ 语义对）：

1. **rect**：`.wrap` 宽 200、children 纵向堆叠坐标正确。
2. **继承**：`<p>` 的 font-size 继承（class 规则）+ inline color 不被父覆盖（坑 161 修复）。
3. **class 命中**：`.page` computed style 含 class 规则声明。
4. **display:none 剪枝**：`.hide` 不进 layout、不产 render node。
5. **无静默语义丢失**：每个元素 kind 映射正确（div→Container/p→TextBlock/span→TextElement/img→Image/button→Button）。

### 7.2 冒烟（真 showcase 页不崩 + 控件映射保真）

`form.html`（无 `@keyframes`，含控件全家 text/select/textarea/radio/range/checkbox）→ 全链 → 断言：
- 进包不崩、instantiate 成功。
- 控件 kind 保真：A' 扩容后 `Slider/Toggle/RadioButton/Dropdown/...` 不塌成 Container。
- **控件行为不验**（Slider 拖动/Toggle 切换是控件束）。

---

## 8. 错误处理 / 打包期校验（不静默降级）

| 场景 | 处理 |
|---|---|
| 未知 SemanticKind / `semantic=None` | 桥报 Diagnostic（防御性，围栏门应已挡） |
| 选择器越界（`:nth-child`/`@keyframes` 等不支持） | fence Diagnostic（Stage 4.5 已有），② scope 内 class/tag/id/后代/伪类 |
| parent_idx 父不先于子 | `Stage::instantiate` 已返 Err（no-panic 契约） |
| kind_tag 未知值 | `read_package` 返 `PkgError::BadKind` |
| img src 引用不在 atlas | `assign_and_validate` 报错（§5.2） |
| tabindex 解析失败 | 桥报 Diagnostic |

---

## 9. 测试策略（全本机 headless）

- **A'**：NodeKind 全 23 变体 pkg roundtrip 保真（Slider 不塌）／from_u8 roundtrip／无 rich_runs 段断言。
- **B**：桥单测——每 SemanticKind→NodeKind／属性抽取（class/id/tabindex/data_controller/src）／parent_idx 局部化／TextNode content／3 差异点（Text/InputDispatch/Template）。
- **C**：packer 多 HTML→pkg.bin roundtrip／referenced_sprites 回接 atlas validate（缺引用报错）。
- **D'**：inherited_set inline bake 回归（坑 161：span 渲染 blue）／base_style = styles[ir_idx] 灌入。
- **E**：smoke 主门 5 断言 + form.html 冒烟（控件 kind 保真）。

CI 门禁：`cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` + feature-gate check（`-p loomgui_core`/`-p loomgui_fence`/`-p loomgui_pkg` 全过）。

---

## 10. 范围外 / Defer 表（每项有 roadmap + 代码双锚点，不丢）

| Defer 项 | 推到 | 为什么 | 锚点 |
|---|---|---|---|
| static bake（class 静态部分进 base_style） | ③ cascade 收尾 | 全 dynamic 已让 smoke 绿，优化推后 | roadmap §2 ③ |
| 选择器子集打包期校验（`:nth-child` 等越界报错） | ③ | ② 子集（class/tag/id/后代/伪类）够用 | roadmap §2 ③ |
| cascade 集成级测试（全量标签 computed style） | ③ | ② smoke 主门覆盖骨架链 | roadmap §2 ③ |
| 控件行为（Slider 拖动 / Toggle 切换 / 输入） | 控件束 | ② 只验映射保真不塌 Container | roadmap §4 控件束 |
| 控件私有状态表（slider value / dropdown index） | 控件束 | Spec-2 §3.3 已定只扩变体 | Spec-2 §3.3 |
| rich text runs（inline 嵌套结构） | 复合束文本模型 | ② 扁平化够骨架期 | roadmap §4 复合束 |
| controller 逻辑（collect / registry） | **不做（退役）** | v1.5 旧范式，R5 WAI-ARIA 替代；② 只保 data_controller 数据 | roadmap §5 退役清单 |
| 攒批回写 / set_transform | 第一个高频控件 | 摸黑期即时过桥兜底 | roadmap §3.5 |
| `@keyframes`/animation 解析 | 视觉束/控件束 | home 动画，smoke 选 form 避开 | roadmap §3.5 ⚠️ |
| Slot / CustomElement 内容投影 | 复合束组件系统 | 当前无人用它做游戏，排最后 | roadmap §4 复合束 |

代码层临时简化标 `ponytail:` 注释（text 扁平化、base_style 全 dynamic），`/ponytail-debt` 兜底收割。

---

## 11. 实现顺序（建议）

1. **A' pkg v18**：NodeKind 加 `#[repr(u8)]` + `from_u8` → 改 write/read kind 映射 → 删 rich_runs_arena/rich_off → 升 v18 + 稳定性测试（Slider 不塌、无 rich_runs 段）。先做：解锁后面桥翻译的 kind 能进包。
2. **B 桥**：packer 加 fence 依赖 → 写 `bridge(ParsedTemplate) -> (Vec<TemplateNode>, controllers)`（SemanticKind total 映射 + 属性抽取 + parent_idx + TextNode + 3 差异点）+ 单测。
3. **D' base_style + 坑 161**：fence css_resolve 接 inherited_set bake + 统一双源 → 桥填 base_style = styles[ir_idx] + 回归测试。
4. **C 编排**：packer::build 加 packages 循环 + write_package + referenced_sprites 回接 atlas validate + runtime.json。
5. **E smoke**：core 集成测试主门 5 断言 + form.html 冒烟。
6. **cargo fmt + clippy + 全测试绿**；重编 .dll（NodeKind repr 改动影响 FFI ABI？核实——enum repr 不改 u32 ABI 判别值布局应兼容，但重编核实）+ commit .dll。
7. 更新 roadmap 进度行（Spec-3 ② 完成）+ 清除 fe81e76 的 `TODO(pkg-format-cleanup)` / 坑 161 标记。

每步后 `cargo build`/`cargo test` 验证。A' 后先确认 v18 roundtrip 绿再进 B。
