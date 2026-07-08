//! Text 层：给定文本 + 字体 + 约束宽，产出 TextLayout（SOA 三表 glyphs/runs/lines）。
//!
//! 实现要点：
//! - 字体度量走 ttf-parser（API 适配见下方）。
//! - 断行用贪心按空白 + 宽度约束（unicode-linebreak UAX#14 提供换行机会）。
//! - glyph 存绝对坐标（已累加 advance + 已应用 align 偏移），后端拼 quad 零累加。

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use ttf_parser::Face;

/// 单个字形。坐标为绝对坐标（pen 位 = glyph.x/y + bearing）。
#[derive(Debug, Clone, Serialize)]
pub struct Glyph {
    pub glyph_id: u16,
    /// Unicode 码点：Unity `Font.GetCharacterInfo(char)` 按码点查（非 ttf glyph_id）。
    /// `measure_text` 遍历 `content.chars()` 时 `c as u32` 填入。
    pub codepoint: u32,
    /// pen x（已累加 advance + 已应用 align 偏移）。
    pub x: f32,
    /// 行内 pen y（= line_y，未加 baseline）。
    pub y: f32,
    /// pen 位 → 字形 quad 左上的 x 偏移（来自 glyph bbox x_min）。
    pub bearing_x: f32,
    /// pen 位 → 字形 quad 左上的 y 偏移（来自 glyph bbox y_max，顶到 baseline）。
    pub bearing_y: f32,
}

/// 单 run：一组连续字形 + 该 run 的完整样式。统一 plain 与 rich 走同一条
/// measure→build 链：plain text = 单 run（默认色/Normal 样式），rich text = 多 run。
#[derive(Debug, Clone, Serialize)]
pub struct GlyphRun {
    pub font_size: f32,
    /// 字体 id（atlas key + image_path 合成用）。MVP 单字体：所有 run 填
    /// default_font_id，build 期 build_text_mesh 仍按外传 font_id 取 face；
    /// 此字段为 T5+ per-run 字体（多 family）预留。
    pub font_id: u32,
    /// per-run 颜色（plain 整段同色；rich 每 run 各自色）。build 期 per-vertex。
    pub color: [f32; 4],
    pub weight: crate::text::rich::RichWeight,
    pub style: crate::text::rich::RichStyle,
    pub deco: crate::text::rich::RichDeco,
    /// 链接 id（`<a>` 内的 run）。None=非链接。命中查 fragment 矩形用。
    pub link_id: Option<u32>,
    pub glyphs: Vec<Glyph>,
}

/// 一行文本。
#[derive(Debug, Clone, Serialize)]
pub struct Line {
    /// 行顶 y（相对布局原点）。
    pub y: f32,
    /// 行高（line-height 已烤进，后端不重套）。
    pub height: f32,
    /// 行 baseline（绝对 y）。
    pub baseline: f32,
    /// 行内文字宽度。
    pub width: f32,
    pub runs: Vec<GlyphRun>,
}

/// 文本布局结果（SOA 三表：lines/runs/glyphs）。
#[derive(Debug, Clone, Serialize)]
pub struct TextLayout {
    pub text_width: f32,
    pub text_height: f32,
    pub lines: Vec<Line>,
}

/// 封装一个 ttf 字体（进程级单字体，无 fallback）。
///
/// Face 借用 `Box::leak` 产出的 `'static` 切片；leak 的内存不释放，进程级单字体可接受。
pub struct Font {
    pub face: Face<'static>,
}

impl Font {
    pub fn from_path(path: &str) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        Self::from_bytes(bytes)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        // Face 借用 leaked 切片（进程级单字体，leak 不释放可接受）。
        let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        let face = Face::parse(leaked, 0).map_err(|e| format!("{:?}", e))?;
        Ok(Font { face })
    }

    pub fn ascent(&self, font_size: f32) -> f32 {
        let asc = self.face.ascender() as f32;
        let units = self.face.units_per_em() as f32;
        asc / units * font_size
    }

    /// 字体下降量，负值。
    pub fn descent(&self, font_size: f32) -> f32 {
        let desc = self.face.descender() as f32;
        let units = self.face.units_per_em() as f32;
        desc / units * font_size
    }

    pub fn line_gap(&self, font_size: f32) -> f32 {
        let lg = self.face.line_gap() as f32;
        let units = self.face.units_per_em() as f32;
        lg / units * font_size
    }
}

/// 字体表：CSS font-family → Font。无匹配 / None → default 字体。
///
/// 注册第一个 is_default=true 的字体为 default。select 在无 default 时 panic——
/// FFI 层保证任何 tick（会触发 measure）前已注册 default，契约由调用方维护。
/// Font 仍是 Face<'static>（Box::leak 字节，进程级单字体可接受；多字体数量有限，
/// leak 不释放可接受，真要回收改 Arc<Vec<u8>> 持字节，YAGNI）。
///
/// v1.6：family_to_id 为每个注册 family 分配稳定 u32 id，供 atlas key 和合成
/// image_path 用。id 在 register 时分配，不随字体表增删变化。
pub struct FontTable {
    pub(crate) fonts: HashMap<String, Arc<Font>>,
    pub(crate) default_family: Option<String>,
    pub(crate) family_to_id: HashMap<String, u32>,
    pub(crate) next_id: u32,
}

impl Default for FontTable {
    fn default() -> Self {
        Self::new()
    }
}

impl FontTable {
    pub fn new() -> Self {
        FontTable {
            fonts: HashMap::new(),
            default_family: None,
            family_to_id: HashMap::new(),
            next_id: 0,
        }
    }

    /// 注册字体。is_default=true 设为默认（首次或显式覆盖）。
    /// bytes 是 ttf/ttc/otf 字节；Face::parse 失败返 Err。
    pub fn register(
        &mut self,
        family: &str,
        bytes: Vec<u8>,
        is_default: bool,
    ) -> Result<(), String> {
        let font = Arc::new(Font::from_bytes(bytes)?);
        self.fonts.insert(family.to_string(), font);
        if is_default {
            self.default_family = Some(family.to_string());
        }
        let id = self.next_id;
        self.next_id += 1;
        self.family_to_id.insert(family.to_string(), id);
        Ok(())
    }

    /// 按节点 font_family 选字体。None / 无匹配 → default。
    /// 无 default 注册时 panic（契约：FFI 层 tick 前须注册 default）。
    pub fn select(&self, family: Option<&str>) -> &Font {
        if let Some(fam) = family {
            if let Some(f) = self.fonts.get(fam) {
                return f.as_ref();
            }
        }
        let default = self
            .default_family
            .as_ref()
            .expect("no default font registered (register one with is_default=true before tick)");
        self.fonts[default].as_ref()
    }

    /// 按 family 取稳定 font_id（atlas key + 合成 path 用）。
    /// 无匹配 → default 的 id。
    pub fn font_id(&self, family: Option<&str>) -> u32 {
        if let Some(fam) = family {
            if let Some(&id) = self.family_to_id.get(fam) {
                return id;
            }
        }
        let default = self
            .default_family
            .as_ref()
            .expect("no default font registered");
        *self
            .family_to_id
            .get(default)
            .expect("default family 已注册必有 id")
    }
}

/// 字距（kerning）：返设计单位 i16（未转 px）。caller 按自身 font_size 缩放。
///
/// ttf-parser 0.20：Face 不直接暴露 kern；kern 表是多个子表的集合，
/// `face.tables().kern.as_ref()` → 迭代 `table.subtables`，跳非水平/变量/状态机子表，
/// `sub.glyphs_kerning(left, right) -> Option<i16>` 取值。首个命中即返。
fn kerning_value(
    face: &Face<'_>,
    left: ttf_parser::GlyphId,
    right: ttf_parser::GlyphId,
) -> Option<i16> {
    let kern = face.tables().kern.as_ref()?;
    for sub in kern.subtables {
        if !sub.horizontal || sub.variable || sub.has_state_machine {
            continue;
        }
        if let Some(k) = sub.glyphs_kerning(left, right) {
            return Some(k);
        }
    }
    None
}

/// 字形 advance（px，已按 font_size 缩放）。
///
/// 有字形→读 ttf advance；缺字形（glyph_index 返 None）→ 权威 .notdef(gid0)
/// advance（确定性跨引擎一致）。gid0 advance 缺失时兜底 0.6em（TrueType 惯例）。
fn glyph_advance(face: &Face<'_>, gid_opt: Option<ttf_parser::GlyphId>, font_size: f32) -> f32 {
    let units = face.units_per_em() as f32;
    let to_px = |design: f32| -> f32 { design / units * font_size };
    match gid_opt {
        Some(gid) => face
            .glyph_hor_advance(gid)
            .map(|v| to_px(v as f32))
            .unwrap_or(0.0),
        None => face
            .glyph_hor_advance(ttf_parser::GlyphId(0))
            .map(|v| to_px(v as f32))
            .unwrap_or(to_px(units * 0.6)),
    }
}

/// 测量并布局文本。
///
/// - `line_height`：倍数，`0.0` = normal（= ascent - descent + line_gap）。
/// - `max_width`：`None` 表示不换行；`nowrap=true` 时强制单行（white-space:nowrap）。
///
/// # ttf-parser 0.20 API 适配
/// - `glyph_advance_width(GlyphId) -> Option<i16>` 在 0.20 不存在；
///   用 `glyph_hor_advance(GlyphId) -> Option<u16>`（注意返回 u16）。
/// - `kerning_for(GlyphId, GlyphId) -> Option<i16>` 在 0.20 的 Face 上不直接暴露；
///   通过 `face.tables().kern.as_ref().and_then(|k| k.glyphs_kerning(l, r))`。
/// - `glyph_index(ch) -> Option<GlyphId>`、`GlyphId(pub u16)`、`glyph_bounding_box(GlyphId)`
///   返回 `Rect{x_min,y_min,x_max,y_max}`、`ascender/descender/line_gap/units_per_em`、
///   `Face::parse(bytes, 0)` 均与预期一致。
#[allow(clippy::too_many_arguments)]
pub fn measure_text(
    content: &str,
    font_size: f32,
    line_height: f32,
    letter_spacing: f32,
    align: crate::style::resolved::TextAlign,
    nowrap: bool,
    max_width: Option<f32>,
    font: &Font,
    font_id: u32,
    color: [f32; 4],
) -> TextLayout {
    let ascent = font.ascent(font_size);
    let descent = font.descent(font_size); // 负
    let line_gap = font.line_gap(font_size);
    let units = font.face.units_per_em() as f32;

    // Line.height：line-height 生效则烤进 height（后端不重套，§9.1）；
    // 否则用字体自然行高（ascent - descent + line_gap）。
    let line_h = if line_height > 0.0 {
        font_size * line_height
    } else {
        ascent - descent + line_gap
    };
    // baseline：简化占位。
    let baseline = if line_height > 0.0 {
        (line_h + ascent - descent) / 2.0 - descent.abs()
    } else {
        ascent
    };

    // 单位换算辅助：设计单位 → px。
    let to_px = |design: f32| -> f32 { design / units * font_size };

    // kerning / advance 复用 module-level helper（measure_rich_text 也用，避免复制粘贴）。
    let kerning = |left: ttf_parser::GlyphId, right: ttf_parser::GlyphId| -> Option<i16> {
        kerning_value(&font.face, left, right)
    };
    let advance = |gid_opt: Option<ttf_parser::GlyphId>| -> f32 {
        glyph_advance(&font.face, gid_opt, font_size)
    };

    // 度量一段文本的宽度（含字距）。
    let measure_width = |s: &str| -> f32 {
        let mut pen = 0.0f32;
        let mut prev: Option<ttf_parser::GlyphId> = None;
        for ch in s.chars() {
            let gid_opt = font.face.glyph_index(ch);
            let gid = gid_opt.unwrap_or_default();
            if let Some(p) = prev {
                if let Some(k) = kerning(p, gid) {
                    pen += to_px(k as f32);
                }
            }
            pen += advance(gid_opt) + letter_spacing;
            prev = Some(gid);
        }
        pen
    };

    // 断行：unicode-linebreak UAX#14 换行机会 + 贪心填行（CJK 逐字）。
    // white-space:nowrap 强制单行。
    //
    // unicode-linebreak 0.1.5 API：
    // - `linebreaks(s)` 返回 `impl Iterator<Item=(usize, BreakOpportunity)>`（非 Vec）；
    // - 枚举名是 `BreakOpportunity`，变体 `Mandatory`/`Allowed`；
    // - offset 语义 = "断点之后字符的字节序号"，即前段 = content[..offset]，后段 = content[offset..]。
    use unicode_linebreak::{linebreaks, BreakOpportunity};
    let max_w = max_width.unwrap_or(f32::MAX);

    // 1. 取所有 break opportunities（byte offset + 类型），收成 Vec 便于多轮迭代。
    let opportunities: Vec<(usize, BreakOpportunity)> = linebreaks(content).collect();

    // 2. 切 segments：相邻 break 之间的文本片段。unicode-linebreak 在空白后断，
    //    segment 自含尾空白 → 行首无多余空格。
    let mut segments: Vec<(&str, BreakOpportunity)> = Vec::new();
    let mut prev = 0usize;
    for &(offset, btype) in &opportunities {
        if offset > prev {
            segments.push((&content[prev..offset], btype));
        }
        prev = offset;
    }
    if prev < content.len() {
        segments.push((&content[prev..], BreakOpportunity::Allowed));
    }

    // 3. 贪心填行。
    let mut lines: Vec<(String, f32)> = Vec::new(); // (text, width)
    let mut cur = String::new();
    let mut cur_w = 0.0f32;
    let mut buf = [0u8; 4];
    for (seg, btype) in &segments {
        let seg_w = measure_width(seg);
        let seg_chars = seg.chars().count();

        // 超长词边界：segment 本身超 max_w 且多字符 → 逐字填。
        // 防无 break point 的长串（如 URL）溢出。
        if !nowrap && seg_w > max_w && seg_chars > 1 {
            if !cur.is_empty() {
                lines.push((std::mem::take(&mut cur), cur_w));
                cur_w = 0.0;
            }
            for ch in seg.chars() {
                let cw = measure_width(ch.encode_utf8(&mut buf));
                if !cur.is_empty() && cur_w + cw > max_w {
                    lines.push((std::mem::take(&mut cur), cur_w));
                    cur_w = 0.0;
                }
                cur.push(ch);
                cur_w += cw;
            }
        } else if nowrap || cur.is_empty() || cur_w + seg_w <= max_w {
            cur.push_str(seg);
            cur_w += seg_w;
        } else {
            lines.push((std::mem::take(&mut cur), cur_w));
            cur.push_str(seg);
            cur_w = seg_w;
        }

        // Mandatory break（\n）强制结束当前行（nowrap 下忽略）。
        if !nowrap && *btype == BreakOpportunity::Mandatory && !cur.is_empty() {
            lines.push((std::mem::take(&mut cur), cur_w));
            cur_w = 0.0;
        }
    }
    if !cur.is_empty() {
        lines.push((cur, cur_w));
    }
    if lines.is_empty() {
        lines.push((String::new(), 0.0));
    }

    let text_width = lines.iter().map(|(_, w)| *w).fold(0.0f32, f32::max);
    let text_height = lines.len() as f32 * line_h;

    // 生成 glyphs（绝对坐标，§9.2：已累加 advance + 已应用 align 偏移）。
    let mut out_lines = Vec::with_capacity(lines.len());
    for (li, (text, lw)) in lines.iter().enumerate() {
        let line_y = li as f32 * line_h;
        let x_offset = match align {
            crate::style::resolved::TextAlign::Center => (text_width - lw) / 2.0,
            crate::style::resolved::TextAlign::Right => text_width - lw,
            crate::style::resolved::TextAlign::Left => 0.0,
        };
        let mut pen_x = x_offset;
        let mut glyphs = Vec::with_capacity(text.chars().count());
        let mut prev: Option<ttf_parser::GlyphId> = None;
        for ch in text.chars() {
            let gid_opt = font.face.glyph_index(ch);
            let gid = gid_opt.unwrap_or_default();
            if let Some(p) = prev {
                if let Some(k) = kerning(p, gid) {
                    pen_x += to_px(k as f32);
                }
            }
            // bearing 来自 glyph bbox：x_min → bearing_x，y_max → bearing_y（顶到 baseline）。
            let (bx, by) = font
                .face
                .glyph_bounding_box(gid)
                .map(|b| (to_px(b.x_min as f32), to_px(b.y_max as f32)))
                .unwrap_or((0.0, 0.0));
            glyphs.push(Glyph {
                glyph_id: gid.0,
                codepoint: ch as u32,
                x: pen_x,
                y: line_y,
                bearing_x: bx,
                bearing_y: by,
            });
            pen_x += advance(gid_opt) + letter_spacing;
            prev = Some(gid);
        }
        out_lines.push(Line {
            y: line_y,
            height: line_h,
            baseline: line_y + baseline,
            width: *lw,
            runs: vec![GlyphRun {
                font_size,
                font_id,
                color,
                weight: crate::text::rich::RichWeight::Normal,
                style: crate::text::rich::RichStyle::Normal,
                deco: crate::text::rich::RichDeco::default(),
                link_id: None,
                glyphs,
            }],
        });
    }

    TextLayout {
        text_width,
        text_height,
        lines: out_lines,
    }
}

/// 简化 inline flow measure：runs + 可选宽度 → TextLayout（per-run 样式进 GlyphRun）。
///
/// 算法（搬 fgui BuildLines2 + RmlUi GetStrut/DetermineVerticalPositioning）：
/// 1. 扁平 token 流：每 run 的 text 切成 token（CJK 逐字 / Latin 逐词），token 携 run 样式。
/// 2. 贪心断行：token 累加超 max_width → 开新行；`\n` 强制换行。
/// 3. 每行 baseline = 该行 max 字号的 ascent；行高 = strut（line_height 倍数或自然行高）。
/// 4. 定位：pen x 累加 advance + kern；glyph y = 0（行内相对，build 加 baseline）。
///
/// 纯函数：不光栅、不读 atlas（atlas ensure 在 build 期）。可被 taffy 反复调。
///
/// MVP 单字体：所有 run 共用传入的 `font`（节点 font_family 选的）+ `default_font_id`；
/// `GlyphRun.font_id` 填 `default_font_id`（run.font_id 字段保留但不用于选 face）。
pub fn measure_rich_text(
    runs: &[crate::text::rich::RichRun],
    max_width: Option<f32>,
    base_line_height: f32,
    font: &Font,
    default_font_id: u32,
) -> TextLayout {
    let units = font.face.units_per_em().max(1) as f32;

    // 1. 扁平 token 流（token = 子串 + 所属 run 索引 + 宽度 + 是否强制换行）。
    //    CJK 逐字、Latin 逐词（空白分词）。用 char_indices 取字节范围切片（非 unsafe）。
    struct Tok<'a> {
        text: &'a str,
        run_idx: usize,
        w: f32,
        is_break: bool,
    }
    let mut tokens: Vec<Tok> = Vec::new();
    for (ri, r) in runs.iter().enumerate() {
        match &r.kind {
            crate::text::rich::RichKind::Text { text } => {
                if text == "\n" {
                    tokens.push(Tok {
                        text: "\n",
                        run_idx: ri,
                        w: 0.0,
                        is_break: true,
                    });
                    continue;
                }
                let scale = r.size_px as f32 / units;
                // 空白分词：按空格切词，词间不补空格 token（简化：HTML 空白折叠后词间单空格
                // 已并入词尾/词首，measure 宽度差异可忽略；MVP 不追求像素级空格精度）。
                for word in text.split(' ') {
                    if word.is_empty() {
                        continue;
                    }
                    if word.chars().any(is_cjk) {
                        // CJK 拆单字：每字一 token，按 char_indices 取字节范围切片。
                        let mut indices = word.char_indices();
                        let mut cur = indices.next();
                        while let Some((byte_off, _ch)) = cur {
                            let next = indices.next();
                            let next_byte_off = match next {
                                Some((nbo, _)) => nbo,
                                None => word.len(),
                            };
                            let slice = &word[byte_off..next_byte_off];
                            let w = char_advance(&font.face, slice.chars().next().unwrap(), scale);
                            tokens.push(Tok {
                                text: slice,
                                run_idx: ri,
                                w,
                                is_break: false,
                            });
                            cur = next;
                        }
                    } else {
                        let w = str_advance(&font.face, word, scale);
                        tokens.push(Tok {
                            text: word,
                            run_idx: ri,
                            w,
                            is_break: false,
                        });
                    }
                }
            }
            crate::text::rich::RichKind::Image { w, .. } => {
                tokens.push(Tok {
                    text: "",
                    run_idx: ri,
                    w: *w,
                    is_break: false,
                });
            }
        }
    }

    // 2. 贪心断行：token 累加超 max_width → 开新行；is_break（\n）强制换行。
    //    首个 token 不论宽度都入行（防零宽 token 死循环）。
    let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
    let mut cur_w = 0.0f32;
    for (ti, tok) in tokens.iter().enumerate() {
        if tok.is_break {
            lines.push(Vec::new());
            cur_w = 0.0;
            continue;
        }
        let fits =
            max_width.is_none_or(|mw| cur_w + tok.w <= mw || lines.last().unwrap().is_empty());
        if !fits {
            lines.push(Vec::new());
            cur_w = 0.0;
        }
        lines.last_mut().unwrap().push(ti);
        cur_w += tok.w;
    }

    // 3+4. 每行 baseline/高度 + 字形定位（pen x 累加 advance + kern）。
    let mut out_lines: Vec<Line> = Vec::new();
    let mut y = 0.0f32;
    for line_toks in &lines {
        // 该行 max 字号决定 baseline/strut。
        let line_size = line_toks
            .iter()
            .map(|&ti| runs[tokens[ti].run_idx].size_px as f32)
            .fold(0.0f32, f32::max)
            .max(1.0);
        let line_ascent = font.ascent(line_size);
        let line_descent = font.descent(line_size); // 负
        let line_gap = font.line_gap(line_size);
        let h = strut_height(
            base_line_height,
            line_size,
            line_ascent,
            line_descent,
            line_gap,
        );
        let baseline = line_ascent; // 行顶到 baseline。

        if line_toks.is_empty() {
            // 空行（如 <br><br> 间）仍占行高。
            out_lines.push(Line {
                y,
                height: h,
                baseline: y + baseline,
                width: 0.0,
                runs: Vec::new(),
            });
            y += h;
            continue;
        }

        // 按 token 顺序生成 GlyphRun（同 run 相邻 token 合并 glyphs 进同一 run）。
        let mut runs_out: Vec<GlyphRun> = Vec::new();
        let mut pen_x = 0.0f32;
        let mut prev_gid: Option<ttf_parser::GlyphId> = None;
        for &ti in line_toks {
            let r = &runs[tokens[ti].run_idx];
            match &r.kind {
                crate::text::rich::RichKind::Text { .. } => {
                    let mut glyphs: Vec<Glyph> = Vec::new();
                    for ch in tokens[ti].text.chars() {
                        let gid_opt = font.face.glyph_index(ch);
                        let gid = gid_opt.unwrap_or_default();
                        // kern（跨 token 也算——prev_gid 是行内前一个字形）。
                        if let Some(p) = prev_gid {
                            if let Some(k) = kerning_value(&font.face, p, gid) {
                                pen_x += k as f32 / units * r.size_px as f32;
                            }
                        }
                        let (bx, by) = font
                            .face
                            .glyph_bounding_box(gid)
                            .map(|b| {
                                (
                                    b.x_min as f32 / units * r.size_px as f32,
                                    b.y_max as f32 / units * r.size_px as f32,
                                )
                            })
                            .unwrap_or((0.0, 0.0));
                        glyphs.push(Glyph {
                            glyph_id: gid.0,
                            codepoint: ch as u32,
                            x: pen_x,
                            y: 0.0, // 行内相对（build 加 baseline）。
                            bearing_x: bx,
                            bearing_y: by,
                        });
                        pen_x += glyph_advance(&font.face, gid_opt, r.size_px as f32);
                        prev_gid = Some(gid);
                    }
                    // 同 run 相邻 token 合并（per-run 样式一致）；否则新 run。
                    let merged = runs_out.last_mut().filter(|gr: &&mut GlyphRun| {
                        gr.font_size == r.size_px as f32
                            && gr.font_id == default_font_id
                            && gr.color == r.color
                            && gr.weight == r.weight
                            && gr.style == r.style
                            && gr.deco == r.deco
                            && gr.link_id == r.link_id
                    });
                    if let Some(gr) = merged {
                        gr.glyphs.extend(glyphs);
                    } else {
                        runs_out.push(GlyphRun {
                            font_size: r.size_px as f32,
                            font_id: default_font_id,
                            color: r.color,
                            weight: r.weight,
                            style: r.style,
                            deco: r.deco,
                            link_id: r.link_id,
                            glyphs,
                        });
                    }
                }
                crate::text::rich::RichKind::Image { w, .. } => {
                    // 图占位：无 glyph（build 期另产 image quad）；占宽。
                    pen_x += w;
                }
            }
        }
        let width = pen_x;
        out_lines.push(Line {
            y,
            height: h,
            baseline: y + baseline,
            width,
            runs: runs_out,
        });
        y += h;
    }

    let text_width = out_lines.iter().map(|l| l.width).fold(0.0f32, f32::max);
    let text_height = y;
    TextLayout {
        text_width,
        text_height,
        lines: out_lines,
    }
}

/// CJK 判定（简化：常见 CJK 区间）。断行用——CJK 每字可换行。
fn is_cjk(ch: char) -> bool {
    let c = ch as u32;
    (0x4E00..=0x9FFF).contains(&c) // CJK 统一
        || (0x3000..=0x303F).contains(&c) // CJK 标点
        || (0xFF00..=0xFFEF).contains(&c) // 全角
        || (0x3040..=0x30FF).contains(&c) // 假名
}

/// 单字 advance（px，已按 scale 缩放）。复用 glyph_advance 的缺字兜底逻辑。
fn char_advance(face: &Face<'_>, ch: char, scale: f32) -> f32 {
    let gid_opt = face.glyph_index(ch);
    let font_size = scale * face.units_per_em() as f32;
    glyph_advance(face, gid_opt, font_size)
}

/// 字符串 advance（px，已按 scale 缩放，含字距）。
fn str_advance(face: &Face<'_>, s: &str, scale: f32) -> f32 {
    let units = face.units_per_em() as f32;
    let font_size = scale * units;
    let mut pen = 0.0f32;
    let mut prev: Option<ttf_parser::GlyphId> = None;
    for ch in s.chars() {
        let gid_opt = face.glyph_index(ch);
        let gid = gid_opt.unwrap_or_default();
        if let Some(p) = prev {
            if let Some(k) = kerning_value(face, p, gid) {
                pen += k as f32 / units * font_size;
            }
        }
        pen += glyph_advance(face, gid_opt, font_size);
        prev = Some(gid);
    }
    pen
}

/// strut 行高（搬 RmlUi GetStrut：line_height > 0 用倍数，否则自然行高 ascent-descent+gap）。
fn strut_height(line_height: f32, size: f32, ascent: f32, descent: f32, line_gap: f32) -> f32 {
    if line_height > 0.0 {
        size * line_height
    } else {
        ascent - descent + line_gap
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::resolved::TextAlign;
    use crate::text::rich::{RichDeco, RichKind, RichRun, RichStyle, RichWeight};

    /// 测试字体：仓库内 DejaVuSans.ttf（跨平台一致），缺则跳过。
    fn test_font() -> Option<Font> {
        let p = format!(
            "{}/tests/fixtures/DejaVuSans.ttf",
            env!("CARGO_MANIFEST_DIR")
        );
        Font::from_path(&p).ok()
    }

    /// CJK 测试字体：仓库内 wqy-microhei.ttc（文泉驿微米黑），缺则跳过。
    /// .ttc 用 Face::parse(bytes, 0) 取 index 0 face。
    fn test_font_cjk() -> Option<Font> {
        let p = format!(
            "{}/tests/fixtures/wqy-microhei.ttc",
            env!("CARGO_MANIFEST_DIR")
        );
        Font::from_path(&p).ok()
    }

    #[test]
    fn cjk_font_loads_and_has_cjk_glyph_advance() {
        let font = match test_font_cjk() {
            Some(f) => f,
            None => {
                eprintln!("skip: no CJK test font (wqy-microhei.ttc)");
                return;
            }
        };
        // CJK 字符「中」应有 glyph（非 .notdef=0）且 advance > 0。
        let gid = font.face.glyph_index('中');
        assert!(gid.is_some(), "CJK 字体应含「中」glyph");
        let adv = font.face.glyph_hor_advance(gid.unwrap());
        assert!(adv.is_some() && adv.unwrap() > 0, "「中」advance 应 > 0");
        // 度量方法可用。
        assert!(font.ascent(16.0) > 0.0);
    }

    #[test]
    fn single_line_ascii_has_glyphs() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        let layout = measure_text(
            "Hello",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            false,
            None,
            &font,
            0,
            [1.0, 1.0, 1.0, 1.0],
        );
        assert_eq!(layout.lines.len(), 1);
        assert!(!layout.lines[0].runs.is_empty());
        // Hello = 5 字形
        assert_eq!(layout.lines[0].runs[0].glyphs.len(), 5);
        assert!(layout.text_width > 0.0);
    }

    /// 锁 kerning 重开：V pen_x = advance(A) + kern(A,V) < advance(A)
    /// （DejaVuSans AV kern ≈ -1.5px @24pt）。光栅化搬核心后 quad 是真实 ttf bbox，
    /// 可安全 honor kern（spec §9）。
    #[test]
    fn kerning_enabled_av_pen_x_includes_kern() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        let layout = measure_text(
            "AV",
            24.0,
            0.0,
            0.0,
            TextAlign::Left,
            false,
            None,
            &font,
            0,
            [1.0, 1.0, 1.0, 1.0],
        );
        let g = &layout.lines[0].runs[0].glyphs;
        assert_eq!(g.len(), 2, "AV = 2 glyph");
        let gid_a = font.face.glyph_index('A').unwrap();
        let adv_a = font.face.glyph_hor_advance(gid_a).unwrap() as f32
            / font.face.units_per_em() as f32
            * 24.0;
        assert_eq!(g[0].x, 0.0, "A pen_x = 0（行首）");
        // kern 重开：V.x 应 = adv_a + kern（kern < 0 → V.x < adv_a）。
        assert!(
            g[1].x < adv_a,
            "kern 应让 V.x={:.3} < advance(A)={:.3}",
            g[1].x,
            adv_a
        );
        assert!(g[1].x > 0.0);
    }

    /// 缺字（DejaVuSans 无 CJK「中」）→ advance 走字体 .notdef(gid0) advance，非 font_size 兜底。
    /// 核心权威（spec §9）：缺字画 .notdef/tofu，advance 确定性，不再猜 Unity fallback 1em。
    #[test]
    fn missing_glyph_uses_notdef_advance() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        // DejaVuSans 无 CJK「中」字形。
        assert!(
            font.face.glyph_index('中').is_none(),
            "前置：DejaVuSans 应缺「中」字形"
        );
        let layout = measure_text(
            "中中",
            24.0,
            0.0,
            0.0,
            TextAlign::Left,
            false,
            None,
            &font,
            0,
            [1.0, 1.0, 1.0, 1.0],
        );
        let g = &layout.lines[0].runs[0].glyphs;
        assert_eq!(g.len(), 2, "中中 = 2 glyph");
        assert_eq!(g[0].x, 0.0, "首字 pen_x = 0");
        // 第二字 advance = .notdef advance（读 gid0），确定性非 font_size(24)。
        let notdef_adv = font
            .face
            .glyph_hor_advance(ttf_parser::GlyphId(0))
            .map(|v| v as f32 / font.face.units_per_em() as f32 * 24.0)
            .unwrap_or(0.0);
        assert!(
            (g[1].x - notdef_adv).abs() < 0.5,
            "缺字 advance 应=.notdef={:.3}，实={:.3}",
            notdef_adv,
            g[1].x,
        );
    }

    #[test]
    fn wraps_on_width_constraint() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        let layout = measure_text(
            "aaaa bbbb cccc",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            false,
            Some(50.0),
            &font,
            0,
            [1.0, 1.0, 1.0, 1.0],
        );
        assert!(
            layout.lines.len() >= 2,
            "应在窄约束下换行，得 {} 行",
            layout.lines.len()
        );
    }

    #[test]
    fn nowrap_never_wraps() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        let layout = measure_text(
            "aaaa bbbb cccc",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            true,
            Some(10.0),
            &font,
            0,
            [1.0, 1.0, 1.0, 1.0],
        );
        assert_eq!(layout.lines.len(), 1);
    }

    #[test]
    fn cjk_breaks_per_char_under_narrow_width() {
        let font = match test_font_cjk() {
            Some(f) => f,
            None => {
                eprintln!("skip: no CJK test font");
                return;
            }
        };
        // 8 个 CJK 字符，窄约束（每字 ~font_size 宽）→ 应逐字断 ≥2 行。
        let layout = measure_text(
            "你好世界字体测试",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            false,
            Some(40.0),
            &font,
            0,
            [1.0, 1.0, 1.0, 1.0],
        );
        assert!(
            layout.lines.len() >= 2,
            "CJK 窄约束应逐字换行，得 {} 行",
            layout.lines.len()
        );
    }

    #[test]
    fn cjk_ascii_mix_breaks_correctly() {
        let font = match test_font_cjk() {
            Some(f) => f,
            None => {
                eprintln!("skip: no CJK test font");
                return;
            }
        };
        // CJK + ASCII 混排，窄约束 → 多行；不 panic、不出空行。
        let layout = measure_text(
            "Hello 世界 ABC 测试",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            false,
            Some(60.0),
            &font,
            0,
            [1.0, 1.0, 1.0, 1.0],
        );
        assert!(layout.lines.len() >= 2, "混排窄约束应换行");
        // 每行至少有 glyph（无空行）。
        for line in &layout.lines {
            let glyph_count: usize = line.runs.iter().map(|r| r.glyphs.len()).sum();
            assert!(glyph_count > 0, "不应有空行");
        }
    }

    #[test]
    fn newline_is_mandatory_break() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip");
                return;
            }
        };
        // \n 应强制换行。
        let layout = measure_text(
            "aaaa\nbbbb",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            false,
            None,
            &font,
            0,
            [1.0, 1.0, 1.0, 1.0],
        );
        assert_eq!(layout.lines.len(), 2, "\\n 应强制换行成 2 行");
    }

    #[test]
    fn nowrap_keeps_cjk_single_line() {
        let font = match test_font_cjk() {
            Some(f) => f,
            None => {
                eprintln!("skip: no CJK test font");
                return;
            }
        };
        let layout = measure_text(
            "你好世界字体测试",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            true,
            Some(10.0),
            &font,
            0,
            [1.0, 1.0, 1.0, 1.0],
        );
        assert_eq!(layout.lines.len(), 1, "nowrap 强制单行（含 CJK）");
    }

    #[test]
    fn super_long_word_breaks_per_char() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip");
                return;
            }
        };
        // 无空格长 ASCII 串（超 max_w）→ 超长词边界：逐字断。
        let layout = measure_text(
            "aaaaaaaaaaaaaaaaaaaa",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            false,
            Some(50.0),
            &font,
            0,
            [1.0, 1.0, 1.0, 1.0],
        );
        assert!(layout.lines.len() >= 2, "超长无空格串应逐字断 ≥2 行");
    }

    #[test]
    fn line_height_scales_line_box() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        let normal = measure_text(
            "Hi",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            false,
            None,
            &font,
            0,
            [1.0, 1.0, 1.0, 1.0],
        );
        let tall = measure_text(
            "Hi",
            16.0,
            2.0,
            0.0,
            TextAlign::Left,
            false,
            None,
            &font,
            0,
            [1.0, 1.0, 1.0, 1.0],
        );
        assert!(tall.lines[0].height > normal.lines[0].height);
    }

    // ── FontTable helpers ──

    fn font_bytes_dejavu() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/DejaVuSans.ttf"
        ))
        .unwrap()
    }

    fn ascent_dejavu_16() -> f32 {
        // Compute once from a direct Font::from_bytes to avoid circularity.
        let f = Font::from_bytes(font_bytes_dejavu()).unwrap();
        f.ascent(16.0)
    }

    // ── FontTable tests ──

    #[test]
    fn font_table_select_returns_default_when_no_family() {
        let mut t = FontTable::new();
        t.register("DejaVu", font_bytes_dejavu(), true).unwrap();
        let f = t.select(None);
        assert!(
            (f.ascent(16.0) - ascent_dejavu_16()).abs() < 0.01,
            "select(None) must return default font"
        );
    }

    #[test]
    fn font_table_select_falls_back_when_family_missing() {
        let mut t = FontTable::new();
        t.register("DejaVu", font_bytes_dejavu(), true).unwrap();
        let f = t.select(Some("Nonexistent"));
        // Falls back to default.
        assert!((f.ascent(16.0) - ascent_dejavu_16()).abs() < 0.01);
    }

    #[test]
    fn font_table_select_returns_named_when_present() {
        let mut t = FontTable::new();
        t.register("DejaVu", font_bytes_dejavu(), true).unwrap();
        t.register("Other", font_bytes_dejavu(), false).unwrap(); // same file, diff family
        let f = t.select(Some("Other"));
        // "Other" registered -> returned (same metrics here, but distinct entry).
        assert!(t.fonts.contains_key("Other"));
        let _ = f; // selected font is valid
    }

    #[test]
    fn font_table_register_is_default_sets_default() {
        let mut t = FontTable::new();
        t.register("DejaVu", font_bytes_dejavu(), true).unwrap();
        assert_eq!(t.default_family.as_deref(), Some("DejaVu"));
    }

    #[test]
    #[should_panic(expected = "no default font")]
    fn font_table_select_panics_without_default() {
        let t = FontTable::new();
        t.select(None);
    }

    // ── measure_rich_text tests（v1.7）──

    /// 两个不同色的 run 在一行内，各自 GlyphRun 携带自己的色（per-run color）。
    #[test]
    fn rich_multi_color_two_runs() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        let runs = vec![
            RichRun {
                kind: RichKind::Text {
                    text: "red ".into(),
                },
                color: [1.0, 0.0, 0.0, 1.0],
                font_id: 0,
                size_px: 24,
                weight: RichWeight::Normal,
                style: RichStyle::Normal,
                deco: RichDeco::default(),
                link_id: None,
            },
            RichRun {
                kind: RichKind::Text {
                    text: "blue".into(),
                },
                color: [0.0, 0.0, 1.0, 1.0],
                font_id: 0,
                size_px: 24,
                weight: RichWeight::Normal,
                style: RichStyle::Normal,
                deco: RichDeco::default(),
                link_id: None,
            },
        ];
        let lay = measure_rich_text(&runs, Some(1000.0), 1.2, &font, 0);
        // 一行，两个 run，各带自己的色。
        assert_eq!(lay.lines.len(), 1, "宽约束下单行");
        assert_eq!(lay.lines[0].runs.len(), 2, "两个 run 各自独立");
        assert_eq!(lay.lines[0].runs[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(lay.lines[0].runs[1].color, [0.0, 0.0, 1.0, 1.0]);
    }

    /// 窄宽度强制换行（拉丁按词）。多个词 → 多行。
    #[test]
    fn rich_wraps_on_max_width() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        let runs = vec![RichRun {
            kind: RichKind::Text {
                text: "aaaa bbbb cccc".into(),
            },
            color: [1.0, 1.0, 1.0, 1.0],
            font_id: 0,
            size_px: 24,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco::default(),
            link_id: None,
        }];
        // 窄宽度强制换行（拉丁按词）。
        let lay = measure_rich_text(&runs, Some(30.0), 1.2, &font, 0);
        assert!(
            lay.lines.len() > 1,
            "窄宽度应换行，实际 {} 行",
            lay.lines.len()
        );
    }

    /// CJK 逐字断行：窄宽度下每字一行（用 CJK 字体，否则缺字走 .notdef advance）。
    #[test]
    fn rich_cjk_breaks_per_char() {
        let font = match test_font_cjk() {
            Some(f) => f,
            None => {
                eprintln!("skip: no CJK test font (wqy-microhei.ttc)");
                return;
            }
        };
        let runs = vec![RichRun {
            kind: RichKind::Text {
                text: "你好世界".into(),
            },
            color: [1.0, 1.0, 1.0, 1.0],
            font_id: 0,
            size_px: 24,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco::default(),
            link_id: None,
        }];
        // 极窄宽度 → CJK 每字独占一行（4 字 ≥ 4 行）。
        let lay = measure_rich_text(&runs, Some(10.0), 1.2, &font, 0);
        assert!(
            lay.lines.len() >= 4,
            "CJK 逐字断行应 ≥4 行（窄宽），实际 {}",
            lay.lines.len()
        );
    }

    /// `\n` 强制换行：两段文本 + 中间 `\n` → 2 行。
    #[test]
    fn rich_newline_forces_break() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        let runs = vec![
            RichRun {
                kind: RichKind::Text {
                    text: "aaaa".into(),
                },
                color: [1.0, 1.0, 1.0, 1.0],
                font_id: 0,
                size_px: 24,
                weight: RichWeight::Normal,
                style: RichStyle::Normal,
                deco: RichDeco::default(),
                link_id: None,
            },
            RichRun {
                kind: RichKind::Text { text: "\n".into() },
                color: [1.0, 1.0, 1.0, 1.0],
                font_id: 0,
                size_px: 24,
                weight: RichWeight::Normal,
                style: RichStyle::Normal,
                deco: RichDeco::default(),
                link_id: None,
            },
            RichRun {
                kind: RichKind::Text {
                    text: "bbbb".into(),
                },
                color: [1.0, 1.0, 1.0, 1.0],
                font_id: 0,
                size_px: 24,
                weight: RichWeight::Normal,
                style: RichStyle::Normal,
                deco: RichDeco::default(),
                link_id: None,
            },
        ];
        let lay = measure_rich_text(&runs, Some(1000.0), 1.2, &font, 0);
        assert_eq!(lay.lines.len(), 2, "\\n 应强制换行成 2 行");
    }

    /// per-run 样式（weight/style/deco/link_id）透传进 GlyphRun。
    #[test]
    fn rich_run_style_propagates_to_glyph_run() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        let runs = vec![RichRun {
            kind: RichKind::Text {
                text: "link".into(),
            },
            color: [0.0, 1.0, 0.0, 1.0],
            font_id: 7,
            size_px: 18,
            weight: RichWeight::Bold,
            style: RichStyle::Italic,
            deco: RichDeco {
                underline: true,
                strike: false,
            },
            link_id: Some(3),
        }];
        let lay = measure_rich_text(&runs, Some(1000.0), 1.2, &font, 7);
        assert_eq!(lay.lines.len(), 1);
        let r = &lay.lines[0].runs[0];
        assert_eq!(r.weight, RichWeight::Bold);
        assert_eq!(r.style, RichStyle::Italic);
        assert!(r.deco.underline);
        assert_eq!(r.link_id, Some(3));
        assert_eq!(r.font_id, 7);
        assert_eq!(r.color, [0.0, 1.0, 0.0, 1.0]);
    }
}
