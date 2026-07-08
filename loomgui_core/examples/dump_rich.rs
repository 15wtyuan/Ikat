//! 诊断：直查 RichText 节点的 runs/行/baseline（验 core 实际状态，不靠 PlayMode 猜）。
//!
//! 跑 `cargo run -p loomgui_core --example dump_rich`。验：
//! 1. parse_rich_markup 把 markup 展平成 runs（per-run 色/weight/link 正确）。
//! 2. measure_rich_text 产 TextLayout（行数/宽高/baseline 合理）。
//!
//! 用 DejaVuSans（仓库内 fixture，跨平台一致）。

use loomgui_core::text::layout::{measure_rich_text, FontStack};
use loomgui_core::text::rich::{parse_rich_markup, RichBaseStyle, RichDeco, RichStyle, RichWeight};

fn main() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/DejaVuSans.ttf"
    ))
    .unwrap();
    let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    let font = loomgui_core::text::layout::Font::from_bytes(leaked.to_vec()).unwrap();

    let base = RichBaseStyle {
        color: [1.0, 1.0, 1.0, 1.0],
        font_size: 24.0,
        weight: RichWeight::Normal,
        style: RichStyle::Normal,
        deco: RichDeco::default(),
    };
    let markup = r#"<b>Bold</b> <span style="color:#ff0000">Red</span> <a href="x">Link</a>"#;
    let runs = parse_rich_markup(markup, base, 0).expect("parse");
    println!("markup: {:?}", markup);
    println!("runs: {} 段", runs.len());
    for (i, r) in runs.iter().enumerate() {
        println!(
            "  [{}] color={:?} weight={:?} style={:?} link={:?} size={}",
            i, r.color, r.weight, r.style, r.link_id, r.size_px
        );
    }

    let lay = measure_rich_text(&runs, Some(400.0), 1.2, &FontStack::single(&font, 0));
    println!(
        "\nlayout: lines={} 宽×高 = {:.1}×{:.1}",
        lay.lines.len(),
        lay.text_width,
        lay.text_height
    );
    for (i, l) in lay.lines.iter().enumerate() {
        let glyph_count: usize = l.runs.iter().map(|r| r.glyphs.len()).sum();
        println!(
            "  line{}: y={:.1} baseline={:.1} w={:.1} runs={} glyphs={}",
            i,
            l.y,
            l.baseline,
            l.width,
            l.runs.len(),
            glyph_count
        );
    }

    // 断言：5 段 run（Bold / " " / Red / " " / Link——标签间空白折叠成独立 run），
    // 至少 1 行；Red 段红色、Bold 段 weight=Bold、Link 段 link_id=Some(1)。
    assert_eq!(runs.len(), 5, "markup 应展平成 5 段 run（含标签间空白）");
    assert!(!lay.lines.is_empty(), "至少 1 行");
    assert_eq!(runs[0].weight, RichWeight::Bold, "Bold 段 weight=Bold");
    assert_eq!(runs[2].color, [1.0, 0.0, 0.0, 1.0], "Red 段 color=红");
    assert_eq!(runs[4].link_id, Some(1), "Link 段 link_id=Some(1)");
    println!("\n── dump_rich 验证通过 ──");
}
