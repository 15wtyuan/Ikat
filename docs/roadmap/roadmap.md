# LoomGUI 路线图（已迁移 GitHub issues）

> **2026-08-24 起活工作项全部在 GitHub issues**，本文件不再维护任务清单。常用查询：
>
> ```bash
> gh issue list --milestone "M2 · Dogfood"          # 当前里程碑
> gh issue list --label deferred                    # backlog（非契约，触发判据见 issue 正文）
> gh issue list --label t1-capability               # 按 track 过滤（t1/t2/t3/tx）
> ```
>
> milestone = `M2 · Dogfood` → `M3 · 跨引擎与契约消化` → `v1.0 · 发版`（门控序列：M 清才进下一个）。label = `t1-capability` / `t2-release` / `t3-expand` / `tx-debt`，非契约项另带 `deferred`。
>
> 本文件只保留北极星判据——愿景不是任务，文档是它正确的家。历史路线图归档在 [`archive/`](./archive/)：迁移前最后版本 [`roadmap-2026-08.md`](./archive/roadmap-2026-08.md)（含延期项登记表原文）、v1 摸黑 + 三束纪元 [`roadmap_old.md`](./archive/roadmap_old.md)、旧里程碑 [`milestones_old.md`](./archive/milestones_old.md)。踩坑全库见 `docs/pitfalls.md`，活的架构契约在 `docs/design/`。

## 北极星：完全体

把 LoomGUI 做成一个**真能拿来发布游戏**的跨引擎游戏 UI 框架。判据（全部满足 = 完全体）：

1. **契约对齐**：三份终态契约（`public-api.md` / `main-design.md` / `fence.md`）100% 落地——公共 API 无 `NotImplementedException` 壳、围栏 CSS 子集完整、架构边界（公共语义 / 内部行为 / 引擎后端三层）干净。**契约承诺 = 必做**：契约项在 M2/M3 消化，v1.0 发版 issue 含契约对齐终审兜底。
2. **跨引擎**：Unity 后端 ✓；Godot-C# 后端跑通同一 showcase——兑现「Rust 核心 + 多引擎投影」赌注。
3. **布局护城河**：showcase 8 页在 Unity 真机全跑通 + 布局 rect 与浏览器对齐（护城河 = AI 能预测布局，不是像素级渲染）。✓ 2026-08-16（里程碑 1 完工）。
4. **可用证明**：一个真实小游戏用它做出来，证明「AI 拼 HTML/CSS 即得界面」核心论点。
5. **发版**：v1.0——公共 API 冻结 + 契约版本化 + 文档 + git URL 分发（release CI 已就绪）。

## 工作方式（不变）

- **track 并行 + 里程碑门**：四条 track 独立推进（issue label 表 track），每个里程碑有可测 done 标准（issue milestone 表门）。不横切大层。
- **showcase / dogfood 逼需求**：不凭空补理论清单，能力由「做页面 / 做游戏」逼出来；`deferred` 项的触发判据就是这个机制的 issue 化。
- **SDD**：每件稍大的事 `spec → plan → implement → review`；坑进 `pitfalls.md`，不进代码。
- **两台机约束**：核心范式在编码机 headless 锁死；真机视觉 / 输入验收在 Unity 机。
- **叙事进 commit，不进 roadmap**：「为什么改方向」写进决策记录 commit / pitfalls；工作项状态看 issues。
