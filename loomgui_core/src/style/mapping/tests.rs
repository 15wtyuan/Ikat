use super::*;
use taffy::style::LengthPercentage;
#[test]
fn parse_length_px_pct_auto() {
    assert!(matches!(parse_lp("100px"), LengthPercentage::Length(100.0)));
    assert!(matches!(parse_lp("50%"), LengthPercentage::Percent(0.5)));
}
/// `width:auto` 必须解析成 `Dimension::Auto`（fit-content），
/// 不能 fallback 到 `Length(0.0)`（→ img rect=(0,0) 不渲染）。
#[test]
fn parse_dimension_auto_is_auto_not_zero() {
    use taffy::style::Dimension;
    assert!(
        matches!(parse_dimension("auto"), Dimension::Auto),
        "auto → Auto"
    );
    assert!(matches!(parse_dimension("80px"), Dimension::Length(80.0)));
    assert!(matches!(parse_dimension("50%"), Dimension::Percent(0.5)));
}
#[test]
fn four_value_expand() {
    assert_eq!(parse_four("4px"), [4.0, 4.0, 4.0, 4.0]);
    assert_eq!(parse_four("4px 8px"), [4.0, 8.0, 4.0, 8.0]);
}
#[test]
fn color_hex() {
    let c = parse_color("#ff0000").unwrap();
    assert_eq!(c, [1.0, 0.0, 0.0, 1.0]);
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
        assert_eq!(c.h, LengthPercentage::Percent(0.5));
        assert_eq!(c.v, LengthPercentage::Percent(0.5));
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
/// LoomGUI 应同：acc 从 IDENTITY 起，每步 `acc = concat(m, acc)`（新值在左）。
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
fn background_linear_gradient_2_stops_four_dirs() {
    // 4 正向 × 2 色 → 返 true 且 background_gradient 已设。
    for (val, expected_dir) in [
        ("to right", GradientDir::ToRight),
        ("to left", GradientDir::ToLeft),
        ("to top", GradientDir::ToTop),
        ("to bottom", GradientDir::ToBottom),
    ] {
        let mut s = ResolvedStyle::default();
        let decl = format!("linear-gradient({val}, #ff0000, #0000ff)");
        assert!(
            apply_decl(&mut s, "background", &decl),
            "background: {decl} 应返回 true"
        );
        let g = s.background_gradient.expect("gradient 已设");
        assert_eq!(g.dir, expected_dir, "方向匹配 {val}");
        assert_eq!(g.color_a, [1.0, 0.0, 0.0, 1.0], "color_a=红 (#ff0000)");
        assert_eq!(g.color_b, [0.0, 0.0, 1.0, 1.0], "color_b=蓝 (#0000ff)");
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
    let g = s.background_gradient.expect("gradient 已设");
    assert_eq!(g.dir, GradientDir::ToTop);
    assert_eq!(g.color_a, [0.0, 1.0, 0.0, 1.0]);
}

#[test]
fn background_linear_gradient_multi_stop_rejected() {
    // >2 色 stop → 静默忽略（返 false），不设 gradient。
    let mut s = ResolvedStyle::default();
    assert!(
        !apply_decl(
            &mut s,
            "background",
            "linear-gradient(to right, #ff0000, #00ff00, #0000ff)"
        ),
        "3 色 stop 围栏外 → false"
    );
    assert!(s.background_gradient.is_none(), "多 stop 不设 gradient");
}

#[test]
fn background_linear_gradient_diagonal_angle_rejected() {
    // 斜角度（45deg 等）→ 静默忽略（返 false）。
    let mut s = ResolvedStyle::default();
    assert!(
        !apply_decl(
            &mut s,
            "background",
            "linear-gradient(45deg, #ff0000, #0000ff)"
        ),
        "斜角度围栏外 → false"
    );
    assert!(s.background_gradient.is_none(), "斜角度不设 gradient");
}

#[test]
fn background_linear_gradient_named_color_rejected() {
    // parse_color 仅认 6 位 hex；命名色（red/blue）→ 解析失败 → 整体返 false。
    let mut s = ResolvedStyle::default();
    assert!(
        !apply_decl(&mut s, "background", "linear-gradient(to right, red, blue)"),
        "命名色围栏外（仅 #rrggbb）→ false"
    );
    assert!(s.background_gradient.is_none());
}

#[test]
fn background_linear_gradient_one_stop_rejected() {
    // 仅 1 色 stop → 段数 < 3 → 拒收。
    let mut s = ResolvedStyle::default();
    assert!(
        !apply_decl(&mut s, "background", "linear-gradient(to right, #ff0000)"),
        "1 色 stop 围栏外 → false"
    );
    assert!(s.background_gradient.is_none());
}

#[test]
fn background_other_shorthand_values_ignored() {
    // `background: red` 等 shorthand 值不在围栏内（纯色须写 background-color）→ false。
    let mut s = ResolvedStyle::default();
    assert!(!apply_decl(&mut s, "background", "red"));
    assert!(!apply_decl(&mut s, "background", "url(a.png)"));
    assert!(s.background_gradient.is_none());
    assert!(s.background_image.is_none());
    assert!(
        s.background_color.is_none(),
        "background shorthand 不影响 background_color"
    );
}
