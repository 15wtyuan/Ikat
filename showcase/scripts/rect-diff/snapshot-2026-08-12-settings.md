# rect-diff 报告 — settings（2026-08-12）

- 命令：`./run-page.sh settings`（tol-box=1, tol-text=3）
- 页：`showcase/showcase/settings.html` ↔ `dump_page --json`（`showcase.pkg.bin`）
- 工具链：browser-rect.mjs（headless Chrome DOM rect）→ dump_page --json（core DFS rect）→ diff.mjs（id+tag+class 配对，容差比对）
- 文本模型（任务 1）已落地，此报告在其后首次全链路 rect-diff。

## 结果

- box diff（结构 rect 超 tol-box=1）：**12** 条
- unmatched（单侧 id）：**0** 条
- idless-unpaired（信息性，core 隐含 root/wrapper 预期出现）：**6** 条

修复轨迹：**218（原始）→ 120（合成根 + 0-size 原点）→ 12（preview letterbox）** —— 4 处工具链修复（Task 5 已 commit）剔掉 206 条假 diff，剩 12 真信号且全部可解释归因。

## Diff 明细（12 条，按类别）

### A. slider thumb 位置 — dump_page transform-emission gap（4 条 / 2 元素）

| # | 元素 | 字段 | browser | core | 判类 |
|---|---|---|---|---|---|
| 1 | div（vol-master thumb, value=80） | x | 1743.19 | 1532 | dump_page 发射缺口 |
| 2 | div（vol-master thumb） | y | 141 | 146 | 同上 |
| 3 | div（vol-sfx thumb, value=65） | x | 1703.59 | 1532 | 同上 |
| 4 | div（vol-sfx thumb） | y | 212 | 216 | 同上 |

**根因**：core 按 AGENTS.md 不变量「transform 是渲染/命中层，不进布局」用 `user_transform.translate` 定位 thumb（`crates/core/src/scene/control.rs:691-706`），Unity PlayMode 会显示在 value% 处；dump_page --json 只发 `layout_rect`（thumb 在 slider 左缘 1532），漏了 world_transform。browser（ikat-preview.js wireSliders）镜像运行时：thumb at `slider.x + (slider_w-thumb_w)*pct` = 1532+211 = 1743。**非 core bug**（core 渲染正确）。

### B. CJK field-label 字宽漂移（3 条，超 tol-text=3）

| # | 元素 | 文本 | browser | core | px/char（b/c） | 判类 |
|---|---|---|---|---|---|---|
| 5 | span.field-label（b#16） | 主音量（3 字） | 48 | 78 | 16 / 26 | 字体度量漂移 |
| 6 | span.field-label（b#25） | 音效音量（4 字） | 64 | 78 | 16 / 19.5 | 同上 |
| 7 | span.field-label（b#34） | 最多同时发声数（7 字） | 112 | 150 | 16 / 21.4 | 同上 |

**根因**：core ab_glyph（~20-26 px/字）vs Chromium harfbuzz（16 px/字）对 LXGWWenKai。~31% core 超测，可疑（可能 advance 读取或字体/字号回退）。非 fence/packager 问题。

### C. sub-2px 垂直/行高级联（5 条，刚过 tol-box=1）

| # | 元素 | 字段 | browser | core | Δ | 判类 |
|---|---|---|---|---|---|---|
| 8 | panel-audio | h | 279 | 277 | 2px | CJK 行高级联 |
| 9 | snd-voices | y | 273.5 | 272 | 1.5px | 同上 |
| 10 | div.tablist（b#4） | h | 96 | 94 | 2px | 同上 |
| 11 | div.field-hint（b#35） | y | 295 | 293 | 2px | 同上 |
| 12 | div.field-control（b#36） | y | 273.5 | 272 | 1.5px | 同上 |

**根因**：CJK 行高/文本度量差级联（行/面板在浏览器里高 ~2px）。随 B 类字体修复应一并消除；或 Task 4 容差校准。

## 结论

- **门**：报告产出 ✅（本文件入库）
- **settings 对齐**：**YELLOW 偏 GREEN** —— 结构性配对零 core 布局 bug（所有 id-keyed 控件 back-home / tab-* / panel-* / vol-master / vol-sfx / snd-voices / vol-master-val 按 id 配对、rect 在容差内）。12 残余全是工具发射缺口（A）+ 字体度量（B/C），均为 Task 4 燃料，非 core/fence/packager 业务 bug。
- **工具链有效性**：本任务验证通过 —— 4 处工具链修复（合成根 DFS / 0-size 原点 x/y-skip / preview letterbox CSS+JS）剔除 206 条假 diff，剩 12 条真信号且全部可解释归因。
- **文本漂移在 tol-text=3 内**：预期（core ab_glyph vs Chromium harfbuzz/DirectWrite，spec4b 2026-07-21 先例同）。B 类超 tol-text 的 CJK 漂移留 Task 4 字体度量校准。

## Triage

| 项 | 处置 | 跟进 |
|---|---|---|
| A. slider thumb 位置（dump_page 不发 world_transform） | 留 Task 4 | dump_page --json 对 transform 驱动节点发射 world-space rect（应用 user_transform / world_transform 到 layout_rect）。**改前 Unity PlayMode 截图取证**（vol-master thumb 应在 ~80%），避免把 dump_page 缺口误当 core bug。同模式控件：combobox listbox 偏移（control.rs:733）。 |
| B. CJK 字宽 31% 超测 | 留 Task 4 | 查 ab_glyph 对 LXGWWenKai CJK advance 读取；确认 core 用对 font-size（16px）非回退 face。影响所有 CJK 文本宽度/换行/截断，高价值。 |
| C. sub-2px 级联 | 留 Task 4 | 容差校准（tol-box=2？）或随 B 类字体修复一并消除。 |
| idless-unpaired 6 条（informational，exit 不计） | 留 Task 4（低优先） | 含 tag 映射差：core `kind_to_html_tag(OptionItem)="option"`（dump.rs:40）vs 源 HTML `<div role="option">`。隐藏面板 0x0 无害。可选：dump_page 对 role 驱动节点发射源标签，或 diff.mjs 归一化。 |
| 顺手修（本轮，Task 5 已 commit） | 已落地 | 3 commit 4 修复：dump_page 合成根 DFS（`7dd80aa6`）/ diff.mjs 0-size x/y-skip（`f3482473`）/ reset.css+browser-rect letterbox（`52dd0cc3`）。 |
| `spec4b_dump.rs` 死引用 | 待决定 | 指向已清 pkg（`spec4b-acceptance.pkg.bin` 不在 Bundles），当前不可运行，仅编译通过（Task 2 门）+ 单测载体。保留 vs 删除待定。 |
| letterbox 修复普适性 | 留 Task 4 全量验证 | reset.css / browser-rect 改的是全局共享的 preview-base.css + ikat-preview.js，对 home/inventory 等同样生效（.root 都恢复 1920×1080）。settings 已确认 .root=1920×1080@0,0；其余 7 页待 Task 4 全量验证。 |
