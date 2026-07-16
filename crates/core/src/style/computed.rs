//! Cascade 解析后的对外只读样式快照（typed，core 内部用）。
//!
//! 从 `ResolvedStyle` 投影一个 curated 子集——排除 internal set-ness 位图（cascade 实现
//! 细节，不泄漏出 core）、taffy 几何（size/min/max/margin/padding 是 layout 产物，走
//! `get_node_layout_rect` 出口）、复杂视觉（gradient/filter/shadow/transform/text-effects，
//! 留视觉束）。供 `Stage::get_node_computed_style` + 集成断言消费。
use crate::style::resolved::{DisplayMode, OverflowMode, ResolvedStyle, TextAlign};

/// Cascade 解析后的非几何样式快照（typed）。跨 FFI 由 `ComputedNodeStyleRepr` 稳定化（Task 4）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComputedNodeStyle {
    pub display_mode: DisplayMode,
    pub flex_direction: taffy::FlexDirection,
    pub overflow_x: OverflowMode,
    pub overflow_y: OverflowMode,
    pub color: [f32; 4],
    pub background_color: Option<[f32; 4]>,
    pub opacity: f32,
    pub border_color: Option<[f32; 4]>,
    pub font_size: f32,
    pub font_weight: u16,
    pub text_align: TextAlign,
    pub line_height: f32,
    pub letter_spacing: f32,
}

impl ComputedNodeStyle {
    /// 从 cascade 后的 `ResolvedStyle`（`Node.style`，rematch 覆写值）投影对外子集。
    pub fn from_resolved(r: &ResolvedStyle) -> Self {
        Self {
            display_mode: r.display_mode,
            flex_direction: r.taffy_style.flex_direction,
            overflow_x: r.overflow_x,
            overflow_y: r.overflow_y,
            color: r.color,
            background_color: r.background_color,
            opacity: r.opacity,
            border_color: r.border_color,
            font_size: r.font_size,
            font_weight: r.font_weight,
            text_align: r.text_align,
            line_height: r.line_height,
            letter_spacing: r.letter_spacing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_resolved_projects_set_fields() {
        let mut r = ResolvedStyle::default();
        r.display_mode = DisplayMode::None;
        r.taffy_style.flex_direction = taffy::FlexDirection::Row;
        r.overflow_x = OverflowMode::Hidden;
        r.color = [0.1, 0.2, 0.3, 1.0];
        r.background_color = Some([1.0, 0.0, 0.0, 1.0]);
        r.border_color = Some([0.0, 1.0, 0.0, 1.0]);
        r.opacity = 0.5;
        r.font_size = 24.0;
        r.font_weight = 700;
        r.text_align = TextAlign::Center;
        r.line_height = 1.5;
        r.letter_spacing = 2.0;
        let c = ComputedNodeStyle::from_resolved(&r);
        assert_eq!(c.display_mode, DisplayMode::None);
        assert_eq!(c.flex_direction, taffy::FlexDirection::Row);
        assert_eq!(c.overflow_x, OverflowMode::Hidden);
        assert_eq!(c.color, [0.1, 0.2, 0.3, 1.0]);
        assert_eq!(c.background_color, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(c.border_color, Some([0.0, 1.0, 0.0, 1.0]));
        assert_eq!(c.opacity, 0.5);
        assert_eq!(c.font_size, 24.0);
        assert_eq!(c.font_weight, 700);
        assert_eq!(c.text_align, TextAlign::Center);
        assert_eq!(c.line_height, 1.5);
        assert_eq!(c.letter_spacing, 2.0);
    }

    #[test]
    fn from_resolved_defaults_match_resolved_default() {
        // 默认 ResolvedStyle → 投影反映默认（flex column / opacity 1 / font 16 / 无背景）。
        let r = ResolvedStyle::default();
        let c = ComputedNodeStyle::from_resolved(&r);
        assert_eq!(c.display_mode, DisplayMode::Flex);
        assert_eq!(c.flex_direction, taffy::FlexDirection::Column);
        assert_eq!(c.opacity, 1.0);
        assert_eq!(c.font_size, 16.0);
        assert_eq!(c.background_color, None);
        assert_eq!(c.border_color, None);
    }
}
