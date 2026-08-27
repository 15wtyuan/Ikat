use super::*;
use taffy::style::LengthPercentage;
#[test]
fn parse_length_px_pct_auto() {
    // taffy 0.12：LengthPercentage 是 tagged pointer struct，用 == 比较而非 matches!。
    assert_eq!(parse_lp("100px"), LengthPercentage::length(100.0));
    assert_eq!(parse_lp("50%"), LengthPercentage::percent(0.5));
}

/// inset 四边的三态解析：px / %（含块百分比，绝对定位居中写法 top:50% 的浏览器
/// 语义）/ auto。% 曾被静默丢弃——fence 广告的 LengthPercentAuto 语法必须兑现。
#[test]
fn inset_declares_px_percent_auto() {
    use taffy::style::LengthPercentageAuto;
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "top", "-9px"));
    assert_eq!(s.taffy_style.inset.top, LengthPercentageAuto::length(-9.0));
    assert!(apply_decl(&mut s, "top", "50%"));
    assert_eq!(s.taffy_style.inset.top, LengthPercentageAuto::percent(0.5));
    assert!(apply_decl(&mut s, "left", "62%"));
    assert_eq!(
        s.taffy_style.inset.left,
        LengthPercentageAuto::percent(0.62)
    );
    assert!(apply_decl(&mut s, "top", "auto"));
    assert_eq!(s.taffy_style.inset.top, LengthPercentageAuto::auto());
    // 围栏外语法的值仍不识别（返回 false，不静默改值）。
    assert!(!apply_decl(&mut s, "top", "1em"));
    assert_eq!(s.taffy_style.inset.top, LengthPercentageAuto::auto());
}
#[test]
fn parse_transform_trs_decomposes_supported_functions() {
    let trs = parse_transform_trs("translate(10px,20px) scale(2,.5) rotate(90deg)")
        .expect("TRS transform");
    let px = |v: f32| crate::transform::LenPct { px: v, pct: 0.0 };
    assert_eq!(trs.translate, Some([px(10.0), px(20.0)]));
    assert_eq!(trs.scale, Some([2.0, 0.5]));
    assert!((trs.rotate.unwrap() - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    assert_eq!(
        parse_transform_trs("translateY(20px)").unwrap().translate,
        Some([
            crate::transform::LenPct::ZERO,
            crate::transform::LenPct { px: 20.0, pct: 0.0 }
        ])
    );
    assert_eq!(parse_transform_trs("none"), Some(Default::default()));
}

/// #77：keyframes transform 百分比形——存描述符（px/pct 分域），不再静默 None。
#[test]
fn parse_transform_trs_accepts_percent_translate() {
    let pct = |v: f32| crate::transform::LenPct { px: 0.0, pct: v };
    let trs = parse_transform_trs("translateX(-50%)").expect("百分比形不再拒");
    assert_eq!(
        trs.translate,
        Some([pct(-50.0), crate::transform::LenPct::ZERO])
    );
    // 混合场景：x 百分比 + y px
    let trs = parse_transform_trs("translate(50%, 10px)").unwrap();
    assert_eq!(
        trs.translate,
        Some([pct(50.0), crate::transform::LenPct { px: 10.0, pct: 0.0 }])
    );
    // 非法形仍拒（percent 后缀双写等）
    assert_eq!(parse_transform_trs("translate(50%%)"), None);
}

#[test]
fn parse_transform_trs_rejects_non_trs_functions() {
    assert_eq!(parse_transform_trs("skewX(10deg)"), None);
    assert_eq!(parse_transform_trs("matrix(1,0,0,1,0,0)"), None);
}

/// `width:auto` 必须解析成 `Dimension::auto()`（fit-content），
/// 不能 fallback 到 `Length(0.0)`（→ img rect=(0,0) 不渲染）。
#[test]
fn parse_dimension_auto_is_auto_not_zero() {
    use taffy::style::Dimension;
    assert!(parse_dimension("auto").is_auto(), "auto → Auto");
    assert_eq!(parse_dimension("80px"), Dimension::length(80.0));
    assert_eq!(parse_dimension("50%"), Dimension::percent(0.5));
}
#[test]
fn four_value_expand() {
    assert_eq!(parse_four("4px").unwrap(), [4.0, 4.0, 4.0, 4.0]);
    assert_eq!(parse_four("4px 8px").unwrap(), [4.0, 8.0, 4.0, 8.0]);
}

/// padding/border-width/gap 围栏为 px-only：非 px（%/em/rem）应让 apply_decl 返 false
/// （围栏外静默忽略），不能静默落 0 还返 true——AI 写 `padding:10%` 期望间距在。
#[test]
fn parse_four_rejects_non_px_units() {
    assert_eq!(parse_four("10%"), None, "padding % 不支持");
    assert_eq!(parse_four("1em"), None, "padding em 不支持");
    assert_eq!(parse_four("1rem"), None, "padding rem 不支持");
    assert_eq!(parse_four("auto"), None, "padding auto 不支持");
    // px 与裸数字仍接受（不回归）
    assert_eq!(parse_four("4px").unwrap(), [4.0; 4]);
    assert_eq!(parse_four("4").unwrap(), [4.0; 4]);
}

#[test]
fn apply_decl_padding_rejects_percent_and_em() {
    let mut s = ResolvedStyle::default();
    assert!(!apply_decl(&mut s, "padding", "10%"), "padding % → false");
    let mut s2 = ResolvedStyle::default();
    assert!(!apply_decl(&mut s2, "padding", "1em"), "padding em → false");
    let mut s3 = ResolvedStyle::default();
    assert!(
        !apply_decl(&mut s3, "border-width", "10%"),
        "border-width % → false"
    );
    let mut s4 = ResolvedStyle::default();
    assert!(!apply_decl(&mut s4, "gap", "10%"), "gap % → false");
    // px 仍生效（不回归）
    let mut s5 = ResolvedStyle::default();
    assert!(apply_decl(&mut s5, "padding", "4px"));
    assert_eq!(s5.taffy_style.padding.top, LengthPercentage::length(4.0));
}

/// margin 围栏 px/%/auto：须真正解析 % 与 auto（之前 parse_four 静默落 0 还返 true，
/// margin:0 auto 居中被吞）。fence 承诺了 %/auto，兑现它。
#[test]
fn apply_decl_margin_supports_percent_and_auto() {
    use taffy::style::LengthPercentageAuto;
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "margin", "10%"));
    // taffy 0.12：LengthPercentageAuto 是 tagged pointer struct。Percent 分支
    // 用 into_raw 解出值校验（ == 无法捕获 v 的近似匹配）。
    let top = s.taffy_style.margin.top;
    let cl = top.into_raw();
    assert_eq!(cl.tag(), taffy::style::CompactLength::PERCENT_TAG);
    assert!((cl.value() - 0.1).abs() < 1e-6, "margin 10% → Percent(0.1)");
    let mut s2 = ResolvedStyle::default();
    assert!(apply_decl(&mut s2, "margin", "auto"));
    assert!(s2.taffy_style.margin.top.is_auto(), "margin auto → Auto");
    // margin:0 auto → top/bottom Length(0)，left/right Auto（居中模式）
    let mut s3 = ResolvedStyle::default();
    assert!(apply_decl(&mut s3, "margin", "0 auto"));
    assert_eq!(s3.taffy_style.margin.top, LengthPercentageAuto::length(0.0));
    assert!(s3.taffy_style.margin.right.is_auto(), "margin right auto");
    // em/rem 仍不支持（fence 未列）
    let mut s4 = ResolvedStyle::default();
    assert!(!apply_decl(&mut s4, "margin", "1em"), "margin em → false");
}
/// margin 单边声明（margin-top/right/bottom/left）必须被解析——与 padding/border 单边
/// 对齐。之前只处理 `margin` 简写，导致 `.side-back{margin-bottom:20px}` 这类常见写法
/// 被静默吞，flex 子元素间距算错（showcase settings 返回首页→标题间距）。
#[test]
fn apply_decl_margin_side_per_side() {
    use taffy::style::LengthPercentageAuto;
    // margin-bottom: 20px → 只设 bottom，其余不动
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "margin-bottom", "20px"));
    assert_eq!(
        s.taffy_style.margin.bottom,
        LengthPercentageAuto::length(20.0),
        "margin-bottom 设上"
    );
    // 其余边保持 default（不被清零）
    assert_eq!(
        s.taffy_style.margin.top,
        ResolvedStyle::default().taffy_style.margin.top,
        "margin-top 不受 margin-bottom 影响"
    );
    // 四边各能单独设
    let mut s2 = ResolvedStyle::default();
    apply_decl(&mut s2, "margin-top", "5px");
    apply_decl(&mut s2, "margin-right", "6px");
    apply_decl(&mut s2, "margin-bottom", "7px");
    apply_decl(&mut s2, "margin-left", "8px");
    assert_eq!(s2.taffy_style.margin.top, LengthPercentageAuto::length(5.0));
    assert_eq!(
        s2.taffy_style.margin.right,
        LengthPercentageAuto::length(6.0)
    );
    assert_eq!(
        s2.taffy_style.margin.bottom,
        LengthPercentageAuto::length(7.0)
    );
    assert_eq!(
        s2.taffy_style.margin.left,
        LengthPercentageAuto::length(8.0)
    );
    // % 和 auto 也走单边（与简写语义一致）
    let mut s3 = ResolvedStyle::default();
    assert!(apply_decl(&mut s3, "margin-left", "auto"));
    assert!(s3.taffy_style.margin.left.is_auto(), "单边 auto");
}
#[test]
fn color_hex() {
    let c = parse_color("#ff0000").unwrap();
    assert_eq!(c, [1.0, 0.0, 0.0, 1.0]);
}
#[test]
fn color_hex_short_expands() {
    // CSS 3 位 hex：每数字重复（#888 = #888888，#3f6 = #33ff66）
    assert_eq!(
        parse_color("#888").unwrap(),
        parse_color("#888888").unwrap()
    );
    assert_eq!(parse_color("#fff").unwrap(), [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(parse_color("#000").unwrap(), [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(
        parse_color("#3f6").unwrap(),
        [
            0x33 as f32 / 255.0,
            0xff as f32 / 255.0,
            0x66 as f32 / 255.0,
            1.0
        ]
    );
}

// CSS 函数式颜色：rgb() / rgba()。AI 常写 rgba()（showcase nav-card/quick-chip 即用），
// parse_color 原先只认 hex 导致静默丢色（卡片透明露底）。这里钉死函数式语法。
#[test]
fn color_rgb_function() {
    // rgb(r,g,b) 0-255 整数 → alpha 1.0。
    assert_eq!(parse_color("rgb(255,0,0)").unwrap(), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(
        parse_color("rgb(21, 36, 51)").unwrap(),
        [21.0 / 255.0, 36.0 / 255.0, 51.0 / 255.0, 1.0]
    );
}

#[test]
fn color_rgba_function() {
    // rgba(r,g,b,a)：a 为 0..1 浮点（0.72）→ 卡片半透明背景。
    let c = parse_color("rgba(21, 36, 51, 0.72)").unwrap();
    assert!((c[0] - 21.0 / 255.0).abs() < 1e-5);
    assert!((c[1] - 36.0 / 255.0).abs() < 1e-5);
    assert!((c[2] - 51.0 / 255.0).abs() < 1e-5);
    assert!((c[3] - 0.72).abs() < 1e-5, "alpha 0.72");
}

#[test]
fn color_rgb_percent() {
    // rgb 支持百分比分量（100% = 255）。CSS 合法形态，AI 偶写。
    assert_eq!(
        parse_color("rgb(100%,0%,0%)").unwrap(),
        [1.0, 0.0, 0.0, 1.0]
    );
}

#[test]
fn color_rgb_alpha_via_slash() {
    // CSS Color 4：rgb(r g b / a) 空格分隔 + 斜杠 alpha（与 rgba 等价）。
    let c = parse_color("rgb(21 36 51 / 0.5)").unwrap();
    assert!((c[3] - 0.5).abs() < 1e-5);
}

// CSS Color Module Level 4：`#rgba` 4 位 hex（3 位色的 alpha 简写）。
// 与 3 位 hex 同构（digit d → d*17），末位为 alpha。box-shadow / overlay 常用，
// showcase lab/shop 即写 #000a / #000c。补 3/8-hex 之间的缺口。
#[test]
fn color_hex_4_alpha_short() {
    // #000a = 黑色 α=0xaa/ff ≈ 0.667（digit a → a*17=0xaa=170）。
    let c = parse_color("#000a").unwrap();
    assert_eq!(c[0], 0.0);
    assert_eq!(c[1], 0.0);
    assert_eq!(c[2], 0.0);
    assert!((c[3] - 170.0 / 255.0).abs() < 1e-5, "alpha = 0xaa/255");
}

#[test]
fn color_hex_4_equivalent_to_8() {
    // #rgba 4 位与展开的 #rrggbbaa 8 位等价（#000a ≡ #000000aa）。
    assert_eq!(
        parse_color("#000a").unwrap(),
        parse_color("#000000aa").unwrap()
    );
    // 不透明：#f00f ≡ #ff0000ff ≡ #ff0000。
    assert_eq!(
        parse_color("#f00f").unwrap(),
        parse_color("#ff0000ff").unwrap()
    );
    assert_eq!(
        parse_color("#f00f").unwrap(),
        parse_color("#ff0000").unwrap()
    );
}

// CSS Color Module Level 4：`#rrggbbaa` 8 位 hex（第 7-8 位为 alpha）。
// StyleMirror 会把 color flush 成此形式，core parse_color 必须收。
#[test]
fn color_hex_8_opaque_red() {
    // aa=ff → 不透明红（与 6-hex #ff0000 等价）。
    let c = parse_color("#ff0000ff").unwrap();
    assert_eq!(c, [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn color_hex_8_alpha_128_of_255() {
    // aa=80 → alpha = 128/255 ≈ 0.502（半透明黑）。
    let c = parse_color("#00000080").unwrap();
    assert_eq!(c[0], 0.0);
    assert_eq!(c[1], 0.0);
    assert_eq!(c[2], 0.0);
    assert!((c[3] - 128.0 / 255.0).abs() < 1e-5, "alpha = 128/255");
}

#[test]
fn color_hex_8_round_trip() {
    // StyleMirror flush #rrggbbaa → core parse → 值对。
    // aa=ff 等价 6-hex a=1.0；aa=00 完全透明。
    let opaque = parse_color("#aabbccff").unwrap();
    assert_eq!(
        opaque,
        [
            0xaa as f32 / 255.0,
            0xbb as f32 / 255.0,
            0xcc as f32 / 255.0,
            1.0
        ]
    );
    let transparent = parse_color("#aabbcc00").unwrap();
    assert_eq!(
        transparent,
        [
            0xaa as f32 / 255.0,
            0xbb as f32 / 255.0,
            0xcc as f32 / 255.0,
            0.0
        ],
        "aa=00 → alpha=0 完全透明"
    );
}

#[test]
fn color_hex_8_no_regress_6_and_3() {
    // 6-hex / 3-hex 仍正确（不回归）。
    assert_eq!(parse_color("#ff0000").unwrap(), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(parse_color("#f00").unwrap(), [1.0, 0.0, 0.0, 1.0]);
}
#[test]
fn apply_width_and_bg() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "width", "100px"));
    assert!(apply_decl(&mut s, "background-color", "#00ff00"));
    assert!(s.background_color == Some([0.0, 1.0, 0.0, 1.0]));
    assert!(apply_decl(&mut s, "border-radius", "4px")); // border-radius 被解析（非装饰忽略）
}
#[test]
fn order_is_stored() {
    // 合法值：存进 ResolvedStyle.order
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "order", "2"));
    assert_eq!(s.order, 2);
    // 非法值：降级为 0（不 panic、不污染）
    let mut s2 = ResolvedStyle::default();
    assert!(apply_decl(&mut s2, "order", "abc"));
    assert_eq!(s2.order, 0);
    // 负值也接受（CSS order 允许负）
    let mut s3 = ResolvedStyle::default();
    assert!(apply_decl(&mut s3, "order", "-1"));
    assert_eq!(s3.order, -1);
}

#[test]
fn pointer_events_none_sets_touchable_false() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "pointer-events", "none"));
    assert!(!s.touchable, "pointer-events:none → touchable=false");
}

#[test]
fn pointer_events_auto_keeps_touchable_true() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "pointer-events", "auto"));
    assert!(s.touchable, "pointer-events:auto → touchable=true");
}

#[test]
fn overflow_shorthand_sets_both_axes() {
    // overflow:scroll → overflow_x=overflow_y=Scroll
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "overflow", "scroll"));
    assert_eq!(s.overflow_x, OverflowMode::Scroll);
    assert_eq!(s.overflow_y, OverflowMode::Scroll);
}

#[test]
fn overflow_shorthand_all_values() {
    for (val, mode) in [
        ("visible", OverflowMode::Visible),
        ("hidden", OverflowMode::Hidden),
        ("scroll", OverflowMode::Scroll),
        ("auto", OverflowMode::Auto),
    ] {
        let mut s = ResolvedStyle::default();
        assert!(
            apply_decl(&mut s, "overflow", val),
            "overflow:{} 被识别",
            val
        );
        assert_eq!(s.overflow_x, mode, "overflow:{} → x", val);
        assert_eq!(s.overflow_y, mode, "overflow:{} → y", val);
    }
}

#[test]
fn overflow_xy_longhand_overrides_shorthand() {
    // shorthand 先设双轴 hidden；longhand 后写 override 单轴
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "overflow", "hidden"));
    assert!(apply_decl(&mut s, "overflow-x", "auto"));
    assert_eq!(s.overflow_x, OverflowMode::Auto, "overflow-x longhand 覆盖");
    assert_eq!(
        s.overflow_y,
        OverflowMode::Hidden,
        "overflow-y 保持 shorthand"
    );
}

#[test]
fn overflow_xy_longhand_y_axis() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "overflow", "visible"));
    assert!(apply_decl(&mut s, "overflow-y", "scroll"));
    assert_eq!(s.overflow_x, OverflowMode::Visible, "overflow-x 保持");
    assert_eq!(s.overflow_y, OverflowMode::Scroll, "overflow-y longhand");
}

#[test]
fn overflow_unknown_value_silently_ignored() {
    // 未知值宽松忽略：既存字段不变（与现 overflow 解析风格一致）
    let mut s = ResolvedStyle::default();
    s.overflow_x = OverflowMode::Scroll;
    s.overflow_y = OverflowMode::Auto;
    assert!(apply_decl(&mut s, "overflow", "bogus"));
    assert_eq!(s.overflow_x, OverflowMode::Scroll, "未知 overflow 不动 x");
    assert_eq!(s.overflow_y, OverflowMode::Auto, "未知 overflow 不动 y");
    assert!(apply_decl(&mut s, "overflow-x", "nonsense"));
    assert_eq!(s.overflow_x, OverflowMode::Scroll, "未知 overflow-x 不动 x");
}

use super::parse_transform;
use crate::transform::Affine2Ext;

#[test]
fn parse_single_translate() {
    let t = parse_transform("translate(10px, 20px)");
    let (x, y) = t.matrix.apply_point(0.0, 0.0);
    assert_eq!((x, y), (10.0, 20.0));
    assert!(t.matrix.is_pure_translation(), "纯 translate 是纯平移");
}

#[test]
fn parse_single_rotate_radians() {
    let t = parse_transform("rotate(90deg)");
    // 90° 旋转：(1,0) → (0,1)
    let (x, y) = t.matrix.apply_point(1.0, 0.0);
    assert!(
        x.abs() < 1e-5 && (y - 1.0).abs() < 1e-5,
        "90deg rotate (1,0)→(0,1)"
    );
}

#[test]
fn parse_single_scale_uniform() {
    let t = parse_transform("scale(2)");
    let (x, y) = t.matrix.apply_point(1.0, 1.0);
    assert_eq!((x, y), (2.0, 2.0), "scale(2) 双轴");
}

#[test]
fn parse_scale_non_uniform_compose_with_rotate_is_skew() {
    // scale(2,1) rotate(45deg)：复合矩阵非纯平移（剪切）
    let t = parse_transform("scale(2, 1) rotate(45deg)");
    assert!(
        !t.matrix.is_pure_translation(),
        "非均匀缩放∘旋转 = 剪切，非纯平移"
    );
}

#[test]
fn parse_unknown_functions_silently_skipped() {
    // skew/matrix() 围栏外 → 静默跳过；translate 仍生效
    let t = parse_transform("translate(5px, 0px) skew(10deg)");
    let (x, y) = t.matrix.apply_point(0.0, 0.0);
    assert_eq!((x, y), (5.0, 0.0), "skew 被跳过，translate 生效");
}

#[test]
fn apply_decl_transform_sets_style() {
    use crate::style::resolved::ResolvedStyle;
    use crate::transform::Affine2Ext;
    let mut s = ResolvedStyle::default();
    let applied = super::apply_decl(&mut s, "transform", "rotate(45deg)");
    assert!(applied, "transform 被识别");
    assert!(
        !s.transform.matrix.is_pure_translation(),
        "rotate 写进 style.transform"
    );
}

#[test]
fn parse_url_extracts_path() {
    use super::parse_url;
    assert_eq!(
        parse_url("url(icons/home.png)"),
        Some("icons/home.png".into())
    );
    assert_eq!(
        parse_url("url(\"icons/home.png\")"),
        Some("icons/home.png".into())
    );
    assert_eq!(
        parse_url("url('icons/home.png')"),
        Some("icons/home.png".into())
    );
    assert_eq!(
        parse_url("url( icons/home.png )"),
        Some("icons/home.png".into()),
        "容忍空格"
    );
    assert_eq!(parse_url("icons/home.png"), None, "非 url() 格式 → None");
    assert_eq!(parse_url("url()"), None, "空 url → None");
    assert_eq!(parse_url(""), None);
    // 自闭合引号回归测试：len < 2 被 len >= 2 guard 拦住，不应 panic
    assert_eq!(
        parse_url("url(')"),
        Some("'".to_string()),
        "自闭合单引号不 panic"
    );
    assert_eq!(
        parse_url("url(\")"),
        Some("\"".to_string()),
        "自闭合双引号不 panic"
    );
}

#[test]
fn apply_background_image_sets_field() {
    use crate::style::resolved::BackgroundSize;
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(
        &mut s,
        "background-image",
        "url(icons/home.png)"
    ));
    assert_eq!(s.background_image.as_deref(), Some("icons/home.png"));
    // 无图时默认 Stretch 不变
    assert_eq!(s.background_size, BackgroundSize::Stretch);
}

#[test]
fn apply_background_size_three_modes() {
    use crate::style::resolved::BackgroundSize;
    for (val, mode) in [
        ("cover", BackgroundSize::Cover),
        ("contain", BackgroundSize::Contain),
        ("100%", BackgroundSize::Stretch),
    ] {
        let mut s = ResolvedStyle::default();
        assert!(
            apply_decl(&mut s, "background-size", val),
            "background-size:{} 被识别",
            val
        );
        assert_eq!(
            s.background_size, mode,
            "background-size:{} → {:?}",
            val, mode
        );
    }
}

#[test]
fn apply_background_size_invalid_ignored() {
    // 围栏外值（auto/px/两值）→ 返回 false，不改默认 Stretch
    use crate::style::resolved::BackgroundSize;
    for val in ["auto", "50px", "100% 50%", "cover contain"] {
        let mut s = ResolvedStyle::default();
        assert!(
            !apply_decl(&mut s, "background-size", val),
            "{} 围栏外 → false",
            val
        );
        assert_eq!(
            s.background_size,
            BackgroundSize::Stretch,
            "{} 不改默认",
            val
        );
    }
}

#[test]
fn parse_radius_group_one_value() {
    let g = parse_radius_group("8px").unwrap();
    assert_eq!(g, [parse_lp("8px"); 4]);
}

#[test]
fn parse_radius_group_two_values() {
    let g = parse_radius_group("4px 12px").unwrap();
    // [v0, v1, v0, v1]（TL/BR=v0, TR/BL=v1）
    assert_eq!(
        g,
        [
            parse_lp("4px"),
            parse_lp("12px"),
            parse_lp("4px"),
            parse_lp("12px")
        ]
    );
}

#[test]
fn parse_radius_group_percent() {
    let g = parse_radius_group("50%").unwrap();
    assert_eq!(g, [parse_lp("50%"); 4]);
}

#[test]
fn parse_radius_group_auto_rejected() {
    // auto/inherit/initial → None（CSS 无效，不落 Length(0)）
    assert!(parse_radius_group("auto").is_none());
    assert!(parse_radius_group("inherit").is_none());
    assert!(parse_radius_group("initial").is_none());
    assert!(parse_radius_group("8px auto").is_none()); // 混入 auto → 整组 None
}

#[test]
fn parse_radius_group_garbage_rejected() {
    assert!(parse_radius_group("4px abc").is_none());
    assert!(parse_radius_group("").is_none());
}

#[test]
fn apply_border_radius_single() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border-radius", "8px"));
    for c in &s.border_radius.corners {
        assert_eq!(c.h, parse_lp("8px"));
        assert_eq!(c.v, parse_lp("8px")); // 无 / → v = h
    }
}

#[test]
fn apply_border_radius_ellipse_syntax() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border-radius", "10px / 5px"));
    for c in &s.border_radius.corners {
        assert_eq!(c.h, parse_lp("10px"), "水平半径 10");
        assert_eq!(c.v, parse_lp("5px"), "垂直半径 5");
    }
}

#[test]
fn apply_border_radius_percent() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border-radius", "50%"));
    for c in &s.border_radius.corners {
        assert_eq!(c.h, LengthPercentage::percent(0.5));
        assert_eq!(c.v, LengthPercentage::percent(0.5));
    }
}

#[test]
fn apply_border_radius_invalid_returns_false() {
    let mut s = ResolvedStyle::default();
    assert!(!apply_decl(&mut s, "border-radius", "auto"));
    assert!(!apply_decl(&mut s, "border-radius", "8px / abc"));
    assert!(!apply_decl(&mut s, "border-radius", "8px /"));
    // 失败时不应改默认
    assert_eq!(s.border_radius, BorderRadius::default());
}

#[test]
fn apply_decl_filter_grayscale_sets_matrix() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "filter", "grayscale(1)"));
    let m = s.color_filter.expect("filter 设了 color_filter");
    // grayscale 行 0 = (LUMA_R, LUMA_G, LUMA_B, 0, 0)
    assert!((m[0] - 0.299).abs() < 1e-4);
    assert!((m[1] - 0.587).abs() < 1e-4);
    assert!((m[2] - 0.114).abs() < 1e-4);
}

#[test]
fn apply_decl_filter_none_clears() {
    let mut s = ResolvedStyle::default();
    s.color_filter = Some(crate::style::color_filter::IDENTITY);
    assert!(apply_decl(&mut s, "filter", "none"));
    assert!(s.color_filter.is_none(), "filter:none 清除 color_filter");
}

#[test]
fn apply_decl_filter_multi_function_concat() {
    // grayscale(1) brightness(1.2) → concat(grayscale, brightness)
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "filter", "grayscale(1) brightness(1.2)"));
    let m = s.color_filter.expect("multi filter");
    // concat(grayscale, brightness(1.2))：brightness 改乘法（对角 1.2，无 offset）。
    // m[0] = grayscale luma(0.299) × 1.2 = 0.3588；m[4] offset = 0（两者都无 offset）。
    assert!(m[4].abs() < 1e-4, "brightness 乘法无 offset → m[4]=0");
    assert!(
        (m[0] - 0.3588).abs() < 1e-4,
        "m[0] = grayscale luma × brightness = 0.299×1.2"
    );
}

/// 多函数 filter 串联顺序与 CSS/fgui 一致（回归测试）。
/// CSS `filter: A B` = 先 A 后 B → 组合矩阵 = B × A（B 在左，最靠近 color）。
/// fgui ConcatValues: `_matrix = newPreset × _matrix`（新值左乘）。
/// Ikat 应同：acc 从 IDENTITY 起，每步 `acc = concat(m, acc)`（新值在左）。
///
/// 用 saturate(0.5) hue-rotate(90deg) —— 二者不可交换（已数学验证），可检出顺序反转。
/// 正确（CSS）：先 saturate 后 hue-rotate → 组合 = H × S = `concat(hue_rotate(90), saturate(0.5))`。
/// 错误（`concat(acc, m)`）：组合 = S × H = `concat(saturate(0.5), hue_rotate(90))`。
#[test]
fn apply_decl_filter_multi_function_concat_order_matches_css() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(
        &mut s,
        "filter",
        "saturate(0.5) hue-rotate(90deg)"
    ));
    let got = s.color_filter.expect("multi filter");

    let sat = color_filter::saturate(0.5);
    let hue = color_filter::hue_rotate(90.0);
    let correct = color_filter::concat(&hue, &sat); // CSS: H × S（hue-rotate 在左）
    let reversed = color_filter::concat(&sat, &hue); // 错误顺序: S × H

    for i in 0..20 {
        assert!(
            (got[i] - correct[i]).abs() < 1e-5,
            "[{}] 应 = concat(hue, saturate)（CSS/fgui 顺序，新值左乘），got={}, expected={}",
            i,
            got[i],
            correct[i]
        );
    }
    assert!(
        (got[0] - reversed[0]).abs() > 1e-4 || (got[1] - reversed[1]).abs() > 1e-4,
        "顺序敏感：与反转矩阵（concat(saturate, hue)）应不同"
    );
}

#[test]
fn apply_decl_border_image_slice_four_values() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border-image-slice", "10 20 30 40"));
    let sl = s.border_image_slice.expect("slice 设了");
    assert_eq!(sl.top, 10.0);
    assert_eq!(sl.right, 20.0);
    assert_eq!(sl.bottom, 30.0);
    assert_eq!(sl.left, 40.0);
}

#[test]
fn apply_decl_border_image_slice_percent() {
    // 25% → 暂存比例值（0.25），渲染期 resolve 乘源图边（同 border-radius % 语义）
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border-image-slice", "25%"));
    let sl = s.border_image_slice.expect("slice 设了");
    assert!((sl.top - 0.25).abs() < 1e-4, "25% 存 0.25，渲染期 resolve");
}

/// `border` 简写 `<width> <style>? <color>?`（CSS 标准、AI 强先验）须解析 width + color，
/// 否则 `border:1px solid #3a3f55` 只取 width、color 丢 → border_color=None → 渲染不画边框。
/// 四边同值（简写语义）。
#[test]
fn apply_border_shorthand_sets_width_and_color() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border", "1px solid #3a3f55"));
    let c = s.border_color.expect("border 简写须解析 color");
    assert_eq!(c[0], 0x3a as f32 / 255.0);
    assert_eq!(c[1], 0x3f as f32 / 255.0);
    assert_eq!(c[2], 0x55 as f32 / 255.0);
    assert_eq!(c[3], 1.0);
    let ts = &s.taffy_style.border;
    assert_eq!(ts.top, LengthPercentage::length(1.0), "四边同宽");
    assert_eq!(ts.right, LengthPercentage::length(1.0));
    assert_eq!(ts.bottom, LengthPercentage::length(1.0));
    assert_eq!(ts.left, LengthPercentage::length(1.0));
}

/// border 简写 width/style/color 任意序；省 color 时只设 width。
#[test]
fn apply_border_shorthand_token_order_and_optional_color() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border", "2px"));
    assert_eq!(
        s.taffy_style.border.top,
        LengthPercentage::length(2.0),
        "border 简写四边同宽"
    );
    assert!(
        s.border_color.is_none(),
        "无 color token → 不设 border_color"
    );

    // color 在前、width 在后（CSS 简写任意序）
    let mut s2 = ResolvedStyle::default();
    assert!(apply_decl(&mut s2, "border", "#ff0000 3px solid"));
    assert_eq!(
        s2.taffy_style.border.top,
        LengthPercentage::length(3.0),
        "color 在前时 width 仍解析"
    );
    let c = s2.border_color.expect("color 在前也解析");
    assert_eq!(c, [1.0, 0.0, 0.0, 1.0]);
}

/// `border-width` 属性（非简写）只设 width，不碰 border_color。
#[test]
fn apply_border_longhand_width_leaves_color_untouched() {
    let mut s = ResolvedStyle::default();
    s.border_color = Some([0.5; 4]);
    assert!(apply_decl(&mut s, "border-width", "4px"));
    assert_eq!(
        s.taffy_style.border.top,
        LengthPercentage::length(4.0),
        "border-width 设四边"
    );
    assert_eq!(
        s.border_color,
        Some([0.5; 4]),
        "border-width 不覆盖 border_color"
    );
}

/// `border-width` 四值 `<t> <r> <b> <l>` 必须分别落到 `ts.border` 四边——旧实现曾把四值
/// 坍缩成 top 一边。单元级锁定四边独立赋值（既有 `apply_border_longhand_width_leaves_color_untouched`
/// 只断言 top，无法检出 right/bottom/left 丢失）。
#[test]
fn apply_border_width_four_values_sets_all_four_sides() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border-width", "1px 2px 3px 4px"));
    let ts = &s.taffy_style.border;
    assert_eq!(ts.top, LengthPercentage::length(1.0), "top=1");
    assert_eq!(ts.right, LengthPercentage::length(2.0), "right=2");
    assert_eq!(ts.bottom, LengthPercentage::length(3.0), "bottom=3");
    assert_eq!(ts.left, LengthPercentage::length(4.0), "left=4");
    assert!(
        s.border_color.is_none(),
        "border-width 只设 width，不碰 border_color"
    );
}

#[test]
fn transition_empty_value_is_none() {
    // apply_decl("transition", "") → style.transition = None（未声明 vs 默认值有不同语义）
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "transition", ""));
    assert!(
        s.transition.is_empty(),
        "空 transition 值 → 空 Vec，不设为默认 spec"
    );
}

#[test]
fn parse_transition_multiple_comma_specs() {
    // CSS 逗号分隔多 spec：background-color 0.3s + color 0.3s → 两个 TransitionSpec。
    // 之前 split_whitespace 不处理逗号，color spec 被 ease 分支吞掉（prop 被覆盖）。
    let ts = parse_transition("background-color 0.3s ease-out, color 0.3s ease-out");
    assert_eq!(ts.len(), 2, "逗号分隔两个 spec");
    assert_eq!(ts[0].prop, Some(crate::tween::TweenProp::BgColor));
    assert!((ts[0].duration - 0.3).abs() < 1e-3);
    assert_eq!(ts[1].prop, Some(crate::tween::TweenProp::TextColor));
    assert!((ts[1].duration - 0.3).abs() < 1e-3);
}

#[test]
fn text_shadow_single_with_blur() {
    // `text-shadow: 2px 2px 4px #000000` → 单 Shadow{ox:2, oy:2, blur:4, color:black}
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "text-shadow", "2px 2px 4px #000000"));
    assert_eq!(s.text_effects.len(), 1, "单阴影 → 1 effect");
    match s.text_effects[0] {
        crate::text::font_effect::FontEffect::Shadow {
            ox,
            oy,
            blur,
            color,
        } => {
            assert!((ox - 2.0).abs() < 1e-4, "ox=2");
            assert!((oy - 2.0).abs() < 1e-4, "oy=2");
            assert!((blur - 4.0).abs() < 1e-4, "blur=4");
            assert_eq!(color, [0.0, 0.0, 0.0, 1.0], "color=#000000 → black");
        }
        _ => panic!("expected Shadow effect"),
    }
}

#[test]
fn text_shadow_without_blur() {
    // blur 省略 → 默认 0（硬边投影，位图 = clone）。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "text-shadow", "3px 1px #ff0000"));
    assert_eq!(s.text_effects.len(), 1);
    match s.text_effects[0] {
        crate::text::font_effect::FontEffect::Shadow {
            ox,
            oy,
            blur,
            color,
            ..
        } => {
            assert!((ox - 3.0).abs() < 1e-4);
            assert!((oy - 1.0).abs() < 1e-4);
            assert!((blur - 0.0).abs() < 1e-4, "blur 省略 → 0");
            assert_eq!(color, [1.0, 0.0, 0.0, 1.0]);
        }
        _ => panic!("expected Shadow"),
    }
}

#[test]
fn text_shadow_multiple_comma_separated() {
    // CSS 逗号分隔多阴影 → 多 Shadow effect（序：前→后，先绘前者在更下层）。
    let mut s = ResolvedStyle::default();
    assert!(
        apply_decl(
            &mut s,
            "text-shadow",
            "1px 1px 2px #ff0000, 3px 3px #0000ff"
        ),
        "合法多阴影应返 true"
    );
    assert_eq!(s.text_effects.len(), 2, "两段逗号 → 2 effect");
    // 第一段：红 shadow blur=2
    match s.text_effects[0] {
        crate::text::font_effect::FontEffect::Shadow { color, blur, .. } => {
            assert_eq!(color, [1.0, 0.0, 0.0, 1.0], "第一段红");
            assert!((blur - 2.0).abs() < 1e-4);
        }
        _ => panic!("第一段应为 Shadow"),
    }
    // 第二段：蓝 shadow blur=0
    match s.text_effects[1] {
        crate::text::font_effect::FontEffect::Shadow { color, blur, .. } => {
            assert_eq!(color, [0.0, 0.0, 1.0, 1.0], "第二段蓝");
            assert!((blur - 0.0).abs() < 1e-4, "第二段无 blur → 0");
        }
        _ => panic!("第二段应为 Shadow"),
    }
}

#[test]
fn text_shadow_bare_numbers_no_px() {
    // CSS 允许裸数字（无 px 后缀），与 box-shadow 一致接受。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "text-shadow", "2 2 4 #000000"));
    assert_eq!(s.text_effects.len(), 1);
    match s.text_effects[0] {
        crate::text::font_effect::FontEffect::Shadow { ox, oy, blur, .. } => {
            assert!((ox - 2.0).abs() < 1e-4);
            assert!((oy - 2.0).abs() < 1e-4);
            assert!((blur - 4.0).abs() < 1e-4);
        }
        _ => panic!("expected Shadow"),
    }
}

#[test]
fn text_shadow_named_color_rejected() {
    // parse_color 仅认 hex 形式（3/6/8 位），命名色（red）静默返 false。
    // CSS 一条声明全有或全无 → 非法色整条忽略。
    let mut s = ResolvedStyle::default();
    assert!(
        !apply_decl(&mut s, "text-shadow", "2px 2px red"),
        "命名色 → 整条声明返 false（parse_color 不认命名色）"
    );
    assert!(s.text_effects.is_empty(), "非法声明不污染 text_effects");
}

#[test]
fn text_shadow_missing_color_uses_default_black() {
    // 缺 color 段（仅 ox oy）→ color 默认黑（CSS 规范：未设 color 继承 currentColor，
    // 但围栏不追 currentColor 继承，降级为黑以保可见）。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "text-shadow", "2px 2px"));
    assert_eq!(s.text_effects.len(), 1);
    match s.text_effects[0] {
        crate::text::font_effect::FontEffect::Shadow { color, .. } => {
            assert_eq!(color, [0.0, 0.0, 0.0, 1.0], "缺 color → 默认黑");
        }
        _ => panic!("expected Shadow"),
    }
}

#[test]
fn text_shadow_empty_value_rejected() {
    // 空值 → 返 false，不污染 text_effects（与 transition 空 Vec 语义不同：
    // text-shadow 空 = 未声明，非"清零既有阴影"）。
    let mut s = ResolvedStyle::default();
    assert!(!apply_decl(&mut s, "text-shadow", ""));
    assert!(s.text_effects.is_empty());
}

#[test]
fn background_linear_gradient_2_stops_four_dirs() {
    // 4 正向关键字 × 2 色 → 归一化为角度（to top=0 / right=90 / bottom=180 / left=270）。
    for (val, expected_angle) in [
        ("to right", 90.0),
        ("to left", 270.0),
        ("to top", 0.0),
        ("to bottom", 180.0),
    ] {
        let mut s = ResolvedStyle::default();
        let decl = format!("linear-gradient({val}, #ff0000, #0000ff)");
        assert!(
            apply_decl(&mut s, "background", &decl),
            "background: {decl} 应返回 true"
        );
        let Gradient::Linear { angle_deg, stops } = s.background_gradient.expect("gradient 已设")
        else {
            panic!("关键字方向必须是 Linear");
        };
        assert!(
            (angle_deg - expected_angle).abs() < 1e-4,
            "{val} → {angle_deg}"
        );
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].color, [1.0, 0.0, 0.0, 1.0], "首 stop=红 (#ff0000)");
        assert_eq!(stops[1].color, [0.0, 0.0, 1.0, 1.0], "末 stop=蓝 (#0000ff)");
    }
}

#[test]
fn background_image_linear_gradient_also_accepted() {
    // `background-image: linear-gradient(...)` 走同一解析路径（与 background 等价）。
    let mut s = ResolvedStyle::default();
    assert!(
        apply_decl(
            &mut s,
            "background-image",
            "linear-gradient(to top, #00ff00, #000000)"
        ),
        "background-image: linear-gradient 应被接受"
    );
    let Gradient::Linear { angle_deg, .. } = s.background_gradient.as_ref().expect("gradient 已设")
    else {
        panic!()
    };
    assert!((angle_deg - 0.0).abs() < 1e-4);
    assert_eq!(
        s.background_gradient.as_ref().unwrap().stops()[0].color,
        [0.0, 1.0, 0.0, 1.0]
    );
}

#[test]
fn background_linear_gradient_multi_stop_accepted() {
    // 3+ stop（含显式位置 + rgba）→ 接受；位置默认值按 CSS 规则烘。
    let mut s = ResolvedStyle::default();
    assert!(
        apply_decl(
            &mut s,
            "background",
            "linear-gradient(to right, #ff0000, rgba(0,255,0,0.5) 25%, #0000ff)"
        ),
        "多 stop 应被接受"
    );
    let Gradient::Linear { stops, .. } = s.background_gradient.expect("gradient 已设") else {
        panic!()
    };
    assert_eq!(stops.len(), 3);
    assert_eq!(stops[0].pos, 0.0, "首 stop 默认 0%");
    assert!((stops[1].pos - 0.25).abs() < 1e-5, "显式 25%");
    assert_eq!(stops[1].color, [0.0, 1.0, 0.0, 0.5]);
    assert_eq!(stops[2].pos, 1.0, "末 stop 默认 100%");
}

#[test]
fn background_linear_gradient_middle_defaults_to_midpoint() {
    // 中间 stop 无位置 → 相邻已定位 stop 的中点（CSS 规范默认位置算法）。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(
        &mut s,
        "background",
        "linear-gradient(to bottom, #ff0000 20%, #00ff00, #0000ff 60%)"
    ));
    let Gradient::Linear { stops, .. } = s.background_gradient.expect("gradient 已设") else {
        panic!()
    };
    assert_eq!(stops.len(), 3);
    assert!((stops[0].pos - 0.2).abs() < 1e-5);
    assert!((stops[1].pos - 0.4).abs() < 1e-5, "20% 与 60% 的中点 = 40%");
    assert!((stops[2].pos - 0.6).abs() < 1e-5);
}

#[test]
fn background_linear_gradient_diagonal_angle_accepted() {
    // 任意角度（45deg / 137deg / 负角）→ 接受，归一化保存。
    for (decl_val, expect) in [("45deg", 45.0), ("137deg", 137.0), ("-90deg", -90.0)] {
        let mut s = ResolvedStyle::default();
        assert!(
            apply_decl(
                &mut s,
                "background",
                &format!("linear-gradient({decl_val}, #ff0000, #0000ff)")
            ),
            "{decl_val} 应被接受"
        );
        let Gradient::Linear { angle_deg, .. } = s.background_gradient.expect("gradient 已设")
        else {
            panic!()
        };
        assert!(
            (angle_deg - expect).abs() < 1e-4,
            "{decl_val} → {angle_deg}"
        );
    }
}

#[test]
fn background_linear_gradient_default_direction_is_to_bottom() {
    // 无方向首参 → CSS 默认 to bottom（180deg）。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(
        &mut s,
        "background",
        "linear-gradient(#ff0000, #0000ff)"
    ));
    let Gradient::Linear { angle_deg, .. } = s.background_gradient.expect("gradient 已设") else {
        panic!()
    };
    assert!((angle_deg - 180.0).abs() < 1e-4);
}

#[test]
fn background_gradient_transparent_keyword() {
    // `transparent`（home 光晕用法）= rgba(0,0,0,0)。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(
        &mut s,
        "background",
        "linear-gradient(to right, #ff0000, transparent)"
    ));
    let stops = &s.background_gradient.as_ref().unwrap().stops();
    assert_eq!(stops[1].color, [0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn background_linear_gradient_named_color_rejected() {
    // parse_color 仅认 hex/rgb()/transparent；命名色（red/blue）→ 整体返 false。
    let mut s = ResolvedStyle::default();
    assert!(
        !apply_decl(&mut s, "background", "linear-gradient(to right, red, blue)"),
        "命名色围栏外 → false"
    );
    assert!(s.background_gradient.is_none());
}

#[test]
fn background_linear_gradient_one_stop_accepted_as_solid() {
    // 单 stop = 纯色填充（CSS 合法语义），pos=0。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(
        &mut s,
        "background",
        "linear-gradient(to right, #ff0000)"
    ));
    let Gradient::Linear { stops, .. } = s.background_gradient.expect("gradient 已设") else {
        panic!()
    };
    assert_eq!(stops.len(), 1);
    assert_eq!(stops[0].pos, 0.0);
}

#[test]
fn background_linear_gradient_nine_stops_rejected() {
    // > GRADIENT_MAX_STOPS（8）→ 拒收（FFI grad_params 列定长 8 槽）。
    let mut s = ResolvedStyle::default();
    let stops = (0..9)
        .map(|i| format!("#0000{}0f", i))
        .collect::<Vec<_>>()
        .join(", ");
    assert!(
        !apply_decl(
            &mut s,
            "background",
            &format!("linear-gradient(to right, {stops})")
        ),
        "9 stops 围栏外 → false"
    );
    assert!(s.background_gradient.is_none());
}

#[test]
fn background_linear_gradient_corner_keyword_rejected() {
    // `to top right` 角点方向 defer → 拒收（围栏外显式 false，非静默错向）。
    let mut s = ResolvedStyle::default();
    assert!(!apply_decl(
        &mut s,
        "background",
        "linear-gradient(to top right, #ff0000, #0000ff)"
    ));
    assert!(s.background_gradient.is_none());
}

#[test]
fn background_radial_default_shape() {
    // 无配置首参 → ellipse + farthest-corner + 50% 50%（CSS 默认）。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(
        &mut s,
        "background",
        "radial-gradient(#ff0000, #0000ff)"
    ));
    let Gradient::Radial {
        extent,
        shape,
        center,
        stops,
    } = s.background_gradient.expect("gradient 已设")
    else {
        panic!("radial-gradient 必须解析成 Radial")
    };
    assert!(matches!(extent, RadialExtent::FarthestCorner));
    assert_eq!(shape, RadialShape::Ellipse, "无 shape 关键字 → ellipse");
    assert_eq!(center, [GradCoord::Pct(0.5), GradCoord::Pct(0.5)]);
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].pos, 0.0);
    assert_eq!(stops[1].pos, 1.0);
}

#[test]
fn background_radial_shape_and_size_keywords() {
    for (cfg, expect) in [
        ("circle", RadialExtent::FarthestCorner),
        ("circle closest-side", RadialExtent::ClosestSide),
        ("ellipse farthest-side", RadialExtent::FarthestSide),
        ("closest-corner", RadialExtent::ClosestCorner),
    ] {
        let mut s = ResolvedStyle::default();
        assert!(
            apply_decl(
                &mut s,
                "background",
                &format!("radial-gradient({cfg}, #ff0000, #0000ff)")
            ),
            "{cfg} 应被接受"
        );
        let Gradient::Radial { extent, .. } = s.background_gradient.expect("gradient 已设")
        else {
            panic!()
        };
        assert_eq!(extent, expect, "{cfg}");
    }
}

#[test]
fn background_radial_home_halo_syntax() {
    // home .root 光晕原句：双长度椭圆 + at 负百分比 + 带位置 stop + transparent。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(
        &mut s,
        "background-image",
        "radial-gradient(1100px 560px at 82% -12%, rgba(95,180,212,0.10), transparent 60%)"
    ));
    let Gradient::Radial {
        extent,
        shape,
        center,
        stops,
    } = s.background_gradient.expect("gradient 已设")
    else {
        panic!()
    };
    assert_eq!(extent, RadialExtent::Explicit(Some(1100.0), Some(560.0)));
    assert_eq!(shape, RadialShape::Ellipse, "双长度显式椭圆");
    assert_eq!(center, [GradCoord::Pct(0.82), GradCoord::Pct(-0.12)]);
    assert_eq!(stops.len(), 2);
    assert_eq!(
        stops[0].color,
        [95.0 / 255.0, 180.0 / 255.0, 212.0 / 255.0, 0.10]
    );
    assert_eq!(stops[1].color, [0.0; 4]);
    assert!((stops[1].pos - 0.6).abs() < 1e-5);
}

#[test]
fn background_radial_single_length_is_circle() {
    // 单长度 = 正圆半径（rx=ry）。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(
        &mut s,
        "background",
        "radial-gradient(100px at 30% 40px, #ff0000, #0000ff)"
    ));
    let Gradient::Radial { extent, center, .. } = s.background_gradient.expect("gradient 已设")
    else {
        panic!()
    };
    assert_eq!(extent, RadialExtent::Explicit(Some(100.0), None));
    assert_eq!(center, [GradCoord::Pct(0.3), GradCoord::Px(40.0)]);
}

#[test]
fn background_radial_malformed_rejected() {
    // 坏语法：未知关键字 / 残缺 at / 位置不是 % 或 0 → false。
    for bad in [
        "radial-gradient(to right, #ff0000, #0000ff)", // to 方向是 linear 语法
        "radial-gradient(circle at, #ff0000, #0000ff)", // at 残缺
        "radial-gradient(circle 50%, #ff0000, #0000ff)", // 百分比尺寸围栏外
        "radial-gradient(#ff0000, red)",               // 命名色
    ] {
        let mut s = ResolvedStyle::default();
        assert!(!apply_decl(&mut s, "background", bad), "{bad} → false");
        assert!(s.background_gradient.is_none(), "{bad} 不设 gradient");
    }
}

#[test]
fn background_conic_and_repeating_rejected() {
    // conic / repeating-* 显式 defer → 拒收。
    for bad in [
        "conic-gradient(#ff0000, #0000ff)",
        "repeating-linear-gradient(to right, #ff0000, #0000ff)",
        "repeating-radial-gradient(#ff0000, #0000ff)",
    ] {
        let mut s = ResolvedStyle::default();
        assert!(!apply_decl(&mut s, "background", bad), "{bad} → false");
        assert!(s.background_gradient.is_none());
    }
}

#[test]
fn background_shorthand_url_and_color_supported() {
    // `background` shorthand 现支持 url() 与纯色（扩展后修复假阳性）。
    // url() → background_image；hex 纯色 → background_color。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "background", "url(a.png)"));
    assert_eq!(s.background_image.as_deref(), Some("a.png"));

    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "background", "#ff0000"));
    assert_eq!(s.background_color, Some([1.0, 0.0, 0.0, 1.0]));
}

#[test]
fn background_shorthand_named_color_rejected() {
    // 命名色（red/blue）parse_color 不收（仅 hex 3/6/8 位）→ 整体返 false（不静默降级）。
    let mut s = ResolvedStyle::default();
    assert!(!apply_decl(&mut s, "background", "red"));
    assert!(s.background_gradient.is_none());
    assert!(s.background_image.is_none());
    assert!(
        s.background_color.is_none(),
        "无法解析的值不影响 background_color"
    );
}

#[test]
fn text_stroke_single_with_color() {
    // `-webkit-text-stroke: 2px #000000` → 单 Stroke{w:2, color:black}
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "-webkit-text-stroke", "2px #000000"));
    assert_eq!(s.text_effects.len(), 1, "单 stroke → 1 effect");
    match s.text_effects[0] {
        crate::text::font_effect::FontEffect::Stroke { w, color } => {
            assert!((w - 2.0).abs() < 1e-4, "w=2");
            assert_eq!(color, [0.0, 0.0, 0.0, 1.0], "color=#000000 → black");
        }
        _ => panic!("expected Stroke effect"),
    }
}

#[test]
fn text_stroke_bare_number_no_px() {
    // 裸数字（无 px 后缀）也应接受。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "-webkit-text-stroke", "3 #ff0000"));
    assert_eq!(s.text_effects.len(), 1);
    match s.text_effects[0] {
        crate::text::font_effect::FontEffect::Stroke { w, color } => {
            assert!((w - 3.0).abs() < 1e-4);
            assert_eq!(color, [1.0, 0.0, 0.0, 1.0]);
        }
        _ => panic!("expected Stroke"),
    }
}

#[test]
fn text_stroke_named_color_rejected() {
    // parse_color 仅认 hex 形式（3/6/8 位），命名色静默返 false。
    let mut s = ResolvedStyle::default();
    assert!(
        !apply_decl(&mut s, "-webkit-text-stroke", "2px red"),
        "命名色 → 整条声明返 false"
    );
    assert!(s.text_effects.is_empty(), "非法声明不污染 text_effects");
}

#[test]
fn text_stroke_empty_value_rejected() {
    let mut s = ResolvedStyle::default();
    assert!(!apply_decl(&mut s, "-webkit-text-stroke", ""));
    assert!(s.text_effects.is_empty());
}

#[test]
fn text_stroke_and_shadow_can_coexist() {
    // text-shadow 和 -webkit-text-stroke 同时声明 → text_effects 两项。
    // 先声明 text-shadow，再 -webkit-text-stroke（CSS 源序）。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "text-shadow", "2px 2px 4px #ff0000"));
    assert!(apply_decl(&mut s, "-webkit-text-stroke", "2px #0000ff"));
    assert_eq!(s.text_effects.len(), 2, "shadow + stroke = 2 effects");
    // 序：先 shadow 后 stroke（声明序）
    assert!(matches!(
        s.text_effects[0],
        crate::text::font_effect::FontEffect::Shadow { .. }
    ));
    assert!(matches!(
        s.text_effects[1],
        crate::text::font_effect::FontEffect::Stroke { .. }
    ));
}

#[test]
fn font_effect_glow_with_color() {
    // `font-effect: glow(3px #ee9900)` → Glow{w:3, color:#ee9900}
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "font-effect", "glow(3px #ee9900)"));
    assert_eq!(s.text_effects.len(), 1, "单 glow → 1 effect");
    match s.text_effects[0] {
        crate::text::font_effect::FontEffect::Glow { w, color } => {
            assert!((w - 3.0).abs() < 1e-4, "w=3");
            assert_eq!(color, [0xee as f32 / 255.0, 0x99 as f32 / 255.0, 0.0, 1.0]);
        }
        _ => panic!("expected Glow effect"),
    }
}

#[test]
fn font_effect_glow_without_color_defaults_white() {
    // color 可省 → 默认白。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "font-effect", "glow(2px)"));
    assert_eq!(s.text_effects.len(), 1);
    match s.text_effects[0] {
        crate::text::font_effect::FontEffect::Glow { w, color } => {
            assert!((w - 2.0).abs() < 1e-4);
            assert_eq!(color, [1.0, 1.0, 1.0, 1.0], "缺 color → 默认白");
        }
        _ => panic!("expected Glow"),
    }
}

#[test]
fn font_effect_blur() {
    // `font-effect: blur(2px)` → Blur{w:2}
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "font-effect", "blur(2px)"));
    assert_eq!(s.text_effects.len(), 1);
    match s.text_effects[0] {
        crate::text::font_effect::FontEffect::Blur { w } => {
            assert!((w - 2.0).abs() < 1e-4, "w=2");
        }
        _ => panic!("expected Blur effect"),
    }
}

#[test]
fn font_effect_comma_multiple() {
    // 逗号分隔多 effect：glow + blur。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(
        &mut s,
        "font-effect",
        "glow(3px #ee9900), blur(2px)"
    ));
    assert_eq!(s.text_effects.len(), 2, "两段逗号 → 2 effect");
    assert!(matches!(
        s.text_effects[0],
        crate::text::font_effect::FontEffect::Glow { .. }
    ));
    assert!(matches!(
        s.text_effects[1],
        crate::text::font_effect::FontEffect::Blur { .. }
    ));
}

#[test]
fn font_effect_unknown_type_ignored() {
    // 未知 type（非 glow/blur）→ 静默忽略，不推入 text_effects。
    let mut s = ResolvedStyle::default();
    assert!(
        !apply_decl(&mut s, "font-effect", "unknown(3px #ee9900)"),
        "未知 type → 返 false（无有效 effect 入列）"
    );
    assert!(s.text_effects.is_empty(), "未知 type 不污染 text_effects");
}

#[test]
fn font_effect_glow_with_later_shadow_and_stroke() {
    // font-effect glow + text-shadow + -webkit-text-stroke 可共存（累积进 text_effects）。
    // text-shadow 使用 retain+extend（仅替换自身 Shadow，保留其他属性 effect），
    // 故声明顺序不影响最终组成。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "text-shadow", "2px 2px #000000"));
    assert!(apply_decl(&mut s, "-webkit-text-stroke", "1px #ff0000"));
    assert!(apply_decl(&mut s, "font-effect", "glow(3px #ee9900)"));
    assert_eq!(
        s.text_effects.len(),
        3,
        "shadow + stroke + glow = 3 effects"
    );
    assert!(matches!(
        s.text_effects[0],
        crate::text::font_effect::FontEffect::Shadow { .. }
    ));
    assert!(matches!(
        s.text_effects[1],
        crate::text::font_effect::FontEffect::Stroke { .. }
    ));
    assert!(matches!(
        s.text_effects[2],
        crate::text::font_effect::FontEffect::Glow { .. }
    ));
}

#[test]
fn font_effect_glow_before_shadow_retains_both() {
    // font-effect glow 声明在 text-shadow 之前 → glow 不会被 text-shadow 清洗。
    // text-shadow 用 retain+extend（仅替换 Shadow，保留 Glow/Stroke）。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "font-effect", "glow(3px #ee9900)"));
    assert!(apply_decl(&mut s, "text-shadow", "2px 2px #000000"));
    assert_eq!(
        s.text_effects.len(),
        2,
        "glow (first) + shadow (later) = 2 effects"
    );
    // glow 先声明，shadow 后 retain+extend → glow 在前 shadow 在后
    assert!(matches!(
        s.text_effects[0],
        crate::text::font_effect::FontEffect::Glow { .. }
    ));
    assert!(matches!(
        s.text_effects[1],
        crate::text::font_effect::FontEffect::Shadow { .. }
    ));
}

#[test]
fn font_effect_percent_rejected() {
    // % 在 font-effect width 中静默忽略（宽是 px 半径，% 无意义）。
    let mut s = ResolvedStyle::default();
    assert!(
        !apply_decl(&mut s, "font-effect", "glow(50% #fff)"),
        "glow(50% #fff) → 拒（% 非法）"
    );
    assert!(
        s.text_effects.is_empty(),
        "glow % reject 不污染 text_effects"
    );
    assert!(
        !apply_decl(&mut s, "font-effect", "blur(50%)"),
        "blur(50%) → 拒（% 非法）"
    );
    assert!(s.text_effects.is_empty());
}

#[test]
fn font_effect_bare_number_no_px() {
    // 裸数字（无 px 后缀）也应接受（与 text-shadow / box-shadow 一致）。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "font-effect", "glow(3 #ff0000)"));
    assert_eq!(s.text_effects.len(), 1);
    match s.text_effects[0] {
        crate::text::font_effect::FontEffect::Glow { w, color } => {
            assert!((w - 3.0).abs() < 1e-4);
            assert_eq!(color, [1.0, 0.0, 0.0, 1.0]);
        }
        _ => panic!("expected Glow"),
    }
}

/// invert(0.3) 应是 30% 反相（矩阵非单位、非全反相），
/// 而非旧实现的阈值二分（x<0.5 → IDENTITY）。
#[test]
fn filter_invert_partial_produces_non_identity_matrix() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "filter", "invert(0.3)"));
    let m = s.color_filter.expect("invert(0.3) 设了 color_filter");

    // 反相矩阵格式：[1-2x, 0, 0, 0, x] / [0, 1-2x, 0, 0, x] / [0, 0, 1-2x, 0, x] / [0,0,0,1,0]
    let x = 0.3;
    let expected_diag = 1.0 - 2.0 * x; // 0.4
    assert!(
        (m[0] - expected_diag).abs() < 1e-5,
        "invert(0.3) diag 应 = 1-2x = 0.4, got {}",
        m[0]
    );
    assert!((m[4] - x).abs() < 1e-5, "invert(0.3) offset 应 = x = 0.3");
    // 不是全反相（全反相 diag=-1, offset=1）
    assert!(m[0] > 0.0, "invert(0.3) 不是全反相（全反相 diag=-1.0）");
    // 不是单位矩阵
    assert!(m[0] != 1.0 || m[4] != 0.0, "invert(0.3) 不是单位矩阵");
}

/// invert(1) → 全反相（回归守卫）
#[test]
fn filter_invert_full_produces_full_invert() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "filter", "invert(1)"));
    let m = s.color_filter.expect("invert(1) 设了 color_filter");

    assert_eq!(m[0], -1.0, "全反相 diag r = -1");
    assert_eq!(m[6], -1.0, "全反相 diag g = -1");
    assert_eq!(m[12], -1.0, "全反相 diag b = -1");
    assert_eq!(m[4], 1.0, "全反相 offset = 1");
}

/// invert(0) → 单位矩阵（无效果）
#[test]
fn filter_invert_zero_is_identity() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "filter", "invert(0)"));
    let m = s.color_filter.expect("invert(0) 设了 color_filter");

    assert_eq!(m[0], 1.0, "diag = 1");
    assert_eq!(m[4], 0.0, "offset = 0");
    // 全阵 = IDENTITY
    assert_eq!(&m, &color_filter::IDENTITY, "invert(0) = IDENTITY");
}

#[test]
fn letter_spacing_em_is_rejected() {
    let mut s = ResolvedStyle::default();
    s.letter_spacing = 5.0; // 预设非零，验不被覆盖
    assert!(
        !apply_decl(&mut s, "letter-spacing", "0.1em"),
        "letter-spacing:0.1em → false（em 围栏外）"
    );
    assert_eq!(s.letter_spacing, 5.0, "拒收时不污染既有 letter_spacing");
}

#[test]
fn letter_spacing_rem_is_rejected() {
    let mut s = ResolvedStyle::default();
    assert!(
        !apply_decl(&mut s, "letter-spacing", "1rem"),
        "letter-spacing:1rem → false"
    );
    assert_eq!(s.letter_spacing, 0.0, "拒收时保持默认 0");
}

#[test]
fn letter_spacing_px_is_accepted() {
    let mut s = ResolvedStyle::default();
    assert!(
        apply_decl(&mut s, "letter-spacing", "2px"),
        "letter-spacing:2px → true"
    );
    assert!(
        (s.letter_spacing - 2.0).abs() < 1e-5,
        "letter-spacing = 2.0"
    );
}

/// border-top/right/bottom/left 单边 longhand：设 ts.border 对应边 + border_color，不动其他三边。
#[test]
fn apply_border_side_longhands_set_one_side_only() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border-bottom", "1px solid #3a3f55"));
    let ts = &s.taffy_style.border;
    assert_eq!(ts.bottom, LengthPercentage::length(1.0), "bottom 设了");
    assert_eq!(ts.top, LengthPercentage::length(0.0), "top 不动（默认 0）");
    assert_eq!(ts.left, LengthPercentage::length(0.0));
    assert_eq!(ts.right, LengthPercentage::length(0.0));
    let c = s.border_color.expect("单边 color 解析");
    assert_eq!(c[0], 0x3a as f32 / 255.0);

    // 累积：再设 top，bottom 仍在
    assert!(apply_decl(&mut s, "border-top", "4px solid #e0e0e0"));
    assert_eq!(s.taffy_style.border.top, LengthPercentage::length(4.0));
    assert_eq!(
        s.taffy_style.border.bottom,
        LengthPercentage::length(1.0),
        "bottom 不被覆盖"
    );
}

#[test]
fn apply_border_side_longhand_rejects_non_px() {
    // 非 px width → 整条 false（围栏外静默忽略），不碰任何字段
    let mut s = ResolvedStyle::default();
    assert!(!apply_decl(&mut s, "border-bottom", "1em solid red"));
    assert_eq!(
        s.taffy_style.border.bottom,
        LengthPercentage::length(0.0),
        "失败不设值"
    );
    assert!(s.border_color.is_none(), "失败不设 color");
}

#[test]
fn apply_border_side_longhand_optional_color() {
    // border-bottom:1px（无 color）→ 设宽度，不碰 border_color
    let mut s = ResolvedStyle::default();
    s.border_color = Some([0.5; 4]);
    assert!(apply_decl(&mut s, "border-bottom", "1px"));
    assert_eq!(s.taffy_style.border.bottom, LengthPercentage::length(1.0));
    assert_eq!(s.border_color, Some([0.5; 4]), "无 color token 不覆盖");
}

#[test]
fn apply_border_style_longhand() {
    use crate::style::resolved::BorderStyle;
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border-style", "solid"));
    assert_eq!(s.border_style, BorderStyle::Solid);
}

#[test]
fn apply_border_shorthand_captures_style() {
    use crate::style::resolved::BorderStyle;
    // border: 2px solid red → width + style + color 都进
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border", "2px solid #ff0000"));
    assert_eq!(s.border_style, BorderStyle::Solid);
    assert_eq!(s.border_color, Some([1.0, 0.0, 0.0, 1.0]));
    // width 四边
    let bw = &s.taffy_style.border;
    assert!((resolve_lp_for_test(bw.left) - 2.0).abs() < 0.01);
}

#[test]
fn apply_border_no_style_keeps_none() {
    use crate::style::resolved::BorderStyle;
    // border: 2px red（无 style）→ border_style 仍 None（CSS 规范：不画）
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "border", "2px #ff0000"));
    assert_eq!(s.border_style, BorderStyle::None);
}

// shorthand 展开 + longhand 补齐。
// flex shorthand：单值 `flex:1` → grow=1/shrink=1（CSS 规范 basis=0%）。
#[test]
fn flex_shorthand_single_value() {
    use taffy::style::Dimension;
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "flex", "1"));
    assert!((s.taffy_style.flex_grow - 1.0).abs() < 0.01);
    assert!((s.taffy_style.flex_shrink - 1.0).abs() < 0.01);
    assert_eq!(
        s.taffy_style.flex_basis,
        Dimension::percent(0.0),
        "单 number → basis=0%（CSS 规范）"
    );
}

// flex shorthand：三值 `flex:2 0 100px` → grow=2/shrink=0/basis=100px。
#[test]
fn flex_shorthand_three_values() {
    use taffy::style::Dimension;
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "flex", "2 0 100px"));
    assert!((s.taffy_style.flex_grow - 2.0).abs() < 0.01);
    assert!((s.taffy_style.flex_shrink - 0.0).abs() < 0.01);
    // basis 必须被实际解析（旧实现漏断言，basis bug 静默过关）。
    assert_eq!(
        s.taffy_style.flex_basis,
        Dimension::length(100.0),
        "basis=100px"
    );
}

// flex shorthand：两值歧义——`flex:1 50%` 第二 token 是 basis（非 shrink）。
// 旧实现把 50% 当 shrink → parse 失败 unwrap_or(1.0) → basis 静默变 0%。
#[test]
fn flex_shorthand_two_values_basis_percent() {
    use taffy::style::Dimension;
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "flex", "1 50%"));
    assert!((s.taffy_style.flex_grow - 1.0).abs() < 0.01);
    assert!(
        (s.taffy_style.flex_shrink - 1.0).abs() < 0.01,
        "basis 形态时 shrink 默认 1"
    );
    assert_eq!(
        s.taffy_style.flex_basis,
        Dimension::percent(0.5),
        "basis=50%"
    );
}

// flex shorthand：两值歧义——`flex:1 auto` 第二 token 是 basis(auto)。
#[test]
fn flex_shorthand_two_values_basis_auto() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "flex", "1 auto"));
    assert!((s.taffy_style.flex_grow - 1.0).abs() < 0.01);
    assert!((s.taffy_style.flex_shrink - 1.0).abs() < 0.01);
    assert!(s.taffy_style.flex_basis.is_auto(), "basis=auto");
}

// flex shorthand：两值都是 number → grow + shrink（basis=0%）。
#[test]
fn flex_shorthand_two_values_grow_shrink() {
    use taffy::style::Dimension;
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "flex", "2 0"));
    assert!((s.taffy_style.flex_grow - 2.0).abs() < 0.01);
    assert!((s.taffy_style.flex_shrink - 0.0).abs() < 0.01);
    assert_eq!(s.taffy_style.flex_basis, Dimension::percent(0.0));
}

// flex shorthand：`auto` 关键字 ≡ `1 1 auto`（CSS spec，与 initial 对称）。
#[test]
fn flex_shorthand_auto_keyword() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "flex", "auto"));
    assert!((s.taffy_style.flex_grow - 1.0).abs() < 0.01);
    assert!((s.taffy_style.flex_shrink - 1.0).abs() < 0.01);
    assert!(s.taffy_style.flex_basis.is_auto());
}

// flex shorthand：畸形值必须返 false（不静默降级）——
// 旧实现 unwrap_or 会把 `flex:abc 2` 静默成 grow=0。锁住不静默降级不变量。
#[test]
fn flex_shorthand_invalid_values_rejected() {
    let mut s = ResolvedStyle::default();
    // 非法 grow
    assert!(!apply_decl(&mut s, "flex", "abc"));
    // 两 token：非法 grow
    assert!(!apply_decl(&mut s, "flex", "abc 2"));
    // 两 token：第二 token 既非 number 也非 basis
    assert!(!apply_decl(&mut s, "flex", "1 xyz"));
    // 三 token：shrink 非法
    assert!(!apply_decl(&mut s, "flex", "1 xyz 100px"));
    // 三 token：basis 非法（裸数字无单位不是合法 basis）
    assert!(!apply_decl(&mut s, "flex", "1 1 5"));
}

// background shorthand：纯色 `background:#ff0000` 应展开成 background_color
// （修复假阳性：原本仅识别 gradient，纯色静默返 false）。
#[test]
fn background_shorthand_color() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "background", "#ff0000"));
    assert_eq!(s.background_color, Some([1.0, 0.0, 0.0, 1.0]));
}

// align-content longhand：补齐缺失分支（与 justify-content 对称的 cross 轴对齐）。
#[test]
fn align_content_longhand_applies() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "align-content", "center"));
    assert_eq!(
        s.taffy_style.align_content,
        Some(taffy::AlignContent::CENTER)
    );
}

// align-content: stretch 是 CSS 默认值 + fence schema 合法 keyword。
// 回归锁：显式写 stretch 必须映射到 STRETCH，不得静默降级成 FLEX_START
//（修复前 align-content 分支复用 parse_justify，后者无 stretch 分支，stretch 被
// `_ => FLEX_START` 吞掉）。
#[test]
fn align_content_stretch_not_downgraded() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "align-content", "stretch"));
    assert_eq!(
        s.taffy_style.align_content,
        Some(taffy::AlignContent::STRETCH)
    );
}

// align-content 全 schema 合法值（flex-start/center/flex-end/stretch/
// space-between/space-around/space-evenly）逐一映射，确保无其它静默降级。
#[test]
fn align_content_all_schema_keywords_map() {
    for (input, expected) in [
        ("flex-start", taffy::AlignContent::FLEX_START),
        ("center", taffy::AlignContent::CENTER),
        ("flex-end", taffy::AlignContent::FLEX_END),
        ("stretch", taffy::AlignContent::STRETCH),
        ("space-between", taffy::AlignContent::SPACE_BETWEEN),
        ("space-around", taffy::AlignContent::SPACE_AROUND),
        ("space-evenly", taffy::AlignContent::SPACE_EVENLY),
    ] {
        let mut s = ResolvedStyle::default();
        assert!(
            apply_decl(&mut s, "align-content", input),
            "{input} 应被识别"
        );
        assert_eq!(
            s.taffy_style.align_content,
            Some(expected),
            "align-content: {input} 映射错误"
        );
    }
}

// row-gap longhand：补齐缺失分支。CSS row-gap 对应 taffy gap.height（行间距=纵向）。
#[test]
fn row_gap_longhand_applies() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "row-gap", "10px"));
    assert!((resolve_lp_for_test(s.taffy_style.gap.height) - 10.0).abs() < 0.01);
}

// row-gap/column-gap 必须与 `gap` shorthand 同口径：px 与裸数字都接受。
// 此前 strip_suffix("px") 拒裸数字——row-gap:0 / column-gap:10 被静默丢弃，
// 连 schema default `0`（裸数字）都过不了。
#[test]
fn row_column_gap_accept_bare_number() {
    // row-gap:0 —— default 值（裸 0），必须生效且落 gap.height=0。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "row-gap", "0"), "row-gap:0 应被接受");
    assert!((resolve_lp_for_test(s.taffy_style.gap.height) - 0.0).abs() < 0.01);

    // column-gap:10 —— 裸数字（无 px），必须生效且落 gap.width=10。
    let mut s = ResolvedStyle::default();
    assert!(
        apply_decl(&mut s, "column-gap", "10"),
        "column-gap:10 应被接受"
    );
    assert!((resolve_lp_for_test(s.taffy_style.gap.width) - 10.0).abs() < 0.01);

    // px 后缀仍接受（不回退）。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "column-gap", "10px"));
    assert!((resolve_lp_for_test(s.taffy_style.gap.width) - 10.0).abs() < 0.01);
}

// 测试用：把 LengthPercentage 解析回 f32（taffy 0.12 是 tagged pointer struct，
// 用 into_raw + tag + value 解构，复用 render 的 resolve_lp 逻辑）。
fn resolve_lp_for_test(lp: taffy::style::LengthPercentage) -> f32 {
    let cl = lp.into_raw();
    cl.value()
}

#[test]
fn caret_color_applies() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "caret-color", "#ff0000"));
    assert_eq!(s.caret_color, Some([1.0, 0.0, 0.0, 1.0]));
}

#[test]
fn selection_background_applies() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "selection-background", "#00ff00"));
    assert_eq!(s.selection_background, Some([0.0, 1.0, 0.0, 1.0]));
}

#[test]
fn selection_color_applies() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "selection-color", "#0000ff"));
    assert_eq!(s.selection_color, Some([0.0, 0.0, 1.0, 1.0]));
}

/// 坏色值 → apply_decl 返 true 但字段落 None（与 background-color 同口径：
/// 静默吞不可解析色，不报错）。render 退回缺省 fallback。
#[test]
fn caret_selection_bad_color_falls_to_none() {
    let mut s = ResolvedStyle::default();
    assert!(
        apply_decl(&mut s, "caret-color", "notacolor"),
        "bad color returns true (same as background-color convention)"
    );
    assert!(
        apply_decl(&mut s, "selection-background", "bogus"),
        "bad color returns true"
    );
    assert!(s.caret_color.is_none(), "bad color → None");
    assert!(s.selection_background.is_none(), "bad color → None");
}

// box-shadow: 括号感知 tokenizer（多层 / inset / blur / spread / spaced rgba）。
// 逗号在括号深度 0 切层；rgba(...) 内部的空格/逗号不能切层。
#[test]
fn box_shadow_multilayer_inset_blur() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(
        &mut s,
        "box-shadow",
        "0 0 0 1px rgba(95,180,212,0.5), inset 0 1px 0 rgba(255,255,255,0.06)"
    ));
    assert_eq!(s.box_shadow.len(), 2);
    // layer 0 outer
    assert!(!s.box_shadow[0].inset);
    assert_eq!(s.box_shadow[0].spread, 1.0);
    assert_eq!(s.box_shadow[0].blur, 0.0);
    assert_eq!(
        s.box_shadow[0].color,
        [95.0 / 255.0, 180.0 / 255.0, 212.0 / 255.0, 0.5]
    );
    // layer 1 inset
    assert!(s.box_shadow[1].inset);
    assert_eq!(s.box_shadow[1].oy, 1.0);
    assert_eq!(s.box_shadow[1].blur, 0.0);
}

#[test]
fn box_shadow_blur_spread_spaced_rgba() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(
        &mut s,
        "box-shadow",
        "0 8px 26px rgba(95, 180, 212, 0.5)"
    ));
    assert_eq!(s.box_shadow.len(), 1);
    assert_eq!(s.box_shadow[0].blur, 26.0);
    assert_eq!(s.box_shadow[0].spread, 0.0);
    assert_eq!(
        s.box_shadow[0].color,
        [95.0 / 255.0, 180.0 / 255.0, 212.0 / 255.0, 0.5]
    );
}

#[test]
fn box_shadow_inset_trailing_keyword() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "box-shadow", "0 0 0 1px #fff inset"));
    assert!(s.box_shadow[0].inset);
}

#[test]
fn box_shadow_illegal_returns_false() {
    let mut s = ResolvedStyle::default();
    assert!(!apply_decl(&mut s, "box-shadow", "10px"), "<2 数值 → false");
    // parse_color 把任何 3/6/8 位 hex 串当合法颜色（#abc = #aabbcc），
    // 故 "abc" 实为合法 3 位 hex；此处用 parse_color 必拒的非 hex 记号验证非法路径。
    assert!(
        !apply_decl(&mut s, "box-shadow", "0 0 0 notacolor"),
        "bad color → false"
    );
    assert!(s.box_shadow.is_empty());
}

// 层数硬限（render 合成 node_id high-byte 编码区：inset 36..=43 / outer 44..=47）。
// 超限层的合成 id 撞相邻编码区 → 错层序/漏 mask 传播，宁可整条拒收（apply_decl false）。
fn layers(n: usize, prefix: &str) -> String {
    std::iter::repeat_n(format!("{prefix} 1px 1px #000"), n)
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn box_shadow_layer_cap_rejects_over_limit() {
    let mut s = ResolvedStyle::default();
    // 边界内放行：8 inset / 4 outer 各自顶格。
    assert!(apply_decl(&mut s, "box-shadow", &layers(8, "inset 0")));
    assert_eq!(s.box_shadow.len(), 8);
    assert!(apply_decl(&mut s, "box-shadow", &layers(4, "0")));
    assert_eq!(s.box_shadow.len(), 4);
    // 超限拒收：第 9 层 inset 的 id 撞 outer 编码区；第 5 层 outer 落识别区外。
    assert!(
        !apply_decl(&mut s, "box-shadow", &layers(9, "inset 0")),
        "9th inset layer overflows the synth-id encoding"
    );
    assert!(
        !apply_decl(&mut s, "box-shadow", &layers(5, "0")),
        "5th outer layer overflows the synth-id encoding"
    );
    // 混合声明：inset/outer 各自计数、不共享额度——总层数超单类上限但两类各自
    // 限内（6 inset + 3 outer = 9 层 > 8）仍放行。
    let mixed_ok = format!("{}, {}", layers(6, "inset 0"), layers(3, "0"));
    assert!(apply_decl(&mut s, "box-shadow", &mixed_ok));
    assert_eq!(s.box_shadow.len(), 9);
    // 一类超限即整条拒收，不留半截（拒绝不覆盖既有值）。
    let mixed_bad = format!("{}, {}", layers(8, "inset 0"), layers(5, "0"));
    assert!(!apply_decl(&mut s, "box-shadow", &mixed_bad));
    assert_eq!(s.box_shadow.len(), 9, "rejected decl keeps prior value");
}

// CSS 级联覆盖：`box-shadow: 0 8px 4px #000; box-shadow: none;` → 后写者胜，清空。
// apply_decl 的 `Some(_)` 分支显式清 `style.box_shadow = Vec::new()`（而非保留旧值），
// 否则 `none` 会被静默忽略，导致设计稿里 "先建后删" 的盒阴影残留在渲染中。
// 同时验证 `none` 大小写不敏感（CSS 关键字 case-insensitive，与 inset 同口径）。
#[test]
fn box_shadow_none_clears_prior() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "box-shadow", "0 8px 4px #000"));
    assert_eq!(s.box_shadow.len(), 1);
    assert!(apply_decl(&mut s, "box-shadow", "none"));
    assert!(s.box_shadow.is_empty(), "none clears prior shadows");
    // CSS 关键字大小写不敏感。
    assert!(apply_decl(&mut s, "box-shadow", "0 4px 2px #000"));
    assert_eq!(s.box_shadow.len(), 1);
    assert!(apply_decl(&mut s, "box-shadow", "NONE"));
    assert!(s.box_shadow.is_empty(), "NONE clears prior shadows");
}

#[test]
fn aspect_ratio_parses_ratio_and_number_and_auto() {
    // CSS <ratio> 三形态。taffy 原生消费 width/height 比。
    let mut s = ResolvedStyle::default();

    // `16/9` —— showcase 用法，此前被 parse::<f32>() 吞掉 → 节点缺高度不可见。
    assert!(apply_decl(&mut s, "aspect-ratio", "16/9"));
    assert!((s.taffy_style.aspect_ratio.unwrap() - 16.0 / 9.0).abs() < 1e-4);

    // 纯数字（CSS 也接受 ratio 写成单 number）。
    assert!(apply_decl(&mut s, "aspect-ratio", "1.5"));
    assert!((s.taffy_style.aspect_ratio.unwrap() - 1.5).abs() < 1e-4);

    // auto —— 显式清除比值。
    assert!(apply_decl(&mut s, "aspect-ratio", "1"));
    assert!(apply_decl(&mut s, "aspect-ratio", "auto"));
    assert!(s.taffy_style.aspect_ratio.is_none());
}

#[test]
fn aspect_ratio_rejects_garbage_not_silent() {
    // 不可解析值必须返 false，让围栏打包期报 FenceBadCssValue（不静默降级）。
    let mut s = ResolvedStyle::default();
    assert!(!apply_decl(&mut s, "aspect-ratio", "abc"));
    assert!(!apply_decl(&mut s, "aspect-ratio", "16/0")); // 分母 0
    assert!(s.taffy_style.aspect_ratio.is_none());
}

#[test]
fn font_family_takes_first_name_stripping_quotes() {
    // CSS 逗号列表 + 引号形式须取首个 family 名——整串存会让 FontTable 精确匹配失配。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(
        &mut s,
        "font-family",
        "\"JetBrainsMono\",monospace"
    ));
    assert_eq!(s.font_family.as_deref(), Some("JetBrainsMono"));
    assert!(apply_decl(&mut s, "font-family", "serif"));
    assert_eq!(s.font_family.as_deref(), Some("serif"));
    assert!(apply_decl(
        &mut s,
        "font-family",
        "'PressStart2P' , monospace"
    ));
    assert_eq!(s.font_family.as_deref(), Some("PressStart2P"));
}

#[test]
fn z_index_parses_integer_and_falls_back_to_zero() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "z-index", "5"));
    assert_eq!(s.z_index, 5);
    assert!(apply_decl(&mut s, "z-index", "-3"));
    assert_eq!(s.z_index, -3);
    // 非法值降级 0（fence 打包期拦 auto/非整数；运行时逃生舱宽松，同 order 策略）。
    assert!(apply_decl(&mut s, "z-index", "auto"));
    assert_eq!(s.z_index, 0);
    assert!(apply_decl(&mut s, "z-index", "bogus"));
    assert_eq!(s.z_index, 0);
}

#[test]
fn animation_longhands_broadcast_and_compose_with_shorthand() {
    // 简写在先、长划改字段：广播写全部既有 spec。
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "animation", "fade .3s, slide .5s"));
    assert_eq!(s.animation.len(), 2);
    assert!(apply_decl(&mut s, "animation-duration", "2s"));
    assert!(s.animation.iter().all(|a| a.duration == 2.0));
    assert!(apply_decl(&mut s, "animation-timing-function", "linear"));
    assert!(s
        .animation
        .iter()
        .all(|a| a.timing_function == crate::tween::Ease::Linear));
    assert!(apply_decl(&mut s, "animation-iteration-count", "infinite"));
    assert!(s.animation.iter().all(|a| a.iteration_count.is_none()));
    assert!(apply_decl(&mut s, "animation-direction", "alternate"));
    assert!(apply_decl(&mut s, "animation-fill-mode", "forwards"));
    assert!(apply_decl(&mut s, "animation-play-state", "paused"));
    assert!(apply_decl(&mut s, "animation-delay", ".1s"));
    assert_eq!(s.animation[0].delay, 0.1);

    // 长划先于 name：惰性 spec（name 空 = 不播），name 到位补齐。
    let mut s2 = ResolvedStyle::default();
    assert!(apply_decl(&mut s2, "animation-duration", ".4s"));
    assert_eq!(s2.animation.len(), 1);
    assert_eq!(s2.animation[0].name, "");
    assert_eq!(s2.animation[0].duration, 0.4);
    assert!(apply_decl(&mut s2, "animation-name", "pop"));
    assert_eq!(s2.animation[0].name, "pop");

    // animation-name:none 清空；非法值（逗号列表 / 坏关键字）返 false。
    assert!(apply_decl(&mut s2, "animation-name", "none"));
    assert!(s2.animation.is_empty());
    assert!(!apply_decl(&mut s2, "animation-duration", ".4s, .8s"));
    assert!(!apply_decl(&mut s2, "animation-direction", "bogus"));
    assert!(!apply_decl(&mut s2, "animation-name", "123bad"));
}

#[test]
fn animation_longhand_nameless_spec_skipped_by_player_sync() {
    // sync_animation_players 对空 name spec 不建 player（长划惰性声明）；
    // name 到位的 spec 正常建——一节点两 spec 只产一个 player。
    use crate::scene::node::{Node, Scene};
    use crate::style::dynamic::sync_animation_players;
    use crate::style::resolved::{
        AnimationDirection, AnimationFillMode, AnimationPlayState, AnimationSpec,
    };
    let initial = |name: &str| AnimationSpec {
        name: name.to_string(),
        duration: 0.4,
        delay: 0.0,
        iteration_count: Some(1),
        direction: AnimationDirection::Normal,
        fill_mode: AnimationFillMode::None,
        timing_function: crate::tween::Ease::CubicOut,
        play_state: AnimationPlayState::Running,
    };
    let mut root = Node::default();
    root.style.animation = vec![initial(""), initial("pop")];
    let mut scene = Scene::from_nodes(vec![root], vec![]);
    let root_id = scene.roots[0];
    // keyframes 命名 "pop"（与 initial("") 的空 name 区分——空 name 无 keyframes 可配）。
    scene.keyframes.insert(
        "pop".into(),
        crate::scene::animation::KeyframesRule {
            name: "pop".into(),
            stops: vec![
                crate::scene::animation::KeyframeStop {
                    selector: crate::scene::animation::KeyframeStopSelector::From,
                    props: crate::scene::animation::AnimatableProps {
                        opacity: Some(0.0),
                        ..Default::default()
                    },
                    timing: None,
                    hook: None,
                },
                crate::scene::animation::KeyframeStop {
                    selector: crate::scene::animation::KeyframeStopSelector::To,
                    props: crate::scene::animation::AnimatableProps {
                        opacity: Some(1.0),
                        ..Default::default()
                    },
                    timing: None,
                    hook: None,
                },
            ],
        },
    );
    sync_animation_players(&mut scene);
    let named: Vec<_> = scene
        .players
        .values()
        .filter(|p| p.node == root_id)
        .collect();
    assert_eq!(named.len(), 1, "只建 name=pop 的 player：{:?}", named.len());
    assert_eq!(named[0].spec.name, "pop");
}

/// 视口相对单位（vw/vh/vmin/vmax）解析：进 `viewport` 平行槽、taffy 落 length(0)
/// 占位；后续 px/% 声明清槽——CSS 级联后者胜出，px 覆写 vw 后 vw 必须失效。
#[test]
fn viewport_units_parse_into_parallel_slots() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "width", "50vw"));
    assert_eq!(
        s.viewport.width,
        Some(ViewportLen {
            value: 50.0,
            unit: ViewportUnit::Vw
        })
    );
    assert_eq!(s.taffy_style.size.width, Dimension::length(0.0));
    assert!(apply_decl(&mut s, "min-height", "10vmin"));
    assert_eq!(
        s.viewport.min_height.map(|v| v.unit),
        Some(ViewportUnit::Vmin)
    );
    assert!(apply_decl(&mut s, "max-width", "2.5vmax"));
    assert_eq!(s.viewport.max_width.map(|v| v.value), Some(2.5));
    assert!(apply_decl(&mut s, "top", "5vh"));
    assert_eq!(
        s.viewport.inset[0].map(|v| (v.value, v.unit)),
        Some((5.0, ViewportUnit::Vh))
    );
    // 级联清槽：后声明 px → 视口覆盖失效、taffy 拿真值
    assert!(apply_decl(&mut s, "width", "100px"));
    assert_eq!(s.viewport.width, None);
    assert_eq!(s.taffy_style.size.width, Dimension::length(100.0));
    // 视口单位不是「任意后缀都吃」：无数字前缀 / 未知单位仍走旧路径
    assert!(apply_decl(&mut s, "height", "auto"));
    assert_eq!(s.viewport.height, None);
}

/// margin 混合 token（`2vh auto`）：视口边进覆盖槽 + taffy 落 0，auto 边保持 auto；
/// 单边 longhand 只动该边视口槽。
#[test]
fn viewport_margin_mixed_tokens() {
    use taffy::style::LengthPercentageAuto;
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "margin", "2vh auto"));
    let vh2 = || ViewportLen {
        value: 2.0,
        unit: ViewportUnit::Vh,
    };
    assert_eq!(s.viewport.margin, [Some(vh2()), None, Some(vh2()), None]);
    assert_eq!(s.taffy_style.margin.top, LengthPercentageAuto::length(0.0));
    assert_eq!(s.taffy_style.margin.left, LengthPercentageAuto::auto());
    // 单边声明清该边槽 + 设新槽
    assert!(apply_decl(&mut s, "margin-left", "1vw"));
    assert_eq!(s.viewport.margin[3].map(|v| v.unit), Some(ViewportUnit::Vw));
    assert!(apply_decl(&mut s, "margin-left", "8px"));
    assert_eq!(s.viewport.margin[3], None);
    assert_eq!(s.taffy_style.margin.left, LengthPercentageAuto::length(8.0));
}

/// px-only 通道（padding/gap/font-size，fence 值域 Length=px）不吃视口单位——
/// 返 false 走围栏诊断，不静默落值。
#[test]
fn viewport_px_only_channels_reject() {
    let mut s = ResolvedStyle::default();
    assert!(!apply_decl(&mut s, "padding", "10vw"));
    assert!(!apply_decl(&mut s, "gap", "1vh"));
    assert!(!apply_decl(&mut s, "padding-top", "3vmin"));
    // font-size 臂是宽容语义（非法值静默保持原值返 true，与 "1em" 同路径）——
    // vw 不生效但也不报错；px-only 承诺靠 padding/gap 的硬拒绝兑现。
    let before = s.font_size;
    assert!(apply_decl(&mut s, "font-size", "2vw"));
    assert_eq!(s.font_size, before);
}

/// 换算数学：vw/vh 按对应维，vmin/vmax 取两维较小/较大者。
#[test]
fn viewport_len_resolve_math() {
    let root = (1080.0, 1920.0);
    let vw = ViewportLen {
        value: 50.0,
        unit: ViewportUnit::Vw,
    };
    assert_eq!(vw.resolve(root), 540.0);
    let vh = ViewportLen {
        value: 10.0,
        unit: ViewportUnit::Vh,
    };
    assert_eq!(vh.resolve(root), 192.0);
    let vmin = ViewportLen {
        value: 10.0,
        unit: ViewportUnit::Vmin,
    };
    assert_eq!(vmin.resolve(root), 108.0);
    let vmax = ViewportLen {
        value: 10.0,
        unit: ViewportUnit::Vmax,
    };
    assert_eq!(vmax.resolve(root), 192.0);
}

/// #65：line-height 三形（倍数 / px / normal）双槽 + 级联完胜 + 围栏外形拒收。
#[test]
fn apply_decl_line_height_three_forms() {
    let mut s = ResolvedStyle::default();
    assert!(apply_decl(&mut s, "line-height", "1.6"));
    assert_eq!(s.line_height, 1.6);
    assert_eq!(s.line_height_px, None);

    assert!(apply_decl(&mut s, "line-height", "27px"));
    assert_eq!(s.line_height_px, Some(27.0));
    assert_eq!(s.line_height, 0.0, "px 形完胜前声明的倍数（不留 stale）");

    assert!(apply_decl(&mut s, "line-height", "normal"));
    assert_eq!(s.line_height, 0.0);
    assert_eq!(s.line_height_px, None);

    // 围栏外形拒收（em / % / 负数）
    assert!(!apply_decl(&mut s, "line-height", "1.5em"));
    assert!(!apply_decl(&mut s, "line-height", "150%"));
    assert!(!apply_decl(&mut s, "line-height", "-2"));
    assert_eq!(s.line_height_px, None);
}

/// #65：effective_line_height 换算——px 形按本元素 font_size（px 继承为 px 的
/// CSS computed 语义），倍数形原样，无效 px 回退倍数槽。
#[test]
fn effective_line_height_resolves_px_against_font_size() {
    let mut s = ResolvedStyle::default();
    s.font_size = 17.0;
    s.line_height_px = Some(27.0);
    assert!((s.effective_line_height() - 27.0 / 17.0).abs() < 1e-4);

    // px 继承为 px：子元素换大字号，换算基准跟着变（27px 仍是 27px 行高）
    s.font_size = 32.0;
    assert!((s.effective_line_height() - 27.0 / 32.0).abs() < 1e-4);

    s.line_height_px = None;
    s.line_height = 1.8;
    assert_eq!(s.effective_line_height(), 1.8);

    s.line_height_px = Some(0.0); // 无效 px（≤0）回退倍数槽
    assert_eq!(s.effective_line_height(), 1.8);
}
