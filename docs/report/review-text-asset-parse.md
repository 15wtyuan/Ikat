# 模块代码审查报告：text / asset / parse / dump / transform

审查日期：2026-07-09

---

## 1. text/layout.rs — 文本测量与断行

### 1.1 `measure_text` 与 `measure_rich_text` baseline 算法不一致

**位置**：`layout.rs:381-385` vs `layout.rs:718`

**代码片段**：
```rust
// measure_text (line 381-385)
let baseline = if line_height > 0.0 {
    (line_h + ascent - descent) / 2.0 - descent.abs()
} else {
    ascent
};

// measure_rich_text (line 718)
let baseline = line_ascent; // 行顶到 baseline
```

**分析**：`measure_text` 在有 line-height 时做了 CSS 标准 half-leading 居中（展开后 = `(line_h + ascent + descent) / 2.0`，正确），而 `measure_rich_text` 始终用 `line_ascent`（不居中）。两者对同节点调用的 baseline 行为不同——如果同一个 Text 节点 switch 到 RichText 模式，文字垂直位置会偏移。虽然当前两函数用于不同的 NodeKind（`Text` vs `RichText`），但渲染层期望 baseline 语义一致。

**严重级别**：🟡 中（行为不一致，但暂不会互相调用）

**修复方向**：`measure_rich_text` 在有 `line_height > 0` 时也应做 CSS half-leading 居中，或至少在函数注释中标明与 `measure_text` 的 baseline 差异是故意的，说明理由。

---

### 1.2 `measure_rich_text` 的 CJK 逐字分词对拉丁字符也生效

**位置**：`layout.rs:626-654`

**代码片段**：
```rust
if word.chars().any(is_cjk) {
    // CJK 拆单字：每字一 token
    ...
} else {
    // Latin 逐词
    ...
}
```

**分析**：判定用的是 `word.chars().any(is_cjk)`——只要词中**有一个** CJK 字符，整个词的所有字符（含其中的拉丁字母、数字）都按逐字拆成 token。例如 `"Hello世"` → 6 个独立 token（H/e/l/l/o/世）。拉丁字母被拆成单字后失去了字距（kerning），且断行机会变多（每字母可换行）。更严重的是，这个判定在 `split(' ')` 之后，意味着 `"Hello 世界"` → `["Hello", "世界"]`，`"Hello"` 无 CJK 走逐词，`"世界"` 有 CJK 走逐字——这没问题；但 `"Hello世界"`（无空格）整个词含 CJK → 全部逐字，拉丁部分丢失字距。

**严重级别**：🟡 中（混排无空格场景受影响，中文环境常见于用户输入粘贴）

**修复方向**：改判定为 per-character：在 `char_indices` 循环内对每个 ch 单独判 `is_cjk`，CJK 单字切 token，连续 Latin 合并后再切 token。

---

### 1.3 `measure_text` 贪心断行依赖 unicode-linebreak，未显式处理 CJK 连字

**位置**：`layout.rs:420-481`

**分析**：`measure_text` 不调用 `is_cjk`，完全信任 `unicode_linebreak::linebreaks` 的 UAX#14 规则。UAX#14 对 CJK（Class=ID 表意文字）允许每个字符前后换行，这基本正确。但 `unicode-linebreak` 0.1.5 依赖的 Unicode 版本可能与本项目测试字体 wqy-microhei 的字符覆盖有差距——部分扩展 CJK（CJK-B/C/D/E/F 及部首补充）的换行 class 可能被映射为 AL（字母）而非 ID，导致在极窄宽度下不会逐字断行，文本溢出。

**严重级别**：🟢 低（扩展 CJK 实际使用极少）

**修复方向**：暂无必要；若将来有用户报告，可在 `measure_text` 的超长词边界逻辑中补充 `is_cjk` 判定。

---

### 1.4 `stack_str_advance` 无字距

**位置**：`layout.rs:585-593`

**代码片段**：
```rust
let stack_str_advance = |s: &str, size_px: f32| -> f32 {
    let mut pen = 0.0f32;
    for ch in s.chars() {
        let (f, _) = stack.pick(ch);
        let gid = f.face.glyph_index(ch);
        pen += glyph_advance(&f.face, gid, size_px);
    }
    pen
};
```

**分析**：token 宽度估算用的是纯字符 advance 累加，不含字距。而最终字形定位（line 746-752）是含字距的。这导致 token 宽度低估（对于有负 kerning 的字对如 "AV"），贪心断行时每行实际容纳的字符数会多于预估，可能出现行溢出容器。

**严重级别**：🟡 中（窄容器 + 负 kerning 字体可能溢出）

**修复方向**：token 宽度估算应复用含 kerning 的度量逻辑，或保守估算（为负 kerning 留余量）。至少注释说明此为已知简化。

---

## 2. text/atlas.rs — 字形图集

### 2.1 无限页增长无上限

**位置**：`atlas.rs:209-235`

**代码片段**：
```rust
fn allocate(&mut self, gw: u32, gh: u32) -> AllocRect {
    // ...
    // 现有页全放不下 → 开新页并在其上分配
    let new_idx = self.pages.len() as u32;
    self.pages.push(AtlasPage::new(PAGE_SIZE as u32, PAGE_SIZE as u32));
    // ...
}
```

**分析**：每页 4096² × R8 = 16MB。如果渲染包含所有 CJK-B/C/D/E（约 70000+ 字符），按每页 3000-4000 CJK 字形估算，需 ~20 页 = 320MB。当前无页数上限或内存水位告警。对游戏 UI 的实际使用量（几百字）不会触发，但作为库应设安全上限。

**严重级别**：🟢 低（实际场景不触发，但防御性编程缺失）

**修复方向**：设 `MAX_PAGES` (如 16 = 256MB)，超出返 Error 或记录 warning。

---

### 2.2 脏页追踪为 O(n) 线性扫描

**位置**：`atlas.rs:137`

**代码片段**：
```rust
if !self.dirty.contains(&alloc.page) {
    self.dirty.push(alloc.page);
}
```

**分析**：`Vec::contains` 是 O(n)。每帧首次遇到新字形时都会扫一遍 dirty 列表。正常情况下 dirty 列表很短（每帧新字形数量），但理论上若一帧首次渲染大量新字形（冷启动）、在多页 atlas 下，每个新分配都 O(dirty_len) 扫描。实际 dirty_len 通常 ≤ 页数，可接受。

**严重级别**：🟢 低

**修复方向**：如果性能数据显示瓶颈，改用 `HashSet<u32>` 追踪脏页。

---

### 2.3 `allocate` 在 `ensure` 的调用链中持有 `&mut self`，与 `self.pages` 的 borrow 有潜在冲突

**位置**：`atlas.rs:102-154`

**代码片段**：
```rust
pub fn ensure(&mut self, face: &Face<'_>, key: GlyphKey) -> GlyphRect {
    // ...
    let alloc = self.allocate(gw, gh);
    let page = &mut self.pages[alloc.page as usize]; // borrow self.pages mutably
    // ...
}
```

**分析**：`allocate` 返回 `AllocRect` 后，`ensure` 再次 `&mut self.pages[...]`。这两个 `&mut self` 调用需要在不同的 NLL borrow scope 内——当前代码通过 `allocate` 返回非引用值（`AllocRect`）来断开 borrow，编译器接受。但如果重构时将 `allocate` 改为内联逻辑且持有 `self.pages.iter_mut()` 的 borrow 跨越到 `self.pages[alloc.page as usize]` 访问，会编译失败。当前代码正确。

**严重级别**：🟢 无

---

### 2.4 `tofu_box` 的宽高非整数倍

**位置**：`atlas.rs:310-329`

**代码片段**：
```rust
let w = ((size_px as f32 / 2.0).ceil() as u32).max(2);
let h = (size_px as u32).max(4);
```

**分析**：tofu 框宽 = size/2 上取整，高 = size。与典型 em 方块近似，但宽高比不是 1:1。这在排版中不影响 advance（advance 由 layout.rs 独立计算），但这意味着 tofu 块视觉上不是正方形（影响开发者查阅缺失字形时的视觉判断）。有意为之（方便区分 tofu 与真字形），可接受。

**严重级别**：🟢 无（调试视觉，不影响正确性）

---

## 3. text/rich.rs — 富文本标记解析

### 3.1 HTML 实体解码不支持数值实体

**位置**：`rich.rs:399-412`

**代码片段**：
```rust
let mapped = match ent {
    "&lt;" => "<",
    "&gt;" => ">",
    // ...
    _ => return None,
};
```

**分析**：`parse_entity` 只处理 6 个命名实体。数值实体 `&#60;`、`&#x3C;`、`&#20013;` 等返回 `None` → `text_buf.push('&')`，`&` 被当作字面量保留。例如 `<div style="display:block">a&#60;b</div>` 会渲染为 "a&b" 而非 "a<b"。CSS `content` 属性也受影响。

**严重级别**：🟡 中（数值实体在 AI 生成的 HTML 中常见）

**修复方向**：在 `parse_entity` 中增加 `&#` 前缀匹配 → 解析十进制/十六进制数 → `char::from_u32`。

---

### 3.2 `<img>` src 缺失时不报错，产空串

**位置**：`rich.rs:218-236`

**代码片段**：
```rust
let src = get_attr(attrs, "src").unwrap_or("").to_string();
```

**分析**：`<img width="16" height="16">` 无 src → src="" → 后续渲染查 image atlas 失败，静默不显示。应该报错（编译期）或在 measure 期忽略（略过不产 `RichImagePlacement`）。

**严重级别**：🟢 低（实际 AI 很少生成无 src 的 img）

**修复方向**：src 为空时跳过不产 token（`continue`），或在 parse 期 warning。

---

### 3.3 `split_tag` 无法处理属性值含空格的标签名

**位置**：`rich.rs:315`

**代码片段**：
```rust
fn split_tag(inner: &str) -> (&str, &str) {
    match inner.find(char::is_whitespace) {
```

**分析**：对 `<span style="color:red">` → `inner = "span style=\"color:red\""` → first whitespace 在 "span style" 之间 → 切出 name="span"、attrs="style=\"color:red\""。正确。但如果有人写 `< span >`（标签名前有空格），`inner = " span "` → first whitespace 在位置 0 → name=""。后续 `match name { "b"|"strong" => ... , other => Err(...) }` 会报 "unsupported rich tag: <>"。这是合理的围栏行为——不允许标签名前有空格。

**严重级别**：🟢 无（合理围栏）

---

### 3.4 无自闭合 `<img/>` 中 `<img ... />` 的单独 token 处理

**位置**：`rich.rs:178-179`

**代码片段**：
```rust
let (raw_name, attrs) = split_tag(tag_inner);
let name = raw_name.trim_end_matches('/').trim();
```

**分析**：`trim_end_matches('/')` 正确处理了 `<img src="a.png" />`（尾部斜杠）。但如果 `inner` 是 `"img/"`（标签名后紧接斜杠再空格再属性），`split_tag` 先按空白切 → name="img/"。`trim_end_matches('/')` → "img"。正确。

**严重级别**：🟢 无

---

## 4. asset/mod.rs — Package 二进制格式

### 4.1 `intern` 函数 u16 溢出

**位置**：`asset/mod.rs:682-694`

**代码片段**：
```rust
fn intern(...) -> u16 {
    // ...
    let i = strings.len() as u16;
    strings.push(s.to_string());
    idx_of.insert(s.to_string(), i);
    i
}
```

**分析**：StringTable 共用一张表存放组件名、所有节点的 text content、img path、classes、id_attr、controller name 等。每个节点至少有 1 个 text/src + 若干 class 名。1000 个节点 + 多点样式就能超 65535。`strings.len() as u16` 在溢出时静默回绕到 0，导致后面的字符串覆盖前面的索引、读取时取到错误字符串。

**严重级别**：🔴 高（大包静默数据损坏）

**修复方向**：`intern` 应返回 `Result<u16, PkgError>` 并在溢出时返回错误；或在 `write_package` 前检查字符串总数 ≤ 65535。

---

### 4.2 包格式无完整性校验

**位置**：`asset/mod.rs:281-472`

**分析**：整个 `.pkg.bin` 格式没有 CRC/checksum 字段。Header 20 字节 + 各段变长数据，若有字节损坏（磁盘、网络、git 合并冲突），`read_package` 仅依赖 `Truncated` 检测（读到文件尾）和一些值域检查如 `root_node_idx` 越界。如果损坏发生在一个字符串内容（如 src path）、style blob 或 dynamic_rules blob 内部，bincode deserialize 可能成功但产错误数据（bincode 无内建校验）。

**严重级别**：🟡 中（实际 bug 罕见，但诊断困难——损坏后症状是 UI 错乱非 crash）

**修复方向**：在包尾追加 CRC32（或更简单的 XOR checksum），`read_package` 验完后 return Ok 之前校验。

---

### 4.3 `read_package` 不检查消费完所有字节

**位置**：`asset/mod.rs:476-669`

**分析**：`Reader` 在各段完成后没有检验 `pos == buf.len()`。如果 pkg.bin 末尾有多余字节（如 append 了其他数据、重复拼接），会静默忽略。虽无害，但不利于诊断拼接/截断问题。

**严重级别**：🟢 低

**修复方向**：read 末尾检查 `r.pos == bytes.len()`，不等则返回 `Truncated("trailing_bytes")` 或 `TooNew`。

---

### 4.4 `component_count` 读后未校验合理范围

**位置**：`asset/mod.rs:491`

**代码片段**：
```rust
let component_count = r.u32("component_count")? as usize;
```

**分析**：如果 component_count 被损坏为极大值（如 0xFFFFFFFF = 4B），后续 `comp_table: Vec::with_capacity(4_000_000_000)` 会 OOM 崩溃。`string_count` 同理。

**严重级别**：🟡 中（损坏包可致 OOM）

**修复方向**：设合理上限（如 component_count ≤ 10000, string_count ≤ 100000），超出返回 `TooNew`。

---

### 4.5 `KIND_IMAGE` / `KIND_TEXT` 读 `src_idx`/`text_idx` 不作 NULL_IDX 检查

**位置**：`asset/mod.rs:556-562`

**代码片段**：
```rust
KIND_IMAGE => NodeKind::Image {
    src: string_at(&strings, src_idx)?,
},
KIND_TEXT => NodeKind::Text {
    content: string_at(&strings, text_idx)?,
},
```

**分析**：`string_at` 遇到 `NULL_IDX(0xFFFF)` 返回 `Ok(String::new())`，所以即使损坏的 pkg 把 Image/Text 节点的 src_idx/text_idx 写成 NULL_IDX，也不会 crash——只会产出空串的 Image/Text 节点。可接受。

**严重级别**：🟢 无

---

### 4.6 `write_package` 不写 root_w/root_h

**位置**：`asset/mod.rs:397-402`

**代码片段**（Header 写入）:
```rust
out.extend_from_slice(&PKG_MAGIC.to_le_bytes());
out.extend_from_slice(&PKG_FORMAT_VERSION.to_le_bytes());
out.extend_from_slice(&0u32.to_le_bytes()); // flags
out.extend_from_slice(&(component_count as u32).to_le_bytes());
out.extend_from_slice(&(strings.len() as u32).to_le_bytes());
```

**分析**：注释说 "Header 不含 root_w/root_h（root_size 归 Stage）"，这意味着运行时解析包时不知道设计尺寸。Stage（Unity 侧）负责传入 root_w/root_h。这解耦正确，但需要文档清晰说明 root_size 的契约（packager 和 runtime 的 root_size 必须一致），否则可能出现布局大小不匹配。

**严重级别**：🟢 低（契约在 main-design.md 中有记录）

---

## 5. parse/dom.rs — HTML 解析与围栏校验

### 5.1 相邻文本节点合并时用空格连接，可能丢换行语义

**位置**：`dom.rs:109-143`

**代码片段**：
```rust
let mut text_parts: Vec<String> = Vec::new();
// ...
text_parts.push(s.to_string());
// ...
let text = if has_text {
    Some(text_parts.join(" "))
} else { None };
```

**分析**：多个相邻文本子节点之间用 `join(" ")` 以空格连接。如果 HTML 中有显式换行依赖的布局（如 `<span>line1</span>\n<span>line2</span>`），换行被替换为空格，语义丢失。但这符合 HTML 标准（空白折叠），且围栏不支持 `<br>` 在普通元素中（`<br>` 是围栏外标签）。

**严重级别**：🟢 低（符合 HTML 规范）

---

### 5.2 `is_inline_display_block` 的去空白匹配可能误匹配

**位置**：`dom.rs:171-178`

**代码片段**：
```rust
fn is_inline_display_block(attrs: &HashMap<String, String>) -> bool {
    let Some(s) = attrs.get("style") else { return false; };
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    compact.contains("display:block")
}
```

**分析**：去空白后子串匹配 `"display:block"`。如果 style 中其他 CSS 属性值包含此子串（例如 `content: "display:block"` 或自定义属性 `--x: display-block`），将误触发 rich text 转换。后者在当前围栏中不存在，前者是极小概率。

**严重级别**：🟢 低（实际触发概率极低）

**修复方向**：用 `cssparser` 或正则做 proper CSS 解析，或至少限制匹配在 `;` 或串首/尾边界。

---

### 5.3 行内混排检测对空白文本节点的处理正确但未文档化

**位置**：`dom.rs:113-117`

**代码片段**：
```rust
scraper::node::Node::Text(t) => {
    let s = t.text.trim();
    if !s.is_empty() {
        text_parts.push(s.to_string());
    }
}
```

**分析**：`trim()` 使得纯空白文本节点（如 HTML 缩进产生的换行+空格）不触发混排检测。正确。但如果 HTML 中有意放置的非断空格 `&nbsp;`（经 scraper 解析后是 `\u{A0}`），它不会被 `trim()` 去掉，会触发混排检测。对于 `<div>&nbsp;<img></div>` 会报 "行内混排不支持"。这合理——`&nbsp;` 本就是有意放置的字符。

**严重级别**：🟢 无

---

## 6. parse/selector.rs — 选择器匹配

### 6.1 伪类不参与 specificity 计算

**位置**：`selector.rs:142-149`

**代码片段**：
```rust
if id.is_some() { spec.0 += 1; }
spec.1 += classes.len() as u32;
spec.1 += attrs.len() as u32;
if tag.is_some() { spec.2 += 1; }
// 伪类（:hover/:active 等）不计入 specificity
```

**分析**：CSS 标准中伪类（`:hover` 等）贡献 (0,1,0)。在此实现中 `:hover` 贡献 0。这意味着 `.btn:hover` 和 `.btn` 有相同的 specificity (0,1,0)，当两条规则同时存在时由源序决定胜负。这与 CSS 标准不符，但代码注释（`spec §4.2`）表明这是有意设计——伪类规则不进 base cascade，由运行时 `rematch_pseudo_classes` 动态应用。因为 base 阶段已过滤掉含伪类的规则（`compound_matches` 返回 false），所以 specificity 差异在 base cascade 中不体现；而在 runtime rematch 中，`match_element_with_state` 只收集命中的动态规则，同一元素同时命中 `.btn:hover` 和 `.btn` 时，后 apply 的胜出（升序排 specificity，高 specificity 后 apply）。如果两条同等 specificity，排序按 specificity 元组 (0,1,0) 相同 → stable sort 保持原序。这会导致依赖特异性的优先级不确定。但当前设计是伪类不进 base、动态规则仅限于伪类规则——同节点同伪类的多规则冲突在实际使用中极少见。

**严重级别**：🟡 中（与 CSS 规范的偏离可能导致 AI 生成的选择器优先级与预期不符）

**修复方向**：给伪类每个 +1 到 spec.1（等同学一个 class），或文档明确说明此偏离及理由。

---

### 6.2 选择器伪类检测错误处理冒号分隔的末尾

**位置**：`selector.rs:154-167`

**代码片段**：
```rust
let mut rest = text.as_str();
while let Some(colon) = rest.find(':') {
    let after = &rest[colon + 1..];
    let end = after.find(['.', '#', ':']).unwrap_or(after.len());
    let name = &after[..end];
    match name { ... }
    rest = &after[end..];
}
```

**分析**：伪类名截断用 `after.find(['.', '#', ':']).unwrap_or(after.len())`。对 `.btn:hover.active`：
- 第一轮：colon 指向第一个 `:`（在 "hover" 之前），after="hover.active"，end 指向第二个 `:`（在 "active" 之前），name="hover"
- 第二轮：rest=after[end..]=":active"，colon 指向首字符 `:`，after="active"，end=after.len()（无更多 `./#/:`），name="active"
正确。

但 `#id:hover` 中：
- `text = "#id:hover"`，第一轮 token 化 `#` → 设 kind='#'，cur="id" → push_token 填 id。然后遇 `:` → flush cur → kind=':'。
- 后续 while 循环用 `text.as_str()`（原始 `#id:hover`）而非逐 token 后的内容——`rest = text.as_str()` 从头开始。find(':') 会在第一个 `:` 处命中 → after="hover" → name="hover"。正确。

边界情况：`[attr="val:x"]:hover` — 属性值内嵌 `:`。parse 阶段（lines 104-129）先处理 `[...]`，扫描到 `]` 后跳过整个 attr。之后 token 化继续时，伪类检测应从 `[...]` 之后开始。由于伪类检测用的是 `text.as_str()`（原始文本，含 `[...]`），`find(':')` 可能在 `[attr="val:x"]` 内部的 `:` 处误命中。需要验证。

看代码：伪类检测在 compound 构建之后（line 150），用的是 `text.as_str()`。若 `text = "div[data-x=\"a:b\"].cls:hover"`：
- find(':') 第一次在 `"a:b"` 内部的冒号命中 → after = `b\"].cls:hover"` → end 在 `]` 处（或下一个 `:`）…实际 end 用 `find(['.', '#', ':'])`，第一个是 `]` 之后的 `.` → name = `b\"].cls` → 不在识别的伪类列表中 → 静默忽略。然后 rest 继续…最终可能漏掉 hover。

**严重级别**：🟡 中（属性值含 `:` 的选择器可能伪类检测失败）

**修复方向**：伪类检测前先 strip 掉已解析的 `[...]` 段，或改用基于 `text` 的 offset 追踪而非 `find(':')` on whole text。

---

### 6.3 `match_element` 用 `std::ptr::eq` 找元素索引

**位置**：`selector.rs:344-352`

**代码片段**：
```rust
let el_id = tree.nodes.iter().position(|n| std::ptr::eq(n, el)).map(ElementId);
let el_id = match el_id {
    Some(id) => id,
    None => return Vec::new(),
};
```

**分析**：如果调用方传入的是 `tree.nodes[i].clone()` 而非 `&tree.nodes[i]`，`ptr::eq` 返回 false → 匹配结果为空。调用方不自知错误，选择器看起来"不匹配任何规则"。注释已说明这个风险。当前所有调用方都传 `&tree.nodes[i]`，无问题。

**严重级别**：🟢 低（注释已文档化）

**修复方向**：把 `match_element` 的签名改为接收 `el_id: ElementId`，消除 ptr 比较的不稳定性。

---

### 6.4 属性选择器 `[attr~="val"]` 围栏外操作符降级可能导致误匹配

**位置**：`selector.rs:190-222`

**代码片段**：
```rust
if name_part.ends_with(['~', '^', '$', '*', '|']) {
    let clean_name = name_part[..name_part.len() - 1].trim_end().to_lowercase();
    return AttrSelector { name: clean_name, op: AttrOp::Exists, value: None };
}
```

**分析**：围栏外操作符 `~=`, `^=`, `$=`, `*=`, `|=` 降级为 `Exists`。例如 `[attr^="val"]` → 变成 `[attr]`（存在即匹配）。这可能导致原本不应匹配的元素现在匹配了（只要它有该属性）。但这是明确的设计选择——围栏外操作符的 CSS 规则不会进 .pkg.bin（打包器 parse 期处理），运行时主要由 `Eq` 和 `Exists` 两种。

**严重级别**：🟢 低（保守降级，不会静默丢信息）

---

## 7. dump.rs — 调试工具

### 7.1 无硬编码路径，无 panic 风险

**位置**：`dump.rs:1-132`

**分析**：`dump_scene_json` 是纯函数，接收 `&Scene`，返回 String。world_transforms 访问有 bounds guard（`scene.world_transforms.len()` 检查），anim 用 `get()` 不 panic。JSON 构建用 `format!` 宏 + `json_escape`，正确转义所有特殊字符。

**严重级别**：🟢 无

---

## 8. transform.rs — 2D 仿射变换

### 8.1 `inverse` 不处理奇异矩阵（det=0）

**位置**：`transform.rs:51-63`

**代码片段**：
```rust
pub fn inverse(m: &Affine2) -> Affine2 {
    let (a, b, c, d, tx, ty) = (m[0], m[1], m[2], m[3], m[4], m[5]);
    let det = a * d - b * c;
    let inv_det = 1.0 / det;
    // ...
}
```

**分析**：如果 det=0（如 scale(0,0)、或变换退化），`1.0/0.0 = f32::INFINITY`，后续所有 inv_* 值变成 INFINITY 或 NaN。这些值传入渲染层 world_matrix → GPU 可能产生不可预测的渲染结果（全屏白/黑/NAN 颜色）。更关键的是，如果 `inverse` 用于 hit_test 逆变换（例如把屏幕坐标转节点本地坐标），NaN 会通过 `apply_point` 传播，导致命中测试彻底失效。

**严重级别**：🔴 高（触发条件：scale(0,*) 动画过渡或编辑态设零缩放；后果：渲染异常 + 点击失效）

**修复方向**：`inverse` 返回 `Option<Affine2>` 或在 det.abs() < epsilon 时返回 `IDENTITY`（降级），并在调用方合理处理。同时在 `from_scale` 等构造函数或动画系统的缩放入口加 `max(EPS, sx)` 钳制。

---

### 8.2 `is_pure_translation` 使用固定 epsilon

**位置**：`transform.rs:72-75`

**代码片段**：
```rust
const EPS: f32 = 1e-6;
(m[0] - 1.0).abs() < EPS && m[1].abs() < EPS && m[2].abs() < EPS && (m[3] - 1.0).abs() < EPS
```

**分析**：1e-6 对 f32 精度合理。但长期积累（如嵌套旋转 10000 次后回到 0）可能超过此阈值，导致"实际是单位旋转"被误判为非纯平移。对于游戏 UI 的动画帧数，不会触发。

**严重级别**：🟢 无

---

### 8.3 `[f32; 6]` 对 2D 仿射来说足够

**分析**：2D 仿射变换由 2×2 线性部分 + 2×1 平移构成 = 6 个参数。`[f32; 6]` 完全足够表示所有 2D 仿射变换（translate, rotate, scale, skew, reflect 及任意组合）。不需要 3×3 矩阵或齐次坐标。

**严重级别**：🟢 无

---

## 总结

| 模块 | 高 | 中 | 低 | 总计 |
|------|:--:|:--:|:--:|:----:|
| text/layout.rs | 0 | 3 | 1 | 4 |
| text/atlas.rs | 0 | 0 | 2 | 2 |
| text/rich.rs | 0 | 1 | 1 | 2 |
| asset/mod.rs | 1 | 2 | 2 | 5 |
| parse/dom.rs | 0 | 0 | 2 | 2 |
| parse/selector.rs | 0 | 2 | 2 | 4 |
| dump.rs | 0 | 0 | 0 | 0 |
| transform.rs | 1 | 0 | 0 | 1 |
| **总计** | **2** | **8** | **10** | **20** |

**需优先处理**：
1. `transform.rs:51` — `inverse` 不处理 det=0，可能导致 NaN/INF 传播到渲染和命中测试。
2. `asset/mod.rs:690` — `intern` 函数 u16 溢出，可能导致大包字符串索引回绕、静默数据损坏。
