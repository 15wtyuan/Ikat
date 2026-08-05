use crate::annotate::annotate;
use crate::consistency_check::check_consistency;
use crate::css_resolve::resolve_inline_styles_with_diags;
use crate::css_rules::{parse_style_block, KeyframesRule};
use crate::diagnostic::{Diagnostic, LineMap};
use crate::fence_gate::run_fence_gate;
use crate::inline_context_check::check_inline_context;
use crate::ir::{IrNodeKind, IrTree};
use crate::structural::run_structural;
use crate::tree_builder::parse_html_to_ir_named;
use loomgui_core::style::dynamic::DynamicRule;
use loomgui_core::style::mapping::parse_url;
use loomgui_core::style::resolved::ResolvedStyle;

/// Final output of the R1 parsing pipeline.
pub struct ParsedTemplate {
    pub tree: IrTree,
    pub styles: Vec<ResolvedStyle>,
    pub dynamic_rules: Vec<DynamicRule>,
    /// @keyframes 规则（对齐 public-api.md「动画全在 CSS」终态契约）。
    /// pkg v30 起 core 类型已就绪（crate::scene::animation）；packer bridge 负责将
    /// fence declarations 转成 AnimatableProps，并把规则写入 ComponentTemplate.keyframes。
    /// player 运行时驱动留后续 M2 task。
    pub keyframes: Vec<KeyframesRule>,
    pub diagnostics: Vec<Diagnostic>,
    pub referenced_sprites: Vec<String>,
}

/// Full six-stage pipeline: Tokenize, Tree Build, Fence Gate, CSS Resolve,
/// Structural, Annotate.
///
/// Collects ALL diagnostics (does not fail-fast).
pub fn parse_template(html: &str, file: &str) -> ParsedTemplate {
    let line_map = LineMap::new(html);

    // Stage 1+2: Tokenize + Tree Build
    let (mut tree, mut diagnostics, style_texts) = parse_html_to_ir_named(html, file.to_string());

    // Stage 3: Fence Gate (per-element validation)
    let gate_diags = run_fence_gate(&tree, file, &line_map);
    diagnostics.extend(gate_diags);

    // Stage 4: CSS Resolve
    let (styles, css_diags) = resolve_inline_styles_with_diags(&tree, file, &line_map);
    diagnostics.extend(css_diags);

    // Stage 4.5: <style> → 动态规则表（CSS cascade 规则，运行时 rematch 消费）。
    // style_texts 由 tree_builder 在 Stage 1 抽出（<style> 元素文本），此处统一解析。
    let mut dynamic_rules = Vec::new();
    let mut keyframes = Vec::new();
    for css in &style_texts {
        let (rules, kf, css_diags) = parse_style_block(css);
        dynamic_rules.extend(rules);
        keyframes.extend(kf);
        diagnostics.extend(css_diags);
    }

    // Stage 5: Structural (content model, IDs)
    let struct_diags = run_structural(&tree, file, &line_map);
    diagnostics.extend(struct_diags);

    // Stage 6: Annotate (fill SemanticKind)
    annotate(&mut tree);

    // Stage 6.5: inline 元素布局上下文检查。LoomGUI 没有 flex 之外的 inline flow——
    // block 容器里的裸 inline 元素会被当 block-level（撑满+竖排），和浏览器不一致。
    // 必须在 Annotate 之后（需 TextBlock 语义判定豁免）+ Stage 4（inline style display）
    // + Stage 4.5（class 规则 display）之后——parent 是 block 还是 flex 要合并两个来源。
    diagnostics.extend(check_inline_context(
        &tree,
        &styles,
        &dynamic_rules,
        file,
        &line_map,
    ));

    // Stage 6.6: 围栏内属性一致性 warning。属性本身围栏合法，但漏写/默认值冲突致
    // HTML 预览（浏览器按 CSS initial 值）≠ 运行时（LoomGUI 默认值）——不阻断打包，
    // 只提醒作者补全声明。必须在 Stage 4（styles 已 cascade）之后。
    diagnostics.extend(check_consistency(&tree, &styles, file, &line_map));

    // Stage 6.7: 控件必须被 CSS 命中。LoomGUI 控件不带 UA 默认样式——写了控件标签却
    // 无匹配 CSS 规则 = 运行时空白（浏览器预览却看着正常，因为浏览器套自己的 UA 表）。
    // 必须在 Annotate 之后（需 IrElement.semantic）+ Stage 4.5 之后（需 dynamic_rules）。
    diagnostics.extend(crate::control_css_check::check_control_css(
        &tree,
        &dynamic_rules,
        file,
        &line_map,
    ));

    // Stage 6.8: role 驱动控件结构契约（必需子角色）。作者自写控件结构
    // （`<div role="combobox"><div role="listbox">...`），可能漏写必需子节点。
    // 打包期严格拦截，不依赖运行时 reparent 兜底。只校验 role 驱动节点（带 role 属性
    // 且在契约表中的控件）。必须在 Annotate 之后（需完整 IrTree）。
    diagnostics.extend(crate::control_structure_check::check_control_structure(
        &tree, file, &line_map,
    ));

    // Extract referenced sprites (img src, background-image url)
    let referenced_sprites = extract_sprites(&tree);

    ParsedTemplate {
        tree,
        styles,
        dynamic_rules,
        keyframes,
        diagnostics,
        referenced_sprites,
    }
}

fn extract_sprites(tree: &IrTree) -> Vec<String> {
    let mut sprites = Vec::new();
    for node in &tree.nodes {
        if let IrNodeKind::Element(el) = &node.kind {
            // img src
            if el.tag == "img" {
                if let Some(src) = el.attributes.iter().find(|a| a.name == "src") {
                    sprites.push(src.value.clone());
                }
            }
            // background-image: url(...) in inline style
            if let Some(style) = el.attributes.iter().find(|a| a.name == "style") {
                for decl in style.value.split(';') {
                    let decl = decl.trim();
                    if let Some(prop) = decl.split(':').next() {
                        if prop.trim() == "background-image" {
                            if let Some(value) = decl.split_once(':').map(|(_, v)| v.trim()) {
                                if let Some(url) = parse_url(value) {
                                    sprites.push(url);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    sprites
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::tag::SemanticKind;

    #[test]
    fn pipeline_simple_template() {
        let result = parse_template(
            r#"<div id="root"><div>Hello <span>x</span></div></div>"#,
            "home.html",
        );
        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );
        assert_eq!(result.tree.roots.len(), 1);

        let root = result.tree.roots[0];
        let el = result.tree.element(root).unwrap();
        assert_eq!(el.tag, "div");
        assert_eq!(el.semantic, Some(SemanticKind::Container));

        // root > div > span
        let mid_id = result.tree.nodes[root.0].children[0];
        let span_id = result.tree.nodes[mid_id.0]
            .children
            .iter()
            .copied()
            .find(|&c| result.tree.element(c).map(|e| e.tag.as_str()) == Some("span"))
            .expect("span under div");
        let span_el = result.tree.element(span_id).unwrap();
        assert_eq!(span_el.semantic, Some(SemanticKind::TextElement));
    }

    #[test]
    fn pipeline_collects_all_errors() {
        let result = parse_template(
            r#"<video></video><div bogus="x" style="z-index:5"></div>"#,
            "bad.html",
        );
        // Should have multiple errors, not just the first
        assert!(
            result.diagnostics.len() >= 2,
            "should collect all errors, got: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn pipeline_referenced_sprites() {
        let result = parse_template(r#"<img src="icons/home.png">"#, "view.html");
        assert!(result
            .referenced_sprites
            .contains(&"icons/home.png".to_string()));
    }
}
