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

/// CSS `font-weight` 数值 → RichWeight。≥700 → Bold；<700 → Normal。
/// plain text 节点的 style.font_weight 经此转成 weight，供 build_text_mesh 合成 bold。
/// rich text 的 weight 由各 RichRun 自带。
pub fn weight_from_font_weight(w: u16) -> RichWeight {
    if w >= 700 {
        RichWeight::Bold
    } else {
        RichWeight::Normal
    }
}

/// 斜体。MVP 合成（build 期 quad skew）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum RichStyle {
    #[default]
    Normal = 0,
    Italic = 1,
}

/// 装饰线位标记（可组合：underline | line-through | overline）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[repr(transparent)]
pub struct TextDecoLines(pub u8);
impl TextDecoLines {
    pub const NONE: Self = Self(0);
    pub const UNDERLINE: Self = Self(1);
    pub const LINE_THROUGH: Self = Self(2);
    pub const OVERLINE: Self = Self(4);
    pub fn underline(self) -> bool {
        self.0 & 1 != 0
    }
    pub fn strike(self) -> bool {
        self.0 & 2 != 0
    }
    pub fn overline(self) -> bool {
        self.0 & 4 != 0
    }
}

/// 装饰线样式（CSS text-decoration-style）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum TextDecoStyle {
    #[default]
    Solid = 0,
    Dashed = 1,
    Dotted = 2,
    Double = 3,
}

/// 装饰线（v1.8：CSS3 text-decoration shorthand）。
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct RichDeco {
    pub lines: TextDecoLines,
    pub style: TextDecoStyle,
    pub color: Option<[f32; 4]>,
    pub thickness: Option<f32>,
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

/// 超链接命中矩形（节点本地坐标）。跨行链接拆多个 rect（首行尾/中行整行/末行头）。
#[derive(Debug, Clone)]
pub struct RichFragment {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub link_id: u32,
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
                lines: TextDecoLines::UNDERLINE,
                style: TextDecoStyle::Solid,
                color: None,
                thickness: None,
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

}
