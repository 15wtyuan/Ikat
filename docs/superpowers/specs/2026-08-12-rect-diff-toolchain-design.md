# rect-diff 工具链打通一页（settings）

- **日期**：2026-08-12
- **状态**：设计（待 review → writing-plans）
- **范围**：roadmap 里程碑 1 任务 2（严格卡门：一页产比对报告）
- **相关**：`docs/roadmap/roadmap.md` 近期任务 2 / 任务 4、`showcase/scripts/rect-diff/`（browser-rect.mjs / diff.mjs / snapshot-2026-07-21.md）、`crates/core/examples/{spec4b_dump,dump_page}.rs`

---

## 1. 背景与问题

### 1.1 任务定义（roadmap）

任务 2 = **rect-diff 工具链打通一页**。门：**rect-diff 在一页产比对报告**。roadmap 明说「工具链可先搭，但结果要等任务 1 文本对了才有意义」——任务 1（文本模型 inline flow）代码侧已 done（commit `5a9cfafc`），此刻跑 rect-diff 才有意义。

### 1.2 现状核实（读码）

| 组件 | 状态 | 缺口 |
|---|---|---|
| `showcase/scripts/rect-diff/browser-rect.mjs` | ✅ Playwright/Chromium 1920×1080 导出 DOM rect `{domIndex,tag,id,classes,x,y,w,h}` | 无 |
| `showcase/scripts/rect-diff/diff.mjs` | ✅ id 主配对 + tag/class 桶回退，`--tol-box`/`--tol-text`，exit 0/1/2 | 无 |
| `crates/core/examples/spec4b_dump.rs --json` | ✅ 输出形状与 diff.mjs 对齐，`kind_to_html_tag` 映射齐全 | **硬编码 spec4b 包/组件**，stage 1280×720 |
| `crates/core/examples/dump_page.rs` | ✅ 任意 showcase 页，stage 1920×1080，字体/图标尺寸正确 | **无 `--json` 模式** |
| `crates/core/src/dump.rs` | `dump_scene_json` 内有 kind→tag 逆映射（`TabList→div`、`Tab→button`） | 非 pub，spec4b_dump 里是私有拷贝 |
| `LoomHost.DumpSceneJson` | ✅ 已实现 + `LoomHostDumpTests.cs` | 输出形状 ≠ diff.mjs；home-machine 事 |

**结论**：任务 2 的「一页产比对报告」在编码机 headless 全闭环可行。缺的只有两小块：core 侧 8 页通用的 `--json` 出口 + 三步串联的 runner。

### 1.3 页选依据

**settings**：静态、无动画/transform/渐变；控件密集（tablist + 5 tab/panel、slider×6、radio×6、combobox×6、textbox×5、switch×5、spinbutton×2），控件束已全交付 → 任何 diff 要么是真 bug 要么是已知文本度量漂移，不混「功能没做」噪声。也是任务 4 静态页第一梯队（settings/character/shop/form/lab）。

---

## 2. 设计决策（brainstorm 已定）

| # | 决策 | 选项 | 理由 |
|---|---|---|---|
| D1 | **范围** | A：严格卡门——工具链 + settings 一页报告，跑完即止 | 门定义清晰，与 roadmap 边界一致；8 页横扫归任务 4 |
| D2 | **diff 处置** | A：报告 + 分流，只修顺手（一眼根因、改动小）；其余记 triage 表留任务 4 | 门=报告产出，不要求 settings 全绿；diff 是任务 4 燃料 |
| D3 | **core 侧出口** | 方案 1：扩 `dump_page.rs` 加 `--json`；不泛化 spec4b_dump、不建新 example | 改动最小，复用现成 showcase 配置；spec4b_dump 是战过场的诊断工具不动 |
| D4 | **rect 语义** | 发射 `layout_rect`（spec4b 先例） | settings 无 transform，layout == 浏览器 box；world-rect 升级留动画页（任务 4 home） |
| D5 | **报告** | diff.mjs stdout（工具输出）+ 手工撰写 md 入库（沿 snapshot-2026-07-21.md 先例） | 报告是证据，入库；JSON 产物是暂态不入库 |

---

## 3. 设计

### 3.1 架构与数据流

```
settings.html ─────────► browser-rect.mjs（已有） ──► browser-settings.json
showcase.pkg.bin ──────► dump_page --json（新）    ──► core-settings.json
                          （形状与 browser 侧对齐）
                                    │
                                    ▼
                        diff.mjs（已有）──► 比对摘要 + exit code
                                    │
                                    ▼
                  snapshot-2026-08-12-settings.md（手工撰写，入库）
```

### 3.2 改动清单（3 处）

1. **`crates/core/examples/dump_page.rs` 加 `--json <out>`**：
   - 照 spec4b_dump 的发射器：DFS 收集 + `kind_to_html_tag` + `layout_rect` → `{domIndex,tag,id,classes,x,y,w,h}`（serde_json 输出，与 diff.mjs 形状对齐）。
   - 复用其已有 showcase 配置（stage 1920×1080、LXGW+wqy 字体、icon_sizes），零新增配置。
2. **`kind_to_html_tag` 提取为 `dump.rs` 的 `pub fn`**：
   - `dump.rs` 的 `dump_scene_json` 已有同款 kind→tag 逆映射，提取为 pub fn 共用；spec4b_dump 和 dump_page 都改用它——消除三处拷贝漂移。
   - spec4b_dump 的迁移是 2 行改动，**其 `--json` 行为不变**（映射语义相同），spec4b 重跑即为迁移回归守卫。
3. **新 runner `showcase/scripts/rect-diff/run-page.sh`**（bash，`set -e`）：
   - `run-page.sh <page>`：① browser-rect.mjs → ② `cargo run -p loomgui_core --example dump_page -- <page> --json` → ③ diff.mjs（`--tol-box=1 --tol-text=3`，与 spec4b 先例一致）。
   - 产物 JSON 落 `showcase/scripts/rect-diff/out/<page>/`；`.gitignore` 加 `out/`（暂态不入库；入库的只有报告 md）。
   - 任一步失败即停，透传 diff.mjs 的 exit code。

### 3.3 报告内容（`snapshot-2026-08-12-settings.md`）

页名/日期/命令、diff 计数摘要（box diff / unmatched / idless-unpaired）、按幅度排序的前 N 条 diff 明细、结论（门状态）、triage 表（顺手修的 + 留任务 4 的）。

---

## 4. 错误处理

| 层 | 失败场景 | 行为 |
|---|---|---|
| runner | 任一子步非零退出 | `set -e` 立即停 |
| browser-rect.mjs | Playwright/Chromium 缺失、页面加载失败 | 非零退出 → runner 停 |
| dump_page | 编译错、页名非法（instantiate panic） | 非零退出 → runner 停 |
| diff.mjs | 用法错=2；有 diff/unmatched=1；全绿=0 | runner 透传 |

**关键语义**：diff.mjs exit 1 =「有 diff」**不等于任务失败**——门是「报告产出」，报告如实记录 diff 计数与结论。runner 退出码即 diff 结果，供任务 4 复用当门判。

---

## 5. 测试与验收门

1. **前置 sanity：spec4b triad 重跑**（最先）——`kind_to_html_tag` 提取的回归守卫：fresh browser-rect + `spec4b_dump --json` + diff.mjs，应回到 ~0 box diffs（文本漂移容忍内，对照 snapshot-2026-07-21）。
2. **主门：`run-page.sh settings` 产报告**——`snapshot-2026-08-12-settings.md` 入库。
3. **已知容差**：settings label/value 文本多 → 预期少量 text diff 在 `--tol-text=3` 内，不是门失败（spec4b 先例）。
4. **风险预案**：browser-rect 注入 reset.css 覆盖 UA 默认（input 控件盒模型）；settings 控件密集，若控件 rect 出现**系统性**偏移，先查是否 reset 引入的假 diff，再判真 bug。
5. **结构性 diff 处置**（D2）：一眼根因 + 改动小的当场修（fence/packager 一行级）；其余记 triage 表留任务 4。

---

## 6. 不在范围（明确 defer）

- **Unity half**（browser vs Unity `DumpSceneJson`）：家里机任务 4；`DumpSceneJson` 输出形状 ≠ diff.mjs，届时需适配（归任务 4 设计）。
- **8 页 dashboard**：任务 4（runner 已参数化，届时循环 + 逐页修 bug）。
- **world-rect 发射**：动画页（home）需要，任务 4 升级。
- **spec4b_dump 泛化 / 合并**：维持原样（spec4b 专用诊断）。
