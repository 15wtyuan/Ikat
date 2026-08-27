use crate::annotate::annotate;
use crate::consistency_check::check_consistency;
use crate::css_resolve::resolve_inline_styles_with_diags;
use crate::css_rules::{parse_style_block_named, KeyframesRule};
use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap};
use crate::fence_gate::run_fence_gate;
use crate::inline_context_check::check_inline_context;
use crate::ir::{IrNodeKind, IrTree};
use crate::rich_text_classify::classify_rich_text;
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
    /// @keyframes 规则（「动画全在 CSS」契约）。
    /// pkg v30 起 core 类型已就绪（crate::scene::animation）；packer bridge 负责将
    /// fence declarations 转成 AnimatableProps，并把规则写入 ComponentTemplate.keyframes。
    /// player 运行时驱动留后续实现。
    pub keyframes: Vec<KeyframesRule>,
    /// Stage 6.4 产物：rich-text-block 根的 ir_idx 集合（block 容器 + 直接子全 inline 级）。
    /// packer bridge 据此烘 TemplateNode.rich_text_block flag，runtime 把这些 inline 子
    /// 拍平成 RichRun 走 inline flow。display:flex 容器不在此列
    /// （其子是 flex item，走 flex 排版）。Stage 6.5 读此集合豁免 img（仍报 button）。
    pub rich_text_blocks: Vec<usize>,
    pub diagnostics: Vec<Diagnostic>,
    pub referenced_sprites: Vec<String>,
}

/// Full six-stage pipeline: Tokenize, Tree Build, Fence Gate, CSS Resolve,
/// Structural, Annotate.
///
/// Collects ALL diagnostics (does not fail-fast).
pub fn parse_template(html: &str, file: &str) -> ParsedTemplate {
    parse_template_with_css(html, file, &|_| None)
}

/// [`parse_template`] 带外部样式表加载器：`<link rel="stylesheet" href>` 的 CSS
/// 由调用方读取（fence 不做 io）。loader 收 workspace 相对路径（href 已按所在
/// HTML 文件词法归一）、返回文件内容；返回 None = 加载失败（error 诊断定位
/// `<link>` 标签）。
pub fn parse_template_with_css(
    html: &str,
    file: &str,
    load_css: &dyn Fn(&str) -> Option<String>,
) -> ParsedTemplate {
    let line_map = LineMap::new(html);

    // Stage 1+2: Tokenize + Tree Build
    let raw = parse_html_to_ir_named(html, file.to_string());
    let mut tree = raw.tree;
    let mut diagnostics = raw.diagnostics;

    // Stage 3: Fence Gate (per-element validation)
    let gate_diags = run_fence_gate(&tree, file, &line_map);
    diagnostics.extend(gate_diags);

    // Stage 4: CSS Resolve
    let (styles, css_diags) = resolve_inline_styles_with_diags(&tree, file, &line_map);
    diagnostics.extend(css_diags);

    // Stage 4.5: <style> → 动态规则表（CSS cascade 规则，运行时 rematch 消费）。
    // style_texts 由 tree_builder 在 Stage 1 抽出（<style> 元素文本），此处统一解析；
    // <link rel="stylesheet"> 引入的外部 CSS 同管線（浏览器语义：href 相对 HTML 文件、
    // CSS 内 url() 相对 CSS 文件——改写成相对 HTML 的等价路径，与内联同基准）。
    let mut dynamic_rules = Vec::new();
    let mut keyframes = Vec::new();
    for css in &raw.style_texts {
        let (rules, kf, css_diags) = parse_style_block_named(css, file);
        dynamic_rules.extend(rules);
        keyframes.extend(kf);
        diagnostics.extend(css_diags);
    }
    for (href, span) in raw.stylesheet_links {
        let css_rel = lexical_join(parent_dir(file), &href);
        match load_css(&css_rel) {
            Some(css) => {
                let (mut rules, kf, css_diags) = parse_style_block_named(&css, &css_rel);
                diagnostics.extend(css_diags);
                rewrite_bg_urls_to_html_base(&mut rules, &css_rel, file);
                dynamic_rules.extend(rules);
                keyframes.extend(kf);
            }
            None => diagnostics.push(Diagnostic::error(
                DiagnosticCode::FenceStylesheetNotFound,
                format!(
                    "stylesheet `{href}` not found (resolved to `{css_rel}`, relative to {file})"
                ),
                line_map.source_location(span.start, file.to_string()),
            )),
        }
    }

    // Stage 5: Structural (content model, IDs)
    let struct_diags = run_structural(&tree, file, &line_map);
    diagnostics.extend(struct_diags);

    // Stage 6: Annotate (fill SemanticKind)
    annotate(&mut tree);

    // Stage 6.4: rich-text-block 分类 + mixed inline/block 报错。须在 6.5 之前：
    // (a) 需 Annotate 已填 semantic（判定 span=TextElement / img=Image）；
    // (b) 6.5 读本阶段产出的 rich_text_blocks 豁免 img。display 判定复用 6.5 的 helper
    // （inline style + tag 默认 + 单 compound class flex 规则；多 compound 保守）。
    let (rich_text_blocks, rich_diags) =
        classify_rich_text(&tree, &styles, &dynamic_rules, file, &line_map);
    diagnostics.extend(rich_diags);
    // #74 `<a>` 专项检查（href 必填 / rich 上下文 / 子内容白名单）。须在 6.4 之后
    //（消费 rich_text_blocks 产物判定合法上下文）。
    diagnostics.extend(crate::rich_text_classify::check_links(
        &tree,
        &rich_text_blocks,
        &dynamic_rules,
        file,
        &line_map,
    ));
    // W4：rich 子树内行内元素的死尺寸声明（width/height 恒无效）。
    crate::rich_text_classify::warn_inline_sizing(
        &tree,
        &rich_text_blocks,
        &dynamic_rules,
        file,
        &line_map,
        &mut diagnostics,
    );

    // Stage 6.5: inline 元素布局上下文检查。LoomGUI 没有 flex 之外的 inline flow——
    // block 容器里的裸 inline 元素会被当 block-level（撑满+竖排），和浏览器不一致。
    // 必须在 Annotate 之后（需 TextBlock 语义判定豁免）+ Stage 4（inline style display）
    // + Stage 4.5（class 规则 display）+ Stage 6.4（rich_text_blocks img 豁免）之后。
    diagnostics.extend(check_inline_context(
        &tree,
        &styles,
        &dynamic_rules,
        &rich_text_blocks,
        file,
        &line_map,
    ));

    // W5：页面侧只可能命中投影内容的类规则（样式墙下恒死代码）。
    crate::projected_check::warn_projected_only_rules(
        &tree,
        &dynamic_rules,
        file,
        &line_map,
        &mut diagnostics,
    );

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

    // Stage 6.7b: 控件结构 CSS 契约（锚点/脱流等结构声明）。命中校验只证明作者在
    // 样式控件；结构声明缺失（如 listbox 漏 position:absolute）在 PlayMode 才显形
    //（弹层撑开容器/定位飞出）。表驱动，与 6.7 同前置。
    diagnostics.extend(crate::control_css_check::check_control_structure_css(
        &tree,
        &dynamic_rules,
        file,
        &line_map,
    ));

    // Stage 6.7c: slider thumb 定位所有权（warning）。thumb 位移由控件按 value 全权
    // 驱动（core 每帧归零 inset/margin 再写 transform），作者定位声明静默不生效且
    // 叠加双偏移——浏览器预览居中、运行时偏移的典型分歧，打包期提示所有权。
    diagnostics.extend(crate::control_css_check::check_slider_thumb_positioning(
        &tree,
        &dynamic_rules,
        file,
        &line_map,
    ));

    // Stage 6.7d: layout transition 端点扫描（#10，error）。transition width/height 的
    // 静态可见端点（inline + 结构匹配 class 规则含伪类变体）必须同域显式——异域/auto
    // 端点运行时是离散跳变而浏览器平滑过渡，先验分歧打包期硬拒。运行时 add_class
    // 组合的动态端点由 core rematch 兜底（snap + EVT_TRANSITION_SNAP 警告事件）。
    diagnostics.extend(
        crate::layout_transition_check::check_layout_transition_endpoints(
            &tree,
            &styles,
            &dynamic_rules,
            file,
            &line_map,
        ),
    );

    // Stage 6.8: role 驱动控件结构契约（必需子角色）。作者自写控件结构
    // （`<div role="combobox"><div role="listbox">...`），可能漏写必需子节点。
    // 打包期严格拦截，不依赖运行时 reparent 兜底。只校验 role 驱动节点（带 role 属性
    // 且在契约表中的控件）。必须在 Annotate 之后（需完整 IrTree）。
    diagnostics.extend(crate::control_structure_check::check_control_structure(
        &tree, file, &line_map,
    ));

    // Stage 6.8b: tabpanel 手写 display:none 拦截。面板显隐所有权归 TabList 运行时
    //（激活 = unset inline display 回落作者样式），作者内联 display:none 烙进
    // base_style 后 unset 清不掉 → 激活面板永久隐身（静默坏），打包期点破。
    diagnostics.extend(
        crate::control_structure_check::check_tabpanel_author_hidden(&tree, file, &line_map),
    );

    // Extract referenced sprites (img src, background-image url)
    let referenced_sprites = extract_sprites(&tree);

    ParsedTemplate {
        tree,
        styles,
        dynamic_rules,
        keyframes,
        rich_text_blocks,
        diagnostics,
        referenced_sprites,
    }
}

/// 正斜杠相对路径的目录部分（"ui/game/main.css" → "ui/game"；顶层文件 → ""）。
fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// 词法拼接并归一（"ui/game" + "../../assets/x.png" → "assets/x.png"）。
/// 纯字符串栈归一，不触文件系统；上溯越过根的 `..` 被丢弃（与打包器 sprite
/// key 归一同语义，越界路径由下游资源校验拦截）。
fn lexical_join(dir: &str, rel: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for seg in dir.split('/') {
        if !seg.is_empty() && seg != "." {
            stack.push(seg);
        }
    }
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            s => stack.push(s),
        }
    }
    stack.join("/")
}

/// 把 workspace 相对路径改写成相对 `base_dir` 的等价路径
/// （"assets/x.png" 相对 "ui/game" → "../../assets/x.png"）。
fn lexical_relativize(target: &str, base_dir: &str) -> String {
    let base: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    let tgt: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    let mut common = 0;
    while common < base.len() && common < tgt.len() && base[common] == tgt[common] {
        common += 1;
    }
    let mut parts: Vec<&str> = vec![".."; base.len() - common];
    parts.extend(&tgt[common..]);
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

/// 外部 CSS 的 url() 相对 CSS 文件（浏览器语义）；而下游 sprite 归一以 HTML 文件
/// 为基准——这里改写成相对 HTML 的等价路径，让内联 / 外链规则同基准、下游零特判。
fn rewrite_bg_urls_to_html_base(rules: &mut [DynamicRule], css_rel: &str, html_rel: &str) {
    let css_dir = parent_dir(css_rel);
    let html_dir = parent_dir(html_rel);
    for r in rules.iter_mut() {
        for d in r.declarations.iter_mut() {
            if (d.prop == "background-image" || d.prop == "background")
                && !d.value.trim().starts_with("linear-gradient(")
            {
                if let Some(path) = parse_url(&d.value) {
                    let ws = lexical_join(css_dir, &path);
                    d.value = format!("url(\"{}\")", lexical_relativize(&ws, html_dir));
                }
            }
        }
    }
}

fn extract_sprites(tree: &IrTree) -> Vec<String> {
    let mut sprites = Vec::new();
    for node in &tree.nodes {
        if let IrNodeKind::Element(el) = &node.kind {
            if el.tag == "img" {
                if let Some(src) = el.attributes.iter().find(|a| a.name == "src") {
                    sprites.push(src.value.clone());
                }
            }
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
            r#"<video></video><div bogus="x" style="visibility:hidden"></div>"#,
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

    #[test]
    fn link_stylesheet_rules_and_keyframes_load() {
        let result = parse_template_with_css(
            r#"<head><link rel="stylesheet" href="theme.css"></head>
               <div class="card"><button>b</button></div>"#,
            "ui/game/main.html",
            &|path| {
                (path == "ui/game/theme.css").then(|| {
                    ".card { display: flex }\
                     @keyframes spin { from { opacity: 1 } to { opacity: 0.5 } }"
                        .to_string()
                })
            },
        );
        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );
        assert!(result
            .dynamic_rules
            .iter()
            .any(|r| r.selector.raw.contains("card")));
        assert!(result.keyframes.iter().any(|k| k.name == "spin"));
    }

    #[test]
    fn link_stylesheet_missing_file_errors() {
        let result = parse_template_with_css(
            r#"<head><link rel="stylesheet" href="nope.css"></head><div>x</div>"#,
            "ui/game/main.html",
            &|_| None,
        );
        let d = result
            .diagnostics
            .iter()
            .find(|d| d.code == crate::diagnostic::DiagnosticCode::FenceStylesheetNotFound)
            .expect("stylesheet-not-found error");
        assert!(
            d.location.file.contains("main.html"),
            "定位在 <link> 所在 HTML"
        );
        assert!(
            d.message.contains("ui/game/nope.css"),
            "报错带归一后路径: {}",
            d.message
        );
    }

    #[test]
    fn link_non_stylesheet_rel_is_never_loaded() {
        let result = parse_template_with_css(
            r#"<head><link rel="icon" href="favicon.ico"></head><div>x</div>"#,
            "m.html",
            &|p| panic!("rel=icon 不得触发加载，got {p}"),
        );
        assert!(
            result.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn link_css_urls_rewrite_relative_to_html() {
        // CSS 在 ui/game/style/theme.css，其中 url() 相对 CSS 文件
        // （../../assets/icon.png → workspace 的 assets/icon.png）→ 改写成相对
        // HTML（ui/game/）的等价路径 ../assets/icon.png，与内联 <style> 同基准。
        let result = parse_template_with_css(
            r#"<head><link rel="stylesheet" href="style/theme.css"></head><div class="bg"></div>"#,
            "ui/game/main.html",
            &|path| {
                (path == "ui/game/style/theme.css")
                    .then(|| ".bg { background-image: url(../../assets/icon.png) }".to_string())
            },
        );
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let rule = result
            .dynamic_rules
            .iter()
            .find(|r| r.selector.raw == ".bg")
            .unwrap();
        let bg = rule
            .declarations
            .iter()
            .find(|d| d.prop == "background-image")
            .unwrap();
        assert_eq!(bg.value, "url(\"../assets/icon.png\")");
    }

    #[test]
    fn lexical_helpers_roundtrip() {
        assert_eq!(parent_dir("ui/game/main.css"), "ui/game");
        assert_eq!(parent_dir("main.css"), "");
        assert_eq!(
            lexical_join("ui/game", "../../assets/x.png"),
            "assets/x.png"
        );
        assert_eq!(lexical_join("", "./a.css"), "a.css");
        assert_eq!(
            lexical_relativize("assets/x.png", "ui/game"),
            "../../assets/x.png"
        );
        assert_eq!(lexical_relativize("ui/game/x.png", "ui/game"), "x.png");
    }
}
