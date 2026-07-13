//! 跨语言契约校验：Unity shader `_GradientScale` 默认值必须 == core `SPREAD + 1`。
//!
//! `SPREAD`（`text/atlas.rs`）是 SDF 编码的核心几何参数；shader 里 `_GradientScale`
//! 用于 distance → 屏幕空间换算（对标 TMP `_GradientScale = atlasPadding + 1`）。两者
//! 分处 Rust 与 HLSL，靠 shader Properties 默认值 + 注释维系——改 `SPREAD` 不同步改
//! shader 默认值会 silent 破坏 SDF AA（过渡带宽度算错、字形过锐/锯齿，零报错难定位）。
//! 本测试守这道跨语言契约：改任一侧不改另一侧即测试红。

use std::path::Path;

/// 从 shader 源码解析 `_GradientScale("Gradient Scale", Float) = <N>` 的默认值 N。
fn parse_gradient_scale_default(shader_src: &str) -> Option<i64> {
    for line in shader_src.lines() {
        let l = line.trim();
        if l.starts_with("_GradientScale(") {
            // 形如：_GradientScale("Gradient Scale", Float) = 13     // = SPREAD(12)+1
            // 先截断行内注释（// 后）——注释里含 '=' 会干扰 rsplit
            let before_comment = l.split("//").next()?;
            let after_eq = before_comment.rsplit('=').next()?.trim();
            return after_eq.parse::<i64>().ok();
        }
    }
    None
}

#[test]
fn shader_gradient_scale_matches_spread_plus_one() {
    let shader_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("loomgui_unity_package")
        .join("Shaders")
        .join("LoomGUI-Unlit.shader");

    let src = std::fs::read_to_string(&shader_path)
        .unwrap_or_else(|e| panic!("无法读取 shader {:?}: {}", shader_path, e));

    let gradient_scale =
        parse_gradient_scale_default(&src).expect("shader 里找不到 _GradientScale Property 声明");

    let spread = loomgui_core::text::atlas::SPREAD as i64;
    assert_eq!(
        gradient_scale,
        spread + 1,
        "shader _GradientScale 默认值 ({}) != SPREAD ({}) + 1。\
         改 SPREAD 必须同步改 LoomGUI-Unlit.shader 的 _GradientScale 默认值，\
         否则 SDF AA 过渡带算错（字形过锐/锯齿，零报错难定位）。",
        gradient_scale,
        spread,
    );
}
