# rect-diff 工具链打通一页（settings）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 rect-diff 在编码机 headless 全闭环——`dump_page --json` + runner 串联 browser-rect → core dump → diff，settings 一页产比对报告（roadmap 任务 2 的门）。

**Architecture:** 三段式：`browser-rect.mjs`（已有，Playwright 导 DOM rect）↔ `dump_page --json`（本轮加，core 导 layout rect，形状对齐）→ `diff.mjs`（已有，id 配对 + 容差比对）→ 报告 md 入库。`kind_to_html_tag` 从 spec4b_dump 私有拷贝提升为 `dump.rs` 的 `pub fn` 共用（浏览器配对语义，与 `dump_scene_json` 的诊断映射不同，后者不动）。

**Tech Stack:** Rust (edition 2021, serde_json 已在 core deps)、Node (Playwright 已有)、bash。

## Global Constraints

- **Rust edition 2021**；零新依赖（serde_json 已在 `crates/core` deps；node 侧零新 npm 包）。
- **`dump_scene_json` 不动**——其 kind→tag 是诊断近似映射（TextNode→`span`/ListView→`div`/CustomElement→`div`），与配对语义不同；`LoomHost.DumpSceneJson`（FFI `loomgui_stage_dump_scene`）是消费者，动它=破坏 Unity 侧。
- **`kind_to_html_tag` 是浏览器配对语义**：TextNode→`#text`（diff.mjs 按此过滤空白文本节点）、ListView→`ul`、CustomElement→`custom`。全部 21 kind 必须有断言覆盖（防漂移）。
- **spec4b_dump 行为逐字节不变**（除删私有拷贝 + 改 import）；spec4b pkg/HTML 已不在库（三束加宽清掉），无法运行验证，**只编译验证**。
- **`.gitignore` 加 `out/`**：JSON 产物暂态不入库；入库的只有报告 md。
- **runner 退出码即 diff 结果**（0=对齐 1=有 diff 2=用法错）；有 diff ≠ 任务失败，门是「报告产出」。
- **执行模型**：subagent 用 `DeepSeek/deepseek-v4-pro`（直连）；**禁用 `netease-codemaker/*`**（公司账号，AGENTS.md 硬规则）。

---

### Task 1: `dump.rs` 新增 `pub fn kind_to_html_tag` + 全表单测

**Files:**
- Modify: `crates/core/src/dump.rs`（加 pub fn + tests mod 加测试）

**Interfaces:**
- Produces: `loomgui_core::dump::kind_to_html_tag(k: NodeKind) -> &'static str`——后续 Task 2/3 消费。

- [ ] **Step 1: 写 failing test**

在 `crates/core/src/dump.rs` 的 `#[cfg(test)] mod tests` 末尾（`dump_escapes_quotes_in_id` 之后）加：

```rust
    #[test]
    fn kind_to_html_tag_matches_browser_pairing_semantics() {
        assert_eq!(kind_to_html_tag(NodeKind::Container), "div");
        assert_eq!(kind_to_html_tag(NodeKind::TextNode), "#text");
        assert_eq!(kind_to_html_tag(NodeKind::TextElement), "span");
        assert_eq!(kind_to_html_tag(NodeKind::Button), "button");
        assert_eq!(kind_to_html_tag(NodeKind::Image), "img");
        assert_eq!(kind_to_html_tag(NodeKind::TextField), "input");
        assert_eq!(kind_to_html_tag(NodeKind::NumberField), "input");
        assert_eq!(kind_to_html_tag(NodeKind::Slider), "input");
        assert_eq!(kind_to_html_tag(NodeKind::Toggle), "input");
        assert_eq!(kind_to_html_tag(NodeKind::RadioButton), "input");
        assert_eq!(kind_to_html_tag(NodeKind::TextArea), "textarea");
        assert_eq!(kind_to_html_tag(NodeKind::Dropdown), "select");
        assert_eq!(kind_to_html_tag(NodeKind::OptionItem), "option");
        assert_eq!(kind_to_html_tag(NodeKind::ProgressBar), "progress");
        assert_eq!(kind_to_html_tag(NodeKind::ListView), "ul");
        assert_eq!(kind_to_html_tag(NodeKind::ListItem), "li");
        assert_eq!(kind_to_html_tag(NodeKind::Slot), "slot");
        assert_eq!(kind_to_html_tag(NodeKind::CustomElement), "custom");
        assert_eq!(kind_to_html_tag(NodeKind::Template), "template");
        assert_eq!(kind_to_html_tag(NodeKind::TabList), "div");
        assert_eq!(kind_to_html_tag(NodeKind::Tab), "button");
    }
```

- [ ] **Step 2: 跑测试确认失败（编译错）**

Run: `cargo test -p loomgui_core dump::tests::kind_to_html_tag_matches_browser_pairing_semantics`
Expected: FAIL with "cannot find function `kind_to_html_tag` in this scope"（函数未定义，编译期失败=测试红）

- [ ] **Step 3: 实现 `kind_to_html_tag`**

在 `crates/core/src/dump.rs` 的 `pub fn dump_scene_json` 之前插入：

```rust
/// NodeKind → 浏览器侧 `tagName.toLowerCase()` 对应 tag 串（rect-diff 配对语义）。
/// 与 `dump_scene_json` 的诊断 tag 映射**不同**（TextNode: `#text` vs `span`；ListView:
/// `ul` vs `div`；CustomElement: `custom` vs `div`）——本函数服务于浏览器 rect 配对
/// （TextNode 在浏览器 `querySelectorAll('body *')` 无元素，diff.mjs 按 `#text` 过滤），
/// 诊断 dump 保留自己的近似映射。全部 21 kind 由单测断言覆盖（防漂移）。
pub fn kind_to_html_tag(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Container => "div",
        NodeKind::TextNode => "#text",
        NodeKind::TextElement => "span",
        NodeKind::Button => "button",
        NodeKind::Image => "img",
        NodeKind::TextField
        | NodeKind::NumberField
        | NodeKind::Slider
        | NodeKind::Toggle
        | NodeKind::RadioButton => "input",
        NodeKind::TextArea => "textarea",
        NodeKind::Dropdown => "select",
        NodeKind::OptionItem => "option",
        NodeKind::ProgressBar => "progress",
        NodeKind::ListView => "ul",
        NodeKind::ListItem => "li",
        NodeKind::Slot => "slot",
        NodeKind::CustomElement => "custom",
        NodeKind::Template => "template",
        NodeKind::TabList => "div",
        NodeKind::Tab => "button",
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p loomgui_core dump::tests::kind_to_html_tag_matches_browser_pairing_semantics`
Expected: PASS（21 断言全绿）

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/dump.rs
git commit -m "feat(core): pub fn kind_to_html_tag — browser pairing tag map + 21-kind unit test"
```

---

### Task 2: spec4b_dump 迁移到共用 `kind_to_html_tag`

**Files:**
- Modify: `crates/core/examples/spec4b_dump.rs`（删私有拷贝 + 改 import）

**Interfaces:**
- Consumes: `loomgui_core::dump::kind_to_html_tag`（Task 1 产出）。
- Produces: 无新接口；`--json` 输出逐字节不变（同一函数体搬进 lib）。

- [ ] **Step 1: 删 spec4b_dump 的私有 `kind_to_html_tag`**

删除 `crates/core/examples/spec4b_dump.rs` 里的私有 `fn kind_to_html_tag(k: NodeKind) -> &'static str { ... }`（从 `/// NodeKind → 浏览器侧...` 注释到函数结束的整个 match 块，约 30 行）。**注意**：文件里还有 `print_diagnostic_row` 内部的另一个局部 `match n.kind`（诊断缩写 kind，`btn`/`#text` 等）——那个**不删**，只删 `kind_to_html_tag` 函数本体。

- [ ] **Step 2: 加 import**

在 `crates/core/examples/spec4b_dump.rs` 的 import 区（`use loomgui_core::scene::node::{Node, NodeId, NodeKind, Scene};` 附近）加：

```rust
use loomgui_core::dump::kind_to_html_tag;
```

JSON 发射处的 `"tag": kind_to_html_tag(n.kind),` 代码不变（同名函数现来自 lib）。

- [ ] **Step 3: 编译验证**

Run: `cargo build -p loomgui_core --example spec4b_dump`
Expected: 编译通过，无警告（删函数后 `NodeKind` import 仍被 print_diagnostic_row 使用，不会 unused）

- [ ] **Step 4: 跑 core 测试确认无回归**

Run: `cargo test -p loomgui_core`
Expected: 全绿（dump.rs 新单测 + 既有测试）

- [ ] **Step 5: Commit**

```bash
git add crates/core/examples/spec4b_dump.rs
git commit -m "refactor(core): spec4b_dump uses shared dump::kind_to_html_tag (drop private copy)"
```

---

### Task 3: `dump_page` 加 `--json` 模式

**Files:**
- Modify: `crates/core/examples/dump_page.rs`

**Interfaces:**
- Consumes: `loomgui_core::dump::kind_to_html_tag`（Task 1 产出）。
- Produces: `dump_page <page> --json <out>`——输出 `{domIndex, tag, id, classes, x, y, w, h}` 数组（DFS 序，含 root），与 `browser-rect.mjs` 形状对齐；Task 4 runner 消费。

- [ ] **Step 1: 加 imports**

`crates/core/examples/dump_page.rs` 现有 import 区：

```rust
use loomgui_core::scene::dynamic::append_child;
use loomgui_core::scene::node::{Node, NodeId, NodeKind};
use loomgui_core::stage::Stage;
```

改为（加两行）：

```rust
use loomgui_core::dump::kind_to_html_tag;
use loomgui_core::scene::dynamic::append_child;
use loomgui_core::scene::node::{Node, NodeId, NodeKind, Scene};
use loomgui_core::stage::Stage;
```

- [ ] **Step 2: main 开头解析 `--json` 参数**

在 `dump_page.rs` main 的 `let page = std::env::args()...` 之后加：

```rust
    let json_out = parse_json_out_arg();
```

- [ ] **Step 3: main 末尾发射 JSON**

在 main 的最后（world_matrix 顶点采样诊断循环结束、`}` 收尾之前）加：

```rust
    if let Some(out_path) = json_out {
        let dfs = collect_dfs(scene, root_id);
        let nodes_json: Vec<serde_json::Value> = dfs
            .iter()
            .enumerate()
            .map(|(i, nid)| {
                let n = scene.get(*nid).expect("DFS node must exist");
                let r = n.layout_rect;
                serde_json::json!({
                    "domIndex": i,
                    "tag": kind_to_html_tag(n.kind),
                    "id": n.id_attr.clone(),
                    "classes": n.classes.clone(),
                    "x": r.x,
                    "y": r.y,
                    "w": r.w,
                    "h": r.h,
                })
            })
            .collect();
        let json_str = serde_json::to_string_pretty(&nodes_json).expect("serialize json");
        std::fs::write(&out_path, json_str).expect("write json");
        eprintln!("wrote {} DFS nodes -> {}", dfs.len(), out_path);
    }
```

**注意**：`scene` 变量在 main 中已存在（`let scene = s.scene.as_ref().unwrap();`，tick 之后）；`root_id` 已存在（`s.create_root`）。JSON 发射块放最末尾，借用都在 main 内，无冲突。

- [ ] **Step 4: 文件末尾加三个 helper 函数**

在 `dump_page.rs` 末尾（`fn icon_sizes()` 之后）加：

```rust
/// 从 `std::env::args` 解析 `--json <path>`。无该参 → None。
/// 不接 clap（零新依赖，CLI 表面极小，手写足够）。
fn parse_json_out_arg() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--json" {
            return Some(args.next().expect("--json requires a <path> argument"));
        }
    }
    None
}

/// DFS 先序收集节点 id（含 root）。与浏览器 `body *` 的 DOM 序同源——子树按子节点出现
/// 顺序递归展开。核心 Scene 只存 `Node.children: Vec<NodeId>`，无需父→子索引构建。
fn collect_dfs(scene: &Scene, root: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    collect_dfs_rec(scene, root, &mut out);
    out
}

fn collect_dfs_rec(scene: &Scene, id: NodeId, out: &mut Vec<NodeId>) {
    out.push(id);
    // 拷 children 出去再递归——避开 scene.nodes 的不可变借用跨递归调用。
    let children: Vec<NodeId> = scene
        .get(id)
        .map(|n| n.children.clone())
        .unwrap_or_default();
    for c in children {
        collect_dfs_rec(scene, c, out);
    }
}
```

- [ ] **Step 5: 编译验证**

Run: `cargo build -p loomgui_core --example dump_page`
Expected: 编译通过，无警告

- [ ] **Step 6: 跑 settings 验证 JSON 形状**

Run: `cargo run -q -p loomgui_core --example dump_page -- settings --json /tmp/settings-core.json`
Expected: 结尾打印 `wrote N DFS nodes -> /tmp/settings-core.json`；然后用

```bash
node -e "const a=require('/tmp/settings-core.json'); console.log(a.length, a[0].domIndex, a[0].tag, a[0].x, typeof a[0].classes)"
```

输出 `N 0 div <number> object`（数组非空、首元素 domIndex=0、tag=div、x 是数字、classes 是数组）。形状与 `browser-rect.mjs` 对齐。

- [ ] **Step 7: Commit**

```bash
git add crates/core/examples/dump_page.rs
git commit -m "feat(core): dump_page --json — rect-diff compatible DFS dump for any showcase page"
```

---

### Task 4: runner `run-page.sh` + `.gitignore`

**Files:**
- Create: `showcase/scripts/rect-diff/run-page.sh`
- Modify: `showcase/scripts/rect-diff/.gitignore`（加 `out/`）

**Interfaces:**
- Consumes: `browser-rect.mjs` / `diff.mjs`（已有）、`dump_page <page> --json`（Task 3 产出）。
- Produces: `run-page.sh <page>`——产物 `out/<page>/browser-<page>.json` + `core-<page>.json`；退出码透传 diff.mjs（0=对齐 1=有 diff 2=用法错）。Task 5/6 消费。

- [ ] **Step 1: 写 runner**

创建 `showcase/scripts/rect-diff/run-page.sh`：

```bash
#!/usr/bin/env bash
# rect-diff runner: browser-rect → core dump_page --json → diff.mjs, one page.
#
# Usage: run-page.sh <page> [--tol-box=N] [--tol-text=N]
#   page ∈ home/settings/inventory/mail/shop/character/form/lab
# Artifacts: out/<page>/browser-<page>.json + core-<page>.json (gitignored)
# Exit: diff.mjs's (0=aligned, 1=diffs/unmatched, 2=usage) — 报告产出为主门，exit 1 ≠ 任务失败
set -euo pipefail

PAGE="${1:?usage: run-page.sh <page> [--tol-box=N] [--tol-text=N]}"
shift || true
TOL_BOX=1
TOL_TEXT=3
for a in "$@"; do
  case "$a" in
    --tol-box=*) TOL_BOX="${a#--tol-box=}" ;;
    --tol-text=*) TOL_TEXT="${a#--tol-text=}" ;;
    *) echo "error: unknown arg: $a" >&2; exit 2 ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
OUT_DIR="$SCRIPT_DIR/out/$PAGE"
mkdir -p "$OUT_DIR"

HTML_PATH="$REPO_ROOT/showcase/showcase/$PAGE.html"
if [ ! -f "$HTML_PATH" ]; then
  echo "error: no such page: $HTML_PATH" >&2
  exit 2
fi

echo "==> 1/3 browser rect ($PAGE)"
node "$SCRIPT_DIR/browser-rect.mjs" "$HTML_PATH" "$OUT_DIR/browser-$PAGE.json"

echo "==> 2/3 core dump ($PAGE)"
(cd "$REPO_ROOT" && cargo run -q -p loomgui_core --example dump_page -- "$PAGE" --json "$OUT_DIR/core-$PAGE.json")

echo "==> 3/3 diff (tol-box=$TOL_BOX tol-text=$TOL_TEXT)"
node "$SCRIPT_DIR/diff.mjs" "$OUT_DIR/browser-$PAGE.json" "$OUT_DIR/core-$PAGE.json" --tol-box="$TOL_BOX" --tol-text="$TOL_TEXT"
```

- [ ] **Step 2: chmod +x**

Run: `chmod +x showcase/scripts/rect-diff/run-page.sh`

- [ ] **Step 3: `.gitignore` 加 out/**

`showcase/scripts/rect-diff/.gitignore` 改为：

```
node_modules/
out/
```

- [ ] **Step 4: 语法验证**

Run: `bash -n showcase/scripts/rect-diff/run-page.sh`
Expected: 无输出（语法 OK）；`ls -l showcase/scripts/rect-diff/run-page.sh` 确认可执行位。

- [ ] **Step 5: Commit**

```bash
git add showcase/scripts/rect-diff/run-page.sh showcase/scripts/rect-diff/.gitignore
git commit -m "feat(rect-diff): run-page.sh — one-page browser→core→diff runner"
```

---

### Task 5: settings 主门 E2E 跑通

**Files:**
- Run: `showcase/scripts/rect-diff/run-page.sh settings`（Task 4 产出）
- Create: `showcase/scripts/rect-diff/out/settings/report.txt`（diff 结果存档，gitignored）

**Interfaces:**
- Consumes: `run-page.sh settings`。
- Produces: settings 一页的 diff 结果（供 Task 6 报告填写）。

- [ ] **Step 1: 跑 runner**

Run: `cd showcase/scripts/rect-diff && ./run-page.sh settings > out/settings/report.txt 2>&1; echo "exit=$?"`
Expected: 三步全走完。① browser-rect 打印 `wrote N elements -> out/settings/browser-settings.json`；② dump_page 打印诊断表 + `wrote M DFS nodes -> out/settings/core-settings.json`；③ diff.mjs 打印 `summary: X rect diffs, Y unmatched, Z idless-unpaired`。`exit=` 后的值即 diff.mjs 退出码（0=对齐 1=有 diff）——**用重定向而非 `| tee` 管道**，否则 diff.mjs 的退出码被 tee 吞掉，门判丢失。

- [ ] **Step 2: 记录结果**

读取 `out/settings/report.txt` 尾部 summary，记录 X/Y/Z 三数。若 X>0，读取 diff 明细行（`--- DIFFS ...` 段），按幅度分类：
- **文本类**（tag 为 `#text`/`span`，幅度在 tol-text=3 内）→ 预期漂移，记入报告「已知容差」。
- **结构性 box diff** → 逐条看根因：
  - 若控件 rect 出现**系统性**偏移（如所有 input 同时偏同一方向），先对照 spec 风险预案：查是否 `browser-rect` 注入的 reset.css 覆盖了 UA 默认盒模型（input/checkbox/radio 默认尺寸差异）造成的假 diff，排除后再判真 bug。
  - 一眼根因 + 改动小（fence/packager 一行级）→ **顺手修**（决策 D2-A），修完重跑确认该条消失。
  - 其余 → 记入 Task 6 报告 triage 表「留任务 4」。

- [ ] **Step 3: 若顺手修了代码，跑相关测试**

Run: `cargo test -p loomgui_fence && cargo test -p loomgui_core && cargo test -p loomgui_pkg`（按改动 crate 取子集）
Expected: 全绿。**改 parse-time 逻辑必须重打 pkg**（AGENTS.md：`cargo run -p loomgui_pkg -- build showcase`）并重跑 runner 验证该 diff 消失。

- [ ] **Step 4: 无 commit**（代码改动若有，随 Task 6 报告一起 commit；本任务只留产物）

---

### Task 6: 报告入库 + commit

**Files:**
- Create: `showcase/scripts/rect-diff/snapshot-2026-08-12-settings.md`（报告，入库）
- Modify: 若有 Task 5 的顺手修代码，一并 commit。

**Interfaces:**
- Consumes: Task 5 的 diff 结果（X/Y/Z + 明细 + triage 决定）。

- [ ] **Step 1: 写报告**

创建 `showcase/scripts/rect-diff/snapshot-2026-08-12-settings.md`，模板如下（X/Y/Z、明细、triage 填实际值）：

```markdown
# rect-diff 报告 — settings（2026-08-12）

- 命令：`run-page.sh settings`（tol-box=1, tol-text=3）
- 页：`showcase/showcase/settings.html` ↔ `dump_page --json`（showcase.pkg.bin）
- 文本模型（任务 1）已落地，此报告在其后首次全链路 rect-diff。

## 结果

- box diff（结构 rect 超 tol-box=1）：X 条
- unmatched（单侧 id）：Y 条
- idless-unpaired（信息性，core 隐含 root/wrapper 预期出现）：Z 条

## Diff 明细（前 N 条，按幅度）

| # | tag | id/class | 位置 | 浏览器 | core | 判类 |
|---|---|---|---|---|---|---|
| 1 | ... | ... | (x,y) | w×h | w×h | 文本漂移 / 真 bug |

## 结论

- 门：报告产出 ✅
- settings 对齐：绿（无 box diff）/ 黄（少量 box diff）/ 红（结构性 diff 多）
- 文本漂移在 tol-text=3 内：预期（core ab_glyph vs Chromium HarfBuzz/DirectWrite，spec4b 先例同）

## Triage

- 顺手修（本轮）：...
- 留任务 4（逐页修 bug 队列）：...
- 发现项：`spec4b_dump.rs` 指向已清 pkg（`spec4b-acceptance.pkg.bin` 不在 Bundles），当前不可运行，仅编译——待决定保留/删除
```

- [ ] **Step 2: 按需更新 roadmap 进度**

若 settings 全绿或显著改进，在 `docs/roadmap/roadmap.md` 近期任务 2 处把「下一件事 = 任务 2」勾掉/注明 done；若仍有结构性 diff，roadmap 不动（任务 4 承接）。

- [ ] **Step 3: Commit**

```bash
git add showcase/scripts/rect-diff/snapshot-2026-08-12-settings.md
# 若有 Task 5 顺手修：git add <改的文件>（如 crates/fence/... crates/packer/...）
git commit -m "chore(rect-diff): settings rect-diff report — <绿/黄/红结论> + triage"
```

---

## 验收门汇总

- **Task 1**：21 kind 映射单测绿（`kind_to_html_tag` 配对语义锁死）。
- **Task 2**：spec4b_dump 编译过，core 测试全绿（迁移零行为变化）。
- **Task 3**：`dump_page settings --json` 输出形状与 browser-rect 对齐（node -e 校验）。
- **Task 4**：`bash -n` 过 + 可执行位。
- **Task 5+6**：`run-page.sh settings` 全链路跑通 + 报告 md 入库（门：报告产出）。
