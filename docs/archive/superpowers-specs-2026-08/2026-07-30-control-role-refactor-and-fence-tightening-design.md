# 控件 role 化重构 + 围栏收紧 + 契约漂移清理

> **状态**：设计定稿（2026-07-30）。取代同日的探索稿 `2026-07-30-control-div-role-refactor.md`（那份的方向正确但工作量估算与前提有多处偏差，见 §9）。

## 1. 动机

### 1.1 `.loom-*` 注入模式违背核心赌注

现状：作者写 `<select>` / `<progress>` / `<input type=range>`，core 运行时**注入固定类名子节点**（`.loom-value`/`.loom-popup`/`.loom-fill`/`.loom-track`/`.loom-thumb`/`.loom-check`），围栏校验作者 CSS 命中这些注入节点。三个问题：

1. **破坏 AI 先验**：`.loom-*` 是框架私有魔法类名，AI/作者无法从 HTML 预测运行时结构。
2. **浏览器预览 ≠ Unity 渲染**：注入节点只在运行时存在，作者在浏览器看到的（原生 `<select>` 系统下拉）和 Unity 自绘的（`.loom-popup`）对不上。
3. **违背游戏 UI 现实**：游戏 UI 没有"浏览器默认样式"这回事。「写一个标签出控件」的浏览器便利，正是游戏 UI 要摒弃的。

根因：原生控件标签在浏览器里有**系统原生 UI 且 CSS 样式化能力有限**（select 下拉完全不可样式化、progress/range 靠 webkit 专属伪元素）。框架想用这些语义标签又要自绘 → 只能注入魔法节点。

### 1.2 围栏契约文档已漂移

标签注册表历史上砍过一波（`crates/fence/src/schema/tag.rs` 实际 13 runtime + 8 shell = 21），但**描述层没跟上**：`AGENTS.md`/`CLAUDE.md` 仍写「31 标签 = 8 shell + 23 runtime」，`main-design.md`/`public-api.md` 仍列 `p`/`a`/`label`/`canvas` 等已删标签，甚至承诺了**从未实现的 C# 类型**（TextBlock/Label/Link/Canvas/LineBreak）。

根因：防漂移门 `crates/fence/tests/schema_contract.rs` 只锁 schema 注册表，**没有任何「文档 ↔ schema」交叉校验**。

两件事强耦合：控件改 role 驱动后那些控件标签才能砍，砍完标签数变化文档才能一次写对。故合为一个 spec。

## 2. 终态

### 2.1 围栏：6 runtime + 8 shell = 14 标签

```
runtime: div span button img template slot
shell:   html head body title meta style link script
```

从 21 砍到 14，下线 7 个：`input` `textarea` `select` `option` `progress` `ul` `li`。

**下线判据**（可复用于将来的标签决策）：
- 浏览器有**不可样式化的原生 UI**（`input`/`textarea`/`select`/`option`/`progress`）→ 砍，改 role
- 已被 role 完整取代的**纯结构标签**（`ul`/`li`）→ 砍，统一走 role

**保留判据**：无 role 等价物（`div`/`span`/`img`/`template`/`slot`），或 CSS 完全可控且 AI 先验极强（`button`）。

### 2.2 控件：`<div role="...">` + 作者自写结构

严格使用 **WAI-ARIA 标准 role 名**——不发明私有 role（`combobox` 而非 `dropdown`）。理由：pivot 的核心论据是「AI 先验 + 消除私有魔法」，自创 role 名与 `.loom-*` 是同一类问题。

| 语义（NodeKind） | 根 role | 必需子角色 | ARIA 状态属性 |
|---|---|---|---|
| ProgressBar | `progressbar` | `data-slot="fill"` | `aria-valuenow` / `aria-valuemin` / `aria-valuemax` |
| Slider | `slider` | `data-slot="thumb"` | `aria-valuenow` / `aria-valuemin` / `aria-valuemax` |
| Toggle | `switch` | —（无必需子节点） | `aria-checked` |
| RadioButton | `radio` | —（无必需子节点） | `aria-checked` |
| Dropdown | `combobox` | `role="listbox"` 子节点，内含 ≥1 个 `role="option"` | `aria-expanded` |
| TextField | `textbox` | — | — |
| TextArea | `textbox` | — | `aria-multiline="true"` |
| NumberField | `spinbutton` | — | `aria-valuenow` / `aria-valuemin` / `aria-valuemax` |
| ListView | `list` | `role="listitem"` | — |

**两套标识机制，职责分明**：
- **`role`** 表达**语义**（这是什么控件 / 这个子节点扮演什么角色）——用于 ARIA 覆盖的概念
- **`data-slot`** 表达**构造**（这是控件的哪个视觉部件）——仅用于 ARIA **明确不覆盖**的场合

`data-slot` 的必要性：ARIA 语义里 `progressbar`/`slider` 是**原子控件**（一个节点 + `aria-valuenow`），内部视觉构造不该有 ARIA 语义。`data-*` 是 HTML 标准为私有扩展预留的机制，用它表达"控件的哪个部件"是正确做法，不是私有魔法。

Toggle/RadioButton **不要求 check 子节点**：`aria-checked` 是根节点状态属性，作者用 `[aria-checked="true"] { ... }` 属性选择器表达选中态即可，比强制一个勾节点更自由、更符合 CSS 惯用法。

### 2.3 删除的 NodeKind：`PasswordField`、`SearchField`

- **PasswordField**：掩码显示是 web 表单概念；游戏中密码输入通常业务层自实现。删除后连带消除坑 173（value 字节偏移 vs 掩码 display 偏移的光标错位）的整类问题。
- **SearchField**：语义等同 TextField，无独立行为（浏览器的清除按钮是原生 UI，本就不适用）。

**NumberField 保留**：数值输入在游戏设置界面（音量/灵敏度/分辨率倍率）是高频真实需求，且有真实行为（输入过滤、clamp、step 量化）。ARIA 标准 role 为 `spinbutton`，保留不引入私有约定。

### 2.4 公共 API 对象树（优化后）

```text
Node
├── Container（子树 = 用户内容，运行时可编排）
│   ├── AbsolutePanel（语法糖：子节点自动 absolute）
│   ├── TextElement（span）
│   ├── Button（button）
│   ├── ListView（role=list）/ ListItem（role=listitem）
│   ├── OptionItem（role=option，从属 Dropdown）
│   ├── Slot / CustomElement
├── TextNode / Image（叶子：内容 / 绘制）
└── 控件（叶子：子树 = 控件构造，公共 API 不暴露编排）
    ├── TextField（role=textbox）/ TextArea（role=textbox + aria-multiline）
    ├── NumberField（role=spinbutton）
    ├── Slider（role=slider）/ ProgressBar（role=progressbar）
    ├── Toggle（role=switch）/ RadioButton（role=radio）
    └── Dropdown（role=combobox）
```

**Container vs Node 叶子的划线依据必须重写**。原依据是 HTML content model（「`<input>` 全家是框架私有内部结构」），pivot 后失效——所有控件的子节点现在都是**作者写的**。新依据：

> **Container = 子树是「用户内容」（运行时可编排）；Node 叶子 = 子树是「控件构造」（设计期写定，框架管理，公共 API 不暴露编排）。**

控件保持 `: Node` 叶子（不改 `: Container`、不引入 `Control` 中间层）：pivot 改的是**谁写结构**（框架注入 → 作者写），没改**谁管结构**（仍是框架）。运行时让用户 `slider.Children.Remove(thumb)` 无合理用例，只会制造半残控件。

`OptionItem`/`ListItem` 保持 `: Container`（它们确实装用户内容），其从属关系（服务于父控件）在文档说明，不做类型层次强化。

**删除的公共 API**：
- 4 个**从未实现的幻影类型**：`TextBlock`、`Label`、`Link`、`Canvas`（对应标签 `p`/`label`/`a`/`canvas` 早已出围栏）+ 对象树图里的 `LineBreak`
- 2 个**真实存在但本次废除**的类型：`PasswordField`、`SearchField`

不留 `[Obsolete]` 过渡：项目未发布，明确不考虑兼容。

### 2.5 终态无双轨（不留兼容）

终态不保留 `<select>` 等标签与 `role=combobox` 的双轨支持。留兼容 = 永久维护两套语义分派 + 两套围栏校验 + 两套结构契约，且 AI 面对两种写法无所适从——正好违背 pivot 初衷。

（实施过程中 role 与旧标签会短暂并存数个 commit，属内部过渡，不对外承诺；见 §8。）

## 3. 机制设计

### 3.1 RoleTable side table

Node **当前不存任何 HTML 属性**（`node.rs` 只有 `kind`/`classes`/`id_attr`），`role` 在 parse 期被白名单放行后即丢弃（fence IR + packer 中 `role` 零命中）。故 role 驱动分派的前置 = **新建属性存储**，这是探索稿遗漏的成本。

采用 **side table**（仿现有 `ControlTable`/`AnimTable` 模式）而非 Node 加 `attrs: HashMap`：

- **稀疏**：只有带 role / aria-* / data-slot 的节点进表，绝大多数节点零开销
- **架构一致**：与既有 side-table 模式同构
- **可扩展**：将来 WAI-ARIA 复合控件（TabList/Tree）需要更多 aria-* 时平滑扩容

打包期写入（role + 必需 aria-* + data-slot），运行时按 NodeId 查。

### 3.2 语义分派：tag → role

`resolve_semantic(tag, input_type)` → 基于 (tag, role, aria 属性) 分派：

- `div` + `role=X` → 对应 SemanticKind（表见 §2.2）
- `div` + `role=textbox` + `aria-multiline="true"` → TextArea；无该属性 → TextField
- 无 role 的 `div` → Container；`span` → TextElement；`button` → Button；`img` → Image
- `template` / `slot` 不变

`input[type]` 机制彻底退场，连带 fence `attr_matches_node` 中硬编码的 `[type=...]` 分支。

### 3.3 注入拆除

- `inject_control_children`（control.rs，约 60 行）**整体删除**
- `sync_control_visuals`（约 134 行）**改写**：不再按 `.loom-*` class 查注入节点，改按 role / data-slot 查**作者写的**节点
- 各调用点（control.rs / hit.rs / style/dynamic.rs / render，约 10 处）的 `find_child_by_class(.loom-*)` → `find_child_by_role(...)` / `find_child_by_slot(...)`

**浮层基建原样保留**：popup 的「render 末尾追加 + `mask_context=0` 跳出祖先 clip」是通用机制（与 scrollbar thumb 同模式），与 `.loom-popup` 类名无耦合。唯一耦合点是 popup 根定位那一行，换成按 `role="listbox"` 查找即可。

### 3.4 围栏严校验（两件事）

注入模式下「子节点必然存在」是框架白送的保证；改作者自写后，**该保证必须从运行时前移到打包期**，否则是拿确定性换自由度——违背「围栏外输入打包期报错，不静默降级」的项目原则。

1. **结构契约**：必需子角色缺失 = 打包期 **error**（`combobox` 缺 `listbox`、`listbox` 无 `option`、`slider` 缺 `data-slot=thumb`、`progressbar` 缺 `data-slot=fill`、`list` 缺 `listitem`）。教学文案说明该写什么结构——这是新的教学载体，取代 `.loom-*` 曾提供的"照着填"提示。
2. **CSS 契约**：控件根节点 + 必需子节点都须有规则命中，否则 error（「控件不带 UA 默认样式」这条不变，子节点无 CSS 同样渲染空白）。

复用现有匹配引擎（`selector_matches_node` / `any_rule_matches`），只改「哪些节点必须被命中」的判定谓词。

**新增**：围栏支持 `[aria-checked]` / `[aria-expanded]` / `[aria-multiline]` 等 ARIA 属性选择器（扩 `attr_matches_node`），使 §2.2 中 Toggle/RadioButton 用 `[aria-checked="true"]{...}` 表达状态样式成立。

## 4. 漂移清理

### 4.1 契约文档重写到终态

- `AGENTS.md` / `CLAUDE.md`：「31 标签 = 8 shell + 23 runtime」→ 14 标签的正确表述；控件段落改 role 化描述
- `docs/design/fence.md`：主表（当前干净）+ 十余处散文示例中的已删标签（`strong`/`em`/`label`/`a`/`canvas`/`p`/`ol`）
- `docs/design/main-design.md`：默认 display 列表、标签→类型表、逐条映射列表、对象树层级图（含不存在的 TextBlock/Label/Link/Canvas/LineBreak）、block 默认标签列表
- `docs/design/public-api.md`：对象树图、Container 子类列表、tag→类型表（`a→Link`）、Label/Canvas 行为契约；按 §2.4 重写划线依据
- `showcase/showcase/preview/README.md`：「23 runtime tags」整行标签列表 + 承诺的富文本/canvas/有序列表特性

### 4.2 死代码清理

- `crates/fence/src/structural.rs` `validate_label_for`：整函数死代码（`label` 已出围栏，条件永不命中，但 pipeline 仍每元素遍历一次）
- `structural.rs` 中 `ol` 分支：永不命中
- `crates/fence/src/schema/attr.rs` `LABEL_STRUCTURAL` / `A_STRUCTURAL`：定义后全仓零引用

### 4.3 假测试与撒谎注释

- `crates/fence/src/inline_context_check.rs` 的 `inline_in_text_block_ok`：用 `<p>...<a>` 构造，但这两个标签在 Stage 3 即被 `FenceUnknownTag` 拒 → **空洞通过**，测的是不存在的 `<p>` 豁免路径
- `diagnostic.rs` 中 `FenceInlineElementInBlockContext` 文档注释宣称「非 `<p>` 豁免」，而功能代码中**无任何该豁免分支**——注释与实现不符
- `inline_context_check.rs` 中描述该豁免的注释

### 4.4 补防漂移门（根因修复）

新增「**文档 ↔ schema 交叉校验**」测试：从 `fence.md` 主表解析标签清单，与 `schema/tag.rs` 注册表比对，不一致即失败。这是本次漂移的根因——既有门只锁 schema 自身，描述层完全不在测试覆盖下。

顺带修 `schema_contract.rs` 的 `shell_tags_are_seven`：本地清单漏 `script` 且断言 `len()==7`，实际 8 个（此为测试漏写，非 schema 漂移）。

## 5. showcase 改写

7 个 HTML 文件（character / form / inventory / mail / settings / shop / components-stat-bar），约 40 个控件实例（27 视觉控件 + 13 文本控件）+ 全部 `ul`/`li` 列表，改写为 `div + role` 结构并补齐必需子角色与 CSS。

showcase 打包 exit 0 是本次的**集成验收面**。

## 6. 测试策略

- **core**：role 驱动语义分派；RoleTable 存取；`sync_control_visuals` 按 role/slot 定位作者节点；popup 按 `role=listbox` 定位；删除 PasswordField/SearchField 后的回归
- **fence**：结构契约校验（每种控件缺各必需子角色的 error 用例）；CSS 契约；ARIA 属性选择器匹配；14 标签注册表锁定；**文档↔schema 交叉门**
- **headless C#**：控件 FFI 值语义回归（预期改动极小——FFI 控件导出全是值语义，操作 ControlState，不依赖注入结构）
- **集成**：showcase 打包 exit 0

## 7. 不受影响的资产（明确复用，不重做）

实证确认以下与节点表示解耦，pivot 后原样保留：

- **FFI 全集**：控件导出（get/set control value / checked / min/max/step / text / selection / dropdown_selected_index / dropdown_open 等）全为值语义，操作 ControlState
- **C# 投影层**：`LoomGUI.Nodes.cs` 中 `.loom-` 零引用，控件类纯 FFI 转调 —— 除删除 PasswordField/SearchField 外实质无需改动
- **文本编辑算法**：insert/delete/cursor/IME/copy/cut/paste 位于 control.rs，与节点表示解耦
- **事件路由**：`input.rs`（指针/键盘/焦点遍历/命中链/拖拽阈值）是共享基建，仅需改 NodeKind 派发臂
- **浮层渲染通道**：render 末尾追加 + mask=0（见 §3.3）
- **ControlState 状态机**：值语义，不含结构假设
- **showcase CSS 设计**：视觉设计保留，仅换标签与选择器

## 8. 实施顺序约束

本次一次性交付，无对外并存期。但 plan 内部 task 排序须遵守一条约束：

> **「下线标签 + 重写文档」必须排在「showcase 全部改写完成」之后。**

否则 showcase 在中途无法打包，集成验收面断供。role 机制与标签在实施过程中短暂并存（数个 commit 的内部过渡），不对外承诺。

## 9. 与探索稿的差异（实证修正）

探索稿 `2026-07-30-control-div-role-refactor.md` 方向正确，但以下估算与前提经代码实证后修正：

| 探索稿说法 | 实证 | 修正 |
|---|---|---|
| control.rs 需重写 ~1500 行 | 注入 60 行 + sync 134 行 ≈ 200 行 | 高估 3-5 倍 |
| input.rs 1403 行 = 文本/光标/IME 重资产 | input.rs 实为事件路由；文本算法在 control.rs，与节点表示解耦 | **定性错误** |
| 纳入文本控件工作量翻倍 | 仅需改 NodeKind 派发臂 + showcase 13 实例 | 高估；「推倒 input.rs」这一不纳入的论据不成立 |
| C# Nodes.cs 内部实现需改 | `.loom-` 零命中，纯 FFI 转调 | 高估，实际改动≈0 |
| role「纯透传不参与分派」 | role **根本不存储**（Node 无 attrs 字段） | 误导；role 驱动需**新建存储**，此成本探索稿未计 |
| showcase 7 文件 / 文本纳入 +50% | 7 文件；27 视觉 + 13 文本 = +48% | 准确 |
| FFI 大部分保留 / 浮层基建可复用 | FFI 全值语义；浮层仅 1 行耦合 | 准确 |

此外探索稿未涵盖**围栏契约文档漂移**（§1.2 / §4），该问题独立存在且与标签下线强耦合，本 spec 一并处理。
