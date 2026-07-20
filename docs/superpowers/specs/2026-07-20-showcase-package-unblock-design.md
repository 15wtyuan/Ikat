# showcase 整体打包解锁 + Playwright 布局回归设施

> 2026-07-20。摸黑结束（Spec-4b）后 §4 加宽的前置：让 showcase 8 页整体打包跑通（标准 B），并建立 Playwright 布局 rect diff 回归设施（§4 全回归复用）。
>
> 本 spec 是 grill 驱动的设计——共识逐条锁定，非拍脑袋清单。设计过程见会话 grill 记录。

## 1. 背景与动机

摸黑结束（Spec-4b 终点线2 Unity 验收逻辑 4 门全绿、视觉 4/5，image-bg 修完即 5/5，另一会话在搞）。roadmap §4 三束加宽的需求来源是「由 showcase 页面逼出需求，不凭空补理论清单」。但 showcase 8 页整体打包当前挂（围栏违规 + packer bug）。不修它，三束加宽没有真需求来源，且后续每束没有可逐页断言布局的基线。

本 spec = 修 showcase 整体打包挂 + 建立 rect diff 设施（让 showcase 8 页成为可逐页断言布局的回归基线）。

## 2. 目标 & 范围

**主线**：showcase 8 页整体打包跑通（**标准 B**：打包过 + Unity 实例化 + 布局 rect 对齐浏览器；视觉特效占位/defer）。

**顺带建**：Playwright 布局 rect diff 设施（§4 全回归复用的长期资产）。

**非目标**（明确划出去）：
- 8 页全量像素对齐 / 全量 rect diff 全绿（= §4 所有束做完后的最终验收，非本 spec）。
- 视觉束（渐变/shadow/filter/animation runtime/transform）。
- 控件束（input 控件行为/TabList）。
- 复合束（ListView/富文本模型）。
- nth-child / state-attr selector（defer，§9）。

## 3. 「整体打包挂」精确阻塞清单（三方核实）

fence selector 解析器（`crates/fence/src/css_rules.rs:5` 注释 + `:504-508` 测试）当前子集 = class / tag / id / 后代空格 / 4 伪类（hover/active/disabled/focus）。

| # | 阻塞项 | 命中位置 | 处置 |
|---|---|---|---|
| 1 | 顶层逗号 selector list `a, b {}` | form:17,18,22,25 / settings:25,30 | 扩 fence |
| 2 | 属性 selector `[type="text"]`（精确匹配，拆 NodeKind 变体，§4.3） | form:17,18,22 / settings:25,30 | 扩 fence + 拆 NodeKind（碰 core，重编 .dll） |
| 3 | `resize: none` CSS prop | form:19 | CssPropSpec noop |
| 4 | src-key packer 路径 bug（`../res/icons/x` vs `res/icons/x` 前缀不匹配） | 全 showcase 通用图标 | packer 归一化 |
| — | `:nth-child(N)` | home:38-41 | **defer**（§9） |
| — | `[aria-selected="true"]` state-attr | settings:15 | **defer**（§9） |

打包期不报错、runtime 不生效（不算「挂」，已 defer）：`@keyframes`/`animation`（4 页）、`transition`（空壳）、`filter`/`transform`/`background-position`（视觉束占位）。

**顺手 doc 漂移**：fence.md §311 说 selector 子集含「子代 `>`」，但 `css_rules.rs:506` 测试断言 `.a > .b` 返 None（本轮不做）→ 改成「后代空格」。

## 4. 关键决策（grill 锁定）

### 4.1 标准 B
打包过 + Unity 实例化 + 布局 rect 对齐浏览器（视觉占位）。契合 §3.4 护城河（布局可预测，非像素）。两台机分工：编码机验打包 + core dump rect 比对；家里机验 Unity rect。

### 4.2 showcase 定位 = 浏览器对标基线，不可改写非标准
showcase 是 §3.4 护城河验收的「浏览器侧」，必须是浏览器原样跑的标准 HTML。遇 fence 不足 → 扩 fence，不改 showcase 成非标准写法（如 `input[type]` → `input.text`）。但「特性 defer」（控件/动画未做）≠ 改写非标准——showcase 保留标准写法、注释 + TODO 标未实现。

### 4.3 属性 selector = 精确匹配（拆 NodeKind 变体）

查证挖到匹配层 gap：Node 不存 type 字面量（`node.rs:219` 只有 `kind`），且 `NodeKind::TextField` 合并 text/password/search（main-design.md:121 / fence.md:108 / public-api.md:296 三方确认）→ `[type="text"]` 与 `[type="password"]` 无法精确区分。grill 选「精确，不近似」——C 近似（`[type="text"]` 误套 password/search）被否，违背「标准 CSS 语义可预测」核心目的，「不能偷懒」。

实现 = **拆 NodeKind 变体**（非「Node 加 input_type 字段」——那让 type 字面量冗余存 Node，且违背「标准 HTML 语义决定类型」原则；[[design-over-effort]]「改动大不是反对理由」支持拆变体这条更干净的路径）：
- `NodeKind::TextField` 拆成 `TextField`(text) / `PasswordField`(password) / `SearchField`(search) 三变体。
- `resolve_semantic`（`tag.rs:115-116`）拆：`text`→TextField / `password`→PasswordField / `search`→SearchField。
- bridge（`bridge.rs:106`）拆 SemanticKind→NodeKind 映射（SemanticKind::TextField 对应拆，或保持合并靠 input_type 区分——实现时按 bridge 已有信息定）。
- 属性 selector `[type="x"]` 匹配查 NodeKind 精确对应（`[type="password"]` 只匹配 PasswordField）。

**顺带收益**：控件束做 password 遮罩 / search 清除按钮时，独立 NodeKind 已就位。

**代价**：改 public-api 终态契约（TextField 拆 3）+ main-design.md/fence.md 文档 + NodeKind/resolve_semantic/bridge/NodeFactory 全链 + pkg v19→v20（kind_tag 判别值加 2）+ 重编 .dll。TextField 控件束都是壳，拆只改契约文档 + 壳 dispatch，**不破坏已实现功能**。

**state-attr**（`aria-selected`，运行时可变 + tab 控件未做）→ defer 到控件束 TabList（§9）。

子集：`[attr="value"]` 等值（含不带引号 `[type=text]`）+ `[attr]` 存在；不做 `~=`/`^=`/`$=`/`*=`（YAGNI，showcase 未用）。本轮属性 selector 匹配只支持 type（拆出的变体），其他 attr name 的属性 selector fence 解析但 core 不匹配（规则不生效，类似 transition noop）。

### 4.4 nth-child 改 defer（推翻初版「做」的结论）
查证挖到：nth-child 要 `Compound` 加 `nth_child: Option<u32>` 字段 → DynamicRule 进 pkg → bincode 形状变 → **pkg v20→v21（§4.3 拆 NodeKind 变体已占 v20）+ 碰 core + 重编 .dll**。且 showcase 唯一用法配 animation-delay（runtime 本就 defer），做了也无真用例锻炼（假绿）。代价大、收益弱 → defer。home 7 条注释 + TODO。**pkg v21 与 keyframes runtime 驱动合并一次 bump**（§9）。

### 4.5 验收用 Playwright + A1 reset + B2 分离
node 已在项目工具链（tauri-cli 走 npm），Playwright 是验收期 node 工具、不进 Cargo（不违背 Rust 零新依赖）。
- **B2 设施三件分离**：浏览器 rect 导出器 / core rect 导出器 / diff 工具。
- **A1 基准对齐**：注入最小 reset（`body{margin:0;padding:0}` + 清浏览器默认 block margin）让浏览器侧模拟「LoomGUI 无 UA 默认」，绝对坐标直接可比。

### 4.6 box-sizing 已对齐（查证坐实，非问题）
LoomGUI 借 taffy 0.5 默认 `BoxSizing::BorderBox`，`layout_rect` 是 border box 口径（`scroll.rs:74`/`:643`），spec4b 实证 card-1 w≈300（width:300+padding:16 border-box 下 = 300）。preview-base.css 全局 `box-sizing:border-box`。两边已对齐，无需实现。main-design.md:91「待实现」指 fence CSS prop 表补 box-sizing prop 让可覆盖，非 layout 改动。

### 4.7 验收门收窄（8 页全绿不可达）
8 页多数用未做特性（input 控件/ListView/富文本/filter/animation runtime）。全量 rect diff 全绿 = §4 最终验收，非本 spec。本 spec 验收门：**打包门（硬）+ 设施门（硬，spec4b 单页）+ 8 页 rect diff 快照（软，特性 gap 仪表盘）**。

### 4.8 spec 粒度 = 合并（原 2 phase 合一）
box-sizing 发现后 Phase 1 基准对齐块消失；属性 selector 拆 NodeKind 变体（§4.3）碰 core + 重编 .dll，但 nth-child defer。设施体量不值得独立 phase 门 → 合并为 task 序列，设施自验作 task 门。

## 5. 设计：task 序列

### ① 搭 Playwright rect diff 设施（门：spec4b 单页 rect diff 绿）

**B2 三件分离**：

- **浏览器 rect 导出器**（node）：Playwright 加载 showcase HTML（注入 A1 reset）→ `querySelectorAll('[data-loom-id], *')` → `getBoundingClientRect()` → JSON `{id, x, y, w, h}`。节点标识：用 showcase 元素的 `id`/`class`/路径定位（具体 scheme 实现时定，需与 core rect 导出器同 scheme 才能比对）。
- **core rect 导出器**（Rust）：扩 `spec4b_dump` example，dump 全节点 `layout_rect`（border box 口径）→ 同结构 JSON。
- **diff 工具**（node，纯数据比对）：两 JSON 按 scheme 配对逐元素比，盒模型节点 ±1px，文本节点宽容（字体度量差异不可消除，具体容差实现时定），超容差报告差异元素 + 期望/实际。

**A1 reset**：`body{margin:0;padding:0}` + 清浏览器默认 block 元素 margin（h1-h6/ul/ol/p 等），让浏览器侧 ≈ LoomGUI 无 UA 默认环境，绝对坐标可比。

**设施自验门**：spec4b-acceptance 单页（已打包、布局简单、no-UA reset 已对齐基准）跑通 rect diff 绿，证明设施 + A1 reset 工作。

### ② fence 扩围（门：fence 单测全绿 + `cargo test -p loomgui_fence`）

**逗号 list**（`css_rules.rs:226-245`）：prelude 按 `,` split，每段 trim 后独立 `parse_selector`，产 N 条 `DynamicRule` 共享同一声明块。套路同 `:316-321` keyframes 逗号 stop。

**属性 selector**（拆 NodeKind 变体 + fence 解析 + 匹配，见 §4.3）：
- **拆 NodeKind 变体**（core）：`NodeKind::TextField` → `TextField`/`PasswordField`/`SearchField`；`resolve_semantic`（tag.rs:115）拆 text/password/search 映射；bridge（bridge.rs:106）拆 SemanticKind→NodeKind；NodeFactory（C# 投影）加 PasswordField/SearchField dispatch；public-api.md/main-design.md/fence.md 终态契约改 TextField 拆 3。**pkg v19→v20**（kind_tag 判别值加 2）+ 重编 .dll。
- **fence 解析**（`css_rules.rs`）：`parse_selector`（`:52`）去掉 `raw.contains('[')` 越界判定；`parse_compound`（`:88`）加 `[attr]` / `[attr="val"]` / `[attr=val]` 解析填 `AttrSelector{name, op, value}`（struct 就绪 `dynamic.rs:64`），name 小写归一。
- **匹配**（`dynamic.rs:233`）：`compound_matches_node` 消费 `c.attrs`——`[type="x"]` 查 NodeKind 精确对应（text→TextField, password→PasswordField, search→SearchField, number→NumberField, range→Slider, checkbox→Toggle, radio→RadioButton）。当前 `:258-260` 对非空 attrs 直接返 false，改成 type attr 查 NodeKind。其他 attr name 不匹配（规则不生效）。
- specificity：属性 selector = class 级（0,0,1,0），已在 `specificity_b` 计数（`:62`）。
- 子集：`AttrOp::Exists` + `AttrOp::Eq`，不做 `~=` 等高阶。

**resize:none**（`fence/schema/css.rs`）：CssPropSpec 表加 `resize`（值域 none/both/horizontal/vertical），parser 接受，core `apply_decl` 不消费（noop，同 `transition` 空壳先例）。

**修 fence.md §311 doc 漂移**：「class/tag/id/后代/子代/伪类」→「class/tag/id/后代空格/伪类」。

**fence 单测**：逗号 list 展开（N 条规则共享声明块）、属性 selector 解析 + specificity + 匹配（structural attr 命中/不命中）、resize 接受。

### ③ packer src-key 路径归一化（门：build.rs 测试含 `../` 归一化用例绿）

落点：`build.rs:55` `referenced_sprites` 收集点。设计意图（`workspace-CLAUDE.md:73`）：`<img src="../../assets/icons/x.png">` resolve 为 sprite_key `assets/icons/x.png`（相对工作区根，去 `../`）。

当前 `bridge.rs:47` `attr(el,"src")` 拿原始 src（不知 HTML 路径上下文）。修法：把当前 HTML 相对 workspace_root 路径（`resolve_html_list` `:78` 已算）传入 src 提取处，把 src（相对 HTML）resolve 成相对 workspace_root。

实现时确认具体落点（bridge 提取时归一化 vs build.rs 收集后按组件归一化）——两者皆可，取改动小、不丢「src 来自哪个 HTML」信息那条。

### ④ showcase 调整 + roadmap tech-debt（无门，文档/注释）

- `showcase/showcase/home.html:38-41` 的 7 条 `.nav-card:nth-child(N){animation-delay:...}` 注释 + TODO 指回 roadmap §4 tech-debt（nth-child 条）。
- `showcase/showcase/settings.html:15` 的 `.tab[aria-selected="true"]` 注释 + TODO 指回 roadmap §4 tech-debt（aria-selected 条）。
- roadmap §4 tech-debt 段加两条（§9 草稿）。

### ⑤ 8 页 rect diff 验收

- **编码机**：8 页全部打包过（打包门）+ spec4b 单页 rect diff 绿（设施门）+ 8 页 rect diff 快照报告（软，记录特性 gap：哪些页/元素一致、哪些因未做束不一致）。
- **家里机**：Unity 实例化 8 页 + Unity rect 与 core rect 快照比对。特性 gap 同 §4.7——已支持特性的页/元素 rect 一致为绿，未做束的页标 gap（软门，不是全绿门）。这是标准 B 的 Unity 半。

## 6. 错误处理 / 边界

- fence 扩围后**越界仍报错不静默**：逗号 list / 属性 selector 进子集，但 `~=`/`^=`/`$=`/`*=`/`an+b`/`+`/`~`/`>`/`:nth-child`/state-attr selector 仍返 None → `FenceBadCssValue` 诊断。
- packer src-key 归一化失败（路径无法 resolve）→ 报错不静默。
- Playwright diff 超容差 → 报告差异元素 + 期望/实际值，不静默通过。
- defer 项（nth-child/aria-selected/animation runtime/transition/视觉特效）：fence 接受语法 / runtime 不跑，showcase 注释 + TODO 指回 roadmap。
- 节点标识 scheme（浏览器侧 ↔ core 侧配对）若不一致 → diff 报告「无法配对」，不静默跳过。

## 7. 测试策略

- **fence 单测**（task ②）：逗号 list 展开 / 属性 selector 解析+specificity+匹配 / resize 接受。
- **设施自验**（task ①）：spec4b 单页 rect diff 绿（设施 + A1 reset 工作）。
- **8 页快照**（task ⑤）：Playwright 全量 diff 报告（绿不是门，是基线快照 + 特性 gap 仪表盘）。
- **防漂移门**：`cargo test -p loomgui_fence`（改 fence 后必跑）。
- **packer 门**：`build.rs` 测试加 `../` 归一化用例（照 `:59` 现有 sprite_key 测试扩）。

## 8. 工作流 / 避让 image-bg

- 本轮 fence 扩围（属性 selector 拆 NodeKind 变体，§4.3）+ packer src-key **碰 core、重编 .dll**（拆变体触发 pkg v20 + NodeKind 全链）。
- 两边碰 core：本 spec 改 NodeKind/resolve_semantic/bridge（属性 selector），image-bg 改 render（合成层）。**不同文件，merge 时无直接冲突**，但都要重编 .dll + 重出 GUI exe。
- **.dll + GUI exe**：worktree 隔离，**worktree 内不 commit 二进制**，merge 到 main 后统一重编 .dll + 重出 GUI exe 一次。
- 改 fence/packer/core 后重出 GUI exe（静态链入 exe，坑 158 同源 stale exe 链）。
- pkg 版本号 v19→v20 是本 spec 占用；image-bg 若也碰 pkg 格式需协调版本号（image-bg 是 render 改动，预期不碰 pkg 格式，但 merge 时确认）。

## 9. roadmap §4 tech-debt 两条草稿

> 写法：症状 / 根因 / 处置路标。

**1. `:nth-child(N)` 选择器（home 7 条注释 defer）**
- 症状：fence 选择器子集不支持 `:nth-child(N)`。showcase home 7 条 `.nav-card:nth-child(N){animation-delay:...}` 注释 defer。
- 根因：nth-child 要 `Compound` 加 `nth_child: Option<u32>` 字段 → DynamicRule 进 pkg → bincode 形状变 → pkg v20→v21（§4.3 拆 NodeKind 变体已占 v20）+ 碰 core + 重编 .dll；且 showcase 唯一用法配 animation-delay（runtime 本就 defer），做了也无真用例锻炼（假绿）。
- 处置路标：`Compound` 加字段 + pkg v21 + 匹配拿 child index（打包期 base cascade + 运行时 rematch 都算兄弟序号）。激活时机 = 有真视觉用例（控件束斑马纹 `:nth-child(odd)` / §4 animation runtime 落地让 home 错峰可验）。**pkg v21 与 keyframes runtime 驱动合并一次 bump**（两者都碰序列化形状，避免两次 bump）。

**2. `[aria-selected]` state-attr selector（settings 1 条注释 defer）**
- 症状：fence 不支持 state-attr selector。showcase settings 1 条 `.tab[aria-selected="true"]` 注释 defer。
- 根因：aria-selected 运行时可变 → 要「content attr 变化触发 rematch」机制 + tab 控件（TabList）本身未做（控件束）。
- 处置路标：跟控件束 WAI-ARIA TabList 一起做；届时定选中态表达机制（伪类 `:selected` vs state-attr selector），showcase 那条激活。

两条都在 showcase 对应位置（home:38-41 / settings:15）留 TODO 指回 roadmap 此处。

## 10. 风险与未决（实现时确认）

- **节点标识 scheme**：浏览器侧（Playwright 量的元素）与 core 侧（spec4b_dump 量的节点）配对方案未定。showcase 元素有 id/class，core 节点有 NodeId + class。scheme 要两边稳定映射。实现时定（候选：按 class/id 配对，或按 DOM 路径序列）。
- **input 控件布局 rect 可达性**：form/settings 大量 input 控件（控件束未做行为）。layout 是否给 NodeKind::TextField 等正确 size（rect 可比）还是塌成空 leaf（rect 不可比）——设施跑起来实测确认。若塌空，form/settings rect diff 归控件束，本 spec 不强求。
- **packer src 归一化落点**（bridge vs build）：实现时取改动小那条（§5 ③）。
- **文本节点容差具体值**：实现时按字体度量差异实测定（候选 ±2-4px 或按行高百分比）。
