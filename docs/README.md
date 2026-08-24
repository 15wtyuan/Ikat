# LoomGUI 文档

跨引擎游戏 UI 框架。Rust 核心 + 多引擎后端（Unity 首发），HTML/CSS 子集 DSL，自绘渲染。

## 文档结构

| 目录 | 内容 | 何时读 |
|---|---|---|
| [`design/`](design/) | [主设计](design/main-design.md)（项目设计真相源）+ [围栏权威](design/fence.md) | 理解"设计成什么样、怎么实现" |
| [`roadmap/`](roadmap/) | 北极星判据 + 归档历史（活工作项在 GitHub issues） | 追溯路线决策史 |
| [`pitfalls.md`](pitfalls.md) | 精炼规则手册：依赖适配 / 跨层闭环 / Unity 平台 / 动态契约（历史 231 条已归档 `archive/`） | 开工前查"坑在哪" |
| [`code-comments.md`](code-comments.md) | 注释哲学：对立观点、共识公理、开源实践谱系与本仓库立场（出处+范例，非操作步骤） | 写/审注释前 |

## 入口

- **开发依据**：[`design/main-design.md`](design/main-design.md) —— 项目设计真相源
- **围栏属性**：[`design/fence.md`](design/fence.md) —— 权威清单（真相源 `crates/fence/src/schema/`）
- **工作项/路线**：GitHub issues（milestone 门控 M2 → M3 → v1.0；label 按 track）；北极星判据见 [`roadmap/roadmap.md`](roadmap/roadmap.md)
- **踩坑/依赖 API 适配**：[`pitfalls.md`](pitfalls.md)（精炼规则手册；历史全文见 [`archive/`](archive/)）
- **AI 工作约束**：根 [`CLAUDE.md`](../CLAUDE.md)

## 维护原则

- **主设计 = 项目设计真相源**：只写设计意图 + 契约。不写机制实现细节（在 roadmap 草稿），不写迭代历史，不写版本标注。
- **决策理由**体现在主设计各章节的设计说明里；历史决策追溯见 git。
- **围栏属性权威 = `fence.md`**（真相源 `crates/fence/src/schema/`）；主设计 §3 只写哲学/原则，不重复属性表。
- **踩坑**进 `pitfalls.md`——只收可复用规则、按主题归位、不编号；bug 编年史不记（代码 + git history 是载体；历史 231 条见 `archive/pitfalls-2026-08.md`）。
- **AI 工作约束 + 高价值可复用经验**进根 `CLAUDE.md`。
- **工作项**进 GitHub issues（2026-08-24 起；`roadmap/` 目录只留北极星判据 stub + 归档历史）。
