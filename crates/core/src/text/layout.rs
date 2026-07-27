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

/// CSS `line-height: normal` 的渲染倍数。
///
/// 不用字体的自然行高（ascent - descent + line_gap）——它因字体而异且常偏小，
/// 与浏览器/AI 预期不符。此处用实测的 Blink 对齐值（LXGWWenKai/sans-serif/monospace
/// 跨字体稳定为 ~1.31，是 Blink 的固定行为而非字体表值）。RmlUi 用 1.2，但
/// LXGWWenKai 的 hhea metrics 本就 ≈1.184，1.2 改善微；1.31 才贴近浏览器。
/// 想调 normal 倍数只改这一处。
const NORMAL_LINE_HEIGHT: f32 = 1.31;

/// 单个字形。坐标为绝对坐标（pen 位 = glyph.x/y + bearing）。
#[derive(Debug, Clone, Serialize)]
pub struct Glyph {
    pub glyph_id: u16,
    /// 该字形实际来源字体的稳定 id（atlas key 用）。
    /// 主字体有此字→主字体 id；否则走回退链，首个有此字的字体 id；全无→主字体 id（画 replacement）。
    /// build 期 `build_text_mesh` 按此 id 取 face 光栅 + 拼 GlyphKey——run 级 `GlyphRun.font_id`
    /// 仍是主字体 id（run 样式归属），per-glyph font_id 才是字形真实来源。
    pub font_id: u32,
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
    /// 字形 advance（布局期水平推进量，用于 text-decoration 装饰线等需要
    /// 知道 run 总宽的场景）。
    pub advance: f32,
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

/// 行内图位置（measure 期记，build 期产 image quad）。
#[derive(Debug, Clone, Serialize)]
pub struct RichImagePlacement {
    pub src: String,
    /// 左上角 x（content 相对坐标，align 后）。
    pub x: f32,
    /// 左上角 y（content 相对坐标，valign 后）。
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// 文本布局结果（SOA 三表：lines/runs/glyphs）。
#[derive(Debug, Clone, Serialize)]
pub struct TextLayout {
    pub text_width: f32,
    pub text_height: f32,
    pub lines: Vec<Line>,
    /// 行内图位置（measure_rich_text 填充，measure_text 为空）。
    pub images: Vec<RichImagePlacement>,
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
    /// 全局回退链（有序 family 名）。shaping 时主字体缺字按序 probe，首个有此字的补上。
    /// source-agnostic：这里只存 family 名，不问字体来源（bundled / 后端喂的系统字体都一样）。
    /// 由 `set_fallback_families` 单独设，与 `register` 解耦——避免改 register 签名连锁改调用点。
    pub(crate) fallback_families: Vec<String>,
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
            fallback_families: Vec::new(),
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

    /// 设全局回退链。families 须已 register（未注册的跳过并记忽略——防拼写错引 panic）。
    /// 重复 family 去重。空切片 = 清空回退（退回单字体行为）。
    pub fn set_fallback_families(&mut self, families: &[String]) {
        self.fallback_families.clear();
        for f in families {
            if self.fonts.contains_key(f) && !self.fallback_families.contains(f) {
                self.fallback_families.push(f.clone());
            }
        }
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

    /// 按 font_id 取字体（build 期 per-glyph 按 `Glyph.font_id` 取 face 光栅用）。
    /// 几个字体线性扫可接受；找不到（不应发生——id 来自 register）回落 default。
    pub fn font_by_id(&self, id: u32) -> &Font {
        for (fam, fid) in &self.family_to_id {
            if *fid == id {
                return self.fonts[fam].as_ref();
            }
        }
        self.select(None)
    }

    /// 为某主 family 构造 shaping 用的 FontStack（主字体 + 回退链 slice）。
    /// 调用方持 `fonts` 借用期间 stack 有效。
    pub fn stack_for(&self, family: Option<&str>) -> FontStack<'_> {
        FontStack {
            primary: self.select(family),
            primary_id: self.font_id(family),
            fallbacks: self
                .fallback_families
                .iter()
                .filter_map(|fam| {
                    self.fonts
                        .get(fam)
                        .map(|f| (f.as_ref(), *self.family_to_id.get(fam).unwrap()))
                })
                .collect(),
        }
    }
}

/// shaping 用的字体栈：主字体 + 有序回退链。per-glyph 按 `pick(ch)` 选字体。
///
/// 照 RmlUi `FontFaceHandleDefault::GetOrAppendGlyph` 模型：主字体没的字遍历回退链，
/// 首个有此字的用之；全无返主字体（caller 画 .notdef/replacement）。行度量走主字体，
/// per-glyph advance/kerning/bbox 走提供方字体（RmlUi 同款）。
///
/// `pick` 返回的 `&Font` 借用自构造时的 `FontTable`，生命周期与 stack 绑定。
pub struct FontStack<'a> {
    pub primary: &'a Font,
    pub primary_id: u32,
    pub fallbacks: Vec<(&'a Font, u32)>,
}

impl<'a> FontStack<'a> {
    /// 单字体栈（无回退）。测试 + 未配回退时用。
    pub fn single(font: &'a Font, id: u32) -> Self {
        FontStack {
            primary: font,
            primary_id: id,
            fallbacks: Vec::new(),
        }
    }

    /// 选含 ch 的字体：主字体优先，否则遍历回退链首个命中；全无返主字体（画 replacement）。
    pub fn pick(&self, ch: char) -> (&'a Font, u32) {
        if self.primary.face.glyph_index(ch).is_some() {
            return (self.primary, self.primary_id);
        }
        for (f, id) in &self.fallbacks {
            if f.face.glyph_index(ch).is_some() {
                return (*f, *id);
            }
        }
        (self.primary, self.primary_id)
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
    stack: &FontStack<'_>,
    color: [f32; 4],
    weight: crate::text::rich::RichWeight,
) -> TextLayout {
    // 行度量走主字体（RmlUi 模型：line height/ascent/descent 由主 face 定，per-glyph 才看回退）。
    let font = stack.primary;
    let ascent = font.ascent(font_size);
    let descent = font.descent(font_size); // 负

    // CSS line-height: normal → NORMAL_LINE_HEIGHT（见常量说明，别处不再复述理据）。
    let lh = if line_height > 0.0 {
        line_height
    } else {
        NORMAL_LINE_HEIGHT
    };

    // Line.height：倍数烤进 height（后端不重套，§9.1）。
    let line_h = font_size * lh;
    // baseline：half-leading 居中。
    let baseline = (line_h + ascent - descent) / 2.0 - descent.abs();

    // 度量一段文本的宽度（含字距）。per-char 按 stack 选字体——回退字形的 advance
    // 用其来源字体算，否则行宽估错导致断行位置偏。
    // kerning 仅相邻同字体才查（跨字体无 kern 表可言）。
    let measure_width = |s: &str| -> f32 {
        let mut pen = 0.0f32;
        let mut prev: Option<(ttf_parser::GlyphId, &Font)> = None;
        for ch in s.chars() {
            let (f, _) = stack.pick(ch);
            let gid_opt = f.face.glyph_index(ch);
            let gid = gid_opt.unwrap_or_default();
            if let Some((p, pf)) = prev {
                if std::ptr::eq(pf, f) {
                    if let Some(k) = kerning_value(&f.face, p, gid) {
                        pen += k as f32 / f.face.units_per_em() as f32 * font_size;
                    }
                }
            }
            pen += glyph_advance(&f.face, gid_opt, font_size) + letter_spacing;
            prev = Some((gid, f));
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
        // align 基准 = 容器宽（max_width），与 measure_rich_text 一致：单行短文本也偏移到
        // 容器内居中/右（浏览器语义）。此前基准 = text_width（最宽行）→ 单行 offset 0、
        // 永远左对齐（text-align:center 失效）。max_width=None（nowrap/无容器）fallback
        // text_width → offset 0。
        let container_w = max_width.unwrap_or(text_width);
        let x_offset = match align {
            crate::style::resolved::TextAlign::Center => ((container_w - lw) / 2.0).max(0.0),
            crate::style::resolved::TextAlign::Right => (container_w - lw).max(0.0),
            crate::style::resolved::TextAlign::Left => 0.0,
        };
        let mut pen_x = x_offset;
        let mut glyphs = Vec::with_capacity(text.chars().count());
        let mut prev: Option<(ttf_parser::GlyphId, &Font)> = None;
        for ch in text.chars() {
            let (f, fid) = stack.pick(ch);
            let gid_opt = f.face.glyph_index(ch);
            let gid = gid_opt.unwrap_or_default();
            // kerning 仅相邻同字体才查（跨字体无 kern 表）。
            if let Some((p, pf)) = prev {
                if std::ptr::eq(pf, f) {
                    if let Some(k) = kerning_value(&f.face, p, gid) {
                        pen_x += k as f32 / f.face.units_per_em() as f32 * font_size;
                    }
                }
            }
            // bearing 来自 glyph bbox：x_min → bearing_x，y_max → bearing_y（顶到 baseline）。
            // 用提供方字体的 units 换算（回退字形来自别的 face）。
            let funits = f.face.units_per_em() as f32;
            let (bx, by) = f
                .face
                .glyph_bounding_box(gid)
                .map(|b| {
                    (
                        b.x_min as f32 / funits * font_size,
                        b.y_max as f32 / funits * font_size,
                    )
                })
                .unwrap_or((0.0, 0.0));
            let adv = glyph_advance(&f.face, gid_opt, font_size);
            glyphs.push(Glyph {
                glyph_id: gid.0,
                font_id: fid,
                codepoint: ch as u32,
                x: pen_x,
                y: line_y,
                bearing_x: bx,
                bearing_y: by,
                advance: adv,
            });
            pen_x += adv + letter_spacing;
            prev = Some((gid, f));
        }
        out_lines.push(Line {
            y: line_y,
            height: line_h,
            baseline: line_y + baseline,
            width: *lw,
            runs: vec![GlyphRun {
                font_size,
                font_id: stack.primary_id,
                color,
                weight,
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
        images: Vec::new(),
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
/// `align`：text-align，每行整体在容器（`max_width`）内偏移——render 期传 `rect.w` 作
/// `max_width`，故单行短文本也能居中/右对齐到盒边（浏览器语义）。`max_width`=None（不换行）
/// 时无容器可对齐，偏移 0。行宽 > 容器（超长 token）不左溢（dx 钳 0）。
///
/// MVP 单字体：所有 run 共用传入的 `font`（节点 font_family 选的）+ `default_font_id`；
/// `GlyphRun.font_id` 填 `default_font_id`（run.font_id 字段保留但不用于选 face）。
pub fn measure_rich_text(
    runs: &[crate::text::rich::RichRun],
    max_width: Option<f32>,
    base_line_height: f32,
    align: crate::style::resolved::TextAlign,
    stack: &FontStack<'_>,
) -> TextLayout {
    let font = stack.primary;

    // per-char 按 stack 选字体的 advance 和（token 宽度用）。回退字形 advance 用来源字体算。
    let stack_str_advance = |s: &str, size_px: f32| -> f32 {
        let mut pen = 0.0f32;
        for ch in s.chars() {
            let (f, _) = stack.pick(ch);
            let gid = f.face.glyph_index(ch);
            pen += glyph_advance(&f.face, gid, size_px);
        }
        pen
    };

    let mut out_images: Vec<RichImagePlacement> = Vec::new();

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
                // 空白分词：按空格切词，词间补空格 token（占 advance；空格无轮廓不画，但占宽 +
                // 词边界断行）。旧版 split 丢空格 → "a b" 渲染成 "ab" + 断行点错。
                // HTML 空白折叠：连续空格 → 单空格（split 产空 part 跳过，词间补单空格）。
                let parts: Vec<&str> = text.split(' ').collect();
                for (pi, word) in parts.iter().enumerate() {
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
                            let w = stack_str_advance(slice, r.size_px as f32);
                            tokens.push(Tok {
                                text: slice,
                                run_idx: ri,
                                w,
                                is_break: false,
                            });
                            cur = next;
                        }
                    } else {
                        let w = stack_str_advance(word, r.size_px as f32);
                        tokens.push(Tok {
                            text: word,
                            run_idx: ri,
                            w,
                            is_break: false,
                        });
                    }
                    // 词间补空格 token（下一 part 非空时）：占 advance，断行可在空格后。
                    if pi < parts.len() - 1 && !parts[pi + 1].is_empty() {
                        let sp_w = stack_str_advance(" ", r.size_px as f32);
                        tokens.push(Tok {
                            text: " ",
                            run_idx: ri,
                            w: sp_w,
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
        let mut prev: Option<(ttf_parser::GlyphId, &Font)> = None;
        // 记本行 image 起点：text-align 偏移时连同 image 一起平移（image 无行号，靠区间定位）。
        let img_start = out_images.len();
        for &ti in line_toks {
            let r = &runs[tokens[ti].run_idx];
            match &r.kind {
                crate::text::rich::RichKind::Text { .. } => {
                    let mut glyphs: Vec<Glyph> = Vec::new();
                    for ch in tokens[ti].text.chars() {
                        let (f, fid) = stack.pick(ch);
                        let gid_opt = f.face.glyph_index(ch);
                        let gid = gid_opt.unwrap_or_default();
                        // kern（跨 token 也算——prev 是行内前一个字形）；仅相邻同字体才查。
                        if let Some((p, pf)) = prev {
                            if std::ptr::eq(pf, f) {
                                if let Some(k) = kerning_value(&f.face, p, gid) {
                                    pen_x +=
                                        k as f32 / f.face.units_per_em() as f32 * r.size_px as f32;
                                }
                            }
                        }
                        let funits = f.face.units_per_em() as f32;
                        let (bx, by) = f
                            .face
                            .glyph_bounding_box(gid)
                            .map(|b| {
                                (
                                    b.x_min as f32 / funits * r.size_px as f32,
                                    b.y_max as f32 / funits * r.size_px as f32,
                                )
                            })
                            .unwrap_or((0.0, 0.0));
                        let adv = glyph_advance(&f.face, gid_opt, r.size_px as f32);
                        glyphs.push(Glyph {
                            glyph_id: gid.0,
                            font_id: fid,
                            codepoint: ch as u32,
                            x: pen_x,
                            y: 0.0, // 行内相对（build 加 baseline）。
                            bearing_x: bx,
                            bearing_y: by,
                            advance: adv,
                        });
                        pen_x += adv;
                        prev = Some((gid, f));
                    }
                    // 同 run 相邻 token 合并（per-run 样式一致）；否则新 run。
                    let merged = runs_out.last_mut().filter(|gr: &&mut GlyphRun| {
                        gr.font_size == r.size_px as f32
                            && gr.font_id == stack.primary_id
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
                            font_id: stack.primary_id,
                            color: r.color,
                            weight: r.weight,
                            style: r.style,
                            deco: r.deco,
                            link_id: r.link_id,
                            glyphs,
                        });
                    }
                }
                crate::text::rich::RichKind::Image { src, w, h, valign } => {
                    let img_h = *h;
                    let img_w = *w;
                    // vertical-align：默认(Baseline)底边贴 baseline；middle 图中线对齐
                    // baseline + half x-height（小写字母中线，CSS middle 语义，非 baseline 本身——
                    // 贴 baseline 会令图相对文字偏下）；top 顶贴行顶；bottom 底贴行底。
                    let xh = font
                        .face
                        .x_height()
                        .map(|x| x as f32 / font.face.units_per_em() as f32 * line_size)
                        .unwrap_or(line_size * 0.5);
                    let y_top = match valign {
                        crate::text::rich::RichVAlign::Middle => baseline - xh * 0.5 - img_h * 0.5,
                        crate::text::rich::RichVAlign::Top => 0.0,
                        crate::text::rich::RichVAlign::Bottom => baseline - img_h,
                        _ => baseline - img_h, // Baseline 默认底边贴
                    };
                    out_images.push(RichImagePlacement {
                        src: src.clone(),
                        x: pen_x,
                        y: y + y_top,
                        w: img_w,
                        h: img_h,
                    });
                    pen_x += img_w;
                }
            }
        }
        let width = pen_x;
        // text-align：本行整体在容器（max_width）内偏移，glyph + image 同 dx。
        // 容器 = max_width（render 传 rect.w）。行宽 > 容器 → dx 钳 0（不左溢）。
        // max_width=None（不换行）无容器 → 不偏移。
        let dx = match align {
            crate::style::resolved::TextAlign::Left => 0.0,
            crate::style::resolved::TextAlign::Center => {
                max_width.map_or(0.0, |mw| ((mw - width) / 2.0).max(0.0))
            }
            crate::style::resolved::TextAlign::Right => {
                max_width.map_or(0.0, |mw| (mw - width).max(0.0))
            }
        };
        if dx != 0.0 {
            for run in &mut runs_out {
                for g in &mut run.glyphs {
                    g.x += dx;
                }
            }
            for img in &mut out_images[img_start..] {
                img.x += dx;
            }
        }
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
        images: out_images,
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

/// strut 行高：line_height>0 用倍数；normal(0) 用 NORMAL_LINE_HEIGHT×size（见常量说明）。
fn strut_height(line_height: f32, size: f32, _ascent: f32, _descent: f32, _line_gap: f32) -> f32 {
    let lh = if line_height > 0.0 {
        line_height
    } else {
        NORMAL_LINE_HEIGHT
    };
    size * lh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::resolved::TextAlign;
    use crate::text::rich::{
        RichDeco, RichKind, RichRun, RichStyle, RichWeight, TextDecoLines, TextDecoStyle,
    };

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
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
        );
        assert_eq!(layout.lines.len(), 1);
        assert!(!layout.lines[0].runs.is_empty());
        // Hello = 5 字形
        assert_eq!(layout.lines[0].runs[0].glyphs.len(), 5);
        assert!(layout.text_width > 0.0);
    }

    /// 单行短文本 + max_width（容器）→ center/right 在容器内偏移（浏览器语义）。
    /// 修复前 align 基准 = text_width（最宽行 = 单行）→ offset 0 → 单行永远左对齐
    /// （A6/B8 "text-align:center 却左对齐" 症状）。
    #[test]
    fn measure_text_aligns_single_line_within_container() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        // "Hello" 宽远小于容器 200 → center 居中：首字 x ≈ (200 - text_w)/2 >> 0
        let layout = measure_text(
            "Hello",
            16.0,
            0.0,
            0.0,
            TextAlign::Center,
            false,
            Some(200.0),
            &FontStack::single(&font, 0),
            [1.0; 4],
            crate::text::rich::RichWeight::Normal,
        );
        let first_x = layout.lines[0].runs[0].glyphs[0].x;
        assert!(
            first_x > 10.0,
            "center 单行在容器内偏移（首字 x={:.1} > 10）",
            first_x
        );

        let layout_r = measure_text(
            "Hello",
            16.0,
            0.0,
            0.0,
            TextAlign::Right,
            false,
            Some(200.0),
            &FontStack::single(&font, 0),
            [1.0; 4],
            crate::text::rich::RichWeight::Normal,
        );
        let glyphs = &layout_r.lines[0].runs[0].glyphs;
        let last_x = glyphs.last().unwrap().x;
        assert!(
            last_x > 100.0,
            "right 单行靠容器右（末字 x={:.1} > 100）",
            last_x
        );
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
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
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
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
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
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
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
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
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
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
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
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
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
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
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
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
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
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
        );
        assert!(layout.lines.len() >= 2, "超长无空格串应逐字断 ≥2 行");
    }

    #[test]
    fn line_height_normal_uses_const_not_font_metrics() {
        // CSS line-height: normal 应对齐浏览器（实测 Blink ~1.31，见 NORMAL_LINE_HEIGHT），
        // 而非字体的自然行高（ascent-descent+line_gap，因字体而异且常偏小）。
        // 最终值待定；调 normal 倍数改常量，本测试自动跟随。
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        let font_size = 36.0;
        let result = measure_text(
            "Hi",
            font_size,
            0.0, // normal
            0.0,
            TextAlign::Left,
            false,
            None,
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
        );
        let line_h = result.lines[0].height;
        let expected = font_size * NORMAL_LINE_HEIGHT;
        assert_eq!(
            line_h, expected,
            "normal line-height 该是 {NORMAL_LINE_HEIGHT}×font_size={expected}，实际 {line_h}"
        );
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
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
        );
        let tall = measure_text(
            "Hi",
            16.0,
            2.0,
            0.0,
            TextAlign::Left,
            false,
            None,
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
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
        let lay = measure_rich_text(
            &runs,
            Some(1000.0),
            1.2,
            TextAlign::Left,
            &FontStack::single(&font, 0),
        );
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
        let lay = measure_rich_text(
            &runs,
            Some(30.0),
            1.2,
            TextAlign::Left,
            &FontStack::single(&font, 0),
        );
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
        let lay = measure_rich_text(
            &runs,
            Some(10.0),
            1.2,
            TextAlign::Left,
            &FontStack::single(&font, 0),
        );
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
        let lay = measure_rich_text(
            &runs,
            Some(1000.0),
            1.2,
            TextAlign::Left,
            &FontStack::single(&font, 0),
        );
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
                lines: TextDecoLines::UNDERLINE,
                style: TextDecoStyle::Solid,
                color: None,
                thickness: None,
            },
            link_id: Some(3),
        }];
        let lay = measure_rich_text(
            &runs,
            Some(1000.0),
            1.2,
            TextAlign::Left,
            &FontStack::single(&font, 7),
        );
        assert_eq!(lay.lines.len(), 1);
        let r = &lay.lines[0].runs[0];
        assert_eq!(r.weight, RichWeight::Bold);
        assert_eq!(r.style, RichStyle::Italic);
        assert!(r.deco.lines.underline());
        assert_eq!(r.link_id, Some(3));
        assert_eq!(r.font_id, 7);
        assert_eq!(r.color, [0.0, 1.0, 0.0, 1.0]);
    }

    /// text-align：center/right 把行整体在容器（max_width）内偏移；left 不偏。
    /// 单行短文本也偏（render 期 max_width=rect.w）——浏览器语义，非仅多行生效。
    #[test]
    fn rich_text_align_offsets_line_within_container() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        let runs = vec![RichRun {
            kind: RichKind::Text { text: "Hi".into() },
            color: [1.0, 1.0, 1.0, 1.0],
            font_id: 0,
            size_px: 24,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco::default(),
            link_id: None,
        }];
        let stack = FontStack::single(&font, 0);
        // 容器远宽于文本（1000 vs ~20px）→ center/right 应把字形推到右侧。
        let left = measure_rich_text(&runs, Some(1000.0), 1.2, TextAlign::Left, &stack);
        let center = measure_rich_text(&runs, Some(1000.0), 1.2, TextAlign::Center, &stack);
        let right = measure_rich_text(&runs, Some(1000.0), 1.2, TextAlign::Right, &stack);
        let first_x = |lay: &TextLayout| lay.lines[0].runs[0].glyphs[0].x;
        let w = left.lines[0].width;
        assert!(first_x(&left) == 0.0, "left 首字 x=0");
        assert!(
            (first_x(&center) - (1000.0 - w) / 2.0).abs() < 0.5,
            "center 首字 x≈(容器-行宽)/2"
        );
        assert!(
            (first_x(&right) - (1000.0 - w)).abs() < 0.5,
            "right 首字 x≈容器-行宽"
        );
        assert!(first_x(&right) > first_x(&center), "right 比 center 更靠右");
    }

    /// 字体回退：主字体（DejaVu，无中文）缺字时，回退链首个含该字的字体（wqy）补上。
    /// 验证：①中文 glyph 的 font_id = 回退字体 id（非主字体 id）；
    ///       ②回退字形 glyph_id 非 0（非 .notdef）；③ASCII 仍用主字体 id。
    /// 照 RmlUi GetOrAppendGlyph fallback 模型。
    #[test]
    fn fallback_picks_cjk_font_for_missing_glyph() {
        let primary = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no DejaVu test font");
                return;
            }
        };
        let cjk = match test_font_cjk() {
            Some(f) => f,
            None => {
                eprintln!("skip: no CJK test font");
                return;
            }
        };
        // 主字体 DejaVu 不含「中」。
        assert!(
            primary.face.glyph_index('中').is_none(),
            "DejaVu 不应含 CJK（前提）"
        );
        let stack = FontStack {
            primary: &primary,
            primary_id: 0,
            fallbacks: vec![(&cjk, 1)],
        };
        let lay = measure_text(
            "A中",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            false,
            None,
            &stack,
            [1.0; 4],
            crate::text::rich::RichWeight::Normal,
        );
        let glyphs: Vec<&Glyph> = lay
            .lines
            .iter()
            .flat_map(|l| l.runs.iter().flat_map(|r| r.glyphs.iter()))
            .collect();
        assert_eq!(glyphs.len(), 2, "A + 中 = 2 字形");
        // 'A' 在主字体 → font_id=0。
        assert_eq!(glyphs[0].codepoint, 'A' as u32);
        assert_eq!(glyphs[0].font_id, 0, "ASCII 用主字体 id");
        assert_ne!(glyphs[0].glyph_id, 0, "A 非 .notdef");
        // '中' 走回退 → font_id=1（wqy），glyph_id 非 0（真有字，非 .notdef 方框）。
        assert_eq!(glyphs[1].codepoint, '中' as u32);
        assert_eq!(glyphs[1].font_id, 1, "中文用回退字体 id");
        assert_ne!(glyphs[1].glyph_id, 0, "中 走回退应得真字形，非 .notdef");
    }

    /// 无回退时：主字体缺字 → .notdef（glyph_id=0），font_id 仍主字体。退化行为锁定。
    #[test]
    fn no_fallback_missing_glyph_becomes_notdef() {
        let primary = match test_font() {
            Some(f) => f,
            None => return,
        };
        let stack = FontStack::single(&primary, 0);
        let lay = measure_text(
            "中",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            false,
            None,
            &stack,
            [1.0; 4],
            crate::text::rich::RichWeight::Normal,
        );
        let g = &lay.lines[0].runs[0].glyphs[0];
        assert_eq!(g.font_id, 0, "无回退→font_id 仍主字体");
        assert_eq!(g.glyph_id, 0, "无回退→.notdef（gid0）");
    }
}
