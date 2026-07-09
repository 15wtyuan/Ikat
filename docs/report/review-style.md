# LoomGUI CSS 样式系统代码审查报告

> 审查范围：`loomgui_core/src/style/` 全模块 + `fence.md` 一致性校验。
> 审查日期：2026-07-09

---

## 1. apply_decl 返回 bool 语义不一致（中）

**精确行号**：`mapping.rs:348-673`

`apply_decl(&mut ResolvedStyle, prop: &str, value: &str) -> bool` 的返回值语义在各 match arm 间不一致，具体分三类：

### A 类：属性名识别 + 值有效 → true；值无效 → false（正确语义）

| 属性 | 代码位置 | 无效值时行为 |
|---|---|---|
| `position` | L626-639 | `fixed`/`sticky` → `false` |
| `background-size` | L520-528 | 非 cover/contain/100% → `false` |
| `border-radius` | L407-430 | 非法值 → `false` |
| `border-image-slice` | L502-511 | 非法值 → `false` |
| `background-image` | L516-519 | 非 url() → `false` |
| `top`/`right`/`bottom`/`left` | L640-666 | `%` 值 → `false` |

### B 类：属性名识别 → 总是 true，值无效时静默不生效

| 属性 | 代码位置 | 无效值时行为 |
|---|---|---|
| `overflow` | L542-549 | `"bogus"` → `true`（值被丢弃，保留旧值） |
| `overflow-x` | L550-556 | 同上 |
| `overflow-y` | L557-562 | 同上 |
| `color` | L563-568 | `parse_color` 返 None → 值不变，仍返回 `true` |
| `font-size` | L569-572 | `parse_px` 返 None → 值不变，仍返回 `true` |
| `aspect-ratio` | L605-610 | 解析失败 → 值不变，仍返回 `true` |
| `gap` | L431-438 | 任意值 → 总是 `true`（`parse_four` 静默回退 0） |
| `flex-direction` | L439-447 | 任意值 → 总是 `true`（非白名单值 fallback Column） |
| `flex-wrap` | L448-454 | 任意值 → 总是 `true`（非 `wrap` fallback NoWrap） |
| `justify-content` | L455-458 | 任意值 → 总是 `true`（非白名单值 fallback FlexStart） |
| `align-items` | L459-462 | 同上 |
| `align-self` | L463-466 | 同上 |
| `text-align` | L581-588 | 任意值 → 总是 `true`（非 center/right fallback Left） |
| `white-space` | L601-604 | 任意值 → 总是 `true` |
| `display` | L479-496 | 任意值 → 总是 `true`（如 `grid` → Flex） |
| `filter` | L497-501 | 任意值 → 总是 `true`（`"none"` 清除，无效函数跳过） |
| `transform` | L622-625 | 任意值 → 总是 `true`（`"skew(10deg)"` 不生效但返 true，见 `fence_contract.rs:265-277`） |
| `transition` | L667-670 | 任意值 → 总是 `true`（空值 → 空 Vec） |

### 问题分析

调用方 `cascade.rs:45` 和 `dynamic.rs:345` 均忽略返回值（直接用 `apply_decl` 无分支），所以两类行为当前不影响功能。但测试 `fence_contract.rs` 宣称"围栏外属性 `apply_decl` 返回 false"——这对 B 类中部分属性不成立（如 `overflow: bogus` 返回 `true`）。`fence_contract.rs:152-172` 测试的 9 个属性全是 `_ => false` 命中的（无 match arm），未测试 B 类属性在无效值下的行为。

### 修复方向

统一语义：**属性名不在围栏内 → `false`；值无效 → `false`**。B 类中值无效时也应返回 `false`。或者明确文档：返回值表示"属性名被识别，与值是否有效无关"，但需同步 `fence.md` 和 `fence_contract.rs` 的口径。

**严重级别**：中（不影响功能，但文档/测试口径不一致，new contributor 易误解）

---

## 2. parse_four 静默吞掉 %/em/rem 返回 0（高）

**精确行号**：`mapping.rs:49-70`

```rust
let p = |i: usize| -> f32 {
    parts
        .get(i)
        .and_then(|x| x.strip_suffix("px").unwrap_or(x).trim().parse::<f32>().ok())
        .unwrap_or(0.0)
};
```

当用户写入 `padding: 10%` 或 `margin: 1rem` 时，`strip_suffix("px")` 返回 `None`，`unwrap_or(x)` 返回原始字符串 `"10%"`，`trim()` 返回 `"10%"`，`parse::<f32>()` 失败 → `.ok()` 返回 `None` → `.unwrap_or(0.0)` 返回 `0.0`。

**影响范围**：
- `padding` (L375)：`padding: 10%` → 四向全变成 0px
- `margin` (L385)：同
- `gap` (L431)：同
- `border`/`border-width` (L395)：同

**严重级别**：高。静默数据损坏——CSS 有效值 `padding: 10%` 被无声地转成 `padding: 0`，且 `apply_decl` 返回 `true`，调用方无感知。用户会看到间距消失，难以定位根因。

**修复方向**：`parse_four` 改为返回 `Option<[f32; 4]>`，非法值返回 `None`，让 `apply_decl` 相应返回 `false`。

---

## 3. color_filter parse_filter 拆分方式脆弱 + invert 非标准 + sepia 占位（中）

### 3.1 split_whitespace 必须依赖函数间空格（低）

**精确行号**：`mapping.rs:82`

```rust
for func in v.split_whitespace() {
```

正常 CSS `filter: grayscale(1) brightness(1.2)` 有空格分隔，没问题。但如果写为 `filter: grayscale(1)brightness(1.2)`（无空格），`split_whitespace` 会产生单个 token `"grayscale(1)brightness(1.2)"`，`split_once('(')` 得到 `("grayscale", "0.5)brightness(1.2")`，`trim_end_matches(')')` 得到 `"0.5)brightness(1.2"`——解析错误但不会报错，结果随机。

**严重级别**：低（实际 CSS 中几乎总是有空格，但代码健壮性差）。

### 3.2 invert 阈值非标准（中）

**精确行号**：`mapping.rs:109-116`

```rust
"invert" => {
    let x = parse_number(arg).unwrap_or(1.0);
    if x >= 0.5 {
        color_filter::invert()
    } else {
        IDENTITY
    }
}
```

CSS `invert(p)` 定义：`p` 是比例，`invert(0.3)` = 30% 反转。此实现只有全量（`invert()` = 100% 反转）或单位矩阵两档，阈值 0.5，丢失了中间值语义。`invert(0.3)` 不生效。

**严重级别**：中（AI 写 `filter: invert(0.3)` 会预期 30% 反转，实际无效果）。

**修复方向**：实现 partial invert 矩阵：`invert(p)` = (1-2p) 对角 + p 偏移。

### 3.3 sepia 退化 grayscale（低，已知）

**精确行号**：`mapping.rs:117-121`

```rust
"sepia" => {
    // ponytail: sepia 完整 Tint 矩阵实现期补，先用 grayscale 占位
    color_filter::grayscale()
}
```

`fence.md:108` 已标注此问题。用 grayscale 占位，色相错误。已知待修。

**严重级别**：低（已文档化，已知 deferred）。

---

## 4. ResolvedStyle 结构体字段分析（低）

**精确行号**：`resolved.rs:110-152`

结构体 22 个字段：

| 类别 | 字段 | 数量 |
|---|---|---|
| 布局（taffy_style 内） | flex/padding/margin/size/min/max/gap/position/inset/aspect_ratio/align/justify/flex_grow/flex_shrink/flex_basis 等 | 1 个大 struct |
| 视觉 | background_color, background_image, background_size, border_radius, border_color, border_width, opacity, overflow_x, overflow_y, color_filter, border_image_slice | 11 |
| 文本 | color, font_size, font_family, font_weight, text_align, line_height, letter_spacing, white_space_nowrap | 8 |
| 其他 | display_mode, order, touchable, transform, transition | 5 |

### 4.1 default() 合理性

`resolved.rs:161-197` 的 `default()` 实现合理：
- `opacity: 1.0`、`font_size: 16.0`、`font_weight: 400`、`color: [0,0,0,1]` 对应 CSS 标准默认
- `flex_direction: Column` 正确覆盖 taffy 的 `Row` 默认
- 可选字段 `None`（font_family, background_color, background_image 等）
- `transition: Vec::new()` 而非 `vec![default_spec]`——通过 `rematch_pseudo_classes` 的 `is_empty()` 判断区分"未声明"和"声明了全默认 spec"

### 4.2 庞大性评估

22 个字段尚在可管理范围。真正的"胖"在 `taffy_style: TaffyStyle`（~40 内部字段）。但这是 taffy 0.5 库设计，无法拆分。拆分 ResolvedStyle 为 LayoutStyle + VisualStyle 会增加跨层传递复杂度，不划算。

**严重级别**：低（可管理，不拆分）。

---

## 5. cascade.rs clone 开销分析（低）

**精确行号**：`cascade.rs:9-68`

每节点构建流程：
1. 预处理 L10-12：`N` 次 `ResolvedStyle::default()` 分配
2. 递归 L21：再次 `ResolvedStyle::default()` 分配
3. L24-31：继承父字段（含 `font_family.clone()` 堆分配）
4. L43-47：应用规则声明
5. L50-53：应用 inline style
6. L59：`out[id.0].clone()` 完整 clone（含 taffy_style(~40 字段)、Vec\<TransitionSpec\>、Option\<String\>）传出给子节点继承

总计：每节点 **2 次 default + 1 次 clone**。

**影响**：`resolve_styles` 仅构建期调用（打包器），非每帧路径。注释 L58 也注明"font_family 是唯一堆分配，开销可接受"。实际场景中场景节点数通常 < 10000，构建期 3 倍分配不影响用户体验。

**严重级别**：低（构建期一次性路径，可接受）。

**轻微优化方向**（非必要）：将 clone 推迟到子节点需要时才做（惰性），或使用 `Rc<ResolvedStyle>` 共享只读部分（但 taffy Style 不是 `Rc` 友好的类型）。

---

## 6. rematch_pseudo_classes O(N×M) 复杂度（低）

**精确行号**：`dynamic.rs:304-363`

```rust
for node_id in node_ids {                          // O(N)
    for r in &rules_with_spec {                    // O(M)
        if match_element_with_state(...) {         // O(depth) 祖先链
            matched.push(r.clone());
        }
    }
}
```

复杂度 O(N × M × depth)，其中 N = 节点数，M = 动态规则数，depth = 祖先链深度。

**当前可接受的原因**：
- 动态规则仅含伪类规则（:hover/:active/:disabled/:focus）和属性选择器规则（[data-page]），一般 10-100 条
- 场景节点通常 < 5000
- 每帧执行，当前实测未构成瓶颈

**优化方向（当 M 增长到数百时可考虑）**：
- 按 tag/class/id 预建规则索引（HashMap），每个节点只查与自己 tag/class/id 相关的规则子集
- 类似浏览器内部的选择器匹配 Bloom filter 或 rule hash

**严重级别**：低（当前规模无瓶颈；如后续动态规则规则增长到数千条，再优化）。

---

## 7. 继承模型审查（低）

**精确行号**：`cascade.rs:22-32`

```rust
// 继承白名单字段
style.color = p.color;
style.font_size = p.font_size;
style.font_family = p.font_family.clone();
style.font_weight = p.font_weight;
style.line_height = p.line_height;
style.letter_spacing = p.letter_spacing;
style.text_align = p.text_align;
style.white_space_nowrap = p.white_space_nowrap;
```

### 7.1 继承字段与 CSS 标准对比

| 属性 | CSS 继承 | LoomGUI 继承 | 一致 |
|---|---|---|---|
| `color` | ✓ | ✓ | ✓ |
| `font-size` | ✓ | ✓ | ✓ |
| `font-family` | ✓ | ✓ | ✓ |
| `font-weight` | ✓ | ✓ | ✓ |
| `line-height` | ✓ | ✓ | ✓ |
| `letter-spacing` | ✓ | ✓ | ✓ |
| `text-align` | ✓ | ✓ | ✓ |
| `white-space` | ✓ | ✓ | ✓ |
| `opacity` | ✗ | ✗ | ✓ |
| `background-color` | ✗ | ✗ | ✓ |
| `border-*` | ✗ | ✗ | ✓ |
| `display` | ✗ | ✗ | ✓ |
| `overflow` | ✗ | ✗ | ✓ |
| `transform` | ✗ | ✗ | ✓ |
| `pointer-events` | ✗ | ✗ | ✓ |

**结论**：继承列表精确匹配 CSS 继承规范，无不一致。

### 7.2 运行时 color 继承补充

`dynamic.rs:362-403` 有 `propagate_color_inheritance`——处理运行时父节点因伪类动态变 color 后子节点继承传播，填补了 rematch 每条独立 cascade 不读父的缺口。逻辑正确。

**严重级别**：低（继承实现正确）。

---

## 8. fence.md 与代码一致性校验（中）

### 8.1 row-gap/column-gap 标注不准确

**fence.md:56**：`row-gap/column-gap` 标"推断·待测"，注明"gap longhand"。

**实际代码**：`apply_decl`（`mapping.rs:348`）中无 `row-gap`、`column-gap` 的 match arm，会落入 `_ => false`。二者**根本未实现**，不是"待测"，是"不支持"。

### 8.2 overflow bogus 值返回 true 与文档语义冲突

**fence.md:84**：`overflow` 只认 visible/hidden/scroll/auto。

**实际代码**：`mapping.rs:542-549` 对 `"bogus"` 返回 `true`（不改变字段，但声称识别）。`fence_contract.rs:139-149` 测试也确认了此行为。文档说"只认四个值"，但代码对四个以外的值也"认"（返回 true）。语义矛盾：该算"静默忽略"还是"识别但无效"？

### 8.3 min-width/min-height 值约束不准确

**fence.md:50**：`width`/`height` 支持 `px/%/auto`，`min-width`/`min-height` 支持 `px/%`。

**实际代码**：`parse_dimension` 对 auto 返回 `Dimension::Auto`。`min-width`/`min-height` 共用同一解析器——所以 `min-width: auto` 也会被接受（解析为 `Dimension::Auto`），但 CSS 规范中 `min-width: auto` 对 flex item 有特殊含义。文档缺少 auto。

### 8.4 display:grid 行为需明确

**fence.md:118**：`display:grid` 标"落 Flex，grid 布局不生效"。

**实际代码**：`mapping.rs:490-493`，非 `"none"`、非 `"block"` 的值一律设 `Display::Flex` + `DisplayMode::Flex`。`apply_decl` 返回 `true`。AI 写 `display: grid` → 预览 Chromium（grid 生效）→ 打包不报错（返回 true）→ Unity 是 Flex——"三处不同结果"的陷阱。

**严重级别**：中（文档与代码之间有 3 处细微不一致，会导致 contributor 和 AI 误解）。

---

## 9. 其他小发现

### 9.1 apply_decl 函数过长（低）

**精确行号**：`mapping.rs:348-673`，326 行。

约 30 个 match arm，模式重复：
- `ts.size.xxx = parse_dimension(value)` ×6（width/height/min/max 四向）
- `parse_color(value)` ×3（background-color/border-color/color）
- `parse_px(value).unwrap_or(default)` ×3（font-size/letter-spacing, border-width 隐式）
- `ts.xxx = match value.trim() {...}` ×6（flex-direction/flex-wrap/justify-content/align-items/align-self/text-align）

**现状**：`fence.md:222-223` 已明确 YAGNI 拒绝数组驱动重构，理由是"值约束（如 background-size 只认 cover/contain/100%）塞不进数组"。认可此判断——当前规模尚可管理。

**潜在简化**：可为"同型赋值"模式引入 `assign_dim!` 和 `assign_color!` 宏减少样板：

```rust
macro_rules! assign_dim {
    ($ts:expr, $field:ident, $dim:ident, $value:expr) => {
        $ts.$field.$dim = parse_dimension($value);
        true
    };
}
```

但这会把逻辑藏进宏、降低 grep 可见性——未必更优。

**严重级别**：低（已知且认可当前形态，有 YAGNI 评估）。

### 9.2 letter-spacing 单位处理半正确（低）

**精确行号**：`mapping.rs:597-600`

```rust
"letter-spacing" => {
    style.letter_spacing = parse_px(value).unwrap_or(0.0);
    true
}
```

`parse_px` 拒 `%`（返回 None），回退 0.0。CSS `letter-spacing` 支持 `em`、`px`、`normal`。`em` 被静默转 0（`parse_px` 也不处理 `em`）。行为：`letter-spacing: 0.1em` → 0px。与上述 `parse_four` 问题同类但影响小。

**严重级别**：低（CSS 中 letter-spacing 几乎只用 px 或 normal）。

### 9.3 color_filter.rs concat 矩阵乘法正确性（无问题）

**精确行号**：`color_filter.rs:104-119`

`concat(a, b)` 实现 a × b（矩阵乘法），与 fgui `ConcatValues` 一致。测试 `tests.rs:419-455` 验证了 `saturate(0.5) hue-rotate(90deg)` 顺序与 CSS 语义一致（先 saturate 后 hue-rotate → 组合 = H × S）。实现正确。

### 9.4 transition 解析对逗号分隔的回归已修复（无问题）

**精确行号**：`mapping.rs:679-681`

```rust
fn parse_transition(value: &str) -> Vec<TransitionSpec> {
    value.split(',').filter_map(parse_one_transition).collect()
}
```

测试 `tests.rs:489-498` 验证逗号分隔多 spec。之前的 bug（split_whitespace 不处理逗号）已修复。

---

## 10. 总结

| 维度 | 发现数 | 最高严重级别 |
|---|---|---|
| apply_decl 返回 bool 语义不一致 | 大量（B 类 ~18 属性） | 中 |
| parse_four 静默吞值 | 1（影响 4 属性） | **高** |
| color_filter 实现缺陷 | 3（split/invert/sepia） | 中/低 |
| ResolvedStyle 结构 | 1 | 低 |
| cascade clone 开销 | 1 | 低 |
| rematch 复杂度 | 1 | 低 |
| 继承模型 | 1（正确） | 低 |
| fence.md 一致性 | 3 处不准确 | 中 |
| 其他小发现 | 3 | 低 |

**优先修复项**：
1. **`parse_four` 静默吞值**（§2）——数据损坏 bug，需从 `[f32; 4]` 改为 `Option<[f32; 4]>`
2. **apply_decl 返回值语义统一**（§1）——明确规范并修复 B 类中值无效仍返 true 的属性
3. **invert 阈值非标准修复**（§3.2）——实现 partial invert 矩阵
4. **fence.md 一致性同步**（§8）——更正 row-gap/column-gap、overflow、min-width 文档
