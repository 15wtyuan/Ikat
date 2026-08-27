//! Text 层：给定文本 + 字体 + 约束宽，产出 TextLayout（SOA 三表 glyphs/runs/lines）。
//!
//! 实现要点：
//! - 字体度量走 ttf-parser（API 适配见下方）。
//! - 断行用贪心按空白 + 宽度约束（unicode-linebreak UAX#14 提供换行机会）。
//! - glyph 存绝对坐标（已累加 advance + 已应用 align 偏移），后端拼 quad 零累加。

use std::collections::HashMap;
use std::sync::Arc;

use ttf_parser::Face;

use crate::scene::node::NodeId;

/// CSS `line-height: normal` 的渲染倍数。
///
/// 不用字体的自然行高（ascent - descent + line_gap）——它因字体而异且常偏小，
/// 与浏览器/AI 预期不符。此处用实测的 Blink 对齐值（LXGWWenKai/sans-serif/monospace
/// 跨字体稳定为 ~1.31，是 Blink 的固定行为而非字体表值）。RmlUi 用 1.2，但
/// LXGWWenKai 的 hhea metrics 本就 ≈1.184，1.2 改善微；1.31 才贴近浏览器。
/// 想调 normal 倍数只改这一处。
const NORMAL_LINE_HEIGHT: f32 = 1.31;

/// 断行装填的亚像素容差（design px）。贪心累加（token/段 求和）与行宽（glyph 逐字
/// pen 累加）的浮点加法顺序不同——非结合性使两侧在"恰好装满"边界可差 ~1e-5，无容差的
/// `<=` 会把最后一个 token 挤到下一行（flex item 定宽 = max-content 的场景必现：
/// item 宽 = max-content，重测约束 = 同值，边界比较必失败）。0.05px 远低于任何可见
/// glyph 碎片，只吃掉浮点噪声。
const WRAP_FIT_EPS: f32 = 0.05;

/// 换行控制参数包（#73）：`white-space` 三轴 + `overflow-wrap`/`word-break`/`text-wrap`。
///
/// 构造：静态文本 `ResolvedStyle::wrap_control()`；文本控件
/// `style::resolved::control_wrap_control`（空白语义冻结 pre 系，保光标字节映射）。
/// 消费：`measure_text`（plain 路径，UAX#14 断行）与 `measure_rich_text`
/// （token 贪心路）。四枚举 Copy/Hash，指纹 memo 直接哈希整包。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct WrapControl {
    pub white_space: crate::style::resolved::WhiteSpace,
    pub overflow_wrap: crate::style::resolved::OverflowWrap,
    pub word_break: crate::style::resolved::WordBreak,
    pub text_wrap: crate::style::resolved::TextWrap,
}

impl WrapControl {
    /// 软换行（自动断行）开关：white-space 允许且 text-wrap 未关。
    /// 强制断行（保留的 `\n` / `<br>`）不受此开关影响——CSS 里 nowrap 的 `\n`
    /// 在折叠阶段并进空白串、pre 的 `\n` 保留，断行源由折叠决定，不由开关压制。
    pub fn wrap_enabled(&self) -> bool {
        matches!(
            self.white_space,
            crate::style::resolved::WhiteSpace::Normal
                | crate::style::resolved::WhiteSpace::PreWrap
                | crate::style::resolved::WhiteSpace::PreLine
        ) && self.text_wrap == crate::style::resolved::TextWrap::Normal
    }

    /// 空白折叠（空白串 → 单空格）。PreLine 折叠空格类但保留 `\n`
    /// （见 [`Self::preserve_newlines`]）。
    pub fn collapse_spaces(&self) -> bool {
        matches!(
            self.white_space,
            crate::style::resolved::WhiteSpace::Normal
                | crate::style::resolved::WhiteSpace::Nowrap
                | crate::style::resolved::WhiteSpace::PreLine
        )
    }

    /// 源换行保留（`\n` 产生强制断行）。false = `\n` 在折叠阶段并进空白串。
    pub fn preserve_newlines(&self) -> bool {
        matches!(
            self.white_space,
            crate::style::resolved::WhiteSpace::Pre
                | crate::style::resolved::WhiteSpace::PreWrap
                | crate::style::resolved::WhiteSpace::PreLine
        )
    }
}

/// kinsoku 行首禁则字符集：不得作为行首（断点须左移，把前一字符一起挪下一行）。
/// 中文排版通用压缩集（句读/闭括号/省略号等）；UAX#14 的 CL 类在 plain 路已天然
/// 避开多数场景，此表服务 rich token 路（无 UAX#14）与逐字拆分（break-word）的
/// 断点调整。
const KINSOKU_NO_LINE_START: &str = "。，、；：？！）】〉」』”’…‥·％‰℃¢°";

/// kinsoku 行尾禁则字符集：不得作为行尾（断点须右移，开括号随词下移）。
const KINSOKU_NO_LINE_END: &str = "（【〈「『“‘《〈";

fn is_kinsoku_no_line_start(ch: char) -> bool {
    KINSOKU_NO_LINE_START.contains(ch)
}

fn is_kinsoku_no_line_end(ch: char) -> bool {
    KINSOKU_NO_LINE_END.contains(ch)
}

/// 可折叠空白（CSS collapsible whitespace：空格/制表/CR/换页；`\n` 视模式另论）。
fn is_fold_ws(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\r' | '\u{000C}')
}

/// plain 路径文本预处理（CSS text processing 子集）：
/// - 折叠模式：空白串 → 单空格；保留 `\n` 的模式下 `\n` 不折（后续 UAX#14 出
///   Mandatory 断点）。首尾空格裁去（CSS：行盒首尾 collapsible 空白移除）。
/// - 保留模式：原样返回（借用，零拷贝）。
///
/// 返回 Cow；调用方只读。折叠删改字符——**光标字节映射会被破坏**，仅供静态文本；
/// 文本控件经 `control_wrap_control` 恒为保留模式，字节 1:1 保持。
fn preprocess_text<'a>(content: &'a str, wrap: &WrapControl) -> std::borrow::Cow<'a, str> {
    if !wrap.collapse_spaces() {
        return std::borrow::Cow::Borrowed(content);
    }
    let mut out = String::with_capacity(content.len());
    let mut in_ws = false;
    for ch in content.chars() {
        if is_fold_ws(ch) || (ch == '\n' && !wrap.preserve_newlines()) {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            in_ws = false;
            out.push(ch); // PreLine 的 \n：preserve_newlines=true，不折叠、直通
        }
    }
    // 首尾单空格裁去（只在折叠模式；PreLine 的 \n 不动）。
    let trimmed = out.trim_matches(' ');
    std::borrow::Cow::Owned(trimmed.to_string())
}

/// 单个字形。坐标为绝对坐标（pen 位 = glyph.x/y + bearing）。
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct GlyphRun {
    pub font_size: f32,
    /// 字体 id（atlas key + image_path 合成用）。MVP 单字体：所有 run 填
    /// default_font_id，build 期 build_text_mesh 仍按外传 font_id 取 face；
    /// 此字段为 per-run 字体（多 family）预留。
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct RichImagePlacement {
    pub src: String,
    /// 左上角 x（content 相对坐标，align 后）。
    pub x: f32,
    /// 左上角 y（content 相对坐标，valign 后）。
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// 单个 input `RichRun` 在某行的命中矩形（content 相对坐标，与 glyph 同坐标系）。
///
/// 命中测试用它把 rich-text-block 内的点细化到 source inline 节点
/// （span/TextNode/image）。跨行 run 拆多条 rect（每行一条）；image run 直接用
/// `RichImagePlacement`。key 粒度 = input `RichRun`（不是渲染层合并后的 `GlyphRun`）——
/// 同 style 相邻 run 渲染合并进一个 GlyphRun，但命中须保留各 run 独立 source。
/// 非序列化：rich-text 运行时产物（同 `RichRun`，不进 pkg.bin）。
#[derive(Debug, Clone)]
pub struct RichRunRect {
    /// 左上角 x（已含 text-align 偏移）。
    pub x: f32,
    /// 左上角 y（= 所在行的行顶）。
    pub y: f32,
    /// 该 run 在本行的宽（glyph advance 跨度）。
    pub w: f32,
    /// 该 run 所在行的行高（命中垂直覆盖整行）。
    pub h: f32,
    /// 产此 rect 的 input `RichRun.source`（inline 节点 NodeId）。
    pub source: NodeId,
}

/// 文本布局结果（SOA 三表：lines/runs/glyphs）。
#[derive(Debug, Clone)]
pub struct TextLayout {
    pub text_width: f32,
    pub text_height: f32,
    pub lines: Vec<Line>,
    /// 行内图位置（measure_rich_text 填充，measure_text 为空）。
    pub images: Vec<RichImagePlacement>,
    /// 每 input `RichRun` 每行的命中矩形（measure_rich_text 填充；measure_text 为空，
    /// plain TextNode 整块即命中目标，无需细化）。见 `RichRunRect`。
    pub run_rects: Vec<RichRunRect>,
}

/// 封装一个 ttf 字体。
///
/// Face 借用 `Box::leak` 产出的 `'static` 切片；leak 的内存不释放（字体数量有限，可接受）。
pub struct Font {
    pub face: Face<'static>,
}

impl Font {
    pub fn from_path(path: &str) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        Self::from_bytes(bytes)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
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
/// family_to_id 为每个注册 family 分配稳定 u32 id，供 atlas key 和合成
/// image_path 用。id 在 register 时分配，不随字体表增删变化。
/// 缺字诊断日志（tofu 取证）：`FontStack::pick` 全链（主字体 + 回退链）都缺某字时
/// 记录 (主 family, char)。会话级去重（同 family+char 只记一次）；pending 由宿主
/// 经 `take_missing_glyph_reports` 取走转引擎日志。tofu 框是开发期故意暴露的信号，
/// 本日志把「哪个字符、哪个字体族」在 Console 点名，免肉眼猜。
#[derive(Default)]
pub struct MissingGlyphLog {
    pending: Vec<(String, char)>,
    seen: std::collections::HashSet<(String, char)>,
}

impl MissingGlyphLog {
    fn record(&mut self, family: &str, ch: char) {
        if self.seen.insert((family.to_string(), ch)) {
            self.pending.push((family.to_string(), ch));
        }
    }
}

pub struct FontTable {
    pub(crate) fonts: HashMap<String, Arc<Font>>,
    pub(crate) default_family: Option<String>,
    pub(crate) family_to_id: HashMap<String, u32>,
    pub(crate) next_id: u32,
    /// 全局回退链（有序 family 名）。shaping 时主字体缺字按序 probe，首个有此字的补上。
    /// source-agnostic：这里只存 family 名，不问字体来源（bundled / 后端喂的系统字体都一样）。
    /// 由 `set_fallback_families` 单独设，与 `register` 解耦——避免改 register 签名连锁改调用点。
    pub(crate) fallback_families: Vec<String>,
    /// 缺字诊断（RefCell：measure 闭包持 `&FontTable` 不可变借用，经内部可变性记录）。
    pub(crate) missing_log: std::cell::RefCell<MissingGlyphLog>,
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
            missing_log: std::cell::RefCell::new(MissingGlyphLog::default()),
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

    /// family 是否已注册。严格入口（如 `Stage::measure_text`）用它拒未知 family——
    /// `stack_for`/`select` 对未知 family 静默 fallback 到默认字体（渲染路径合理，
    /// 测量路径不行：拿错字体估宽没有意义）。
    pub fn contains_family(&self, family: &str) -> bool {
        self.fonts.contains_key(family)
    }

    /// 为某主 family 构造 shaping 用的 FontStack（主字体 + 回退链 slice）。
    /// 调用方持 `fonts` 借用期间 stack 有效。
    pub fn stack_for<'a>(&'a self, family: Option<&'a str>) -> FontStack<'a> {
        FontStack {
            primary: self.select(family),
            primary_id: self.font_id(family),
            primary_family: self.effective_family_name(family),
            log: Some(&self.missing_log),
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

    /// 主 family 实际生效名：显式命中用之；未命中 / None → default 名。
    /// select 同款 fallback 语义，仅供诊断日志报「哪个 family 缺字」。
    fn effective_family_name<'a>(&'a self, family: Option<&'a str>) -> &'a str {
        match family {
            Some(f) if self.fonts.contains_key(f) => f,
            _ => self
                .default_family
                .as_deref()
                .expect("no default font registered"),
        }
    }

    /// 取走自上次调用以来新发现的缺字记录（会话级去重：同 family+char 只报一次，
    /// 不因未及时取而重复刷屏）。每条已格式化为可读诊断行，宿主转引擎日志。
    pub fn take_missing_glyph_reports(&mut self) -> Vec<String> {
        self.missing_log
            .get_mut()
            .pending
            .drain(..)
            .map(|(family, ch)| {
                format!(
                    "font-family \"{family}\" has no glyph for '{ch}' (U+{:04X}); \
fallback chain exhausted, drawn as tofu box. Fix: `ikat font add` a font \
containing it with --fallback, or replace the character.",
                    ch as u32
                )
            })
            .collect()
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
    /// 主 family 实际生效名（缺字诊断用；single() 测试栈为空串）。
    pub primary_family: &'a str,
    /// 缺字日志（FontTable 持有；single() 测试栈无）。
    log: Option<&'a std::cell::RefCell<MissingGlyphLog>>,
    pub fallbacks: Vec<(&'a Font, u32)>,
}

impl<'a> FontStack<'a> {
    /// 单字体栈（无回退、无诊断）。测试 + 未配回退时用。
    pub fn single(font: &'a Font, id: u32) -> Self {
        FontStack {
            primary: font,
            primary_id: id,
            primary_family: "",
            log: None,
            fallbacks: Vec::new(),
        }
    }

    /// 选含 ch 的字体：主字体优先，否则遍历回退链首个命中；全无返主字体（画 replacement）。
    /// 全链缺字时记入缺字日志（tofu 取证：family + char，会话级去重）。
    pub fn pick(&self, ch: char) -> (&'a Font, u32) {
        if self.primary.face.glyph_index(ch).is_some() {
            return (self.primary, self.primary_id);
        }
        for (f, id) in &self.fallbacks {
            if f.face.glyph_index(ch).is_some() {
                return (*f, *id);
            }
        }
        if let Some(log) = self.log {
            log.borrow_mut().record(self.primary_family, ch);
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

/// 跨帧 measure_text memo：每节点两槽，各带 fingerprint。命中（fingerprint 匹配）→ 复用
/// TextLayout，跳过 shaping。intrinsic = `max_width=None` 的 max-content 测量（短文本唯一 pass；
/// 长文本 taffy 先测一次 max-content）；constrained = `max_width=Some(w)` 的换行测量。
///
/// fingerprint 含 content hash → set_text / slot 换内容自动 miss；style 改、约束宽变（量化桶）
/// 也 miss。设计为后续增量布局的地基：fingerprint 源可从 content-hash 换成 dirty-version
/// 而本结构（每节点两槽 + fingerprint 比对）不变。
#[derive(Clone, Debug, Default)]
pub struct TextMeasureCache {
    /// max_width=None 的测量结果 + 其 fingerprint。
    pub intrinsic: Option<(u64, TextLayout)>,
    /// max_width=Some(w) 的测量结果 + 其 fingerprint（约束宽量化进 fingerprint）。
    pub constrained: Option<(u64, TextLayout)>,
}

/// 文本测量 fingerprint：content + style + 约束宽（0.25px 量化）→ u64。同 fp → measure_text
/// 结果同 → 可复用。`mw` None/Some 区分 intrinsic/constrained 两槽（discriminator 进 hash）。
///
/// 用 `DefaultHasher::new()`（固定 key，跨进程确定性）——不能用 `RandomState`（每进程随机 →
/// 持久缓存跨 tick 失效）。CJK content 是主要成本，hash ~µs/节点，vs shaping ~100µs/节点。
#[allow(clippy::too_many_arguments)]
pub fn text_fingerprint(
    content: &str,
    font_size: f32,
    line_height: f32,
    letter_spacing: f32,
    align: crate::style::resolved::TextAlign,
    wrap: WrapControl,
    font_weight: u16,
    family: Option<&str>,
    mw: Option<f32>,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut h);
    font_size.to_bits().hash(&mut h);
    line_height.to_bits().hash(&mut h);
    letter_spacing.to_bits().hash(&mut h);
    align.hash(&mut h);
    wrap.hash(&mut h);
    font_weight.hash(&mut h);
    family.hash(&mut h);
    match mw {
        // None/Some 用 discriminator 区分；Some 的 w 量化到 0.25px 桶——静止时 max_width
        // 稳定（tween 不动 layout），桶稳定 → 命中；resize 时桶短暂漂移后收敛。
        None => 0u32.hash(&mut h),
        Some(w) => {
            1u32.hash(&mut h);
            ((w * 4.0).round() as i64).hash(&mut h);
        }
    }
    h.finish()
}

/// 富文本测量 fingerprint：runs + base style + 约束宽（0.25px 量化）→ u64。同 fp →
/// `measure_rich_text` 结果同 → 可复用 TextLayout（跳过 shaping，同 `text_fingerprint`）。
///
/// runs 是每帧现编译的（`compile_rich_runs` 便宜，O(inline 子)），故指纹键是 runs 全量
/// 而非单个 content 字符串。每 run 进 hash 的字段：kind 判别（Text/Image 0/1）+ payload
/// （Text:text；Image:src+w+h+valign）+ color bits + font_id + size_px + weight + style
/// + deco 全子字段 + link_id + **source NodeId**。
///
/// `source`（NodeId）必须进 hash：两个不同 span 文本相同也不应共享缓存（命中路由会错），
/// span 换色/换内容 → runs 变 → fp 变 → 自动 miss 重测。不依赖 dirty_text 传播（现仅标
/// 文本节点自身，无 "span 改色标父" 路径——指纹 memo 闭环更干净）。
///
/// `mw` 同 `text_fingerprint`：None/Some 用 discriminator 区分两槽（intrinsic/constrained），
/// Some 量化到 0.25px 桶避亚像素抖动 thrash 缓存。
pub fn rich_text_fingerprint(
    runs: &[crate::text::rich::RichRun],
    line_height: f32,
    letter_spacing: f32,
    align: crate::style::resolved::TextAlign,
    wrap: WrapControl,
    family: Option<&str>,
    mw: Option<f32>,
) -> u64 {
    use crate::text::rich::RichKind;
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    letter_spacing.to_bits().hash(&mut h);
    // 换行控制进键（#73）：white-space/word-break 等改变断行结果，不进键会串缓存。
    wrap.hash(&mut h);
    runs.len().hash(&mut h);
    for r in runs {
        // kind 判别 + payload。Text vs Image 用 0/1 区分；Image 的 f32 用 to_bits 哈希
        // （f32 不 impl Hash）。
        match &r.kind {
            RichKind::Text { text } => {
                0u8.hash(&mut h);
                text.hash(&mut h);
            }
            RichKind::Image {
                src,
                w,
                h: ih,
                valign,
            } => {
                1u8.hash(&mut h);
                src.hash(&mut h);
                w.to_bits().hash(&mut h);
                ih.to_bits().hash(&mut h);
                valign.hash(&mut h);
            }
        }
        // per-run 样式通道。color 是 [f32;4] → 逐通道 to_bits（f32 不 impl Hash）。
        for c in r.color.iter() {
            c.to_bits().hash(&mut h);
        }
        r.font_id.hash(&mut h);
        r.size_px.hash(&mut h);
        r.weight.hash(&mut h);
        r.style.hash(&mut h);
        // RichDeco 子字段：lines/style 整数 backed 可 Hash；color/thickness 含 f32 → 手动
        // （Option<...> 用 0/1 discriminator 区分 Some/None，防两档碰撞）。
        r.deco.lines.hash(&mut h);
        r.deco.style.hash(&mut h);
        match r.deco.color {
            Some(c) => {
                1u8.hash(&mut h);
                for v in c.iter() {
                    v.to_bits().hash(&mut h);
                }
            }
            None => 0u8.hash(&mut h),
        }
        match r.deco.thickness {
            Some(t) => {
                1u8.hash(&mut h);
                t.to_bits().hash(&mut h);
            }
            None => 0u8.hash(&mut h),
        }
        r.link_id.hash(&mut h);
        r.source.hash(&mut h);
    }
    line_height.to_bits().hash(&mut h);
    align.hash(&mut h);
    family.hash(&mut h);
    match mw {
        None => 0u32.hash(&mut h),
        Some(w) => {
            1u32.hash(&mut h);
            ((w * 4.0).round() as i64).hash(&mut h);
        }
    }
    h.finish()
}

/// 测量并布局文本。
///
/// - `line_height`：倍数，`0.0` = normal（= ascent - descent + line_gap）。
/// - `wrap`：换行控制（#73，white-space 三轴 + overflow-wrap/word-break/text-wrap）。
///   静态文本传 `ResolvedStyle::wrap_control()`，文本控件传
///   `control_wrap_control`（空白语义冻结，保光标字节映射）。
/// - `max_width`：`None` 表示不换行（intrinsic）；`Some` 为 content area 宽。
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
    wrap: WrapControl,
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

    // Line.height：倍数烤进 height（后端不重套）。
    let line_h = font_size * lh;
    // baseline：half-leading 居中。
    let baseline = (line_h + ascent - descent) / 2.0 - descent.abs();

    // 度量一段文本的宽度（含字距）。per-char 按 stack 选字体——回退字形的 advance
    // 用其来源字体算，否则行宽估错导致断行位置偏。kerning 仅相邻同字体才查（跨字体
    // 无 kern 表可言）。
    //
    // `prev` 传（可变引用）行内前一字符的 (gid, font_id)：段首与行内前段的跨段 kern 也
    // 计入。贪心累加必须与 glyph 生成（对整行文本连续算 kern）完全同参——否则 max-content
    // 宽度作约束重测时两侧差一个 kern 量，最后一字被挤到下一行（flex item 定宽 =
    // max-content 的场景必现）。font_id 等值即同字体（id 全局唯一），kern 查表用当前字体。
    let measure_width = |s: &str, prev: &mut Option<(ttf_parser::GlyphId, u32)>| -> f32 {
        let mut pen = 0.0f32;
        for ch in s.chars() {
            let (f, fid) = stack.pick(ch);
            let gid_opt = f.face.glyph_index(ch);
            let gid = gid_opt.unwrap_or_default();
            if let Some((p, pf)) = *prev {
                if pf == fid {
                    if let Some(k) = kerning_value(&f.face, p, gid) {
                        pen += k as f32 / f.face.units_per_em() as f32 * font_size;
                    }
                }
            }
            pen += glyph_advance(&f.face, gid_opt, font_size) + letter_spacing;
            *prev = Some((gid, fid));
        }
        pen
    };

    // 断行（#73 换行控制全集）：预处理（折叠/保留）→ UAX#14 换行机会（word-break 调制）
    // → 贪心填行。UAX#14 的 LB13/LB14 类规则天然覆盖普通软断行的避头尾（行首不出
    // 闭标点）；禁则集只在逐字拆分（overflow-wrap:break-word）的断点上手工调整。
    //
    // unicode-linebreak 0.1.5 API：
    // - `linebreaks(s)` 返回 `impl Iterator<Item=(usize, BreakOpportunity)>`（非 Vec）；
    // - 枚举名是 `BreakOpportunity`，变体 `Mandatory`/`Allowed`；
    // - offset 语义 = "断点之后字符的字节序号"，即前段 = content[..offset]，后段 = content[offset..]。
    use unicode_linebreak::{linebreaks, BreakOpportunity};
    let max_w = max_width.unwrap_or(f32::MAX);
    let soft_wrap = wrap.wrap_enabled();
    // 折叠模式（normal/nowrap/pre-line）下空白串已并成单空格；保留模式原样。
    let content = preprocess_text(content, &wrap);

    // 1+2. 取 break opportunities 并切 segments。
    //    - word-break:keep-all：CJK 字间（两侧均 CJK）的 Allowed 机会撤掉——CJK 按
    //      「词」不折行，只留空格/标点/强制边界（浏览器 keep-all 同义）。
    //    - word-break:break-all：绕过 UAX#14，逐字符切 segment（拉丁词内也可断；
    //      `\n` 保留 Mandatory 段）。禁则由贪心出口的断点调整兜住。
    let mut segments: Vec<(&str, BreakOpportunity)> = Vec::new();
    if wrap.word_break == crate::style::resolved::WordBreak::BreakAll {
        for (off, ch) in content.char_indices() {
            let btype = if ch == '\n' {
                BreakOpportunity::Mandatory
            } else {
                BreakOpportunity::Allowed
            };
            segments.push((&content[off..off + ch.len_utf8()], btype));
        }
    } else {
        let keep_all = wrap.word_break == crate::style::resolved::WordBreak::KeepAll;
        let opportunities: Vec<(usize, BreakOpportunity)> = linebreaks(&content)
            .filter(|&(off, btype)| {
                if keep_all && btype == BreakOpportunity::Allowed {
                    let prev_cjk = content[..off].chars().next_back().is_some_and(is_cjk);
                    let next_cjk = content[off..].chars().next().is_some_and(is_cjk);
                    !(prev_cjk && next_cjk)
                } else {
                    true
                }
            })
            .collect();
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
    }

    // 3. 贪心填行。cur_prev 跟踪行内末字符（跨段 kern）；换行重置（新行段首无 kern）。
    //    超长词（segment 无断点且宽 > 行宽）：
    //    - overflow-wrap:break-word → 逐字填（词独行仍放不下才拆，CSS 语义），
    //      拆点过禁则调整（行首禁则/行尾禁则，见循环内）。
    //    - overflow-wrap:normal → 词独占一行并横向溢出（浏览器一致，不静默拆词）。
    let overflow_break = wrap.overflow_wrap == crate::style::resolved::OverflowWrap::BreakWord;
    let mut lines: Vec<(String, f32)> = Vec::new(); // (text, width)
    let mut cur = String::new();
    let mut cur_w = 0.0f32;
    let mut cur_prev: Option<(ttf_parser::GlyphId, u32)> = None;
    let mut buf = [0u8; 4];
    for (seg, btype) in &segments {
        // mandatory 段自含尾 \n：换行语义由下方 flush 表达，\n 本身不进文本/宽度/glyph
        // 流（送进 shaper 会落 .notdef 字形 = tofu）。rich 路径对 \n token 同此处理。
        let seg = if *btype == BreakOpportunity::Mandatory {
            seg.strip_suffix('\n').unwrap_or(seg)
        } else {
            seg
        };
        // 折叠模式：换行后的行首纯空白段跳过（pre-line 的换行后行首空格移除——
        // preprocess 只折叠串内空白，段级行首悬挂交给这里）。
        if wrap.collapse_spaces() && cur.is_empty() && seg.chars().all(is_fold_ws) {
            continue;
        }
        // probe 不提交：换行时段首 kern 重置，需以无前段状态重测。
        let mut probe = cur_prev;
        let seg_w = measure_width(seg, &mut probe);
        let seg_chars = seg.chars().count();

        if soft_wrap && overflow_break && seg_w > max_w && seg_chars > 1 {
            if !cur.is_empty() {
                lines.push((std::mem::take(&mut cur), cur_w));
                cur_w = 0.0;
                cur_prev = None;
            }
            for ch in seg.chars() {
                let ch_s = ch.encode_utf8(&mut buf);
                let mut probe = cur_prev;
                let mut cw = measure_width(ch_s, &mut probe);
                if !cur.is_empty() && cur_w + cw > max_w + WRAP_FIT_EPS {
                    // 禁则断点调整：ch 将成为下一行行首——若它是行首禁则字符（句读/
                    // 闭括号），或 cur 末字符是行尾禁则字符（开括号），则把 cur 末字符
                    // 一并挪下一行（断点左移，挪下字符随新行重排，不丢失）。连锁
                    //（"。。"、"（（"）由 while 吸收；cur 仅剩 1 字符时退界（防空行循环）。
                    let mut moved: Vec<char> = Vec::new();
                    while cur.chars().count() > 1
                        && (is_kinsoku_no_line_start(ch)
                            || cur.chars().next_back().is_some_and(is_kinsoku_no_line_end))
                    {
                        if let Some(mc) = cur.pop() {
                            moved.insert(0, mc);
                        }
                        let mut fresh = None;
                        cur_w = measure_width(&cur, &mut fresh);
                    }
                    lines.push((std::mem::take(&mut cur), cur_w));
                    cur_w = 0.0;
                    // 挪下字符先进新行（宽度随入行累加；禁则优先于宽度，超宽也随行）。
                    let mut mbuf = [0u8; 4];
                    for mc in moved {
                        let m_s = mc.encode_utf8(&mut mbuf);
                        let mut fresh = None;
                        cur_w += measure_width(m_s, &mut fresh);
                        cur.push(mc);
                    }
                    let mut fresh = None;
                    cw = measure_width(ch_s, &mut fresh);
                    probe = fresh;
                }
                cur.push(ch);
                cur_w += cw;
                cur_prev = probe;
            }
        } else if !soft_wrap || cur.is_empty() || cur_w + seg_w <= max_w + WRAP_FIT_EPS {
            cur.push_str(seg);
            cur_w += seg_w;
            cur_prev = probe;
        } else {
            lines.push((std::mem::take(&mut cur), cur_w));
            cur.push_str(seg);
            let mut fresh = None;
            cur_w = measure_width(seg, &mut fresh);
            cur_prev = fresh;
        }

        // Mandatory break（保留的 \n）强制结束当前行——soft_wrap 关（pre / nowrap 组合）
        // 也照断：断行源是字符本身，不受软换行开关压制。无条件 push：连续 \n 产空行
        //（cur 已剥 \n 可能为空），空行仍占行高（与 rich 路径 break token 一致）。
        if *btype == BreakOpportunity::Mandatory {
            lines.push((std::mem::take(&mut cur), cur_w));
            cur_w = 0.0;
            cur_prev = None;
        }
    }
    // 内容以 \n 结尾（"abc\n"）：mandatory flush 已结束末行，还须补一个空行承载
    // 换行后的光标/后续输入（编辑器语义：回车后 caret 落在新空行）。折叠模式下 \n
    // 已并进空白串，不会误判。注意 unicode-linebreak 把文本末尾也产一个 Mandatory
    // 哨兵断点——不能拿「末段 btype==Mandatory」判定（"abc" 也会命中），须看内容
    // 本身是否以 \n 结尾。
    let ends_mandatory = content.ends_with('\n');
    if !cur.is_empty() || ends_mandatory {
        lines.push((cur, cur_w));
    }
    if lines.is_empty() {
        lines.push((String::new(), 0.0));
    }

    let text_width = lines.iter().map(|(_, w)| *w).fold(0.0f32, f32::max);
    let text_height = lines.len() as f32 * line_h;

    // 生成 glyphs（绝对坐标：已累加 advance + 已应用 align 偏移）。
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
        run_rects: Vec::new(),
    }
}

/// 简化 inline flow measure：runs + 可选宽度 → TextLayout（per-run 样式进 GlyphRun）。
///
/// 算法（搬 fgui BuildLines2 + RmlUi GetStrut/DetermineVerticalPositioning）：
/// 1. 扁平 token 流：每 run 的 text 按 [`WrapControl`] 切 token（CJK 逐字 / Latin
///    逐词，keep-all/break-all 调制；折叠/保留模式决定空白与 `\n` 的 token 形态）。
/// 2. 贪心断行：token 累加超 max_width → 开新行（软换行开关可关）；`\n`/`<br>`
///    强制换行；出口做 kinsoku 断点调整；break-word 超长 token 就地逐字拆。
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
#[allow(clippy::too_many_arguments)]
pub fn measure_rich_text(
    runs: &[crate::text::rich::RichRun],
    max_width: Option<f32>,
    base_line_height: f32,
    letter_spacing: f32,
    align: crate::style::resolved::TextAlign,
    wrap: WrapControl,
    stack: &FontStack<'_>,
) -> TextLayout {
    use crate::style::resolved::{OverflowWrap, WordBreak};
    let font = stack.primary;

    // per-char 按 stack 选字体的 advance 和（token 宽度用）。回退字形 advance 用来源字体算。
    // 含 letter_spacing（与 glyph 定位累加同参）。
    let stack_str_advance = |s: &str, size_px: f32| -> f32 {
        let mut pen = 0.0f32;
        for ch in s.chars() {
            let (f, _) = stack.pick(ch);
            let gid = f.face.glyph_index(ch);
            pen += glyph_advance(&f.face, gid, size_px) + letter_spacing;
        }
        pen
    };

    let mut out_images: Vec<RichImagePlacement> = Vec::new();
    // 命中 rect 累加器（每 input run 每行一条）。见 `RichRunRect`。
    let mut run_rects: Vec<RichRunRect> = Vec::new();

    // 1. 扁平 token 流（token = 子串 + 所属 run 索引 + 宽度 + 是否强制换行）。
    //    词切分（两模式共用）：Latin 逐词；含 CJK 的词逐字拆（keep-all 时整词不拆、
    //    break-all 时 Latin 也逐字）。
    #[derive(Clone, Copy)]
    struct Tok<'a> {
        text: &'a str,
        run_idx: usize,
        w: f32,
        is_break: bool,
        /// 折叠模式补的空格 token：行首可丢弃（浏览器行首悬挂空格移除）。
        /// 保留模式的空格 token false（pre-wrap 空格可见，不可丢）。
        droppable_ws: bool,
    }
    let collapse = wrap.collapse_spaces();
    let keep_all = wrap.word_break == WordBreak::KeepAll;
    let break_all = wrap.word_break == WordBreak::BreakAll;
    // 词 → token 序列（word = 无空白连续串）：break-all 或含 CJK（keep-all 除外）→
    // 逐字；其余整词一 token。嵌套 fn 可见本函数体的 Tok item。
    fn push_word_tokens<'a>(
        tokens: &mut Vec<Tok<'a>>,
        word: &'a str,
        ri: usize,
        size: f32,
        keep_all: bool,
        break_all: bool,
        adv: &dyn Fn(&str, f32) -> f32,
    ) {
        let per_char = break_all || (word.chars().any(is_cjk) && !keep_all);
        // 宏而非闭包：&mut Vec<Tok<'a>> 对 'a 不变（invariant），闭包参数 &str 塞进
        // Tok<'a> 会被判逃逸。宏是文本展开，无此问题。
        macro_rules! push {
            ($s:expr) => {
                tokens.push(Tok {
                    text: $s,
                    run_idx: ri,
                    w: adv($s, size),
                    is_break: false,
                    droppable_ws: false,
                });
            };
        }
        if per_char {
            for (off, ch) in word.char_indices() {
                push!(&word[off..off + ch.len_utf8()]);
            }
        } else {
            push!(word);
        }
    }
    let mut tokens: Vec<Tok> = Vec::new();
    for (ri, r) in runs.iter().enumerate() {
        match &r.kind {
            crate::text::rich::RichKind::Text { text } => {
                if text == "\n" {
                    // `<br>` 编译产 "\n" run：任何模式下都是强制断行。
                    tokens.push(Tok {
                        text: "\n",
                        run_idx: ri,
                        w: 0.0,
                        is_break: true,
                        droppable_ws: false,
                    });
                    continue;
                }
                let size = r.size_px as f32;
                if collapse {
                    // 折叠模式（normal/nowrap/pre-line）：CSS 空白折叠——\t/\r/换页/空格
                    // 串 → 单空格 token；`\n` 依模式折叠（normal/nowrap 并进空白串）或
                    // 产强制断行 token（pre-line）。此前 \n 一律折叠，源换行语义缺失。
                    let mut word_start: Option<usize> = None;
                    let mut in_ws = false;
                    for (bo, ch) in text.char_indices() {
                        let newline_break = ch == '\n' && wrap.preserve_newlines();
                        let is_sep = is_fold_ws(ch) || (ch == '\n' && !wrap.preserve_newlines());
                        if newline_break {
                            if let Some(ws) = word_start.take() {
                                if ws < bo {
                                    push_word_tokens(
                                        &mut tokens,
                                        &text[ws..bo],
                                        ri,
                                        size,
                                        keep_all,
                                        break_all,
                                        &stack_str_advance,
                                    );
                                }
                            }
                            tokens.push(Tok {
                                text: "\n",
                                run_idx: ri,
                                w: 0.0,
                                is_break: true,
                                droppable_ws: false,
                            });
                            in_ws = false;
                        } else if is_sep {
                            if let Some(ws) = word_start.take() {
                                if ws < bo {
                                    push_word_tokens(
                                        &mut tokens,
                                        &text[ws..bo],
                                        ri,
                                        size,
                                        keep_all,
                                        break_all,
                                        &stack_str_advance,
                                    );
                                }
                            }
                            in_ws = true;
                        } else if in_ws {
                            tokens.push(Tok {
                                text: " ",
                                run_idx: ri,
                                w: stack_str_advance(" ", size),
                                is_break: false,
                                droppable_ws: true,
                            });
                            in_ws = false;
                            word_start = Some(bo);
                        } else if word_start.is_none() {
                            word_start = Some(bo);
                        }
                    }
                    // 尾部 flush：词或折叠空格（跨 run 的词尾空格是真实内容，保留 token）。
                    let end = text.len();
                    if let Some(ws) = word_start {
                        if ws < end {
                            push_word_tokens(
                                &mut tokens,
                                &text[ws..end],
                                ri,
                                size,
                                keep_all,
                                break_all,
                                &stack_str_advance,
                            );
                        }
                    } else if in_ws && !text.is_empty() {
                        tokens.push(Tok {
                            text: " ",
                            run_idx: ri,
                            w: stack_str_advance(" ", size),
                            is_break: false,
                            droppable_ws: true,
                        });
                    }
                } else {
                    // 保留模式（pre/pre-wrap）：空白不折叠——每个空白字符独立 token
                    //（占 advance、不可丢）；\n 产断行 token；词切分同折叠模式。
                    let mut word_start: Option<usize> = None;
                    for (bo, ch) in text.char_indices() {
                        if ch == '\n' {
                            if let Some(ws) = word_start.take() {
                                if ws < bo {
                                    push_word_tokens(
                                        &mut tokens,
                                        &text[ws..bo],
                                        ri,
                                        size,
                                        keep_all,
                                        break_all,
                                        &stack_str_advance,
                                    );
                                }
                            }
                            tokens.push(Tok {
                                text: "\n",
                                run_idx: ri,
                                w: 0.0,
                                is_break: true,
                                droppable_ws: false,
                            });
                        } else if is_fold_ws(ch) {
                            if let Some(ws) = word_start.take() {
                                if ws < bo {
                                    push_word_tokens(
                                        &mut tokens,
                                        &text[ws..bo],
                                        ri,
                                        size,
                                        keep_all,
                                        break_all,
                                        &stack_str_advance,
                                    );
                                }
                            }
                            tokens.push(Tok {
                                text: &text[bo..bo + ch.len_utf8()],
                                run_idx: ri,
                                w: stack_str_advance(&text[bo..bo + ch.len_utf8()], size),
                                is_break: false,
                                droppable_ws: false,
                            });
                        } else if word_start.is_none() {
                            word_start = Some(bo);
                        }
                    }
                    if let Some(ws) = word_start {
                        if ws < text.len() {
                            push_word_tokens(
                                &mut tokens,
                                &text[ws..text.len()],
                                ri,
                                size,
                                keep_all,
                                break_all,
                                &stack_str_advance,
                            );
                        }
                    }
                }
            }
            crate::text::rich::RichKind::Image { w, .. } => {
                tokens.push(Tok {
                    text: "",
                    run_idx: ri,
                    w: *w,
                    is_break: false,
                    droppable_ws: false,
                });
            }
        }
    }

    // 2. 贪心断行（#73 模式机）：token 累加超 max_width → 开新行；is_break（\n/<br>）
    //    强制换行（不受软换行开关压制）。首个 token 不论宽度都入行（防零宽 token 死循环）。
    //    line_prev 跟踪行内末字符 (gid, font_id)：token 首字符与行内前 token 末字符的跨
    //    token kern 计入累加（glyph 定位对整行连续算 kern）——否则 max-content 宽度作
    //    约束重测时两侧差一个 kern 量，token 被提前挤到下一行（flex item 定宽场景必现）。
    let soft_wrap = wrap.wrap_enabled();
    let overflow_break = wrap.overflow_wrap == OverflowWrap::BreakWord;
    let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
    let mut cur_w = 0.0f32;
    let mut line_prev: Option<(ttf_parser::GlyphId, u32)> = None;
    let mut ti = 0usize;
    while ti < tokens.len() {
        let tok = tokens[ti]; // Copy：下面 break-word 分支要 splice，不能持借用
        if tok.is_break {
            lines.push(Vec::new());
            cur_w = 0.0;
            line_prev = None;
            ti += 1;
            continue;
        }
        // 行首可丢弃空格（折叠模式）：content 首/换行后的行首空格直接跳过（浏览器
        // 行首悬挂空格移除语义）。保留模式的空格 token 不可丢。
        if tok.droppable_ws && lines.last().is_some_and(|l| l.is_empty()) {
            ti += 1;
            continue;
        }
        // overflow-wrap:break-word：token 独占一行仍超行宽 → 就地拆成逐字 token 重进
        // 循环（拆点的禁则由下方断点调整兜住）。
        if soft_wrap
            && overflow_break
            && lines.last().is_some_and(|l| l.is_empty())
            && tok.text.chars().count() > 1
            && max_width.is_some_and(|mw| tok.w > mw)
        {
            let ri = tok.run_idx;
            let size = runs[ri].size_px as f32;
            let text = tok.text;
            let split: Vec<Tok> = text
                .char_indices()
                .map(|(off, ch)| {
                    let s = &text[off..off + ch.len_utf8()];
                    Tok {
                        text: s,
                        run_idx: ri,
                        w: stack_str_advance(s, size),
                        is_break: false,
                        droppable_ws: false,
                    }
                })
                .collect();
            tokens.splice(ti..ti + 1, split);
            continue; // 以首字符 token 重进循环
        }
        // token 首字符的跨 token kern（按 token 所属 run 字号缩放，与 glyph 定位同参）。
        let kern0 = match (line_prev, tok.text.chars().next()) {
            (Some((p, pf)), Some(ch)) => {
                let (f, fid) = stack.pick(ch);
                let gid = f.face.glyph_index(ch).unwrap_or_default();
                if pf == fid {
                    kerning_value(&f.face, p, gid)
                        .map(|k| {
                            k as f32 / f.face.units_per_em() as f32
                                * runs[tok.run_idx].size_px as f32
                        })
                        .unwrap_or(0.0)
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        let mut eff = tok.w + kern0;
        // WRAP_FIT_EPS：见常量注释——贪心累加与 glyph pen 累加的浮点顺序差。
        let fits = !soft_wrap
            || max_width.is_none_or(|mw| cur_w + eff <= mw + WRAP_FIT_EPS)
            || lines.last().is_some_and(|l| l.is_empty());
        if !fits {
            lines.push(Vec::new());
            cur_w = 0.0;
            line_prev = None;
            eff = tok.w; // 行首 token 无前段 kern。
                         // kinsoku 断点调整：tok 将成为新行行首——若它是行首禁则字符（句读/闭
                         // 括号），或上一行末 token 首字符是行尾禁则字符（开括号），把上一行末
                         // token 挪下新行（断点左移）。连锁（"。。"/"（（"）由 loop 吸收；上一行
                         // 只剩 1 个 token 时退界。挪下来的宽度累进 cur_w（后续 fits 判定用）。
            loop {
                let prev = lines.len() - 2;
                if lines[prev].len() <= 1 {
                    break;
                }
                let moved_ti = *lines[prev].last().unwrap();
                let next_first = lines[lines.len() - 1]
                    .first()
                    .map(|&mti| tokens[mti].text.chars().next())
                    .unwrap_or_else(|| tok.text.chars().next());
                let prev_last = tokens[moved_ti].text.chars().next();
                if next_first.is_some_and(is_kinsoku_no_line_start)
                    || prev_last.is_some_and(is_kinsoku_no_line_end)
                {
                    lines[prev].pop();
                    lines.last_mut().unwrap().insert(0, moved_ti);
                    cur_w += tokens[moved_ti].w;
                } else {
                    break;
                }
            }
        }
        lines.last_mut().unwrap().push(ti);
        cur_w += eff;
        // 行内末字符 (gid, font_id)；image token（空 text）保持 prev 不变（glyph 定位同此）。
        if let Some(last_ch) = tok.text.chars().next_back() {
            let (f, fid) = stack.pick(last_ch);
            let gid = f.face.glyph_index(last_ch).unwrap_or_default();
            line_prev = Some((gid, fid));
        }
        ti += 1;
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
        // baseline：half-leading 居中（与 plain measure_text 同公式）。此前直接取 ascent
        // 会忽略 line-height 倍数——同一 flex 行里 span（rich 路径）与匿名文本（plain
        // 路径）基线差数 px，视觉上不齐。
        let baseline = (h + line_ascent - line_descent) / 2.0 - line_descent.abs();

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
        // 命中 rect 累加器（本行）：text 按 input run_idx 记 x 跨度（pre-dx），image 记完整 rect。
        // key=run_idx 而非 source——同一 source（如 span 多 TextNode 子）的多个 run 仍独立，
        // 保留 span 内夹 image 时的命中粒度（左 text / image / 右 text 三段不合并）。
        let mut text_extents: Vec<(usize, f32, f32)> = Vec::new(); // (run_idx, x0, x1)
        let mut img_extents: Vec<(usize, f32, f32, f32, f32)> = Vec::new(); // (run_idx, x, y, w, h)
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
                        pen_x += adv + letter_spacing;
                        prev = Some((gid, f));
                    }
                    // 命中 rect：记本 run 在本行的 x 跨度（pre-align-dx，末尾统一加 dx）。
                    // 渲染层下方会把同 style 相邻 run 合并进一个 GlyphRun，但这里按 input
                    // run_idx 独立记账——保留命中粒度（点落左 run → 其 source，右 run → 其 source）。
                    if let (Some(first), Some(last)) = (glyphs.first(), glyphs.last()) {
                        let (x0, x1) = (first.x, last.x + last.advance);
                        match text_extents
                            .iter_mut()
                            .find(|(ri, _, _)| *ri == tokens[ti].run_idx)
                        {
                            Some(slot) => {
                                slot.1 = slot.1.min(x0);
                                slot.2 = slot.2.max(x1);
                            }
                            None => text_extents.push((tokens[ti].run_idx, x0, x1)),
                        }
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
                    // 命中 rect：image run 直接用 placement（pre-dx，末尾统一加 dx）。
                    img_extents.push((tokens[ti].run_idx, pen_x, y + y_top, img_w, img_h));
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
        // 推本行 run_rects（text + image），dx 已统一应用。source 取 input run 的 source
        // （span 子 TextNode → span.id；rich-text-block 直接 TextNode 子 → TextNode.id）。
        for (run_idx, x0, x1) in text_extents {
            run_rects.push(RichRunRect {
                x: x0 + dx,
                y,
                w: (x1 - x0).max(0.0),
                h,
                source: runs[run_idx].source,
            });
        }
        for (run_idx, ix, iy, iw, ih) in img_extents {
            run_rects.push(RichRunRect {
                x: ix + dx,
                y: iy,
                w: iw,
                h: ih,
                source: runs[run_idx].source,
            });
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
        run_rects,
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
    use crate::scene::node::NodeId;
    use crate::style::resolved::TextAlign;
    /// 测试用换行控制快捷构造：normal / nowrap。
    fn wc_normal() -> WrapControl {
        WrapControl::default()
    }
    fn wc_nowrap() -> WrapControl {
        WrapControl {
            white_space: crate::style::resolved::WhiteSpace::Nowrap,
            ..Default::default()
        }
    }
    /// 文本控件语义（control_wrap_control 的折叠面）：保留空格与换行。
    fn wc_pre_wrap() -> WrapControl {
        WrapControl {
            white_space: crate::style::resolved::WhiteSpace::PreWrap,
            ..Default::default()
        }
    }

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

    /// tofu 取证日志：pick 全链缺字记录（family+char）、会话级去重、take 排空。
    /// 回退链覆盖的字不算缺（不画 tofu）；清空回退后同字才进报告。
    #[test]
    fn missing_glyph_pick_records_dedups_and_drains() {
        let dejavu = format!(
            "{}/tests/fixtures/DejaVuSans.ttf",
            env!("CARGO_MANIFEST_DIR")
        );
        let wqy = format!(
            "{}/tests/fixtures/wqy-microhei.ttc",
            env!("CARGO_MANIFEST_DIR")
        );
        let (Some(dj_bytes), Some(wk_bytes)) =
            (std::fs::read(&dejavu).ok(), std::fs::read(&wqy).ok())
        else {
            return; // fixture 缺则跳过
        };
        let mut ft = FontTable::new();
        ft.register("DejaVu", dj_bytes, true).unwrap();
        ft.register("WQY", wk_bytes, false).unwrap();

        // 有字 / 回退覆盖：不记录。
        let stack = ft.stack_for(Some("DejaVu"));
        stack.pick('a');
        ft.set_fallback_families(&["WQY".to_string()]);
        let stack = ft.stack_for(Some("DejaVu"));
        stack.pick('中'); // DejaVu 缺，WQY 补上 → 无 tofu，不记录
        assert!(ft.take_missing_glyph_reports().is_empty());

        // 无回退：全链缺 → 记录一次，重复 pick 去重。
        ft.set_fallback_families(&[]);
        let stack = ft.stack_for(Some("DejaVu"));
        stack.pick('中');
        stack.pick('中');
        let reports = ft.take_missing_glyph_reports();
        assert_eq!(reports.len(), 1, "same family+char deduped: {reports:?}");
        assert!(
            reports[0].contains("DejaVu"),
            "names the family: {}",
            reports[0]
        );
        assert!(
            reports[0].contains("U+4E2D"),
            "names the codepoint: {}",
            reports[0]
        );
        assert!(
            ft.take_missing_glyph_reports().is_empty(),
            "take drains pending"
        );

        // 未命中 family → 按实际生效（default）名记录。换字避开会话去重（'中' 已报过）。
        let stack = ft.stack_for(Some("NoSuchFamily"));
        stack.pick('あ');
        let reports = ft.take_missing_glyph_reports();
        assert!(
            reports[0].contains("DejaVu"),
            "unknown family reports default: {}",
            reports[0]
        );
    }

    #[test]
    fn newline_produces_line_break_not_tofu_glyph() {
        let Some(f) = test_font() else { return };
        let stack = FontStack::single(&f, 0);
        // "a\nb" → 2 行，glyph 只含 a/b（\n 落 .notdef = tofu 是旧 bug）。
        let l = measure_text(
            "a\nb",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_pre_wrap(),
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(l.lines.len(), 2);
        let cps: Vec<u32> = l
            .lines
            .iter()
            .flat_map(|li| li.runs.iter())
            .flat_map(|r| r.glyphs.iter())
            .map(|g| g.codepoint)
            .collect();
        assert!(
            !cps.contains(&('\n' as u32)),
            "newline must not become a glyph (tofu): {cps:?}"
        );
        assert_eq!(cps, vec!['a' as u32, 'b' as u32]);
    }

    #[test]
    fn trailing_newline_yields_empty_caret_line() {
        // 编辑器语义：value 以 \n 结尾（回车后）→ 光标落在新空行（须有该空行承载）。
        // unicode-linebreak 末尾 Mandatory 哨兵断点不算（"abc" 不得多出空行）。
        let Some(f) = test_font() else { return };
        let stack = FontStack::single(&f, 0);
        let l = measure_text(
            "a\n",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_pre_wrap(),
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(l.lines.len(), 2);
        let plain = measure_text(
            "abc",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_pre_wrap(),
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(plain.lines.len(), 1);
        // caret 在 value 末字节 → 落第 2 行行首。
        let ranges = crate::scene::text_cursor::line_byte_ranges(&l, "a\n");
        assert_eq!(ranges, vec![(0, 2), (2, 2)]);
        let (_, li) = crate::scene::text_cursor::cursor_pixel_x(&l, &ranges, 2);
        assert_eq!(li, 1);
    }

    #[test]
    fn consecutive_newlines_produce_empty_lines() {
        let Some(f) = test_font() else { return };
        let stack = FontStack::single(&f, 0);
        // "a\n\nb" → 3 行（中间空行占行高），value 的 \n 由 caret 映射补消费。
        let l = measure_text(
            "a\n\nb",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_pre_wrap(),
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(l.lines.len(), 3);
        assert!(l.lines[1].runs.iter().all(|r| r.glyphs.is_empty()));
        let ranges = crate::scene::text_cursor::line_byte_ranges(&l, "a\n\nb");
        assert_eq!(ranges, vec![(0, 2), (2, 3), (3, 4)]);
    }

    #[test]
    fn plain_remeasure_at_max_content_width_never_wraps() {
        // 边界回归（WRAP_FIT_EPS）：max-content 宽作约束重测必须仍单行——
        // flex item 定宽 = max-content 场景，浮点顺序差会挤掉末 token。
        let Some(f) = test_font() else { return };
        let stack = FontStack::single(&f, 0);
        let text = "ab cd ef gh";
        let l = measure_text(
            text,
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_normal(),
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        let l2 = measure_text(
            text,
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_normal(),
            Some(l.text_width),
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(
            l2.lines.len(),
            1,
            "re-measure at own max-content width must not wrap"
        );
    }

    /// 标签间空白文本节点（HTML 源码换行+缩进）不再产 tofu：折叠为单空格 token，
    /// 无 .notdef(gid 0) 字形进渲染。修复前的形态："\n    " run 的换行成为独立
    /// word token，cmap 无控制字符映射 → gid 0 → tofu 框 + .notdef advance。
    #[test]
    fn rich_inter_element_whitespace_collapses_not_tofu() {
        let Some(f) = test_font() else { return };
        let stack = FontStack::single(&f, 0);
        let mk = |t: &str| crate::text::rich::RichRun {
            kind: crate::text::rich::RichKind::Text { text: t.into() },
            color: [1.0; 4],
            font_id: 0,
            size_px: 16,
            weight: Default::default(),
            style: crate::text::rich::RichStyle::Normal,
            deco: Default::default(),
            link_id: None,
            source: crate::scene::NodeId::INVALID,
        };
        // run 形态 = <span>a</span>\n    <span>b</span> 的编译产物。
        let runs = vec![mk("a"), mk("\n    "), mk("b")];
        let l = measure_rich_text(
            &runs,
            None,
            0.0,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
            &stack,
        );
        let glyphs: Vec<u16> = l
            .lines
            .iter()
            .flat_map(|ln| ln.runs.iter())
            .flat_map(|r| r.glyphs.iter().map(|g| g.glyph_id))
            .collect();
        assert!(
            glyphs.iter().all(|&g| g != 0),
            "no .notdef glyphs allowed, got {glyphs:?}"
        );
        // 单行 a␣b：3 字形（a + 空格 + b），宽度 = 三个 advance 之和。
        assert_eq!(l.lines.len(), 1);
        assert_eq!(glyphs.len(), 3, "a + collapsed space + b");
        let adv = |c: char| {
            let gid = f.face.glyph_index(c).unwrap();
            f.face.glyph_hor_advance(gid).unwrap() as f32 / f.face.units_per_em() as f32 * 16.0
        };
        assert!(
            (l.text_width - (adv('a') + adv(' ') + adv('b'))).abs() < 0.05,
            "width = a + space + b, got {}",
            l.text_width
        );
        // 词内换行（"a\nb" 单 run）同样折叠：不分 tofu、成 a␣b。
        let runs = vec![mk("a\nb")];
        let l = measure_rich_text(
            &runs,
            None,
            0.0,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
            &stack,
        );
        let glyphs: Vec<u16> = l
            .lines
            .iter()
            .flat_map(|ln| ln.runs.iter())
            .flat_map(|r| r.glyphs.iter().map(|g| g.glyph_id))
            .collect();
        assert!(
            glyphs.iter().all(|&g| g != 0),
            "in-word \n folded: {glyphs:?}"
        );
        assert_eq!(glyphs.len(), 3, "a + space + b");
    }

    #[test]
    fn rich_baseline_applies_half_leading_for_line_height() {
        // rich 路径 baseline 与 plain 同公式（half-leading 居中）：line-height 倍数
        // 必须把 baseline 压低 (line_h - (A+D))/2，而非钉在 ascent——否则同 flex 行里
        // span（rich）与匿名文本（plain）基线错位数 px。
        let Some(f) = test_font() else { return };
        let stack = FontStack::single(&f, 0);
        let runs = vec![crate::text::rich::RichRun {
            kind: crate::text::rich::RichKind::Text { text: "x".into() },
            color: [1.0; 4],
            font_id: 0,
            size_px: 16,
            weight: Default::default(),
            style: crate::text::rich::RichStyle::Normal,
            deco: Default::default(),
            link_id: None,
            source: NodeId(0),
        }];
        let lh = 2.0f32;
        let plain = measure_text(
            "x",
            16.0,
            lh,
            0.0,
            TextAlign::Left,
            wc_normal(),
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        let rich = measure_rich_text(
            &runs,
            None,
            lh,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
            &stack,
        );
        assert_eq!(plain.lines[0].baseline, rich.lines[0].baseline);
        let ascent = f.ascent(16.0);
        assert!(
            rich.lines[0].baseline > ascent,
            "baseline {:.2} should exceed raw ascent {:.2} (half-leading)",
            rich.lines[0].baseline,
            ascent
        );
    }

    #[test]
    fn rich_letter_spacing_widens_layout() {
        let Some(f) = test_font() else { return };
        let stack = FontStack::single(&f, 0);
        let runs = vec![crate::text::rich::RichRun {
            kind: crate::text::rich::RichKind::Text { text: "abc".into() },
            color: [1.0; 4],
            font_id: 0,
            size_px: 16,
            weight: Default::default(),
            style: crate::text::rich::RichStyle::Normal,
            deco: Default::default(),
            link_id: None,
            source: NodeId(0),
        }];
        let w0 = measure_rich_text(
            &runs,
            None,
            0.0,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
            &stack,
        )
        .text_width;
        let w5 = measure_rich_text(
            &runs,
            None,
            0.0,
            5.0,
            TextAlign::Left,
            WrapControl::default(),
            &stack,
        )
        .text_width;
        assert!(
            w5 > w0 + 14.0,
            "letter-spacing 5px x3 chars should widen by ~15: {w0} -> {w5}"
        );
    }

    #[test]
    fn fingerprint_deterministic_for_same_inputs() {
        let a = text_fingerprint(
            "hello",
            16.0,
            1.5,
            0.0,
            TextAlign::Left,
            wc_normal(),
            400,
            None,
            None,
        );
        let b = text_fingerprint(
            "hello",
            16.0,
            1.5,
            0.0,
            TextAlign::Left,
            wc_normal(),
            400,
            None,
            None,
        );
        assert_eq!(a, b, "同输入必同 fingerprint（DefaultHasher 固定 key）");
    }

    #[test]
    fn fingerprint_differs_on_content() {
        let a = text_fingerprint(
            "alice",
            16.0,
            1.5,
            0.0,
            TextAlign::Left,
            wc_normal(),
            400,
            None,
            None,
        );
        let b = text_fingerprint(
            "bob",
            16.0,
            1.5,
            0.0,
            TextAlign::Left,
            wc_normal(),
            400,
            None,
            None,
        );
        assert_ne!(
            a, b,
            "content 变 → fingerprint 变（set_text/slot 换内容失效靠这个）"
        );
    }

    #[test]
    fn fingerprint_differs_on_style() {
        let base = |fs| {
            text_fingerprint(
                "hi",
                fs,
                1.5,
                0.0,
                TextAlign::Left,
                WrapControl::default(),
                400,
                None,
                None,
            )
        };
        assert_ne!(base(16.0), base(18.0), "font_size 变 → fp 变");
        let w = |fw| {
            text_fingerprint(
                "hi",
                16.0,
                1.5,
                0.0,
                TextAlign::Left,
                WrapControl::default(),
                fw,
                None,
                None,
            )
        };
        assert_ne!(w(400), w(700), "font_weight 变 → fp 变");
    }

    #[test]
    fn fingerprint_intrinsic_vs_constrained_differ() {
        let intrinsic = text_fingerprint(
            "hi",
            16.0,
            1.5,
            0.0,
            TextAlign::Left,
            wc_normal(),
            400,
            None,
            None,
        );
        let constrained = text_fingerprint(
            "hi",
            16.0,
            1.5,
            0.0,
            TextAlign::Left,
            wc_normal(),
            400,
            None,
            Some(200.0),
        );
        assert_ne!(intrinsic, constrained, "None vs Some 必区分（两槽各自键）");
    }

    #[test]
    fn fingerprint_quantizes_max_width_to_quarter_px_bucket() {
        let base = |w| {
            text_fingerprint(
                "hi",
                16.0,
                1.5,
                0.0,
                TextAlign::Left,
                wc_normal(),
                400,
                None,
                Some(w),
            )
        };
        // 同 0.25px 桶内（w*4 round 同值）→ 同 fp（避免亚像素抖动 thrash 缓存）
        // 桶边界：round(w*4)=800 ⟺ w*4∈[799.5,800.5) ⟺ w∈[199.875,200.125)
        assert_eq!(
            base(200.0),
            base(200.1),
            "200.0(800) 与 200.1(800.4→800) 同桶"
        );
        assert_eq!(base(200.0), base(200.12), "200.12(800.48→800) 仍同桶");
        // 跨桶 → 不同 fp
        assert_ne!(base(200.0), base(200.2), "200.2(800.8→801) 跨桶");
        assert_ne!(base(200.0), base(201.0), "201.0 跨桶");
    }

    #[test]
    fn measure_cache_default_both_slots_none() {
        let c = TextMeasureCache::default();
        assert!(c.intrinsic.is_none());
        assert!(c.constrained.is_none());
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
            wc_normal(),
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
    /// （"text-align:center 却左对齐" 症状）。
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
            wc_normal(),
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
            wc_normal(),
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
    /// 可安全 honor kern。
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
            wc_normal(),
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
    /// 核心权威：缺字画 .notdef/tofu，advance 确定性，不再猜 Unity fallback 1em。
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
            wc_normal(),
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
            wc_normal(),
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
            wc_nowrap(),
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
            wc_normal(),
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
            wc_normal(),
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
        let layout = measure_text(
            "aaaa\nbbbb",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_pre_wrap(),
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
            wc_nowrap(),
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
        // #73：overflow-wrap:break-word 才拆超长词（normal = 词独行溢出，CSS 语义）。
        let break_word = WrapControl {
            overflow_wrap: crate::style::resolved::OverflowWrap::BreakWord,
            ..Default::default()
        };
        let layout = measure_text(
            "aaaaaaaaaaaaaaaaaaaa",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            break_word,
            Some(50.0),
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
        );
        assert!(
            layout.lines.len() >= 2,
            "break-word 超长无空格串应逐字断 >=2 行"
        );
        // normal：同样串不拆——单行且宽超约束（溢出，浏览器一致）。
        let layout = measure_text(
            "aaaaaaaaaaaaaaaaaaaa",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_normal(),
            Some(50.0),
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
        );
        assert_eq!(
            layout.lines.len(),
            1,
            "overflow-wrap:normal 超长词不拆（溢出）"
        );
        assert!(layout.text_width > 50.0);
    }

    // ===== #73 换行控制全集测试 =====

    /// CJK rich run 快捷构造（wqy 全宽字符 advance ≈ size_px）。
    fn cjk_run(text: &str) -> RichRun {
        RichRun {
            kind: RichKind::Text { text: text.into() },
            color: [1.0; 4],
            font_id: 0,
            size_px: 16,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco::default(),
            link_id: None,
            source: NodeId(0),
        }
    }

    fn line_codepoints(l: &TextLayout, i: usize) -> Vec<char> {
        l.lines[i]
            .runs
            .iter()
            .flat_map(|r| r.glyphs.iter())
            .map(|g| char::from_u32(g.codepoint).unwrap())
            .collect()
    }

    #[test]
    fn white_space_normal_folds_newline_and_runs() {
        let Some(f) = test_font() else { return };
        let stack = FontStack::single(&f, 0);
        // \n 折叠为空格 → 单行；连续空格折叠为单空格（宽度与 "a b" 逐位相等）。
        let nl = measure_text(
            "a\nb",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_normal(),
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(nl.lines.len(), 1, "normal 模式 \\n 应折叠为空格（单行）");
        let folded = measure_text(
            "a  b",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_normal(),
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        let single = measure_text(
            "a b",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_normal(),
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(folded.text_width, single.text_width);
        // 首尾空白裁去："  a  " 宽 == "a" 宽。
        let edge = measure_text(
            "  a  ",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_normal(),
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        let bare = measure_text(
            "a",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_normal(),
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(edge.text_width, bare.text_width);
    }

    #[test]
    fn white_space_pre_preserves_newlines_and_disables_wrap() {
        let Some(f) = test_font() else { return };
        let stack = FontStack::single(&f, 0);
        let pre = WrapControl {
            white_space: crate::style::resolved::WhiteSpace::Pre,
            ..Default::default()
        };
        let l = measure_text(
            "a\nb",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            pre,
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(l.lines.len(), 2, "pre 保留 \\n 强制断行");
        let long = measure_text(
            "aaaaaaaaaaaa",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            pre,
            Some(40.0),
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(long.lines.len(), 1, "pre 不自动换行");
        assert!(long.text_width > 40.0);
    }

    #[test]
    fn white_space_pre_wrap_preserves_spaces_and_wraps() {
        let Some(f) = test_font() else { return };
        let stack = FontStack::single(&f, 0);
        let pw = WrapControl {
            white_space: crate::style::resolved::WhiteSpace::PreWrap,
            ..Default::default()
        };
        // 空格不折叠："a  b" 宽 > "a b" 宽。
        let two = measure_text(
            "a  b",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            pw,
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        let one = measure_text(
            "a b",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            pw,
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert!(two.text_width > one.text_width, "pre-wrap 空格保留");
        // 自动换行开：带断点的长串拆行；\n 仍强制断（不可断长词在 pre-wrap 下
        // 溢出不拆——overflow-wrap:normal 语义，见 break_word 测试）。
        let l = measure_text(
            "aaa aaa aaa\nb",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            pw,
            Some(40.0),
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert!(l.lines.len() >= 3, "pre-wrap 自动换行 + \\n 强制断");
    }

    #[test]
    fn white_space_pre_line_collapses_spaces_keeps_newlines() {
        let Some(f) = test_font() else { return };
        let stack = FontStack::single(&f, 0);
        let pl = WrapControl {
            white_space: crate::style::resolved::WhiteSpace::PreLine,
            ..Default::default()
        };
        // 空格折叠、\n 保留；换行后行首空格移除（line2 直起 'b'）。
        let l = measure_text(
            "a  b\n  c",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            pl,
            None,
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(l.lines.len(), 2, "pre-line 保留 \\n 断行");
        assert_eq!(
            line_codepoints(&l, 0),
            vec!['a', ' ', 'b'],
            "行内空格折叠为单空格"
        );
        assert_eq!(line_codepoints(&l, 1), vec!['c'], "换行后行首空格移除");
    }

    #[test]
    fn white_space_nowrap_folds_newline_and_never_wraps() {
        let Some(f) = test_font() else { return };
        let stack = FontStack::single(&f, 0);
        let l = measure_text(
            "a\nb ccc",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_nowrap(),
            Some(30.0),
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(l.lines.len(), 1, "nowrap 折叠 \\n 且不自动换行");
        assert!(l.text_width > 30.0);
    }

    #[test]
    fn text_wrap_nowrap_keeps_forced_breaks_disables_soft_wrap() {
        let Some(f) = test_font() else { return };
        let stack = FontStack::single(&f, 0);
        // pre-wrap + text-wrap:nowrap：空格/换行保留，\\n 断行，软换行关。
        let tw = WrapControl {
            white_space: crate::style::resolved::WhiteSpace::PreWrap,
            text_wrap: crate::style::resolved::TextWrap::Nowrap,
            ..Default::default()
        };
        let l = measure_text(
            "aaa\nbbbbbbbb",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            tw,
            Some(30.0),
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(l.lines.len(), 2, "\\n 仍断、软换行关");
        assert!(l.lines[1].width > 30.0);
    }

    #[test]
    fn word_break_keep_all_cjk_one_line() {
        let Some(f) = test_font_cjk() else { return };
        let stack = FontStack::single(&f, 0);
        let keep = WrapControl {
            word_break: crate::style::resolved::WordBreak::KeepAll,
            ..Default::default()
        };
        let text = "一二三四五";
        let ka = measure_text(
            text,
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            keep,
            Some(32.0),
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(ka.lines.len(), 1, "keep-all CJK 词内不断（溢出单行）");
        let n = measure_text(
            text,
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_normal(),
            Some(32.0),
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert!(n.lines.len() >= 3, "normal CJK 逐字断");
    }

    #[test]
    fn word_break_break_all_latin_per_char() {
        let Some(f) = test_font() else { return };
        let stack = FontStack::single(&f, 0);
        let ba = WrapControl {
            word_break: crate::style::resolved::WordBreak::BreakAll,
            ..Default::default()
        };
        let l = measure_text(
            "aaaaaa",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            ba,
            Some(24.0),
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert!(l.lines.len() >= 2, "break-all 拉丁词逐字断");
        let n = measure_text(
            "aaaaaa",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_normal(),
            Some(24.0),
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert_eq!(n.lines.len(), 1, "normal 拉丁整词不断（溢出）");
    }

    #[test]
    fn kinsoku_plain_never_starts_line_with_full_stop() {
        let Some(f) = test_font_cjk() else { return };
        let stack = FontStack::single(&f, 0);
        let l = measure_text(
            "一二三四。五六七。八九",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_normal(),
            Some(40.0),
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert!(l.lines.len() >= 3);
        for (i, _) in l.lines.iter().enumerate() {
            let cps = line_codepoints(&l, i);
            assert!(
                cps.first().is_none_or(|c| !is_kinsoku_no_line_start(*c)),
                "行 {} 以禁则字符起头：{:?}",
                i,
                cps
            );
        }
    }

    #[test]
    fn kinsoku_break_word_moves_not_drops_chars() {
        // break-word 逐字拆 + 禁则断点左移：挪下的字符必须随新行保留（字符守恒），
        // 不许在调整中丢失。wqy 同时覆盖拉丁与 CJK 标点，单字体即可触发。
        let Some(f) = test_font_cjk() else { return };
        let stack = FontStack::single(&f, 0);
        let bw = WrapControl {
            overflow_wrap: crate::style::resolved::OverflowWrap::BreakWord,
            ..Default::default()
        };
        let text = "aaaaaaaa。bbbb";
        let l = measure_text(
            text,
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            bw,
            Some(40.0),
            &stack,
            [1.0; 4],
            Default::default(),
        );
        assert!(
            l.lines.len() >= 3,
            "应触发逐字拆 + 禁则调整：{}",
            l.lines.len()
        );
        let joined: String = l
            .lines
            .iter()
            .flat_map(|li| li.runs.iter())
            .flat_map(|r| r.glyphs.iter())
            .map(|g| char::from_u32(g.codepoint).unwrap())
            .collect();
        assert_eq!(joined, text, "禁则调整不得丢字");
        for (i, _) in l.lines.iter().enumerate() {
            let cps = line_codepoints(&l, i);
            assert!(
                cps.first().is_none_or(|c| !is_kinsoku_no_line_start(*c)),
                "行 {} 以禁则字符起头：{:?}",
                i,
                cps
            );
        }
    }

    #[test]
    fn kinsoku_rich_moves_break_point_left() {
        let Some(f) = test_font_cjk() else { return };
        let stack = FontStack::single(&f, 0);
        let runs = vec![cjk_run("一二三四。五")];
        // 宽 32 = 2 字/行。贪心原断点：一二 | 三四 | 。五（行首禁则违例）→
        // 断点左移：一二 | 三 | 四。 | 五。
        let l = measure_rich_text(
            &runs,
            Some(32.0),
            1.2,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
            &stack,
        );
        assert!(
            l.lines.len() >= 4,
            "应有禁则调整后的 4 行：{}",
            l.lines.len()
        );
        assert_eq!(
            line_codepoints(&l, 2),
            vec!['四', '。'],
            "句号随前字下移（断点左移），行 3 = 四。"
        );
    }

    #[test]
    fn kinsoku_rich_never_ends_line_with_open_bracket() {
        let Some(f) = test_font_cjk() else { return };
        let stack = FontStack::single(&f, 0);
        let runs = vec![cjk_run("一二（三")];
        // 宽 48 恰容 3 字：贪心原断点 一二（ | 三 → 行尾禁则违例（开括号悬行尾）→
        // 断点右移：一二 | （三。
        let l = measure_rich_text(
            &runs,
            Some(48.0),
            1.2,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
            &stack,
        );
        assert!(l.lines.len() >= 2);
        let cps0 = line_codepoints(&l, 0);
        assert!(
            cps0.last().is_none_or(|c| !is_kinsoku_no_line_end(*c)),
            "行 1 不得以开括号收尾：{:?}",
            cps0
        );
    }

    #[test]
    fn rich_nowrap_now_respected() {
        // #73 前 rich 路忽略 nowrap（MeasureContext 携值未消费）——现在真正接线。
        let Some(f) = test_font_cjk() else { return };
        let runs = vec![cjk_run("一二三四五六")];
        let l = measure_rich_text(
            &runs,
            Some(32.0),
            1.2,
            0.0,
            TextAlign::Left,
            wc_nowrap(),
            &FontStack::single(&f, 0),
        );
        assert_eq!(l.lines.len(), 1, "rich nowrap 单行");
        assert!(l.text_width > 32.0);
    }

    #[test]
    fn overflow_wrap_break_word_rich_splits_long_token() {
        let Some(f) = test_font() else { return };
        let bw = WrapControl {
            overflow_wrap: crate::style::resolved::OverflowWrap::BreakWord,
            ..Default::default()
        };
        let runs = vec![RichRun {
            kind: RichKind::Text {
                text: "aaaaaaaaaaaa".into(),
            },
            color: [1.0; 4],
            font_id: 0,
            size_px: 16,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco::default(),
            link_id: None,
            source: NodeId(0),
        }];
        let split = measure_rich_text(
            &runs,
            Some(40.0),
            1.2,
            0.0,
            TextAlign::Left,
            bw,
            &FontStack::single(&f, 0),
        );
        assert!(split.lines.len() >= 2, "break-word 超长 token 逐字拆");
        let n = measure_rich_text(
            &runs,
            Some(40.0),
            1.2,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
            &FontStack::single(&f, 0),
        );
        assert_eq!(n.lines.len(), 1, "normal 超长 token 独行溢出");
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
            wc_normal(),
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
            wc_normal(),
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
            wc_normal(),
            None,
            &FontStack::single(&font, 0),
            [1.0, 1.0, 1.0, 1.0],
            crate::text::rich::RichWeight::Normal,
        );
        assert!(tall.lines[0].height > normal.lines[0].height);
    }

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
                source: NodeId(0),
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
                source: NodeId(0),
            },
        ];
        let lay = measure_rich_text(
            &runs,
            Some(1000.0),
            1.2,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
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
            source: NodeId(0),
        }];
        let lay = measure_rich_text(
            &runs,
            Some(30.0),
            1.2,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
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
            source: NodeId(0),
        }];
        // 极窄宽度 → CJK 每字独占一行（4 字 ≥ 4 行）。
        let lay = measure_rich_text(
            &runs,
            Some(10.0),
            1.2,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
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
                source: NodeId(0),
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
                source: NodeId(0),
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
                source: NodeId(0),
            },
        ];
        let lay = measure_rich_text(
            &runs,
            Some(1000.0),
            1.2,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
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
            source: NodeId(0),
        }];
        let lay = measure_rich_text(
            &runs,
            Some(1000.0),
            1.2,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
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
            source: NodeId(0),
        }];
        let stack = FontStack::single(&font, 0);
        // 容器远宽于文本（1000 vs ~20px）→ center/right 应把字形推到右侧。
        let left = measure_rich_text(
            &runs,
            Some(1000.0),
            1.2,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
            &stack,
        );
        let center = measure_rich_text(
            &runs,
            Some(1000.0),
            1.2,
            0.0,
            TextAlign::Center,
            WrapControl::default(),
            &stack,
        );
        let right = measure_rich_text(
            &runs,
            Some(1000.0),
            1.2,
            0.0,
            TextAlign::Right,
            WrapControl::default(),
            &stack,
        );
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
            primary_family: "",
            log: None,
            fallbacks: vec![(&cjk, 1)],
        };
        let lay = measure_text(
            "A中",
            16.0,
            0.0,
            0.0,
            TextAlign::Left,
            wc_normal(),
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

    /// `run_rects`：单 run 跨行换行 → 拆 ≥2 rect（每行一个），source 全等于输入 run 的
    /// source，几何 sane（w/h>0、y 落在行顶、y+h ≤ text_height）。
    #[test]
    fn rich_run_rects_populated_for_wrapped_text() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        // "aaaa bbbb cccc" @24px in 30px → 换行（与 rich_wraps_on_max_width 同输入）。
        let runs = vec![RichRun::text(
            "aaaa bbbb cccc",
            [1.0, 1.0, 1.0, 1.0],
            0,
            24,
            NodeId(5),
        )];
        let lay = measure_rich_text(
            &runs,
            Some(30.0),
            1.2,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
            &FontStack::single(&font, 0),
        );
        assert!(
            lay.lines.len() >= 2,
            "前置：窄宽应换行 ≥2 行，实际 {}",
            lay.lines.len()
        );
        assert!(!lay.run_rects.is_empty(), "run_rects 应非空");
        // 跨行单 run → ≥2 rect（每行一个）。
        assert!(
            lay.run_rects.len() >= 2,
            "跨行 run 应拆 ≥2 rect，实际 {}",
            lay.run_rects.len()
        );
        // 所有 rect 的 source = 输入 run 的 source。
        for r in &lay.run_rects {
            assert_eq!(r.source, NodeId(5), "rect source 应为输入 run 的 source");
            assert!(r.w > 0.0, "rect 宽 > 0（got {}）", r.w);
            assert!(r.h > 0.0, "rect 高 > 0（got {}）", r.h);
            assert!(r.x >= -0.01, "rect x 非负（left align 起于 0）");
            assert!(
                r.y >= -0.01 && r.y + r.h <= lay.text_height + 0.5,
                "rect y={} h={} 应落在 [0, text_height={}]",
                r.y,
                r.h,
                lay.text_height
            );
        }
        // 跨行 rect 落在不同行 → 至少 2 个不同的 y。
        let distinct_ys: std::collections::BTreeSet<u32> =
            lay.run_rects.iter().map(|r| r.y.to_bits()).collect();
        assert!(
            distinct_ys.len() >= 2,
            "跨行 run 的 rect 应跨 ≥2 个不同 y（行），实际 {}",
            distinct_ys.len()
        );
    }

    /// `run_rects`：image run → rect 直接用 RichImagePlacement（x/y/w/h），source 为 image run
    /// 的 source。验证 w/h 精确匹配、y 等于 placement.y（baseline 对齐已烤进）。
    #[test]
    fn rich_run_rects_image_uses_placement() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        let runs = vec![RichRun {
            kind: RichKind::Image {
                src: "icon".into(),
                w: 20.0,
                h: 16.0,
                valign: crate::text::rich::RichVAlign::default(),
            },
            color: [1.0; 4],
            font_id: 0,
            size_px: 24,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco::default(),
            link_id: None,
            source: NodeId(9),
        }];
        let lay = measure_rich_text(
            &runs,
            Some(1000.0),
            1.2,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
            &FontStack::single(&font, 0),
        );
        assert_eq!(lay.run_rects.len(), 1, "单个 image run → 1 rect");
        assert_eq!(lay.images.len(), 1, "前置：应有 1 个 image placement");
        let r = &lay.run_rects[0];
        assert_eq!(r.source, NodeId(9), "image rect source = image run source");
        assert!(
            (r.w - 20.0).abs() < 0.01,
            "image rect w 应=20（got {}）",
            r.w
        );
        assert!(
            (r.h - 16.0).abs() < 0.01,
            "image rect h 应=16（got {}）",
            r.h
        );
        assert!(
            (r.x - lay.images[0].x).abs() < 0.01,
            "image rect x 应=placement.x"
        );
        assert!(
            (r.y - lay.images[0].y).abs() < 0.01,
            "image rect y 应=placement.y（baseline 对齐烤进）"
        );
    }

    /// `run_rects`：同行多 run（不同 source）→ 各自独立 rect（即使 style 相同被渲染合并
    /// 进一个 GlyphRun，命中粒度仍按 input run 保留）。source 一一对应。
    #[test]
    fn rich_run_rects_multi_run_keep_per_source_granularity() {
        let font = match test_font() {
            Some(f) => f,
            None => {
                eprintln!("skip: no test font");
                return;
            }
        };
        // 两个同色同字号 run（不同 source），不换行 → 同行相邻。
        // 渲染层会因 style 相同把它们合并进一个 GlyphRun，但 run_rects 必须保留两条
        // （命中点落在左半 → source=2，右半 → source=7）。
        let runs = vec![
            RichRun::text("left", [0.0, 0.0, 0.0, 1.0], 0, 24, NodeId(2)),
            RichRun::text("right", [0.0, 0.0, 0.0, 1.0], 0, 24, NodeId(7)),
        ];
        let lay = measure_rich_text(
            &runs,
            Some(1000.0),
            1.2,
            0.0,
            TextAlign::Left,
            WrapControl::default(),
            &FontStack::single(&font, 0),
        );
        assert_eq!(lay.lines.len(), 1, "前置：宽约束单行");
        assert_eq!(
            lay.run_rects.len(),
            2,
            "两个 input run → 2 rect（即便渲染合并）"
        );
        // source 一一对应，且按 x 升序（left 在前）。
        let mut rects = lay.run_rects.clone();
        rects.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
        assert_eq!(rects[0].source, NodeId(2), "左 rect source=left run");
        assert_eq!(rects[1].source, NodeId(7), "右 rect source=right run");
        // 两 rect 在 x 上衔接（不重叠、不空隙）：left.x1 ≈ right.x0。
        assert!(
            (rects[0].x + rects[0].w - rects[1].x).abs() < 0.5,
            "相邻 run rect 应 x 衔接：left.x+w={} right.x={}",
            rects[0].x + rects[0].w,
            rects[1].x
        );
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
            wc_normal(),
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
