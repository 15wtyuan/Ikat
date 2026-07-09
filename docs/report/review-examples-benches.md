# 诊断工具 & 基准测试代码审查报告

> 审查范围：`loomgui_core/examples/` 11 个诊断工具 + `loomgui_core/benches/frame_emit.rs` 1 个基准测试  
> 审查日期：2026-07-09

---

## 1. 总览

| 文件 | 行数 | 硬编码路径 | panic/unwrap | 重复代码 |
|------|------|-----------|-------------|---------|
| `dump_text.rs` | 240 | pkg.bin 硬编码 | 3 处 unwrap | Stage 初始化 + 字体注册 |
| `dump_showcase_text.rs` | 173 | HTML/CSS 硬编码 | 多处 expect | HTML+CSS 管线 |
| `dump_scroll.rs` | 45 | pkg.bin 硬编码 | 3 处 unwrap | Stage 初始化 |
| `dump_rich.rs` | 67 | 无（用 fixture） | 3 处 unwrap | 无 |
| `dump_render.rs` | 111 | HTML/CSS 硬编码 | 多处 expect | HTML+CSS 管线 + payload_str |
| `dump_nativehost_slot.rs` | 151 | CLI 参数 | 1 处 panic | 中等（Stage + root 实例化） |
| `dump_interact.rs` | 191 | 无（inline HTML） | 多处 unwrap | payload dump 逻辑 |
| `dump_img.rs` | 57 | pkg.bin 硬编码 | 2 处 unwrap | Stage 初始化 |
| `dump_controller.rs` | 190 | CLI 参数 | 1 处 panic | Stage + root 实例化 |
| `dump_bg.rs` | 93 | pkg.bin 硬编码 | 2 处 expect/unwrap | Stage 初始化 + payload dump |
| `verify_showcase_pkg.rs` | 41 | CLI 参数 | 1 处 expect | 无 |
| `frame_emit.rs` | 135 | fixture 字体 | 多处 unwrap | HTML+CSS 管线重复 |

---

## 2. 硬编码路径问题

### 2.1 pkg.bin 硬编码（严重）

以下 4 个工具使用 `concat!(env!("CARGO_MANIFEST_DIR"), ...)` 硬编码指向 showcase.pkg.bin：

| 文件 | 行号 |
|------|------|
| `dump_text.rs` | 143–146 |
| `dump_scroll.rs` | 7–10 |
| `dump_img.rs` | 14–17 |
| `dump_bg.rs` | 10–12 |

**问题：** 路径指向仓库相对位置 `../loomgui_unity/Assets/StreamingAssets/showcase.pkg.bin`，依赖 repo 目录结构。若 pkg 不在预期位置，工具静默跳过（`eprintln + return`）或被 panic。

**对比：** `dump_nativehost_slot.rs`、`dump_controller.rs`、`verify_showcase_pkg.rs` 使用 `env::args().nth(1)` 接受命令行参数并带默认值——更好的实践。

**修复方向：** 统一改用命令行参数模式，带合理的默认路径 fallback。

### 2.2 HTML/CSS 路径硬编码（中等）

| 文件 | 行号 | 硬编码路径 |
|------|------|-----------|
| `dump_showcase_text.rs` | 20, 24 | `../loomgui_unity/Assets/LoomUI/showcase/page_text.html`、`preview-base.css` |
| `dump_render.rs` | 16–23 | `../samples/v1-showcase/index.html`、`style.css` |

这些路径依赖 LoomUI 工作区和 samples 目录结构，在 CI 或不同 checkout 可能不存在。

---

## 3. Panic / Unwrap 风险

### 3.1 所有诊断工具都有 unwrap/expect（中等）

无一例外。典型模式：

```rust
// dump_scroll.rs:32
let scene = s.scene.as_ref().unwrap();

// dump_text.rs:161
s.register_font("wqy-microhei", std::fs::read(font_path).unwrap(), true).unwrap();

// dump_nativehost_slot.rs:35
std::fs::read(&pkg_path).unwrap_or_else(|e| panic!("read pkg.bin ({pkg_path}): {e}"));
```

**分析：**
- 诊断工具不是生产代码，`unwrap()` 本身可接受——它提供清晰的 panic 现场。
- 但 `dump_text.rs` 和 `dump_scroll.rs` 对**可能不存在的 pkg 文件**做了优雅降级（`eprintln + return`），风格不一致。
- `dump_nativehost_slot.rs:35` 和 `dump_controller.rs:16` 用 `unwrap_or_else(|e| panic!(...))`，等价于 `expect()` 但更啰嗦。

**修复方向：** 不要求消除所有 unwrap——诊断工具炸掉时给出明确 panic 信息是合理的。只需统一风格：要么全 `expect("含义清晰的信息")`，要么全 `unwrap()`。`unwrap_or_else(|e| panic!(...))` 直接改成 `expect(format!(...))`。

### 3.2 CLI 参数缺少默认值时的 panic（中低）

```rust
// verify_showcase_pkg.rs:9
let path = env::args().nth(1).expect("usage: verify_showcase_pkg <pkg.bin>");
```

无参数直接 panic，给用户提示。可接受，但不如 `dump_nativehost_slot` 的 `unwrap_or_else(default)` 友好。

---

## 4. 代码重复分析

### 4.1 Stage + 字体 + 加载 pkg 管线（严重重复）

以下 5 个工具包含几乎相同的 15–25 行 Stage 初始化代码：

`dump_text.rs` (lines 138–174)、`dump_scroll.rs` (lines 11–32)、`dump_img.rs` (lines 25–38)、`dump_bg.rs` (lines 13–17)、`dump_controller.rs` (lines 24–37)

重复模式：
```rust
let font = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/DejaVuSans.ttf");
let mut s = Stage::new((1080.0, 1920.0)).expect("Stage::new");
s.register_font("DejaVu", std::fs::read(font).unwrap(), true).unwrap();
// ... load_package or build_scene ...
s.tick_and_render();
```

`dump_nativehost_slot.rs` 和 `dump_controller.rs` 还额外重复了 `create_root` + `instantiate` + `append_child` 模式（lines 54–63, lines 28–37）。

### 4.2 HTML+CSS 解析管线（严重重复）

以下 3 处重复 parse → resolve → build_scene 管线：

`dump_showcase_text.rs` (lines 44–49)、`dump_render.rs` (lines 30–33)、`frame_emit.rs` (lines 23–34，封装为 `load_html_css`)

`frame_emit.rs` 中的 `load_html_css` 是对这个管线的封装，但诊断工具并未复用它。

### 4.3 payload 格式化/打印逻辑（中等重复）

`payload_str` 函数在 `dump_render.rs:86-110` 实现，`dump_interact.rs:52-73` 内联了几乎相同的逻辑（连变量名和 `c1/c2/c3` 辅助函数都一样），`dump_bg.rs:54-91` 也有 90% 重合的 payload dump。

### 4.4 `dump_interact.rs` 的 c1/c2/c3 辅助函数（轻微古怪）

```rust
// dump_interact.rs:90-98
fn c1(c: [f32; 4]) -> f32 { c[1] * 255.0 }
fn c2(c: [f32; 4]) -> f32 { c[2] * 255.0 }
fn c3(c: [f32; 4]) -> f32 { c[3] }
```

函数名 `c1/c2/c3` 含义不明（看起来像 "component 1/2/3"，但 `c1` 实际返回的是 Green 通道 `c[1]`）。这像是某次 patch 将内联的 `c0[1] * 255.0` 提取成函数时出的命名问题。且 `dump_render.rs` 中同类功能是直接内联的——两者不一致。

### 4.5 是否值得抽象成共享 dump 库？

**当前判断：不值得。**

理由：
1. 每个诊断工具服务于特定的回归验证场景（文本换行、滚动 overlap、交互伪类、controller display 继承等），彼此不共享业务逻辑。
2. 共享库会引入"诊断工具之间的耦合"——改一个工具可能不小心影响另一个的诊断语义。
3. 按 ponytail 原则，11 个诊断文件总共约 1500 行，每个文件重复 ~20 行 boilerplate 是可接受的冗余。

**但可以做一件很小的事：** 统一"加载 pkg/Stage/font"的模式（如提供一个 `fn setup_with_pkg(pkg_path: &str) -> Stage` helper），不触及业务诊断逻辑。改动量约 30 行，消灭 5 个文件的重复。

或者更懒的方式：在每个文件头部加一行注释说明可跑的 CLI 命令，避免用户在错误目录执行。

---

## 5. 逐个工具评注

### 5.1 `dump_text.rs` — 质量：良好，但有结构问题

- **行 11–134（GlyphAtlas 验证）** 和 **行 138–240（文本 dump）** 是两个独立功能强行塞进同一个 `main()`。若字体文件不在 pkg 路径导致 pkg 加载失败，前面 GlyphAtlas 验证的输出仍然有效——但用户需要手动过滤输出。
- 行 30：`std::fs::read(font_path).unwrap()` — 同一个字体文件被读了两次（行 30 用于 Font::from_path，行 161 用于 FontTable::register 的 bytes），冗余。
- 行 172：注释声明 `"load_package only stores resources; example would need create_root/instantiate..."` — 说明 `scene` 可能为 None 导致跳过 dump。但实际行为依赖于 Stage 内部实现细节（load_package 是否自动建 scene）。若某次重构改变此行为，本工具静默跳过，用户不会察觉。

### 5.2 `dump_showcase_text.rs` — 质量：良好

- 是最好的诊断工具之一：注释清楚说明了诊断目的和验证维度。
- 不依赖 pkg.bin，直接走 HTML+CSS 全管线——可独立运行。
- `extract_style` 函数（行 166–172）是一个简单但脆弱的 `<style>` 标签提取器：不处理多个 `<style>` 块、不处理属性（如 `type="text/css"`）。当前 showcase 只有一个 `<style>`，所以够用。

### 5.3 `dump_scroll.rs` — 质量：简洁有效

- 45 行，最简洁的工具，功能单一明确。
- 行 32：`scene.as_ref().unwrap()` 在 load_package 不建 scene 时会炸——但当前实现是建的，暂时安全。

### 5.4 `dump_rich.rs` — 质量：良好，有内存泄漏

- 行 18：`Box::leak(bytes.into_boxed_slice())` — 故意泄漏内存以获取 `'static` 生命周期给 `FontStack::single`。对于一次性诊断工具可接受，但值得注释说明原因。
- 唯一带 `assert!` 自我验证的诊断工具——其他工具只打印输出，不自动判断 PASS/FAIL。

### 5.5 `dump_render.rs` — 质量：中等

- 行 7：注释提到 "v1.4-a T4：load_inline 已砍"，但本工具直接走 parse_html + build_scene 绕过了这个问题。
- 硬编码 `../samples/v1-showcase/` 路径——此目录可能已过时（当前 showcase 在 `loomgui_unity/Assets/LoomUI/showcase/`）。

### 5.6 `dump_nativehost_slot.rs` — 质量：优秀

- 最好的诊断工具：CLI 参数覆盖、清晰的 PASS/FAIL 结论、好的注释解释背景。
- 行 127 注释 "坑 127" — 违反 CLAUDE.md 规定"坑号只属于 docs/pitfalls.md，不进代码"。

### 5.7 `dump_interact.rs` — 质量：良好，但冗长

- 不依赖任何外部文件（HTML/CSS 内联），开箱即跑。
- 行 67–69：`c1(c0), c2(c0), c3(c0)` — 函数抽象过度。且颜色通道打印在 `alpha` 位置（第 4 个值）用的是 `c3(c0)` 返回原始 alpha [0,1]，但前三个通道用了 `*255.0` 转换 [0,255]。这个不一致可能导致用户读输出时困惑。
- 行 87：`let _ = scene;` — 抑制未使用变量警告，说明 `scene` 变量在 `dump_frame` 末尾被计算但未被使用（它被闭包捕获了）。可删。

### 5.8 `dump_img.rs` — 质量：低，疑似过时

- 行 6：注释说 "T5 instantiate 后恢复 scene 转储" — 这个 TODO 未完成。
- 行 39–40：注释说 "T4：scene 未建" 和 "T5 instantiate 后改回 scene 转储"——当前代码遍历 `s.packages` 而非 `s.scene`，无法 dump 实例化后的节点布局信息（如 layout_rect）。
- **这个工具目前功能受限**：只能查看组件模板中的 Image 节点，看不到实例化后的实际布局。

### 5.9 `dump_controller.rs` — 质量：优秀

- 结构好的工具：分 [controllers]、[nodes]、[display:none 子树]、[text color 继承] 四个维度。
- 行 119–157：display:none 子树遍历使用手动栈（DFS），实现正确。
- 行 127 注释 "坑 131-133" — 同上，坑号不应进代码。
- 行 91–101：`keywords` 过滤器数组与 `hover-demo` id 特殊处理——硬编码的过滤列表，若页面上相关 class/id 改了则 dump 不完整。

### 5.10 `dump_bg.rs` — 质量：良好

- 行 21：`want` 数组硬编码关注节点名（bg-demo/cf-demo/ns-demo/br-demo）— 若 showcase 中类名变了则 dump 内容为空。
- 功能覆盖：bg-image/background-size/color_filter/border_image_slice + UV 范围检查——诊断维度完整。

### 5.11 `verify_showcase_pkg.rs` — 质量：好

- 最干净的工具：35 行逻辑，清晰的验证维度（组件列表 + asset manifest + root parent_idx 校验）。

### 5.12 注释/文档覆盖情况

| 方面 | 评估 |
|------|------|
| 模块级 `//!` 文档 | 10/11 有，`verify_showcase_pkg.rs` 只有 1 行 |
| 诊断目的说明 | 大部分有（如 dump_text 清楚说明验随机语义） |
| 跑法说明 | dump_showcase_text、dump_nativehost_slot、dump_controller 有 |
| 期望输出说明 | dump_nativehost_slot 有 PASS/FAIL，其余靠人眼比对 |
| 自动验证 | 只有 dump_rich 有 asserts；dump_text 有 assert；dump_nativehost_slot 有程序化 PASS/FAIL |

---

## 6. Benchmark 审查（`frame_emit.rs`）

### 6.1 Bench 设计

三个 bench 场景：
- `static_frame` — 第 2 帧（全 Unchanged），测 Hash 比对路径
- `cold_frame` — 首帧（全 Dirty），测全量 emit
- `page_turn_frame` — 换页重新加载后首帧（全 Dirty），模拟页面切换

设计合理，覆盖了主要性能场景。

### 6.2 问题

**中：setup 成本混入基准**（行 66–75，91–97，113–123）

`iter_batched` 的 setup 闭包中包含：
- 注册字体（读文件 I/O）
- `html_500()` 字符串生成
- `load_html_css`（parse HTML+CSS + resolve_styles + build_scene）
- 首帧 tick_and_render（建基线，包含 layout/render 全程）

这些操作在 criterion 的 `iter_batched` 中被计入 **迭代时间**（setup + bench 的总时间用于计算吞吐量）。虽然 criterion 会分开计时，但 `BatchSize::SmallInput` 意味着每组迭代数较少（~10 次），setup 波动可能影响结果稳定性。建议用 `PerIteration` 并将重 I/O（字体文件读取）移到 module-level `lazy_static`。

**低：字体文件路径用 `format!`**（行 15–18）

```rust
fn font_path() -> (String, usize) {
    let p = format!("{}/tests/fixtures/DejaVuSans.ttf", env!("CARGO_MANIFEST_DIR"));
    (p.clone(), p.len())
}
```

`p.clone()` 冗余——返回 `(String, usize)` 但调用方只用 `.0`。应直接用 `concat!`。

**低：未使用的 `_fplen`**（行 61、86、108）

```rust
let (fp, _fplen) = font_path();
```

`_fplen` 在所有三个 bench 函数中未使用。可能是调试遗留。

**低：inline CSS 重复 500 次**（行 36–46）

`html_500()` 在 500 个 div 上各写了一套 inline `style="..."`，导致 CSS 解析器需要处理 500 次 style 属性解析，而非 1 个 class selector + 500 次 class 匹配。对于"日常场景"的代表性有限——真实 HTML 会大量使用 class selector。

**低：无 warm-up 声明**

criterion 默认做 warm-up，这里未显式配置——没问题。但如果将来需要更精确的控制，可加 `warm_up_time`。

### 6.3 验收线对照

注释声明 "验收线：冷帧/换页帧 ≤2ms（v1-scope §4）"。当前 bench 只测量时间，不自动断言——需要在 CI 中额外解析 criterion 报告。这符合 bench 常见做法。

---

## 7. Cargo.toml 配置问题

**中：只有 3 个 example 显式声明了 `required-features`**

```toml
[[example]]
name = "dump_showcase_text"
required-features = ["parse"]

[[example]]
name = "dump_render"
required-features = ["parse"]

[[example]]
name = "dump_interact"
required-features = ["parse"]
```

以下同样依赖 `parse` feature 的 example 未声明：`dump_text.rs`（使用 `Stage` 加载 pkg）、`dump_controller.rs`（使用 `asset::read_package`）、`dump_bg.rs`、`dump_nativehost_slot.rs`。

**后果：** 使用 `--no-default-features` 编译时，这些未声明的 example 会编译失败而非被优雅跳过。CLAUDE.md 提到 CI 会跑 `--no-default-features --all-targets`，这可能在 CI 上引发未预期的编译错误。

`dump_rich.rs` 和 `verify_showcase_pkg.rs` 可能不需要 `parse` feature（需验证）。

---

## 8. 汇总建议（按优先级）

| 优先级 | 问题 | 建议 |
|--------|------|------|
| **高** | pkg 路径硬编码（4 工具） | 改为 CLI 参数 + 默认值（参照 dump_nativehost_slot） |
| **高** | `dump_img.rs` 功能过时 | 更新为使用 scene 而非 packages 字典，或标注 DEPRECATED |
| **中** | Cargo.toml `required-features` 缺失 | 为全部依赖 parse 的 example 补充声明 |
| **中** | 重复的 Stage 初始化代码（5 处） | 可提取一个 `fn setup_demo_stage(font_path: &str, pkg_path: &str) -> Stage` （~25 行） |
| **中** | `dump_interact.rs` c1/c2/c3 命名混乱 | 重命名为 `green_255`/`blue_255`/`alpha_raw` 或直接内联 |
| **低** | 坑号进代码注释（2 处） | 改为描述性注释，移坑号到 pitfalls.md |
| **低** | 多处 `.unwrap_or_else(\|e\| panic!(...))` 可简化为 `.expect(...)` | 统一用 expect |
| **低** | bench `font_path()` 返回冗余 `usize` + `format!` | 改用 `concat!` |
| **低** | bench inline CSS 重复 500 次 | 用 class selector + 1 条 CSS 规则更真实 |
| **低** | `dump_rich.rs` Box::leak 无注释 | 加一行注释说明 'static 需求 |
