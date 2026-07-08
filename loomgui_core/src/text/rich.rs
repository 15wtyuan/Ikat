//! 富文本（v1.7）：run 模型 + 子集 markup 解析器 + 简化 inline flow。
//!
//! 富文本是一个叶子 NodeKind（非围栏标签），inline flow 封装在其 measure/build 里。
//! per-run 多色多字号（v1.6 atlas key 已含 font_id/size_px，per-run color 走 per-vertex）。
//! 简化模型：扁平 run 流 + 单遍断行 + max-baseline-per-line（非完整 CSS IFC）。

/// 加粗。MVP 合成（build 期几何加粗，非字体变体）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum RichWeight {
    #[default]
    Normal = 0,
    Bold = 1,
}

/// 斜体。MVP 合成（build 期 quad skew）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum RichStyle {
    #[default]
    Normal = 0,
    Italic = 1,
}

/// 装饰线（下划线/删除线，build 期纯 quad）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct RichDeco {
    pub underline: bool,
    pub strike: bool,
}

/// 行内图垂直对齐（简化：baseline 默认底边贴基线）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum RichVAlign {
    #[default]
    Baseline = 0,
    Middle = 1,
    Top = 2,
    Bottom = 3,
}

/// 一段同样式富文本。per-glyph 多色靠多个相邻 run（非 per-glyph 字段）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RichRun {
    pub kind: RichKind,
    pub color: [f32; 4],
    pub font_id: u32,
    pub size_px: u16,
    pub weight: RichWeight,
    pub style: RichStyle,
    pub deco: RichDeco,
    /// 属于某超链接（`<a>` 内）；命中查 fragment 矩形时返此 id。None=非链接。
    pub link_id: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum RichKind {
    Text {
        text: String,
    },
    /// 行内图（含 emoji-图片）：复用 image_path 机制，进 image atlas（非 glyph atlas）。
    Image {
        src: String,
        w: f32,
        h: f32,
        valign: RichVAlign,
    },
}

impl RichRun {
    /// 纯文本 run 的便捷构造（继承 base 色/字体）。
    pub fn text(text: impl Into<String>, color: [f32; 4], font_id: u32, size_px: u16) -> Self {
        RichRun {
            kind: RichKind::Text { text: text.into() },
            color,
            font_id,
            size_px,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco::default(),
            link_id: None,
        }
    }
}

/// 富文本 base 样式（caller 从节点 ResolvedStyle 转换来）。
#[derive(Debug, Clone, Copy)]
pub struct RichBaseStyle {
    pub color: [f32; 4],
    pub font_size: f32, // px
    pub weight: RichWeight,
    pub style: RichStyle,
    pub deco: RichDeco,
}

/// 解析富内容 markup 子集 → flatten 后的 run 序列。
///
/// 子集标签：`b/strong`(粗) `i/em`(斜) `u`(下划线) `s/del/strike`(删除线)
/// `span style="..."`(内联样式) `a href="..."`(链接，link_id=1-based 文档序)
/// `img src=.. width=.. height=.. vertical-align=..` `br`(换行)。
/// 嵌套标签 flatten：每个 text run 携完整 effective style（栈式 cascade）。
/// 未闭合/未知标签 → Err（静态打包期硬挡；动态 set_rich_text 也 Err→FFI 返 -1，
/// chat 输入不可信，调用方应保证 markup 合法或自行清洗）。
///
/// 不走 scraper——scraper 是 parse-feature 门控的结构文档解析器；本解析器要 runtime
/// 可用给动态 chat（set_rich_text），手写 tokenizer 直接吃 markup 字符串。
pub fn parse_rich_markup(
    markup: &str,
    base: RichBaseStyle,
    default_font_id: u32,
) -> Result<Vec<RichRun>, String> {
    let mut runs: Vec<RichRun> = Vec::new();
    let mut link_next: u32 = 1;
    // 栈底 = base；每个 open tag push 一个 Eff clone（栈式 cascade）。
    let bottom = Eff {
        color: base.color,
        size_px: base.font_size.round().max(1.0) as u16,
        weight: base.weight,
        style: base.style,
        deco: base.deco,
        link_id: None,
    };
    let mut stack: Vec<Eff> = vec![bottom];
    let mut tag_stack: Vec<&str> = vec![""]; // 平行跟踪标签名（close 校验用）

    let mut text_buf = String::new();
    // 按 Unicode 码点遍历（非按字节）——多字节 UTF-8 文本须正确切分。
    let mut chars = markup.char_indices().peekable();
    while let Some((pos, ch)) = chars.next() {
        if ch == '<' {
            // flush 前置文本
            if !text_buf.is_empty() {
                emit_text(
                    &mut runs,
                    &text_buf,
                    *stack.last().unwrap(),
                    default_font_id,
                );
                text_buf.clear();
            }
            // 读 tag：找到下一个 '>'（'>' 是 ASCII，不会落在多字节序列内部）
            let end = match markup[pos..].find('>') {
                Some(e) => pos + e,
                None => return Err("unclosed tag (missing '>')".into()),
            };
            let tag_inner = &markup[pos + 1..end];
            // 推进迭代器越过整个 tag（含 '>'）
            while let Some(&(p, _)) = chars.peek() {
                if p > end {
                    break;
                }
                let _ = chars.next();
            }
            if let Some(rest) = tag_inner.strip_prefix('/') {
                // close tag
                let name = rest.trim();
                if stack.len() <= 1 {
                    return Err(format!("unexpected </{name}>"));
                }
                let expected = *tag_stack.last().unwrap();
                if expected != name {
                    return Err(format!("mismatched close: <{expected}> vs </{name}>"));
                }
                stack.pop();
                tag_stack.pop();
            } else {
                let (raw_name, attrs) = split_tag(tag_inner);
                let name = raw_name.trim_end_matches('/').trim();
                let cur = *stack.last().unwrap();
                match name {
                    "b" | "strong" => {
                        let mut e = cur;
                        e.weight = RichWeight::Bold;
                        // 保留用户写的标签名入栈，close 时按原名匹配
                        // （<strong>...</strong> 合法，不必规范化为 </b>）。
                        push_open(&mut stack, &mut tag_stack, e, name);
                    }
                    "i" | "em" => {
                        let mut e = cur;
                        e.style = RichStyle::Italic;
                        push_open(&mut stack, &mut tag_stack, e, name);
                    }
                    "u" => {
                        let mut e = cur;
                        e.deco.underline = true;
                        push_open(&mut stack, &mut tag_stack, e, name);
                    }
                    "s" | "del" | "strike" => {
                        let mut e = cur;
                        e.deco.strike = true;
                        push_open(&mut stack, &mut tag_stack, e, name);
                    }
                    "span" => {
                        let mut e = cur;
                        if let Some(style_val) = get_attr(attrs, "style") {
                            apply_inline_style(&mut e, style_val);
                        }
                        push_open(&mut stack, &mut tag_stack, e, name);
                    }
                    "a" => {
                        let id = link_next;
                        link_next += 1;
                        let mut e = cur;
                        e.link_id = Some(id);
                        push_open(&mut stack, &mut tag_stack, e, name);
                    }
                    "img" => {
                        // img 自闭合：直接产 Image run，不入栈
                        let src = get_attr(attrs, "src").unwrap_or("").to_string();
                        let w = get_attr(attrs, "width").and_then(parse_px).unwrap_or(0.0);
                        let h = get_attr(attrs, "height").and_then(parse_px).unwrap_or(0.0);
                        let valign = get_attr(attrs, "vertical-align")
                            .map(parse_valign)
                            .unwrap_or_default();
                        runs.push(RichRun {
                            kind: RichKind::Image { src, w, h, valign },
                            color: cur.color,
                            font_id: default_font_id,
                            size_px: cur.size_px,
                            weight: cur.weight,
                            style: cur.style,
                            deco: cur.deco,
                            link_id: cur.link_id,
                        });
                    }
                    "br" => {
                        // 换行：插一个只含 "\n" 的 text run（measure 据此断行）
                        emit_text(&mut runs, "\n", *stack.last().unwrap(), default_font_id);
                    }
                    other => return Err(format!("unsupported rich tag: <{other}>")),
                }
            }
        } else if ch.is_whitespace() {
            // HTML 空白折叠：连续空白压成单空格
            if !text_buf.ends_with(' ') {
                text_buf.push(' ');
            }
        } else if ch == '&' {
            // HTML 实体解码
            if let Some((entity, consumed)) = parse_entity(&markup[pos..]) {
                text_buf.push_str(&entity);
                // 推进迭代器越过整个实体
                while let Some(&(p, _)) = chars.peek() {
                    if p >= pos + consumed {
                        break;
                    }
                    let _ = chars.next();
                }
            } else {
                text_buf.push('&');
            }
        } else {
            text_buf.push(ch);
        }
    }
    // flush 末尾文本
    if !text_buf.is_empty() {
        emit_text(
            &mut runs,
            &text_buf,
            *stack.last().unwrap(),
            default_font_id,
        );
    }
    if stack.len() != 1 {
        return Err(format!("unclosed tag: <{}>", tag_stack.last().unwrap()));
    }
    Ok(runs)
}

/// 解析过程中栈帧：完整 effective style（嵌套 cascade 累积）。
#[derive(Clone, Copy)]
struct Eff {
    color: [f32; 4],
    size_px: u16,
    weight: RichWeight,
    style: RichStyle,
    deco: RichDeco,
    link_id: Option<u32>,
}

fn push_open<'a>(stack: &mut Vec<Eff>, tag_stack: &mut Vec<&'a str>, e: Eff, name: &'a str) {
    stack.push(e);
    tag_stack.push(name);
}

fn emit_text(runs: &mut Vec<RichRun>, text: &str, eff: Eff, font_id: u32) {
    if text.is_empty() {
        return;
    }
    runs.push(RichRun {
        kind: RichKind::Text { text: text.into() },
        color: eff.color,
        font_id,
        size_px: eff.size_px,
        weight: eff.weight,
        style: eff.style,
        deco: eff.deco,
        link_id: eff.link_id,
    });
}

/// 拆 tag inner → (tag 名, 属性串)。第一个空白前是 tag 名。
fn split_tag(inner: &str) -> (&str, &str) {
    match inner.find(char::is_whitespace) {
        Some(p) => (&inner[..p], inner[p..].trim()),
        None => (inner, ""),
    }
}

/// 从属性串里取某属性值（支持双引号/单引号/无引号）。
fn get_attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=");
    let idx = attrs.find(&needle)?;
    let rest = &attrs[idx + needle.len()..];
    let quote = rest.chars().next()?;
    if quote == '"' || quote == '\'' {
        // 引号包裹：找匹配的关闭引号
        let val_end = rest[1..].find(quote)?;
        Some(&rest[1..1 + val_end])
    } else {
        // 无引号：读到下个空白
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        Some(&rest[..end])
    }
}

/// 解析 px 值（"16" / "16px" → f32）。
fn parse_px(val: &str) -> Option<f32> {
    let v = val.trim().trim_end_matches("px").trim();
    v.parse::<f32>().ok()
}

fn parse_valign(val: &str) -> RichVAlign {
    match val.trim() {
        "middle" => RichVAlign::Middle,
        "top" => RichVAlign::Top,
        "bottom" => RichVAlign::Bottom,
        _ => RichVAlign::Baseline,
    }
}

/// 应用 `style="..."` 内联声明到栈帧（每条 `prop:val;` 单独处理）。
/// 未识别属性静默忽略（围栏哲学：rich 子集不扩 CSS 全集）。
fn apply_inline_style(eff: &mut Eff, style_attr: &str) {
    for decl in style_attr.split(';') {
        let mut parts = decl.splitn(2, ':');
        let prop = parts.next().unwrap_or("").trim();
        let val = parts.next().unwrap_or("").trim();
        if prop.is_empty() {
            continue;
        }
        match prop {
            "color" => {
                if let Some(c) = crate::style::mapping::parse_color(val) {
                    eff.color = c;
                }
            }
            "font-size" => {
                if let Some(px) = parse_px(val) {
                    eff.size_px = px.round().max(1.0) as u16;
                }
            }
            "font-weight" => match val {
                "bold" | "700" | "800" | "900" => eff.weight = RichWeight::Bold,
                "normal" | "400" | "300" | "200" | "100" => eff.weight = RichWeight::Normal,
                _ => {}
            },
            "font-style" => match val {
                "italic" | "oblique" => eff.style = RichStyle::Italic,
                "normal" => eff.style = RichStyle::Normal,
                _ => {}
            },
            "text-decoration" => {
                if val.contains("underline") {
                    eff.deco.underline = true;
                }
                if val.contains("line-through") {
                    eff.deco.strike = true;
                }
            }
            _ => {} // unsupported inline prop 忽略
        }
    }
}

/// 最小 HTML 实体解析。返回 (解码串, 实体字节长度)。
fn parse_entity(s: &str) -> Option<(String, usize)> {
    let end = s.find(';')?;
    let ent = &s[..=end]; // 含 ;
    let mapped = match ent {
        "&lt;" => "<",
        "&gt;" => ">",
        "&amp;" => "&",
        "&nbsp;" | "&#160;" => " ",
        "&quot;" => "\"",
        "&apos;" => "'",
        _ => return None,
    };
    Some((mapped.into(), ent.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rich_run_serde_roundtrip() {
        let r = RichRun {
            kind: RichKind::Text { text: "hi".into() },
            color: [1.0, 0.0, 0.0, 1.0],
            font_id: 2,
            size_px: 24,
            weight: RichWeight::Bold,
            style: RichStyle::Italic,
            deco: RichDeco {
                underline: true,
                strike: false,
            },
            link_id: Some(7),
        };
        let bytes = bincode::serialize(&r).unwrap();
        let back: RichRun = bincode::deserialize(&bytes).unwrap();
        assert_eq!(back.font_id, 2);
        assert_eq!(back.weight, RichWeight::Bold);
        assert_eq!(back.link_id, Some(7));
    }

    #[test]
    fn rich_run_image_serde_roundtrip() {
        let r = RichRun {
            kind: RichKind::Image {
                src: "icons/emote.png".into(),
                w: 16.0,
                h: 16.0,
                valign: RichVAlign::Bottom,
            },
            color: [1.0, 1.0, 1.0, 1.0],
            font_id: 0,
            size_px: 16,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco::default(),
            link_id: None,
        };
        let bytes = bincode::serialize(&r).unwrap();
        let back: RichRun = bincode::deserialize(&bytes).unwrap();
        match &back.kind {
            RichKind::Image { src, w, h, valign } => {
                assert_eq!(src, "icons/emote.png");
                assert_eq!(*w, 16.0);
                assert_eq!(*h, 16.0);
                assert_eq!(*valign, RichVAlign::Bottom);
            }
            RichKind::Text { .. } => panic!("expected Image"),
        }
    }

    #[test]
    fn rich_text_helper_constructs_text_run() {
        let r = RichRun::text("hello", [0.0, 0.0, 0.0, 1.0], 1, 14);
        match &r.kind {
            RichKind::Text { text } => assert_eq!(text, "hello"),
            RichKind::Image { .. } => panic!("expected Text"),
        }
        assert_eq!(r.font_id, 1);
        assert_eq!(r.size_px, 14);
        assert_eq!(r.weight, RichWeight::Normal);
    }

    /// runs Vec<RichRun> 的整段 bincode 序列化往返——验证 pkg 序列化通道。
    #[test]
    fn runs_vec_serde_roundtrip() {
        let runs = vec![
            RichRun::text("a", [1.0, 0.0, 0.0, 1.0], 0, 12),
            RichRun {
                kind: RichKind::Text { text: "b".into() },
                color: [0.0, 1.0, 0.0, 1.0],
                font_id: 0,
                size_px: 12,
                weight: RichWeight::Bold,
                style: RichStyle::Normal,
                deco: RichDeco::default(),
                link_id: Some(3),
            },
        ];
        let bytes = bincode::serialize(&runs).unwrap();
        let back: Vec<RichRun> = bincode::deserialize(&bytes).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[1].link_id, Some(3));
        assert_eq!(back[1].weight, RichWeight::Bold);
    }

    // ---- parse_rich_markup 测试 ----

    fn base() -> RichBaseStyle {
        RichBaseStyle {
            color: [1.0, 1.0, 1.0, 1.0],
            font_size: 24.0,
            weight: RichWeight::Normal,
            style: RichStyle::Normal,
            deco: RichDeco::default(),
        }
    }

    #[test]
    fn parse_plain_text_one_run() {
        let runs = parse_rich_markup("hello", base(), 0).unwrap();
        assert_eq!(runs.len(), 1);
        assert!(matches!(&runs[0].kind, RichKind::Text { text } if text == "hello"));
    }

    #[test]
    fn parse_bold_color_flattens_nested() {
        let m = r#"<b>a <span style="color:#ff0000">red</span> b</b>"#;
        let runs = parse_rich_markup(m, base(), 0).unwrap();
        // 3 text runs: "a "(bold), "red"(bold+red), " b"(bold)
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].weight, RichWeight::Bold);
        assert_eq!(
            runs[1].weight,
            RichWeight::Bold,
            "嵌套继承：red run 也 bold"
        );
        assert_eq!(runs[1].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(runs[2].weight, RichWeight::Bold, "close span 后仍 bold");
    }

    #[test]
    fn parse_link_assigns_id() {
        let runs = parse_rich_markup(r#"<a href="x">click</a>"#, base(), 0).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].link_id, Some(1));
    }

    #[test]
    fn parse_img_self_closing() {
        let runs =
            parse_rich_markup(r#"<img src="i.png" width="16" height="16">x"#, base(), 0).unwrap();
        assert!(matches!(
            &runs[0].kind,
            RichKind::Image { src, w, .. } if src == "i.png" && *w == 16.0
        ));
    }

    #[test]
    fn parse_br_emits_newline() {
        let runs = parse_rich_markup("a<br>b", base(), 0).unwrap();
        // "a", "\n", "b"（或合并），断行靠 measure 看 "\n"
        assert!(runs
            .iter()
            .any(|r| matches!(&r.kind, RichKind::Text { text } if text.contains('\n'))));
    }

    #[test]
    fn parse_unclosed_tag_errors() {
        assert!(parse_rich_markup("<b>no close", base(), 0).is_err());
    }

    #[test]
    fn parse_unknown_tag_errors() {
        assert!(parse_rich_markup("<div>x</div>", base(), 0).is_err());
    }

    #[test]
    fn parse_entity_decode() {
        let runs = parse_rich_markup("a&lt;b", base(), 0).unwrap();
        assert!(matches!(&runs[0].kind, RichKind::Text { text } if text == "a<b"));
    }

    /// 非法嵌套 close（`<b><i></b>`）应报错——栈式 cascade 的强校验。
    #[test]
    fn parse_mismatched_close_errors() {
        assert!(parse_rich_markup("<b><i></b></i>", base(), 0).is_err());
    }

    /// 连续空白压单空格（HTML 空白折叠）。
    #[test]
    fn parse_whitespace_collapses() {
        let runs = parse_rich_markup("a    b\n\tc", base(), 0).unwrap();
        assert!(matches!(&runs[0].kind, RichKind::Text { text } if text == "a b c"));
    }

    /// 所有未识别实体（如 `&foo;`）应原样保留 `&` 后继续解析——降级而非吃字符。
    #[test]
    fn parse_unknown_entity_keeps_ampersand() {
        let runs = parse_rich_markup("a&unknown;b", base(), 0).unwrap();
        // &unknown; 不在子集 → 返 None → '&' 作字面量入 buf
        assert!(matches!(
            &runs[0].kind,
            RichKind::Text { text } if text.contains('&')
        ));
    }

    /// `<a>` 文档序 1-based：多链接 id 递增，跨嵌套也保持顺序。
    #[test]
    fn parse_multiple_links_increment() {
        let runs = parse_rich_markup(r#"<a href="1">x</a><a href="2">y</a>"#, base(), 0).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].link_id, Some(1));
        assert_eq!(runs[1].link_id, Some(2));
    }

    /// 多字节 UTF-8 文本须按字符而非字节处理——中文不能被拆成单字节 codepoint。
    #[test]
    fn parse_multibyte_utf8_text() {
        let runs = parse_rich_markup("你好<b>世界</b>", base(), 0).unwrap();
        assert_eq!(runs.len(), 2);
        assert!(matches!(&runs[0].kind, RichKind::Text { text } if text == "你好"));
        assert!(matches!(
            &runs[1].kind,
            RichKind::Text { text } if text == "世界"
        ));
        assert_eq!(runs[1].weight, RichWeight::Bold);
    }

    /// italic / underline / strike 子集标签都生效（b/i 之外分支覆盖）。
    #[test]
    fn parse_italic_underline_strike() {
        let runs = parse_rich_markup("<i>a</i><u>b</u><s>c</s>", base(), 0).unwrap();
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].style, RichStyle::Italic);
        assert!(runs[1].deco.underline);
        assert!(runs[2].deco.strike);
    }

    /// 别名标签（strong/em/del/strike）映射到同一样式。
    #[test]
    fn parse_tag_aliases() {
        let runs = parse_rich_markup(
            "<strong>a</strong><em>b</em><del>c</del><strike>d</strike>",
            base(),
            0,
        )
        .unwrap();
        assert_eq!(runs.len(), 4);
        assert_eq!(runs[0].weight, RichWeight::Bold);
        assert_eq!(runs[1].style, RichStyle::Italic);
        assert!(runs[2].deco.strike);
        assert!(runs[3].deco.strike);
    }
}
