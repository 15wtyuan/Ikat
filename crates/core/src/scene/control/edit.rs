use crate::input::{EventRecord, EVT_SUBMITTED, EVT_VALUE_CHANGED};
use crate::scene::node::{Composition, ControlState, EditState, NodeId, NodeKind, Scene};
use crate::scene::text_cursor::{cursor_pixel_x, hit_byte_offset, line_byte_ranges};

/// 返回 measure/render 应用的「显示文本」及其 composition 字节区间：value 原样，把
/// composition 预提交文本拼到 composition.pos 处（用户输拼音须可见）。
///
/// 返回 `(String, Option<(usize, usize)>)`：第一个是显示文本，第二个是 composition 在该
/// 显示文本里的字节区间 `[start, end)`（无 composition 或空 composition 时为 `None`）。
/// 这个区间是 render 下划线 / 光标几何 / IME 候选窗定位的统一真相源。
///
/// 标记子串模型（RmlUi/FairyGUI 共识）：composition 不是独立 buffer，而是拼进显示文本的
/// 一个段落，作为一个 text run 参与 measure/换行/光标几何。该段落在 render 时由 composition
/// 分支按下划线区间绘制。无 composition 时显示文本即为 value 原文。
///
/// composition.pos 是基于 value 的字节偏移，可能落在多字节字符中间。插入按字符计数对齐：
/// pos 落在 value 的某字符边界，显示串同位字符位置 = 该边界前的字符数。composition 占据
/// 显示串 `[pos_char, pos_char + composition_chars)` 字符位，再换算成字节区间。
pub fn display_value(e: &EditState) -> (String, Option<(usize, usize)>) {
    let base = e.value.clone();
    let Some(c) = e.composition.as_ref() else {
        return (base, None);
    };
    let mut chars: Vec<char> = base.chars().collect();
    // composition.pos 钳到原始 value 的合法 UTF-8 字符边界（防后端传越界/中间字节 pos）。
    // value[..aligned] 仅在 aligned 是字符边界时合法；回退到最近 char 起始字节避免切片 panic。
    let mut aligned = c.pos.min(e.value.len());
    while aligned > 0 && !e.value.is_char_boundary(aligned) {
        aligned -= 1;
    }
    // composition 插入按字符计数对齐：value 边界前的字符数 = 显示串里的对应字符位置。
    // 钳到显示串当前长度内（防 pos 越界）。
    let pos_char = e.value[..aligned].chars().count();
    let insert_start_char = pos_char.min(chars.len());
    let comp_chars: Vec<char> = c.text.chars().collect();
    for (i, ch) in comp_chars.iter().enumerate() {
        // 插入点越界（composition.pos 在显示串末尾之外）时追加到末尾，不丢字符。
        let at = (insert_start_char + i).min(chars.len());
        chars.insert(at, *ch);
    }
    let display: String = chars.iter().collect();
    if comp_chars.is_empty() {
        return (display, None);
    }
    // composition 在 display 串里的真实字节区间 [start, end)。render 据此画下划线，
    // 对齐预提交文本本身。
    let comp_end_char = insert_start_char + comp_chars.len();
    let comp_start_byte = char_index_to_byte(&display, insert_start_char);
    let comp_end_byte = char_index_to_byte(&display, comp_end_char);
    (display, Some((comp_start_byte, comp_end_byte)))
}

/// 字符索引 → 字节偏移：返回 s 里第 `char_idx` 个字符的起始字节。`char_idx` 超出字符数
/// 时返回 `s.len()`（串末尾）。用于 [`display_value`] 把 composition 的字符区间换算成
/// 字节区间（multi-byte 字符下字符数 ≠ 字节数）。
fn char_index_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// `-webkit-text-security` 的掩码字符（disc ● / circle ○ / square ■）。
pub fn mask_char(sec: crate::style::resolved::TextSecurity) -> char {
    match sec {
        crate::style::resolved::TextSecurity::Disc => '●',
        crate::style::resolved::TextSecurity::Circle => '○',
        crate::style::resolved::TextSecurity::Square => '■',
    }
}

/// [`display_value`] 的掩码变体：显示字符逐个替换为掩码字符（换行保留），1 char : 1 char
/// ——字符数不变、字节宽变化。`comp_range` 照常返回，但换算到掩码串的字节空间。
pub fn display_value_masked(e: &EditState, mask: Option<char>) -> (String, Option<(usize, usize)>) {
    let (display, comp) = display_value(e);
    let Some(m) = mask else {
        return (display, comp);
    };
    let masked: String = display
        .chars()
        .map(|c| if c == '\n' { c } else { m })
        .collect();
    // comp 区间按字符数换算（display 与 masked 同字符数，字节宽不同）。
    let comp_masked = comp.map(|(s, end)| {
        let cs = display[..s].chars().count();
        let ce = display[..end].chars().count();
        (
            char_index_to_byte(&masked, cs),
            char_index_to_byte(&masked, ce),
        )
    });
    (masked, comp_masked)
}

/// value 字节偏移 → 掩码显示串字节偏移（按字符数换算；掩码 1:1 保字符数）。
/// 越界/中间字节输入钳到最近字符边界（调用方 cursor/selection 按构造已对齐，防御性兜底）。
/// composition 拼进 display 时映射按字符数近似（password 字段 IME 组合期是边角；
/// 有 composition 的 caret 定位走 comp_range 锚点路径，不经此换算）。
pub fn value_to_display_byte(value: &str, display: &str, off: usize) -> usize {
    let mut c = off.min(value.len());
    while !value.is_char_boundary(c) {
        c -= 1;
    }
    let n = value[..c].chars().count();
    char_index_to_byte(display, n)
}

/// 掩码显示串字节偏移 → value 字节偏移（[`value_to_display_byte`] 的反向）。
pub fn display_to_value_byte(display: &str, value: &str, off: usize) -> usize {
    let mut c = off.min(display.len());
    while !display.is_char_boundary(c) {
        c -= 1;
    }
    let n = display[..c].chars().count();
    char_index_to_byte(value, n)
}

/// 光标闪烁周期（秒）。stage tick 每帧累计，周期到翻转 cursor_visible。
/// 0.7s 对齐常见平台光标闪烁频率（~1.4Hz 全周期，0.7s 半周期 ON/OFF）。
const CURSOR_BLINK_PERIOD: f32 = 0.7;

/// 推进光标闪烁 timer（每帧由 Stage tick 调用，单一动画时钟不变量）。
///
/// 仅处理 TextField/TextArea/NumberField 的 EditState：
/// - 有焦点：累计 cursor_timer += dt，每 CURSOR_BLINK_PERIOD (0.7s) 翻转 cursor_visible。
/// - 无焦点：cursor_visible = false（隐藏光标）。
///
/// 先取 `scene.focused_node` 副本再可变迭代 controls，避免对 scene 的双借冲突。
pub fn advance_cursor_blink(scene: &mut Scene, dt: f32) {
    let focused = scene.focused_node;
    for (&id, state) in scene.controls.0.iter_mut() {
        if let ControlState::TextField(e)
        | ControlState::TextArea(e)
        | ControlState::NumberField { edit: e, .. } = state
        {
            if Some(id) == focused {
                e.cursor_timer += dt;
                if e.cursor_timer >= CURSOR_BLINK_PERIOD {
                    e.cursor_timer = 0.0;
                    e.cursor_visible = !e.cursor_visible;
                }
            } else {
                e.cursor_visible = false;
            }
        }
    }
}

/// 光标视口跟随的边缘余量（design px）：光标贴窗口边时至少露出这个上下文
/// （近一字符宽），不至于光标压着边框看不见邻字。窄框时收缩防吃掉整个窗口。
const EDIT_VIEW_MARGIN: f32 = 4.0;

/// 单行文本控件的视口跟随（RmlUi `MoveToCursor` 同型）：内容超宽时钳 `EditState.view_x`
/// 使光标留在可视窗 `[view_x, view_x + content_w]` 内（留 [`EDIT_VIEW_MARGIN`] 上下文），
/// 内容不超宽时归零。tick 管线每帧在 `measure_text_controls` 后调（缓存 TextLayout 已新），
/// 所有光标变更路径（键入 / 方向键 / IME / pointer 点击）统一收口于此，无需各路径自查。
///
/// 只动 TextField/NumberField（单行）；TextArea 横向换行不溢出，恒 0。选区拖选跟随
/// （anchor 侧）待拖选交互落地后一并（#49）。IME composition 时光标锁 composition 起点
/// （与 `Stage::cursor_rect` 的 cur 同源），组字期间窗口跟随预提交文本起点。
pub fn sync_edit_view(scene: &mut Scene) {
    use crate::render::resolve_lp;
    let ids: Vec<NodeId> = scene.controls.0.keys().copied().collect();
    for id in ids {
        // 读阶段：不可变借（controls/nodes/text_layouts 皆为 scene 的不相交字段）在此块
        // 内终结，产出目标视口；None = 无须变更。
        let target: Option<f32> = {
            let e = match scene.controls.get(id) {
                Some(ControlState::TextField(e))
                | Some(ControlState::NumberField { edit: e, .. }) => e,
                Some(ControlState::TextArea(_)) => continue, // 单行专属：TextArea 恒 0
                _ => continue,
            };
            let Some(n) = scene.get(id) else { continue };
            let Some(layout) = scene.text_layouts[id.index()].as_ref() else {
                continue; // 首帧未缓存（空 value/placeholder）——无内容可跟随
            };
            let content_w = (n.layout_rect.w
                - resolve_lp(n.style.taffy_style.border.left)
                - resolve_lp(n.style.taffy_style.padding.left)
                - resolve_lp(n.style.taffy_style.border.right)
                - resolve_lp(n.style.taffy_style.padding.right))
            .max(0.0);
            if content_w <= 0.0 {
                continue;
            }
            let mask = n.style.text_security.map(mask_char);
            let (display, comp_range) = display_value_masked(e, mask);
            if display.is_empty() {
                // 无内容 → 视口归零（可能已非 0：刚删空）
                Some(0.0)
            } else {
                let ranges = line_byte_ranges(layout, &display);
                // 滚动上界取 max(行宽, 行末光标 x)：advance 累计与 line.width 有亚像素差，
                // 只用行宽会把行末光标截在窗外 ~0.3px。
                let (end_x, _) = cursor_pixel_x(layout, &ranges, display.len());
                let text_w = layout
                    .lines
                    .first()
                    .map(|l| l.width)
                    .unwrap_or(0.0)
                    .max(end_x);
                let max_vx = (text_w - content_w).max(0.0);
                if max_vx <= 0.0 {
                    // 内容不超宽 → 视口归零（可能已非 0：刚删短）
                    Some(0.0)
                } else {
                    let cur = match comp_range {
                        Some((start, _)) => start,
                        None => value_to_display_byte(&e.value, &display, e.cursor),
                    };
                    let (cx, _li) = cursor_pixel_x(layout, &ranges, cur);
                    let m = EDIT_VIEW_MARGIN.min(content_w * 0.25);
                    let mut vx = e.view_x;
                    if cx < vx + m {
                        vx = cx - m;
                    }
                    if cx > vx + content_w - m {
                        vx = cx + m - content_w;
                    }
                    Some(vx.clamp(0.0, max_vx))
                }
            }
        };
        // 写阶段：读借已终结，可变借落值。
        if let Some(vx) = target {
            if let Some(
                ControlState::TextField(e)
                | ControlState::TextArea(e)
                | ControlState::NumberField { edit: e, .. },
            ) = scene.controls.get_mut(id)
            {
                if e.view_x != vx {
                    e.view_x = vx;
                }
            }
        }
    }
}

// 编辑内核原语。insert_text/delete_char/move_cursor/sanitize_value 是 textinput channel
// 与 control-key 路由的底层原语。它们是纯函数——仅读写 EditState（无 Scene 访问），故可
// 独立单测。读写光标/锚点后由调用方决定是否同步渲染。
//
// 不变量：cursor/anchor 必须永远落在合法 UTF-8 字符边界上（char 起始字节）。CJK 字符
// 占 3 字节，若停在中间字节则后续 str slice panic。下面三个边界助手保证所有偏移合法。
//
// max_length 按 UTF-8 字符数计（value.chars().count()），非字节——用户感知「字数」
// 而非内存占用，与 HTML maxlength 语义一致。0 = 无限。
//
// readonly 守卫：insert/delete 在 readonly=true 时 no-op 返 false（照 HTML disabled/readonly）。
//
// 单行 vs 多行：sanitize 按 NodeKind 分派——TextArea 保留换行（删 \r/\t），其余
// （TextField/NumberField/...）删 \n/\r/\t。paste/输入到单行框时换行被滤。

/// 向左找前一个 UTF-8 字符的起始字节（即 idx 左侧那个 char 的开头）。
///
/// backspace 删除左侧字符 / move-cursor 左移时用：cursor 在某 char 之后（落在该 char 的
/// 起始字节上），prev 边界 = 左侧那个 char 的起始字节。idx=0（无前驱）时返回 0。
///
/// 与 [`next_char_boundary`] 对称：后者从 idx+1 向右扫，本函数从 idx-1 向左扫——
/// 若直接从 idx 起扫则 idx 落在边界时会原地返回（delete/move 会 no-op，ASCII 场景全坏）。
fn prev_char_boundary(value: &str, idx: usize) -> usize {
    if idx == 0 {
        return 0;
    }
    let mut i = idx - 1;
    while i > 0 && !value.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// 向右找最近的 UTF-8 字符边界（下一个 char 起始字节，或末尾 len）。
///
/// delete（向前删）用：cursor 在某 char 之前，next 边界 = 该 char 结束字节 = 下一 char 起始。
/// 从 idx+1 起扫（idx 自身可能是边界，但删右侧需跨过当前 char）。
fn next_char_boundary(value: &str, idx: usize) -> usize {
    let mut i = idx + 1;
    while i < value.len() && !value.is_char_boundary(i) {
        i += 1;
    }
    i.min(value.len())
}

/// 把任意字节偏移 clamp 到合法 UTF-8 边界（向左回退到最近 char 起始字节）。
///
/// sanitize_value 在重写 value 后重对齐 cursor/anchor 用——旧偏移可能因 value 变短越界
/// 或落在 char 中间。先 clamp 到 [0, len]，再回退到 char 边界。
pub(super) fn clamp_boundary(value: &str, idx: usize) -> usize {
    let mut i = idx.min(value.len());
    while i > 0 && !value.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// 按 NodeKind 净化字符串：TextArea 保留 `\n`（多行换行），其余控件删 `\n`/`\r`/`\t`。
///
/// 单行输入框（TextField/NumberField/...）不应含换行/制表符——
/// paste 带换行的多行文本进单行框时滤成单行（照 HTML 单行 input 行为）。
/// TextArea 保留 `\n`（用户可手动换行）但仍删 `\r`/`\t`（CR 与 TAB 在文本域内无意义）。
fn sanitize_str(kind: NodeKind, s: &str) -> String {
    // 过滤控制字符（C0 < 0x20 + DEL 0x7f）：IME 通道偶发把 backspace(\b)/其他控制字符
    // 塞进文本输入，不过滤会进 value 渲染成 tofu。TextArea 保留 \n（多行换行）。
    match kind {
        NodeKind::TextArea => s
            .chars()
            .filter(|&c| c == '\n' || (c >= ' ' && c != '\u{7f}'))
            .collect(),
        _ => s.chars().filter(|&c| c >= ' ' && c != '\u{7f}').collect(),
    }
}

/// 在 cursor 处插入文本（若有选区则先删选区再插）。成功改动返回 true，否则 false。
///
/// 步骤：readonly → no-op；sanitize 输入（单行框滤换行）；空串插入 no-op；
/// max_length 校验须在 delete_selection 之前——按「删后长度 = 当前 - 选区 + 新增」算，
/// 超额则干净拒绝（不删选区、不改 value），否则被拒的插入会静默丢掉用户选区；
/// 有选区则 delete_selection；insert_str 后 cursor/anchor 同步到新末尾，
/// 重置光标闪烁 timer（显示光标）。返回 true 表示 value 已变（调用方据此产 change 事件）。
pub fn insert_text(e: &mut EditState, kind: NodeKind, text: &str) -> bool {
    if e.readonly {
        return false;
    }
    let text = sanitize_str(kind, text);
    if text.is_empty() {
        return false;
    }
    // max_length 校验在任何 mutation 之前：post-delete 长度 = 当前字符数 - 选区字符数 + 新增字符数。
    // selection_range 返回的字节区间必落在 char 边界上，可安全切片计字符数。
    if e.max_length > 0 {
        let (sel_b, sel_e) = e.selection_range();
        let sel_chars = e.value[sel_b..sel_e].chars().count();
        let cur = e.value.chars().count();
        let add = text.chars().count();
        if cur - sel_chars + add > e.max_length {
            return false;
        }
    }
    delete_selection(e);
    e.value.insert_str(e.cursor, &text);
    e.cursor += text.len();
    e.anchor = e.cursor;
    e.ideal_cursor_valid = false;
    e.cursor_visible = true;
    e.cursor_timer = 0.0;
    true
}

/// 删除选区 [min(anchor,cursor), max]。无选区（anchor==cursor）时 no-op 返 false。
///
/// replace_range 删区间后 cursor=anchor=选区起点。供 insert_text（先删后插）与
/// delete_char（有选区时退化为删选区）复用。
pub fn delete_selection(e: &mut EditState) -> bool {
    let (b, end) = e.selection_range();
    if b == end {
        return false;
    }
    e.value.replace_range(b..end, "");
    e.cursor = b;
    e.anchor = b;
    true
}

/// 删一个字符。backspace=true 删左（cursor 前），false 删右（cursor 后）。有选区时删选区。
///
/// readonly → no-op。无选区时按方向用 prev/next 边界确定删除区间（保证跨多字节字符不 panic）。
/// 边界 case（cursor 在头/尾且方向越界）no-op 返 false。
pub fn delete_char(e: &mut EditState, _kind: NodeKind, backspace: bool) -> bool {
    if e.readonly {
        return false;
    }
    e.ideal_cursor_valid = false;
    if e.anchor != e.cursor {
        return delete_selection(e);
    }
    if backspace && e.cursor > 0 {
        let nc = prev_char_boundary(&e.value, e.cursor);
        e.value.replace_range(nc..e.cursor, "");
        e.cursor = nc;
        e.anchor = nc;
        true
    } else if !backspace && e.cursor < e.value.len() {
        let end = next_char_boundary(&e.value, e.cursor);
        e.value.replace_range(e.cursor..end, "");
        e.anchor = e.cursor;
        true
    } else {
        false
    }
}

/// 移动光标一个字符。right=true 右移，false 左移。select=true 扩展选区（anchor 不动），
/// 否则折叠（cursor=anchor=新位）。跨越按 UTF-8 字符（非字节），保证停在 char 边界。
///
/// 重置光标闪烁 timer（移动后立显光标）。无返回值（光标移动必生效，无失败语义）。
pub fn move_cursor(e: &mut EditState, _kind: NodeKind, right: bool, select: bool) {
    let nc = if right {
        next_char_boundary(&e.value, e.cursor)
    } else {
        prev_char_boundary(&e.value, e.cursor)
    };
    e.cursor = nc;
    if !select {
        e.anchor = nc;
    }
    e.ideal_cursor_valid = false;
    e.cursor_visible = true;
    e.cursor_timer = 0.0;
}

/// ctrl+Left/Right 词移动（浏览器惯例：forward 跳到词尾后、backward 跳到词首前）。
/// 词 = 连续 `is_alphanumeric` 段（Unicode：CJK 连续段=一词，跳整段——Chrome 同款）；
/// 标点/空白是词间分隔，forward 先跳过分隔再消费词，backward 对称。select 同 move_cursor。
pub fn move_word(e: &mut EditState, right: bool, select: bool) {
    let nc = if right {
        next_word_end(&e.value, e.cursor)
    } else {
        prev_word_start(&e.value, e.cursor)
    };
    e.cursor = nc;
    if !select {
        e.anchor = nc;
    }
    e.ideal_cursor_valid = false;
    e.cursor_visible = true;
    e.cursor_timer = 0.0;
}

/// forward 词尾：跳过当前词间分隔（非词字符），再消费整段词字符——落点是词尾后。
fn next_word_end(s: &str, mut pos: usize) -> usize {
    while pos < s.len() {
        let ch = s[pos..].chars().next().unwrap();
        if ch.is_alphanumeric() {
            break;
        }
        pos += ch.len_utf8();
    }
    while pos < s.len() {
        let ch = s[pos..].chars().next().unwrap();
        if !ch.is_alphanumeric() {
            break;
        }
        pos += ch.len_utf8();
    }
    pos
}

/// backward 词首：跳过身后词间分隔，再消费整段词字符——落点是词首。
fn prev_word_start(s: &str, mut pos: usize) -> usize {
    while pos > 0 {
        let start = prev_char_boundary(s, pos);
        if s[start..].chars().next().unwrap().is_alphanumeric() {
            break;
        }
        pos = start;
    }
    while pos > 0 {
        let start = prev_char_boundary(s, pos);
        if !s[start..].chars().next().unwrap().is_alphanumeric() {
            break;
        }
        pos = start;
    }
    pos
}

/// ctrl+Backspace/Delete 词删除（textfield spec §5.3）：删光标到前/后词边界的区间。
/// 选区非零先删选区（同 delete_char）；到边界无词可删（串首 backspace / 串尾 delete）返 false。
pub fn delete_word(e: &mut EditState, backspace: bool) -> bool {
    if e.readonly {
        return false;
    }
    if e.anchor != e.cursor {
        return delete_selection(e);
    }
    let bound = if backspace {
        prev_word_start(&e.value, e.cursor)
    } else {
        next_word_end(&e.value, e.cursor)
    };
    if bound == e.cursor {
        return false;
    }
    if backspace {
        e.value.replace_range(bound..e.cursor, "");
        e.cursor = bound;
    } else {
        e.value.replace_range(e.cursor..bound, "");
    }
    e.anchor = e.cursor;
    e.ideal_cursor_valid = false;
    true
}

/// TextArea 上下行导航的布局上下文（process_keys 在 controls 可变借外克隆，解借用冲突）。
/// display 是掩码后的显示串（无掩码/无 composition 时与 value 相同）——layout glyphs
/// 按 display 排，一切命中/定位先在 display 字节空间做，再经 value↔display 换算。
pub struct TextNavCtx {
    pub layout: crate::text::layout::TextLayout,
    pub display: String,
}

/// 收集 TextArea 键盘导航所需上下文。无缓存 layout（首帧未 measure）→ None（调用方 no-op）。
pub fn text_nav_context(scene: &Scene, id: NodeId) -> Option<TextNavCtx> {
    let layout = scene.text_layouts[id.index()].as_ref().cloned()?;
    let mask = scene
        .get(id)
        .and_then(|n| n.style.text_security)
        .map(mask_char);
    let display = match scene.controls.get(id) {
        Some(
            ControlState::TextField(e)
            | ControlState::TextArea(e)
            | ControlState::NumberField { edit: e, .. },
        ) => display_value_masked(e, mask).0,
        _ => return None,
    };
    Some(TextNavCtx { layout, display })
}

/// TextArea 上下行导航（视觉行 = 布局折行走行，浏览器对齐）。sticky x：ideal 有效则
/// 复用，无效（一切移动光标的原语都会使其失效）则先从当前光标像素重算再跳——防短行
/// 中转截断列位。select 同 move_cursor。越顶/底行 no-op（仍重置闪烁 timer 照浏览器）。
pub fn move_vertical(e: &mut EditState, ctx: &TextNavCtx, down: bool, select: bool) {
    let ranges = line_byte_ranges(&ctx.layout, &ctx.display);
    let disp_off = value_to_display_byte(&e.value, &ctx.display, e.cursor);
    let (cur_x, cur_line) = cursor_pixel_x(&ctx.layout, &ranges, disp_off);
    if !e.ideal_cursor_valid {
        e.ideal_cursor_x = cur_x;
        e.ideal_cursor_valid = true;
    }
    let target = if down {
        (cur_line + 1).min(ctx.layout.lines.len() - 1)
    } else {
        cur_line.saturating_sub(1)
    };
    if target != cur_line {
        // 行中点 y 强制命中目标行（hit_byte_offset 的 y 扫描单调，中点必落本行）。
        let y = ctx.layout.lines[target].y + ctx.layout.lines[target].height * 0.5;
        let disp_hit = hit_byte_offset(&ctx.layout, &ranges, e.ideal_cursor_x, y);
        let off = clamp_boundary(
            &e.value,
            display_to_value_byte(&ctx.display, &e.value, disp_hit),
        );
        e.cursor = off;
        if !select {
            e.anchor = off;
        }
    }
    e.cursor_visible = true;
    e.cursor_timer = 0.0;
}

/// TextArea 行级 Home/End（裸键=当前视觉行首/尾；ctrl+Home/End 文档级在调用方直写）。
/// 行尾语义：该行区间含终结 \n 时退一格——光标停在 \n 前（视觉上行尾），与浏览器一致。
pub fn line_home_end(e: &mut EditState, ctx: &TextNavCtx, home: bool, select: bool) {
    let ranges = line_byte_ranges(&ctx.layout, &ctx.display);
    let disp_off = value_to_display_byte(&e.value, &ctx.display, e.cursor);
    let (_, cur_line) = cursor_pixel_x(&ctx.layout, &ranges, disp_off);
    let (start, end) = ranges[cur_line];
    let disp_hit = if home {
        start
    } else if end > start && ctx.display.as_bytes().get(end - 1) == Some(&b'\n') {
        end - 1
    } else {
        end
    };
    let off = clamp_boundary(
        &e.value,
        display_to_value_byte(&ctx.display, &e.value, disp_hit),
    );
    e.cursor = off;
    if !select {
        e.anchor = off;
    }
    e.ideal_cursor_valid = false;
    e.cursor_visible = true;
    e.cursor_timer = 0.0;
}

/// 按 NodeKind 净化 EditState.value（重写 value + 重对齐 cursor/anchor 到合法边界）。
///
/// 供 paste/FFI 设值后调用：外部注入的 value 可能含单行框不该有的换行，或 cursor 落在
/// char 中间。sanitize_str 重写 value 后用 clamp_boundary 把 cursor/anchor 回退到
/// 合法 char 边界（value 变短后旧偏移可能越界，clamp 到 [0,len] + char 边界）。
pub fn sanitize_value(e: &mut EditState, kind: NodeKind) {
    e.value = sanitize_str(kind, &e.value);
    e.cursor = clamp_boundary(&e.value, e.cursor);
    e.anchor = clamp_boundary(&e.value, e.anchor);
}

/// 推一条 EVT_VALUE_CHANGED@node（文本框值变更后调用）。payload 无额外字段——
/// 文本值变更不报新值进 EventRecord（文本框的 value 走 Get<T> 直读 ControlState，
/// 与 Slider 的 x=新值 不同）。对照 EVT_VALUE_CHANGED 现有约定：Slider 用 x 装新 float，
/// 文本框语义是「值已变」，业务通过 API 读当前值。
pub fn emit_value_changed(out: &mut Vec<EventRecord>, node: NodeId) {
    out.push(EventRecord {
        node_id: node.0,
        event_type: EVT_VALUE_CHANGED,
        click_count: 0,
        pad: [0, 0],
        touch_id: 0,
        x: 0.0,
        y: 0.0,
        dx: 0.0,
        dy: 0.0,
    });
}

/// 回车键处理（单行/多行分派）。
///
/// 单行框（TextField/NumberField/...）→ 不改 value，推一条 EVT_SUBMITTED@node
/// （照 HTML 单行 input Enter=表单提交语义）。TextArea → 插入 `\n`（insert_text）+
/// ValueChanged；不发 Submitted（多行框 Enter 是换行，非提交）。
///
/// readonly 单行框仍发 Submitted（提交是意图表达，不受只读限制）；readonly TextArea 的
/// insert_text 自身 no-op 返 false（不发 ValueChanged）。
pub fn line_break(e: &mut EditState, kind: NodeKind, out: &mut Vec<EventRecord>, node: NodeId) {
    match kind {
        NodeKind::TextArea => {
            if insert_text(e, kind, "\n") {
                emit_value_changed(out, node);
            }
        }
        _ => {
            out.push(EventRecord {
                node_id: node.0,
                event_type: EVT_SUBMITTED,
                click_count: 0,
                pad: [0, 0],
                touch_id: 0,
                x: 0.0,
                y: 0.0,
                dx: 0.0,
                dy: 0.0,
            });
        }
    }
}

// IME 渠道。后端读平台 IME 的 compositionString 回灌 core——set_composition 存进
// EditState.composition，commit_composition 落定进 value。显示侧由 [`display_value`] 把
// composition 拼进显示文本（measure/render 同源），下划线由 render 的 composition 分支画。
//
// composition.pos 是基于原始 value 的字节偏移（光标在 value 中的位置）。提交时把光标定位到
// composition.pos，再 insert_text 落定（insert_text 自带选区删除 + sanitize + max_length
// 校验，复用它保持与普通字符输入一致的落定语义）。

/// 设置 composition（后端读 IME compositionString 回灌）。pos 钳到 value 合法字节边界。
///
/// **空串 = 取消 composition**：`text` 为空时清掉 `e.composition`（设为 None），与 FFI 文档
/// 约定（传空串取消正在进行的 composition）一致，不存空 composition（否则 commit/render 会
/// 拿到一个零宽 composition，下游边界判断 `comp_end > comp_start` 退化）。
///
/// 非空 text 时重置光标闪烁 timer（显示光标）——与编辑原语（insert_text/move_cursor）一致，
/// 让用户在输入过程中能看到光标位置。连续 set_composition（每帧更新 composition string）是常态。
pub fn set_composition(e: &mut EditState, text: &str, pos: usize) {
    if text.is_empty() {
        e.composition = None;
        return;
    }
    let mut p = pos.min(e.value.len());
    // 钳到 UTF-8 字符边界：后端传的 pos 可能落在多字节字符中间，直接存会让下游
    // value[..pos] 切片 panic。回退到最近的 char 起始字节。
    while p > 0 && !e.value.is_char_boundary(p) {
        p -= 1;
    }
    e.composition = Some(Composition {
        text: text.to_string(),
        pos: p,
    });
    e.ideal_cursor_valid = false;
    e.cursor_visible = true;
    e.cursor_timer = 0.0;
}

/// 提交 composition：把 composition.text 落定进 value（在 composition.pos 插入）。
///
/// 光标先定位到 composition.pos（并折叠选区），再调 insert_text 插 composition.text——
/// 复用 insert_text 的选区删除/sanitize/max_length 校验逻辑，保持与普通字符输入一致的
/// 落定语义。有 composition 且 value 改变时返 true，无 composition 返 false。
pub fn commit_composition(e: &mut EditState, kind: NodeKind) -> bool {
    let Some(comp) = e.composition.take() else {
        return false;
    };
    e.cursor = comp.pos;
    e.anchor = comp.pos;
    insert_text(e, kind, &comp.text)
}
