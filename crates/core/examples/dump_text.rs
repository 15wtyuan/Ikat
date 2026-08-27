//! 诊断：dump showcase 所有 Text 节点，比对
//!   ① layout 阶段语义：measure_text(content, None)         —— intrinsic（不换行）
//!   ② render 阶段实际：measure_text(content, Some(rect.w))  —— 用 taffy 最终宽作 max_width 重测
//!   ③ 自指模拟：       measure_text(content, Some(text_width①))
//! flag：② 行数 ≠ ① 行数 → render 二次测量换行（回归现场）。
//! 用 CJK 字体（showcase 含中文标题），与 Unity 实际字体族接近。

use ikat_core::text::atlas::{GlyphAtlas, GlyphKey};
use ikat_core::text::layout::{Font, FontTable};

fn main() {
    // 直接用测试字体文件构造 Font，独立 exercise atlas API，确保 atlas 模块自身健康。
    let font_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wqy-microhei.ttc"
    );
    let font = match Font::from_path(font_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Font::from_path({}): {}", font_path, e);
            return;
        }
    };
    let face = &font.face;

    // 构造字体表获取 font_id（atlas key 需要）
    let mut fonts = FontTable::new();
    fonts
        .register("wqy-microhei", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    let font_id = fonts.font_id(None);
    let mut atlas = GlyphAtlas::new();

    // ensure 字形 'H'（单一 SDF：key 只 font_id+glyph_id，size 不进 key）
    let gid_h = face.glyph_index('H').unwrap_or(ttf_parser::GlyphId(0));
    let r = atlas.ensure(
        face,
        GlyphKey {
            font_id,
            glyph_id: gid_h.0,
        },
    );
    println!("─── v1.6 GlyphAtlas 验证 ───");
    println!(
        "ensure 'H'  page={}  uv=({:.4},{:.4})-({:.4},{:.4})  px={}x{}",
        r.page, r.u0, r.v0, r.u1, r.v1, r.px_w, r.px_h
    );
    // UV 必须在 [0,1] 归一化区间内（atlas 槽位子区域）。
    assert!((0.0..=1.0).contains(&r.u0) && (0.0..=1.0).contains(&r.u1));
    assert!((0.0..=1.0).contains(&r.v0) && (0.0..=1.0).contains(&r.v1));
    assert!(r.px_w > 0 && r.px_h > 0, "字形像素尺寸应 >0");

    // 脏页：首次 ensure 应标脏
    let dirty = atlas.dirty_pages();
    println!("dirty_pages: {:?}  (len={})", dirty, dirty.len());
    assert!(!dirty.is_empty(), "ensure 后应有脏页");

    // page 0 字节非空
    let (bytes, w, h) = atlas.page_bytes(0);
    println!(
        "page0: {}x{}  {} bytes (R8, expected {})",
        w,
        h,
        bytes.len(),
        w * h
    );
    assert!(w > 0 && h > 0, "page0 宽高应 >0");
    assert_eq!(bytes.len() as u32, w * h, "R8 字节数 = w*h");

    // .notdef (gid 0) ensure：缺字 tofu 路径
    let missing = atlas.ensure(
        face,
        GlyphKey {
            font_id,
            glyph_id: 0,
        },
    );
    println!(
        ".notdef(gid0)  page={}  uv=({:.4},{:.4})-({:.4},{:.4})  px={}x{}",
        missing.page, missing.u0, missing.v0, missing.u1, missing.v1, missing.px_w, missing.px_h
    );
    assert!(
        missing.px_w > 0 && missing.px_h > 0,
        ".notdef tofu 应有非零尺寸"
    );

    // 再次 dirty_pages：页 0 已存在（.notdef 可能挤到新页）
    println!("dirty_pages after .notdef: {:?}", atlas.dirty_pages());

    // page_bytes OOB 安全：大 page 号应返空切片不 panic
    let (ob, ow, oh) = atlas.page_bytes(999);
    assert_eq!((ob.len(), ow, oh), (0, 0, 0), "page_bytes OOB 安全返空");
    println!("page_bytes(999) OOB: ({}, {}, {})  -- OK", ob.len(), ow, oh);

    // 二次 ensure 同字形：SDF 单槽共享，必须命中同一 UV
    let r48 = atlas.ensure(
        face,
        GlyphKey {
            font_id,
            glyph_id: gid_h.0,
        },
    );
    println!(
        "ensure 'H' (二次)  page={}  uv=({:.4},{:.4})-({:.4},{:.4})  px={}x{}",
        r48.page, r48.u0, r48.v0, r48.u1, r48.v1, r48.px_w, r48.px_h
    );
    assert_eq!((r.u0, r.v0), (r48.u0, r48.v0), "SDF 单槽：同字形命中同 UV");

    // CJK 字形：确保中文字形能分配上（用于验证 CJK 字体路径）
    let gid_cjk = face.glyph_index('中').unwrap_or(ttf_parser::GlyphId(0));
    let r_cjk = atlas.ensure(
        face,
        GlyphKey {
            font_id,
            glyph_id: gid_cjk.0,
        },
    );
    println!(
        "ensure '中'  page={}  uv=({:.4},{:.4})-({:.4},{:.4})  px={}x{}",
        r_cjk.page, r_cjk.u0, r_cjk.v0, r_cjk.u1, r_cjk.v1, r_cjk.px_w, r_cjk.px_h
    );

    println!("─── GlyphAtlas 验证通过 ───\n");

    run_text_dump(font_path);
}

fn run_text_dump(font_path: &str) {
    use ikat_core::scene::node::NodeKind;
    use ikat_core::stage::Stage;
    use ikat_core::text::layout::measure_text;

    let pkg_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../unity/showcase-unity/Assets/StreamingAssets/showcase.pkg.bin"
    );
    let pkg = match std::fs::read(pkg_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("skip text dump: read pkg: {}", e);
            return;
        }
    };
    let mut s = match Stage::new((1080.0, 1920.0)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Stage::new({}): {}", font_path, e);
            return;
        }
    };
    s.register_font("wqy-microhei", std::fs::read(font_path).unwrap(), true)
        .unwrap();
    if let Err(e) = s.load_package("showcase", &pkg) {
        eprintln!("load_package: {}", e);
        return;
    }
    s.tick_and_render();
    let scene = match s.scene.as_ref() {
        Some(sc) => sc,
        None => {
            eprintln!("tick_and_render: scene is None (load_package only stores resources; example would need create_root/instantiate to build a scene before tick)");
            return;
        }
    };
    println!("n_nodes={} font=wqy-microhei", scene.nodes.len());
    println!(
        "{:<22} {:>8} {:>9} {:>8} {:>8} {:>8}  content",
        "id", "rect.w", "none.tw", "none.ln", "before", "after"
    );
    let mut flagged = 0;
    for n in scene.nodes.values() {
        let content = match &n.kind {
            NodeKind::TextNode => scene.text_contents.get(&n.id).cloned().unwrap_or_default(),
            _ => continue,
        };
        let st = &n.style;
        let rect_w = n.layout_rect.w;
        let m_none = measure_text(
            &content,
            st.font_size,
            st.line_height,
            st.letter_spacing,
            st.text_align,
            st.wrap_control(),
            None,
            &s.fonts.stack_for(st.font_family.as_deref()),
            st.color,
            ikat_core::text::rich::weight_from_font_weight(st.font_weight),
        );
        let before = measure_text(
            &content,
            st.font_size,
            st.line_height,
            st.letter_spacing,
            st.text_align,
            st.wrap_control(),
            Some(rect_w),
            &s.fonts.stack_for(st.font_family.as_deref()),
            st.color,
            ikat_core::text::rich::weight_from_font_weight(st.font_weight),
        )
        .lines
        .len();
        let after = scene
            .text_layouts
            .get(n.id.index())
            .cloned()
            .flatten()
            .map(|l| l.lines.len())
            .unwrap_or(0);
        let id = n.id_attr.clone().unwrap_or_default();
        let flag = before != after;
        if flag {
            flagged += 1;
        }
        println!(
            "{:<22} {:>8.3} {:>9.3} {:>8} {:>8} {:>8}{}  {:?}",
            id,
            rect_w,
            m_none.text_width,
            m_none.lines.len(),
            before,
            after,
            if flag { "  <<< FIXED" } else { "" },
            content.chars().take(24).collect::<String>(),
        );
    }
    println!(
        "\nflagged (修复前后 render 行数差异，应 = 短标题数): {}",
        flagged
    );
}
