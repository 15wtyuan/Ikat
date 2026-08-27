---
name: session-summary
description: >
  Use when the user says "summarize this session", "总结到 skill", "更新踩坑",
  "总结到 AGENTS.md", or after completing a significant Ikat feature or bugfix and wanting to persist
  the session's learnings into the project knowledge base. 把当前 session 的 Ikat 经验分类沉淀：
  可复用坑规则进 docs/pitfalls.md（按主题归位，不记 bug 编年史），高价值原则/调试/偏好进 AGENTS.md。
---

# Ikat Session Summary

将当前会话中与 Ikat 相关的经验分类沉淀进项目知识库。**设计契约变化或契约文档漂移都要同步 `docs/design/main-design.md`**——不只看本轮改了什么，还要 grep 确认设计文档和实现一致（见 Process 步骤 3 的强制检查）。

## 两个去处（判据：是否高价值可复用 + 不希望 AI 遗忘）

| 类型 | 去向 | 判据 |
|---|---|---|
| **坑的可复用规则**（依赖 API 事实、跨层动态契约、平台特性） | `docs/pitfalls.md` | **只有「看代码看不出来、未来会再触发」的规则才进**；具体 bug 的症状/根因/修复过程不记（代码 + git history 是载体） |
| **依赖 API 适配**（crate 版本签名差异、草稿与实际不符） | `docs/pitfalls.md` §1 | 具体 crate API 差异（属上一行的可复用规则） |
| **设计契约变化/漂移**（新围栏 CSS 属性、新 Node/RenderNode 字段、blob 列结构变、FFI 签名变、架构调整；**或本轮改了契约但设计文档对应段还停在旧版本**） | `docs/design/main-design.md` | 改了契约才动；纯实现不进。**漂移也要修**：改了 blob 列/FFI/字段必 grep main-design 对应段（§13.3 FFI 契约 / §13.6 镜像生命周期 / §3 围栏 / §12 动态树等）确认同步 |
| **对外文档同步核查**（本轮改了 fence schema / 公共 API / preview 行为 / CLI 命令面，但没按 AGENTS「文档涟漪表」动 `crates/packer/pkg/templates/`） | `crates/packer/pkg/templates/` 对应文件 | 收尾兜网：templates/ 就是对外文档（随 scaffold 分发给消费侧 AI），AI 最容易漏。名字集合漂移会被 `cargo test -p ikat_pkg --test consumer_doc_sync` 拦住，但**语义措辞漂移只有这里能兜**——按涟漪表核对措辞后，有漏的先补再结题 |
| **高价值原则/设计哲学/调试技巧/用户偏好** | `AGENTS.md` | **必须高价值、可复用、不希望 AI 遗忘**才放——不是什么都放 |

## Process

### 1. Review Session Context

从当前会话提取 Ikat 相关工作：
- 改了哪些文件/模块？
- 解决了什么问题？
- 踩了什么坑（尤其**依赖 API 与草稿/plan 不符**、**AI 可预测性约束违背**）？
- 有哪些新机制/调试技巧/原则变化？

### 2. 读现状 + grep 防重复

- 读 `docs/pitfalls.md` 先 grep 确认要加的坑没重复
- 读 `AGENTS.md` 当前内容（确认要加的原则/调试不在里面）

### 3. 分类沉淀

**坑的可复用规则** → `docs/pitfalls.md` 按主题归位（依赖适配 / 跨层闭环 / Unity 平台 / 动态契约四组之一），并入已有条目或新增短条目（规则 + why，1-5 行）。先过门槛：这条规则不看代码能知道吗？未来还会再触发吗？两个都不是就只留在代码/commit 里。

**依赖 API 适配** → `docs/pitfalls.md` §1 对应 crate 子节（或新子节）。

**设计契约变化/漂移** → `docs/design/main-design.md` 对应章节（围栏属性变化还要同步 `fence.md` + `fence_contract.rs` 测试）。

**强制：契约文档一致性检查**（本轮改了以下任一就必跑，防文档漂移）：
- 改了 blob 列结构（VERSION 变/加列）→ grep main-design `列` / `SOA` / `blob v`，确认 §13.3 列数 + 字段列表同步
- 改了 FFI 入口/签名 → grep main-design `FFI` / `extern`，确认 §13.3 + 调用方描述同步
- 改了 RenderNode/Node 字段 → grep main-design 字段名，确认 §13.3 列描述 + §13.6 镜像生命周期同步
- 改了 ChangeLevel/dirty hash 机制 → 确认 §13.3"沿用上帧"段 + §13.6 同步段描述同步
- 改了围栏属性（apply_decl 加 arm）→ 确认 §3.2 + fence.md 同步
- 改了架构不变量（tick 时序/分层/复用机制）→ 确认对应段同步

**漂移判定**：grep 到的描述和本轮实现不符（如文档写"18 列"实现已 22 列、写"Unchanged"实现已 ChangeLevel、写"按 node_id"实现已双 dict）= 漂移，**必修**，即使本轮没碰那段文档。SDD 多 task 割裂易留漂移——final review 只审本轮改动，漂移要靠本步骤兜底。

**高价值原则/调试/偏好** → `AGENTS.md`。**严格判据**：
- ✅ 放：AI 下次会重犯的错（如 stale .dll、改 parse-time 必重打 pkg）、跨任务可复用的方法论（如"草稿常不符，查 crate 源码"）、设计哲学约束（AI 可预测性 8 条）、高价值调试路径（dump_*.rs）
- ❌ 不放：一次性实现细节、具体函数签名、版本号快照、单 feature 流程——这些靠代码 + git history
- 边界：拿不准的进 pitfalls.md，AGENTS.md 宁缺毋滥（它是 AI 每次都读的，太长会稀释）

### 4. Commit

```bash
git add docs/pitfalls.md AGENTS.md docs/design/main-design.md
# 围栏属性变化另加：docs/design/fence.md
# 范围/路线变化另加：docs/roadmap/roadmap.md
git commit -m "docs: 总结 session — <一句话主题>"
```
注：main-design.md 默认 add——本轮若改了契约（blob 列/FFI/字段/围栏/架构）几乎必有同步项（步骤 3 强制检查）；若 grep 确认无漂移再排除。

## 常见误判

| 误判 | 纠正 |
|---|---|
| 把实现细节写进 main-design.md | 设计契约才进 docs/design；实操不进任何文档，靠代码 |
| 什么都往 AGENTS.md 塞 | AGENTS.md 只放高价值可复用；实现细节靠代码 |
| 什么都记进 pitfalls.md | 只收可复用规则（看代码看不出来的）；具体 bug 编年史不记——代码 + git history 是载体 |
| 重复已有规则 | 先 grep pitfalls.md，同主题合并而非另起新条 |
| 坑写成长篇调试流水账 | 规则 + why，≤5 行 |
| 漏掉依赖 API 适配 | 这是最易重复踩的（plan 草稿常与 crate 实际不符），每次踩必进 pitfalls.md §1 |
| **改了契约但没同步 main-design 对应段**（文档漂移） | 改 blob 列/FFI/字段/围栏/架构后必 grep main-design 对应段确认同步；SDD task 割裂易留漂移，final review 只审本轮改动兜不住——本 skill 步骤 3 强制检查是兜底 |
| session 结束不总结 | 经验丢失 = 下次重复踩；完成功能/修复即触发本 skill |
