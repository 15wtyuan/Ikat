use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::ir::{IrNodeKind, IrTree};
use crate::schema::css::{find_css_prop, find_shorthand, validate_animation_value, CssValueParser};
use crate::schema::tag::{find_tag, DisplayDefault, SemanticKind};
use loomgui_core::style::mapping::apply_decl;
use loomgui_core::style::resolved::{DisplayMode, ResolvedStyle, TextAlign};

/// Resolve inline styles for all nodes in the tree.
///
/// Returns one `ResolvedStyle` per node, in node-index order.
/// Uses the existing `apply_decl` for value application, but validates
/// property names and keyword values against the CSS schema first.
pub fn resolve_inline_styles(tree: &IrTree) -> Vec<ResolvedStyle> {
    resolve_inline_styles_with_diags(tree, "<inline>", &LineMap::new("")).0
}

/// Resolve inline styles, also returning diagnostics for invalid CSS.
pub fn resolve_inline_styles_with_diags(
    tree: &IrTree,
    file: &str,
    line_map: &LineMap,
) -> (Vec<ResolvedStyle>, Vec<Diagnostic>) {
    let mut styles: Vec<ResolvedStyle> = (0..tree.nodes.len())
        .map(|_| ResolvedStyle::default())
        .collect();
    let mut diagnostics = Vec::new();

    for (idx, node) in tree.nodes.iter().enumerate() {
        let IrNodeKind::Element(el) = &node.kind else {
            continue;
        };

        let mut flex_direction_set = false;

        // Apply DisplayDefault from schema (overrides ResolvedStyle::default
        // which hardcodes Flex + Column for legacy reasons).
        if let Some(spec) = find_tag(&el.tag) {
            match spec.display {
                DisplayDefault::Block => {
                    styles[idx].display_mode = DisplayMode::Block;
                    // Schema-level default for block tags (div/header/nav/p/...).
                    // Must set taffy Display::Block here too — otherwise the
                    // taffy_style.display field keeps its Flex default from
                    // ResolvedStyle::default() and explicit display:block in
                    // mapping.rs can't rescue plain <div> without inline style.
                    // Explicit display:flex/none in inline style still wins: this
                    // runs first, apply_decl overwrites later.
                    styles[idx].taffy_style.display = taffy::Display::Block;
                }
                DisplayDefault::Inline => {
                    styles[idx].display_mode = DisplayMode::Flex;
                    // inline -> flex for taffy compatibility; flex-direction
                    // stays Row (taffy default) per CSS standard.
                }
                DisplayDefault::None => {
                    styles[idx].display_mode = DisplayMode::None;
                }
            }
            // UA 样式表等价：button 默认 text-align: center（浏览器 UA 行为）。
            // LoomGUI 无 UA 样式表概念——直接在 tag default 处硬编码。运行时
            // propagate_inherited 会把此值继承给 text 子节点（"Buy" 等居中）。
            // 同时 set INH_TEXT_ALIGN bit，把 UA 默认视为"显式声明"——防
            // propagate_inherited 用父（卡片/列表项）的 text-align 覆盖 button。
            // 用户显式 text-align 声明仍走 inline apply_decl 分支覆盖（CSS 级联）。
            if spec.semantic == SemanticKind::Button {
                styles[idx].text_align = TextAlign::Center;
                if let Some(bit) = loomgui_core::style::dynamic::inherited_bit("text-align") {
                    styles[idx].inherited_set.0 |= bit;
                }
                // UA 容器居中：button 默认 justify-content + align-items = center（CSS 浏览器 UA
                // 行为：button content 居中）。Bug B（commit 3916a1c）只修 text-align center（
                // text *内部* 居中），但 button 作为 flex 容器在缺省 justify/align=flex-start/stretch
                // 时，text 子作为 flex item 仍从 padding-left 起——core dump 实证 text.x=266 而非
                // 居中 268.5。justify-content/align-items 非继承属性 → 无 INH bit，仅本节点生效，
                // 运行时 rematch 从 base_style 重起，UA 默认每帧稳定。
                styles[idx].taffy_style.justify_content = Some(taffy::JustifyContent::CENTER);
                styles[idx].taffy_style.align_items = Some(taffy::AlignItems::CENTER);
            }
        }

        // Apply inline style declarations
        if let Some(style_attr) = el.attributes.iter().find(|a| a.name == "style") {
            for decl in style_attr.value.split(';') {
                let decl = decl.trim();
                if decl.is_empty() {
                    continue;
                }
                let (prop, value) = match decl.split_once(':') {
                    Some((p, v)) => (p.trim(), v.trim()),
                    None => continue,
                };

                // Validate property name
                let is_known = find_css_prop(prop).is_some() || find_shorthand(prop).is_some();
                if !is_known {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::FenceUnknownCssProp,
                        format!("CSS property \"{}\" is not in the fence", prop),
                        line_map.source_location(node.span.start, file.to_string()),
                    ));
                    continue;
                }

                // Validate keyword values against schema
                if let Some(spec) = find_css_prop(prop) {
                    match &spec.parser {
                        CssValueParser::Keyword(allowed) => {
                            if !allowed.contains(&value) {
                                diagnostics.push(Diagnostic::error(
                                    DiagnosticCode::FenceBadCssValue,
                                    format!(
                                        "value \"{}\" is not valid for CSS property \"{}\"",
                                        value, prop
                                    ),
                                    line_map.source_location(node.span.start, file.to_string()),
                                ));
                                continue;
                            }
                        }
                        CssValueParser::Animation => {
                            // animation 简写语法校验（捕捉拼写错误）。runtime 驱动留 §4 视觉束，
                            // 本轮不存值——apply_decl 不识别 "animation"，跳过避免误报 FenceBadCssValue。
                            if !validate_animation_value(value) {
                                diagnostics.push(Diagnostic::error(
                                    DiagnosticCode::FenceBadCssValue,
                                    format!(
                                        "value \"{}\" is not valid for CSS property \"{}\"",
                                        value, prop
                                    ),
                                    line_map.source_location(node.span.start, file.to_string()),
                                ));
                            }
                            continue; // 校验过即可，不调 apply_decl（runtime 不存 animation）
                        }
                        _ => {}
                    }
                }

                // Track explicit flex-direction
                if prop == "flex-direction" {
                    flex_direction_set = true;
                }

                // Apply using existing apply_decl.
                // If it returns false, the value failed to parse -- report it.
                if !apply_decl(&mut styles[idx], prop, value) {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::FenceBadCssValue,
                        format!(
                            "value \"{}\" is not valid for CSS property \"{}\"",
                            value, prop
                        ),
                        line_map.source_location(node.span.start, file.to_string()),
                    ));
                } else if let Some(bit) = loomgui_core::style::dynamic::inherited_bit(prop) {
                    // inline 可继承声明 bake 进 inherited_set，避免运行时
                    // propagate_inherited 用父值覆盖子的 inline 声明。
                    styles[idx].inherited_set.0 |= bit;
                }
            }
        }

        // CSS spec: flex-direction initial value is row.
        // ResolvedStyle::default() hardcodes Column (legacy).
        // If display ended up as Flex and no explicit flex-direction was
        // applied, override to Row per CSS standard.
        if styles[idx].display_mode == DisplayMode::Flex && !flex_direction_set {
            styles[idx].taffy_style.flex_direction = taffy::FlexDirection::Row;
        }
    }

    (styles, diagnostics)
}

/// Private helper for tests: resolve without file/line_map (uses empty).
#[cfg(test)]
fn resolve_for_test(tree: &IrTree) -> Vec<ResolvedStyle> {
    resolve_inline_styles_with_diags(tree, "<inline>", &LineMap::new("")).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_builder::parse_html_to_ir;

    #[test]
    fn inline_style_applies_color() {
        let (tree, _) = parse_html_to_ir(r#"<div style="color:#ff0000"></div>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_eq!(styles[id.0].color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn display_block_overrides_default() {
        let (tree, _) = parse_html_to_ir(r#"<div style="display:block"></div>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_eq!(styles[id.0].display_mode, DisplayMode::Block);
    }

    #[test]
    fn display_grid_reports_error() {
        let (tree, _) = parse_html_to_ir(r#"<div style="display:grid"></div>"#);
        let (_, diags) = resolve_inline_styles_with_diags(&tree, "test.html", &LineMap::new(""));
        assert!(diags
            .iter()
            .any(|d| d.code == DiagnosticCode::FenceBadCssValue));
    }

    #[test]
    fn flex_defaults_to_row_direction() {
        let (tree, _) = parse_html_to_ir(r#"<div style="display:flex"></div>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_eq!(
            styles[id.0].taffy_style.flex_direction,
            taffy::FlexDirection::Row
        );
    }

    #[test]
    fn explicit_flex_direction_preserved() {
        let (tree, _) =
            parse_html_to_ir(r#"<div style="display:flex; flex-direction:column"></div>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_eq!(
            styles[id.0].taffy_style.flex_direction,
            taffy::FlexDirection::Column
        );
    }

    #[test]
    fn inline_inherited_sets_bit() {
        let (tree, _) = parse_html_to_ir(r#"<span style="color:blue"></span>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        let color_bit = loomgui_core::style::dynamic::inherited_bit("color").unwrap();
        assert!(
            styles[id.0].inherited_set.0 & color_bit != 0,
            "inline color must set inherited_set COLOR bit"
        );
    }

    #[test]
    fn inline_non_inherited_sets_no_bit() {
        let (tree, _) = parse_html_to_ir(r#"<div style="width:100px"></div>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_eq!(
            styles[id.0].inherited_set.0, 0,
            "non-inherited width must not set any inherited bit"
        );
    }

    #[test]
    fn inline_font_size_sets_bit() {
        let (tree, _) = parse_html_to_ir(r#"<div style="font-size:20px"></div>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        let fs_bit = loomgui_core::style::dynamic::inherited_bit("font-size").unwrap();
        assert_eq!(
            styles[id.0].inherited_set.0 & fs_bit,
            fs_bit,
            "inline font-size must set inherited_set FONT_SIZE bit"
        );
    }

    /// 浏览器 UA 样式表：button 默认 text-align: center（继承到 text 子节点）。
    /// LoomGUI 无 UA 样式表概念——按 tag semantic 直接设默认。
    /// 修前根因：button 元素 text-align=Left（无 UA 表，回落 ResolvedStyle::default Left）
    /// → text 子节点继承 Left → "Buy" 字不居中。
    #[test]
    fn button_default_text_align_is_center() {
        let (tree, _) = parse_html_to_ir(r#"<button>Buy</button>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_eq!(
            styles[id.0].text_align,
            loomgui_core::style::resolved::TextAlign::Center,
            "button UA 默认 text-align: center"
        );
        // UA 默认视为"显式声明"，set INH_TEXT_ALIGN bit——防 propagate_inherited
        // 把父（卡片/列表项等）的 text-align 覆盖到 button。
        let ta_bit = loomgui_core::style::dynamic::inherited_bit("text-align").unwrap();
        assert_eq!(
            styles[id.0].inherited_set.0 & ta_bit,
            ta_bit,
            "button UA text-align 必须置 INH_TEXT_ALIGN bit 防 propagate 覆盖"
        );
    }

    /// 用户显式声明 text-align 覆盖 button UA default（CSS 级联优先级）。
    #[test]
    fn explicit_text_align_overrides_button_default() {
        let (tree, _) = parse_html_to_ir(r#"<button style="text-align:left">Buy</button>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_eq!(
            styles[id.0].text_align,
            loomgui_core::style::resolved::TextAlign::Left,
            "显式 text-align:left 覆盖 button UA center"
        );
    }

    /// 非 button 元素 text-align 保持 default Left（不应被误改）。
    #[test]
    fn non_button_keeps_default_text_align() {
        let (tree, _) = parse_html_to_ir(r#"<div>hi</div>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_eq!(
            styles[id.0].text_align,
            loomgui_core::style::resolved::TextAlign::Left,
            "div UA 无 text-align 默认（保持 Left）"
        );
    }

    /// Bug 续修：button UA 容器居中（justify-content + align-items = center）。
    /// Bug B（commit 3916a1c）只修 text-align center（text 内部居中），未修容器居中，
    /// text 子作为 flex item 从 padding-left 起——core dump 实证 text.x=266 应 268.5。
    /// 非继承属性 → 无 INH bit，仅本节点生效，但每帧 rematch 从 base_style 重起，稳定。
    #[test]
    fn button_default_flex_centering() {
        let (tree, _) = parse_html_to_ir(r#"<button>Buy</button>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_eq!(
            styles[id.0].taffy_style.justify_content,
            Some(taffy::JustifyContent::CENTER),
            "button UA justify-content: center"
        );
        assert_eq!(
            styles[id.0].taffy_style.align_items,
            Some(taffy::AlignItems::CENTER),
            "button UA align-items: center"
        );
    }

    /// 用户显式 justify/align 覆盖 button UA center（CSS 级联优先级）。
    #[test]
    fn explicit_justify_align_overrides_button_default() {
        let (tree, _) = parse_html_to_ir(
            r#"<button style="justify-content:flex-start; align-items:flex-end">x</button>"#,
        );
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_eq!(
            styles[id.0].taffy_style.justify_content,
            Some(taffy::JustifyContent::FLEX_START),
            "显式 justify-content 覆盖 button UA center"
        );
        assert_eq!(
            styles[id.0].taffy_style.align_items,
            Some(taffy::AlignItems::FLEX_END),
            "显式 align-items 覆盖 button UA center"
        );
    }

    /// 非 button 元素不沾 button UA center（防误改）。
    #[test]
    fn non_button_keeps_default_justify_align() {
        let (tree, _) = parse_html_to_ir(r#"<div>hi</div>"#);
        let styles = resolve_for_test(&tree);
        let id = tree.roots[0];
        assert_ne!(
            styles[id.0].taffy_style.justify_content,
            Some(taffy::JustifyContent::CENTER),
            "div 不沾 button UA center"
        );
    }

    /// animation 行内声明：合法值不报诊断（语法校验通过，apply_decl 不存）。
    /// runtime §4 没实现 → fence 接受语法 + 静默不跑动画。
    #[test]
    fn inline_animation_valid_no_diagnostic() {
        let (tree, _) = parse_html_to_ir(r#"<div style="animation:fadeIn .4s both"></div>"#);
        let (_styles, diags) =
            resolve_inline_styles_with_diags(&tree, "test.html", &LineMap::new(""));
        assert!(diags.is_empty(), "合法 animation 值不应报诊断: {diags:?}");
    }

    /// animation 行内声明：非法值报诊断（语法错误非静默）。
    #[test]
    fn inline_animation_invalid_reports_diagnostic() {
        let (tree, _) = parse_html_to_ir(r#"<div style="animation:bogusKeyword"></div>"#);
        let (_styles, diags) =
            resolve_inline_styles_with_diags(&tree, "test.html", &LineMap::new(""));
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::FenceBadCssValue
                    && d.message.contains("animation")),
            "非法 animation 值应报 FenceBadCssValue: {diags:?}"
        );
    }
}
