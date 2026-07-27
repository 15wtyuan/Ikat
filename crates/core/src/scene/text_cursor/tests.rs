use super::*;
use crate::style::resolved::TextAlign;
use crate::text::layout::{measure_text, Font, FontStack};
use crate::text::rich::RichWeight;

/// 测试字体：仓库内 DejaVuSans.ttf（跨平台一致），缺则跳过。
fn test_font() -> Option<Font> {
    let p = format!(
        "{}/tests/fixtures/DejaVuSans.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    Font::from_path(&p).ok()
}

/// 用默认字体建单行左对齐无换行约束的 TextLayout。
fn make_layout(text: &str, font: &Font) -> TextLayout {
    measure_text(
        text,
        16.0,
        0.0,
        0.0,
        TextAlign::Left,
        false,
        None,
        &FontStack::single(font, 0),
        [1.0, 1.0, 1.0, 1.0],
        RichWeight::Normal,
    )
}

// ─────────────────────────── line_byte_ranges ───────────────────────────

#[test]
fn line_byte_ranges_single_line_ascii() {
    let font = match test_font() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let layout = make_layout("abc", &font);
    let ranges = line_byte_ranges(&layout, "abc");
    assert_eq!(ranges.len(), 1, "single line → 1 range");
    assert_eq!(ranges[0], (0, 3), "abc = 3 bytes");
}

#[test]
fn line_byte_ranges_empty_text_returns_single_range() {
    let font = match test_font() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let layout = make_layout("", &font);
    let ranges = line_byte_ranges(&layout, "");
    assert_eq!(ranges, vec![(0, 0)], "empty → [(0, 0)]");
}

#[test]
fn line_byte_ranges_multiline_includes_newline_bytes() {
    let font = match test_font() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let layout = make_layout("ab\ncd", &font);
    let ranges = line_byte_ranges(&layout, "ab\ncd");
    // measure_text 把 \n 合进前一行（带 .notdef glyph），故首行含 3 字形（a,b,\n）。
    assert_eq!(ranges.len(), 2, "two lines → 2 ranges");
    // 首行 start=0, 末行 end=5("ab\ncd" 总 5 byte)
    assert_eq!(ranges[0].0, 0, "first line starts at 0");
    assert_eq!(ranges[1].1, 5, "last line ends at total bytes(5)");
    // 两行覆盖全部字节
    assert_eq!(ranges[0].1, ranges[1].0, "ranges contiguous");
}

// ─────────────────────────── cursor_pixel_x ───────────────────────────

#[test]
fn cursor_pixel_x_returns_zero_at_byte_offset_zero() {
    let font = match test_font() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let layout = make_layout("abc", &font);
    let ranges = line_byte_ranges(&layout, "abc");
    let (x, li) = cursor_pixel_x(&layout, &ranges, 0);
    assert_eq!(x, 0.0, "offset 0 → x=0");
    assert_eq!(li, 0, "offset 0 → line 0");
}

#[test]
fn cursor_pixel_x_increases_monotonically() {
    let font = match test_font() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let layout = make_layout("abc", &font);
    let ranges = line_byte_ranges(&layout, "abc");
    let x0 = cursor_pixel_x(&layout, &ranges, 0).0;
    let x1 = cursor_pixel_x(&layout, &ranges, 1).0;
    let x2 = cursor_pixel_x(&layout, &ranges, 2).0;
    let x3 = cursor_pixel_x(&layout, &ranges, 3).0;
    assert!(x1 > x0, "offset 1 ({x1}) > offset 0 ({x0})");
    assert!(x2 > x1, "offset 2 ({x2}) > offset 1 ({x1})");
    assert!(x3 > x2, "offset 3 ({x3}) > offset 2 ({x2})");
}

#[test]
fn cursor_pixel_x_end_equals_total_advances() {
    let font = match test_font() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let layout = make_layout("abc", &font);
    let ranges = line_byte_ranges(&layout, "abc");
    let total: f32 = layout.lines[0].runs[0]
        .glyphs
        .iter()
        .map(|g| g.advance)
        .sum();
    let (x_end, _) = cursor_pixel_x(&layout, &ranges, 3);
    assert!(
        (x_end - total).abs() < 0.001,
        "offset=3 x ({x_end}) ≈ total advances ({total})"
    );
}

#[test]
fn cursor_pixel_x_first_glyph_advance_matches_glyph() {
    let font = match test_font() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let layout = make_layout("abc", &font);
    let ranges = line_byte_ranges(&layout, "abc");
    let adv_a = layout.lines[0].runs[0].glyphs[0].advance;
    let (x1, _) = cursor_pixel_x(&layout, &ranges, 1);
    assert!(
        (x1 - adv_a).abs() < 0.001,
        "offset=1 x ({x1}) ≈ advance('a') ({adv_a})"
    );
}

// ─────────────────────────── hit_byte_offset ───────────────────────────

#[test]
fn hit_byte_offset_left_of_first_glyph_returns_zero() {
    let font = match test_font() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let layout = make_layout("abc", &font);
    let ranges = line_byte_ranges(&layout, "abc");
    assert_eq!(hit_byte_offset(&layout, &ranges, -10.0, 0.0), 0);
}

#[test]
fn hit_byte_offset_right_of_last_glyph_returns_end() {
    let font = match test_font() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let layout = make_layout("abc", &font);
    let ranges = line_byte_ranges(&layout, "abc");
    let total: f32 = layout.lines[0].runs[0]
        .glyphs
        .iter()
        .map(|g| g.advance)
        .sum();
    assert_eq!(hit_byte_offset(&layout, &ranges, total + 10.0, 0.0), 3);
}

#[test]
fn hit_byte_offset_midpoint_snaps_before_vs_after() {
    let font = match test_font() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    let layout = make_layout("abc", &font);
    let ranges = line_byte_ranges(&layout, "abc");
    let g0 = &layout.lines[0].runs[0].glyphs[0];
    let g1 = &layout.lines[0].runs[0].glyphs[1];

    // Before midpoint of first glyph → offset 0
    let mid_a = g0.advance / 2.0;
    assert_eq!(
        hit_byte_offset(&layout, &ranges, mid_a - 0.1, 0.0),
        0,
        "left of mid→byte 0"
    );
    // At/after midpoint of first glyph → offset 1
    assert_eq!(
        hit_byte_offset(&layout, &ranges, mid_a, 0.0),
        1,
        "at mid→byte 1"
    );

    // Before midpoint of second glyph → offset 1
    let mid_b = g0.advance + g1.advance / 2.0;
    assert_eq!(
        hit_byte_offset(&layout, &ranges, mid_b - 0.1, 0.0),
        1,
        "left of mid B → byte 1"
    );
}

#[test]
fn hit_byte_offset_y_determines_line() {
    let font = match test_font() {
        Some(f) => f,
        None => {
            eprintln!("skip: no test font");
            return;
        }
    };
    // Two-line text with known layout.
    let layout = make_layout("ab\ncd", &font);
    let ranges = line_byte_ranges(&layout, "ab\ncd");

    // y=0 → first line
    assert_eq!(
        hit_byte_offset(&layout, &ranges, -10.0, 0.0),
        0,
        "y=0 → first line"
    );
    // y = very large → last line (offset 3 = byte after 'c')
    assert_eq!(
        hit_byte_offset(&layout, &ranges, -10.0, 9999.0),
        3,
        "y=9999 → last line start"
    );
}
