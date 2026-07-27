//! 字形位置查询函数：面向编辑器的 glyph↔byte 偏移转换。
//!
//! 纯函数（只读 TextLayout + value string），不涉场景/状态变更。
//! 由 Task 7（光标命中）、Task 12（光标几何）依赖。

use crate::text::layout::TextLayout;

/// 每行的字节区间 `(byte_start, byte_end)`。
///
/// `byte_end` = 该行最后一个 glyph 之后的字节位置（即 `line[i+1].first_glyph` 的 byte_off）。
/// 使用 `Glyph.codepoint` 恢复每个 glyph 对应的字节宽度（`char::len_utf8`），而非重新 parse value，
/// 因为 layout 已对 value 做了断行处理（\n 被消化为 mandatory break 并在 glyph 流中存在），
/// glyph↔char 对齐基于 measure 顺序而非 value 的原始断点。
pub fn line_byte_ranges(layout: &TextLayout, value: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut byte_pos = 0usize;
    let mut chars = value.chars();
    for line in &layout.lines {
        let start = byte_pos;
        for run in &line.runs {
            for _g in &run.glyphs {
                if let Some(ch) = chars.next() {
                    byte_pos += ch.len_utf8();
                }
            }
        }
        ranges.push((start, byte_pos));
    }
    if ranges.is_empty() {
        ranges.push((0, 0));
    }
    ranges
}

/// 给定 byte offset，返该偏移对应光标的像素 x 和行索引。
///
/// 返回 `(pixel_x, line_index)`。`pixel_x` 是笔位累计 advance 后对应字节偏移的 x。
/// offset = 0 → x = 0（行首）。
/// offset = 行末 → x = 该行总 advance 之和。
///
/// ## Preconditions
///
/// `layout.lines` must be non-empty (as produced by `measure_text` on non-empty content).
/// If empty, returns `(0.0, 0)` defensively.
pub fn cursor_pixel_x(
    layout: &TextLayout,
    ranges: &[(usize, usize)],
    offset: usize,
) -> (f32, usize) {
    if layout.lines.is_empty() {
        return (0.0, 0);
    }
    for (li, &(start, end)) in ranges.iter().enumerate() {
        if offset < end || li == ranges.len() - 1 {
            let line = &layout.lines[li];
            let mut x = 0.0;
            let mut cur = start;
            'outer: for run in &line.runs {
                for g in &run.glyphs {
                    if cur >= offset {
                        break 'outer;
                    }
                    x += g.advance;
                    cur += char::from_u32(g.codepoint)
                        .map(|c| c.len_utf8())
                        .unwrap_or(1);
                }
            }
            return (x, li);
        }
    }
    (0.0, 0)
}

/// 给定像素坐标 (x, y)，返最近字符的 byte offset。
///
/// y 决定行（越下取末行），x 决定字形（取中点最近，中点左=前一字，中点右=后一字）。
/// x 在最后一个字形中点右 → offset = 末字符后（即字符串长度 byte）。
/// x 在第一个字形中点左 → offset = 0。
///
/// ## Preconditions
///
/// `layout.lines` must be non-empty (as produced by `measure_text` on non-empty content).
/// If empty, returns `0` defensively.
pub fn hit_byte_offset(layout: &TextLayout, ranges: &[(usize, usize)], x: f32, y: f32) -> usize {
    if layout.lines.is_empty() {
        return 0;
    }
    let mut li = 0;
    for (i, line) in layout.lines.iter().enumerate() {
        if y >= line.y {
            li = i;
        }
    }
    let line = &layout.lines[li];
    let (start, _end) = ranges[li];
    let mut pen = 0.0;
    let mut cur = start;
    for run in &line.runs {
        for g in &run.glyphs {
            let mid = pen + g.advance / 2.0;
            if x < mid {
                return cur;
            }
            pen += g.advance;
            cur += char::from_u32(g.codepoint)
                .map(|c| c.len_utf8())
                .unwrap_or(1);
        }
    }
    cur
}

#[cfg(test)]
mod tests;
