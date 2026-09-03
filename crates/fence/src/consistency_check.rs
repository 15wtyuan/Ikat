//! Stage 6.6：围栏内属性一致性 warning。
//!
//! 围栏只拦「输入非法」（机制一：围栏外标签/属性/值 → error）。但还有一类问题：**属性本身
//! 围栏合法，但漏写或默认值冲突导致 HTML 预览 ≠ 游戏运行时**。浏览器按 CSS 规范的 initial
//! 值渲染，Ikat 的部分默认值与之不同 → 设计期预览（浏览器）和运行时（自绘）会不一致，
//! 作者却无法察觉。本 pass 在打包期检测这类组合并发 warning（不阻断打包）。
//!
//! 当前检测：
//! - **W1**：`border-width` 有但 `border-style` 缺省。CSS initial `border-style:none` = 不画
//!   边框，浏览器预览看不到边框；而 Ikat 历史实现会画 → 预览和运行时行为不一致。
//!   提醒作者显式声明 `border-style`（或用 `border` 简写一并声明）。
//! - **W2**：`background-image` 有但 `background-size` 缺省。CSS 默认 `auto`（原始尺寸），
//!   Ikat 默认 `stretch`（拉伸填满）→ 预览和运行时尺寸不同。提醒作者显式声明
//!   `background-size`。
//! - **E1**（error，#52）：`overflow:scroll/auto` 与 `clip-path` 同元素——shape 裁滚动
//!   视口无清晰语义，硬拒。
//! - **E2**（error，#52）：裁剪链深度 > 4（overflow 裁剪器 + clip-path 裁剪器沿祖先链
//!   总数）——后端 clip uniform 槽 4 组定长，authored 超深在此拒。

use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::ir::{IrNodeKind, IrTree};
use ikat_core::style::resolved::{BackgroundSize, BorderStyle, OverflowMode, ResolvedStyle};

/// 把 taffy `LengthPercentage` 解析为 px。
///
/// taffy 0.12：`LengthPercentage` 是 `pub struct(CompactLength)` tagged pointer，内字段私有
/// 无法 match 变体——用 `into_raw` + `tag` 解构。border-width 在 mapping 里只产 `Length`
/// （px-only 属性，见 mapping.rs），故实际只命中 `LENGTH_TAG`；`Percent` 分支当 0 处理
/// （与 `render::resolve_lp` 同口径，打包期无 content-box 上下文无法解析百分比）。
fn resolve_lp(lp: taffy::style::LengthPercentage) -> f32 {
    let cl = lp.into_raw();
    if cl.tag() == taffy::style::CompactLength::LENGTH_TAG {
        cl.value()
    } else {
        0.0
    }
}

/// 任一边 border-width > 0（px）。
fn has_border_width(s: &ResolvedStyle) -> bool {
    let bw = &s.taffy_style.border;
    resolve_lp(bw.top) > 0.0
        || resolve_lp(bw.right) > 0.0
        || resolve_lp(bw.bottom) > 0.0
        || resolve_lp(bw.left) > 0.0
}

/// background-image 有非空、非 `none` 的值。
fn has_background_image(s: &ResolvedStyle) -> bool {
    match &s.background_image {
        Some(img) => !img.is_empty() && img != "none",
        None => false,
    }
}

/// background-size 仍是默认值（Stretch）。
fn is_default_bg_size(s: &ResolvedStyle) -> bool {
    matches!(s.background_size, BackgroundSize::Stretch)
}

/// 检测围栏内属性一致性 warning（漏写/默认值冲突致预览 ≠ 运行时）。
///
/// 入参：
/// - `tree`：IrTree（取元素节点 span 用于定位）。
/// - `styles`：Stage 4 css_resolve 产物（按 node index 索引，与 `tree.nodes` 对齐）。
/// - `file` / `line_map`：定位诊断到源码行。
///
/// 返回 warning 列表（severity=Warning，不阻断打包）。
pub fn check_consistency(
    tree: &IrTree,
    styles: &[ResolvedStyle],
    file: &str,
    line_map: &LineMap,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for (idx, node) in tree.nodes.iter().enumerate() {
        // 只查元素节点（text/comment/doctype 无 style）。
        if !matches!(node.kind, IrNodeKind::Element(_)) {
            continue;
        }
        let Some(s) = styles.get(idx) else {
            continue;
        };

        if has_border_width(s) && s.border_style == BorderStyle::None {
            diags.push(Diagnostic::warning(
                DiagnosticCode::FenceBorderWithoutStyle,
                "border-width declared without border-style — the CSS initial value \
                 `border-style:none` means browsers render NO border, but Ikat \
                 draws one, so the browser preview will not match the runtime. \
                 Add `border-style:solid` (or use the `border:2px solid <color>` \
                 shorthand) so both render the border consistently."
                    .to_string(),
                line_map.source_location(node.span.start, file.to_string()),
            ));
        }

        if has_background_image(s) && is_default_bg_size(s) {
            diags.push(Diagnostic::warning(
                DiagnosticCode::FenceBgImageWithoutSize,
                "background-image declared without background-size — the CSS initial \
                 value is `auto` (natural image size), but Ikat defaults to \
                 `stretch` (fill the box), so the browser preview will not match the \
                 runtime. Add explicit `background-size` (cover/contain/stretch) so \
                 both render identically."
                    .to_string(),
                line_map.source_location(node.span.start, file.to_string()),
            ));
        }

        // E1：scroll 容器上声明 clip-path——shape 裁滚动视口无清晰语义（scroll 视口
        // 裁剪是矩形 + scroll_pos 平移）。web 上合法但我们不做：响亮拒优于静默忽略。
        // overflow:hidden + clip-path 合法（两条测试取交集，web 原义）。
        if s.clip_path.is_some()
            && (s.overflow_x == OverflowMode::Scroll || s.overflow_y == OverflowMode::Scroll)
        {
            diags.push(Diagnostic::error(
                DiagnosticCode::FenceClipPathScrollCombo,
                "clip-path on a scroll container (overflow:scroll/auto) is not supported — the scroll viewport clip is a rect translated by scroll position and has no shape equivalent. Use overflow:hidden if you only need clipping, or clip the content inside the scroller.".to_string(),
                line_map.source_location(node.span.start, file.to_string()),
            ));
        }

        // E2：裁剪链深度。clipper 判定与 core dfs_mask 同源（overflow 非 Visible
        // 或 clip-path 声明）；沿祖先链计数超上限即拒（后端 clip uniform 槽定长）。
        // 链从本元素自身起算（自身若是 clipper 也占一槽）。
        if s.clip_path.is_some()
            || s.overflow_x != OverflowMode::Visible
            || s.overflow_y != OverflowMode::Visible
        {
            let mut depth = 1usize;
            let mut cur = node.parent;
            while let Some(pid) = cur {
                let pnode = &tree.nodes[pid.0];
                let is_clipper = styles.get(pid.0).is_some_and(|ps| {
                    ps.clip_path.is_some()
                        || ps.overflow_x != OverflowMode::Visible
                        || ps.overflow_y != OverflowMode::Visible
                });
                if is_clipper {
                    depth += 1;
                }
                cur = pnode.parent;
            }
            if depth > MAX_CLIP_CHAIN {
                diags.push(Diagnostic::error(
                    DiagnosticCode::FenceClipChainTooDeep,
                    format!(
                        "clip chain too deep: {} nested clippers (overflow + clip-path) along the ancestor chain — the backend reserves clip slots for at most {} levels. Flatten a layer or remove one clip.",
                        depth, MAX_CLIP_CHAIN
                    ),
                    line_map.source_location(node.span.start, file.to_string()),
                ));
            }
        }
    }
    diags
}

/// 裁剪链深度上限——与 core `render::MAX_CLIP_CHAIN` 同值（后端 clip uniform 槽
/// 4 组定长）。双处常量由 fence↔core 测试对账（改一处须同步另一处）。
const MAX_CLIP_CHAIN: usize = 4;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Severity;
    use crate::pipeline::parse_template;

    fn has_diag(result: &crate::pipeline::ParsedTemplate, code: DiagnosticCode) -> bool {
        result.diagnostics.iter().any(|d| d.code == code)
    }

    fn has_warning(result: &crate::pipeline::ParsedTemplate, code: DiagnosticCode) -> bool {
        result
            .diagnostics
            .iter()
            .any(|d| d.code == code && d.severity == Severity::Warning)
    }

    /// border-width 单独声明（border-style 缺省）→ W1 warning。
    #[test]
    fn w1_border_width_without_style_warns() {
        let html = r#"<div style="border-width:2px;border-color:#ff0000"></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            has_warning(&r, DiagnosticCode::FenceBorderWithoutStyle),
            "border-width 无 border-style 应发 W1 warning: {:?}",
            r.diagnostics
        );
    }

    /// border 简写带 solid（border-style 已声明）→ 不发 W1。
    #[test]
    fn w1_border_with_style_no_warn() {
        let html = r#"<div style="border:2px solid #ff0000"></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            !has_diag(&r, DiagnosticCode::FenceBorderWithoutStyle),
            "border 简写带 style 不应发 W1: {:?}",
            r.diagnostics
        );
    }

    /// border-style longhand 显式声明 → 不发 W1。
    #[test]
    fn w1_border_style_longhand_no_warn() {
        let html = r#"<div style="border-width:2px;border-style:solid"></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            !has_diag(&r, DiagnosticCode::FenceBorderWithoutStyle),
            "border-style longhand 已声明不应发 W1: {:?}",
            r.diagnostics
        );
    }

    /// 无任何 border 声明 → 不发 W1（border-width 全 0）。
    #[test]
    fn w1_no_border_no_warn() {
        let html = r#"<div style="background-color:#fff"></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            !has_diag(&r, DiagnosticCode::FenceBorderWithoutStyle),
            "无 border 声明不应发 W1: {:?}",
            r.diagnostics
        );
    }

    /// background-image 单独声明（background-size 缺省）→ W2 warning。
    #[test]
    fn w2_bg_image_without_size_warns() {
        let html = r#"<div style="background-image:url(a.png)"></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            has_warning(&r, DiagnosticCode::FenceBgImageWithoutSize),
            "background-image 无 background-size 应发 W2 warning: {:?}",
            r.diagnostics
        );
    }

    /// background-image + background-size 显式 → 不发 W2。
    #[test]
    fn w2_bg_image_with_size_no_warn() {
        let html = r#"<div style="background-image:url(a.png);background-size:cover"></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            !has_diag(&r, DiagnosticCode::FenceBgImageWithoutSize),
            "background-image + size 不应发 W2: {:?}",
            r.diagnostics
        );
    }

    /// 无 background-image → 不发 W2。
    #[test]
    fn w2_no_bg_image_no_warn() {
        let html = r#"<div style="background-color:#fff"></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            !has_diag(&r, DiagnosticCode::FenceBgImageWithoutSize),
            "无 background-image 不应发 W2: {:?}",
            r.diagnostics
        );
    }

    /// 纯函数层：has_border_width / has_background_image / is_default_bg_size 行为。
    #[test]
    fn helpers_detect_declared_values() {
        let mut s = ResolvedStyle::default();
        assert!(!has_border_width(&s), "默认无 border-width");
        assert!(!has_background_image(&s), "默认无 background-image");
        assert!(is_default_bg_size(&s), "默认 background-size=Stretch");

        s.taffy_style.border = taffy::geometry::Rect::length(3.0_f32);
        assert!(has_border_width(&s), "四边 3px 应判为有 border-width");

        s.background_image = Some("x.png".into());
        assert!(has_background_image(&s), "有 url 应判为有 background-image");

        s.background_size = BackgroundSize::Cover;
        assert!(!is_default_bg_size(&s), "Cover 非默认");
    }

    /// E1（#52）：overflow:scroll/auto 与 clip-path 同元素 → 硬错。
    #[test]
    fn e1_clip_path_on_scroll_container_errors() {
        let html = r#"<div style="overflow:scroll;clip-path:circle(50%)"></div>"#;
        let r = parse_template(html, "t.html");
        assert!(
            has_diag(&r, DiagnosticCode::FenceClipPathScrollCombo),
            "scroll + clip-path 应发 E1 error: {:?}",
            r.diagnostics
        );
        // overflow:hidden + clip-path 合法（交集原义）——不发 E1。
        let ok = parse_template(
            r#"<div style="overflow:hidden;clip-path:circle(50%)"></div>"#,
            "t.html",
        );
        assert!(
            !has_diag(&ok, DiagnosticCode::FenceClipPathScrollCombo),
            "hidden + clip-path 合法"
        );
    }

    /// E2（#52）：裁剪链深度 > 4 → 硬错；== 4 合法。
    #[test]
    fn e2_clip_chain_depth_gate() {
        let deep = r#"
            <div style="overflow:hidden"><div style="overflow:hidden">
            <div style="overflow:hidden"><div style="overflow:hidden">
            <div style="clip-path:circle(50%)"></div></div></div></div></div>"#;
        let r = parse_template(deep, "t.html");
        assert!(
            has_diag(&r, DiagnosticCode::FenceClipChainTooDeep),
            "5 层裁剪链应发 E2 error: {:?}",
            r.diagnostics
        );
        let four = r#"
            <div style="overflow:hidden"><div style="overflow:hidden">
            <div style="clip-path:circle(50%)"><div style="overflow:hidden">
            </div></div></div></div>"#;
        let ok = parse_template(four, "t.html");
        assert!(
            !has_diag(&ok, DiagnosticCode::FenceClipChainTooDeep),
            "4 层合法"
        );
    }
}
