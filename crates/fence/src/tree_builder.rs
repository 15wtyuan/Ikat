use crate::diagnostic::{Diagnostic, DiagnosticCode, LineMap, SourceLocation};
use crate::ir::{IrAttribute, IrElement, IrNodeId, IrTree, Span};
use crate::schema::tag::{find_tag, is_shell_tag};
use html5gum::emitters::callback::{Callback, CallbackEmitter, CallbackEvent};
use html5gum::{Span as GumSpan, Tokenizer};

/// Token produced by our html5gum callback -- consumed by the tree builder.
pub enum IrToken {
    StartTag {
        name: String,
        attributes: Vec<IrAttribute>,
        self_closing: bool,
        span: Span,
    },
    EndTag {
        name: String,
        span: Span,
    },
    String {
        text: String,
        span: Span,
    },
    Comment {
        text: String,
        span: Span,
    },
    Error {
        message: String,
        span: Span,
    },
}

// == html5gum callback ==

struct PendingTag {
    name: String,
    attributes: Vec<IrAttribute>,
    start_span: Span,
}

#[derive(Default)]
struct IrCallback {
    pending_tag: Option<PendingTag>,
    current_attr: Option<(String, String, usize)>,
}

impl Callback<IrToken, usize> for IrCallback {
    fn handle_event(&mut self, event: CallbackEvent<'_>, span: GumSpan<usize>) -> Option<IrToken> {
        let s = Span {
            start: span.start,
            end: span.end,
        };
        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.pending_tag = Some(PendingTag {
                    name: String::from_utf8_lossy(name).into_owned(),
                    attributes: Vec::new(),
                    start_span: s,
                });
                None
            }
            CallbackEvent::AttributeName { name } => {
                // Flush the previous attribute into the pending tag
                if let Some((pname, pval, pstart)) = self.current_attr.take() {
                    if let Some(tag) = &mut self.pending_tag {
                        tag.attributes.push(IrAttribute {
                            name: pname,
                            value: pval,
                            span: Span {
                                start: pstart,
                                end: span.start,
                            },
                        });
                    }
                }
                self.current_attr = Some((
                    String::from_utf8_lossy(name).into_owned(),
                    String::new(),
                    span.start,
                ));
                None
            }
            CallbackEvent::AttributeValue { value } => {
                if let Some((_, val, _)) = &mut self.current_attr {
                    *val = String::from_utf8_lossy(value).into_owned();
                }
                None
            }
            CallbackEvent::CloseStartTag { self_closing } => {
                if let Some((name, value, name_start)) = self.current_attr.take() {
                    if let Some(tag) = &mut self.pending_tag {
                        tag.attributes.push(IrAttribute {
                            name,
                            value,
                            span: Span {
                                start: name_start,
                                end: span.end,
                            },
                        });
                    }
                }
                let mut tag = self.pending_tag.take()?;
                tag.name.make_ascii_lowercase();
                Some(IrToken::StartTag {
                    name: tag.name,
                    attributes: tag.attributes,
                    self_closing,
                    span: Span {
                        start: tag.start_span.start,
                        end: span.end,
                    },
                })
            }
            CallbackEvent::EndTag { name } => {
                let mut name = String::from_utf8_lossy(name).into_owned();
                name.make_ascii_lowercase();
                Some(IrToken::EndTag { name, span: s })
            }
            CallbackEvent::String { value } => Some(IrToken::String {
                text: String::from_utf8_lossy(value).into_owned(),
                span: s,
            }),
            CallbackEvent::Comment { value } => Some(IrToken::Comment {
                text: String::from_utf8_lossy(value).into_owned(),
                span: s,
            }),
            CallbackEvent::Doctype { .. } => None,
            CallbackEvent::Error(error) => Some(IrToken::Error {
                message: error.as_str().to_string(),
                span: s,
            }),
        }
    }
}

/// Tokenize HTML using html5gum with our custom callback.
pub fn tokenize(html: &str) -> Vec<IrToken> {
    let mut emitter = CallbackEmitter::new(IrCallback::default());
    emitter.naively_switch_states(true);
    Tokenizer::new_with_emitter(html, emitter)
        .flatten()
        .collect()
}

// == Tree builder ==

struct TreeBuilder {
    tree: IrTree,
    stack: Vec<IrNodeId>,
    diagnostics: Vec<Diagnostic>,
    line_map: LineMap,
    file: String,
    in_body: bool,
    in_head: bool,
    in_style: bool,
    style_texts: Vec<String>,
}

impl TreeBuilder {
    fn new(html: &str, file: String) -> Self {
        Self {
            tree: IrTree::default(),
            stack: Vec::new(),
            diagnostics: Vec::new(),
            line_map: LineMap::new(html),
            file,
            in_body: false,
            in_head: false,
            in_style: false,
            style_texts: Vec::new(),
        }
    }

    fn loc(&self, offset: usize) -> SourceLocation {
        self.line_map.source_location(offset, self.file.clone())
    }

    fn current_parent(&self) -> Option<IrNodeId> {
        self.stack.last().copied()
    }

    fn process_token(&mut self, token: IrToken) {
        match token {
            IrToken::StartTag {
                name,
                attributes,
                self_closing,
                span,
            } => {
                self.handle_start_tag(name, attributes, self_closing, span);
            }
            IrToken::EndTag { name, span } => {
                self.handle_end_tag(name, span);
            }
            IrToken::String { text, span } => {
                if self.in_style {
                    self.style_texts.push(text);
                } else if !self.in_head && !text.is_empty() {
                    // 顶层空白（元素间换行/缩进，栈空）不进树——否则多行 HTML 会冒出
                    // 孤立 Text root，破坏 bridge 单根契约。元素内空白（parent=Some）
                    // 仍保留为 Text 子节点。
                    let is_top_level_ws = self.current_parent().is_none() && text.trim().is_empty();
                    if !is_top_level_ws {
                        self.tree.push_text(text, span, self.current_parent());
                    }
                }
            }
            IrToken::Comment { text: _, span: _ } => {}
            IrToken::Error { message, span } => {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::TokenizerError,
                    format!("HTML tokenizer error: {}", message),
                    self.loc(span.start),
                ));
            }
        }
    }

    fn handle_start_tag(
        &mut self,
        name: String,
        attributes: Vec<IrAttribute>,
        self_closing: bool,
        span: Span,
    ) {
        if name == "style" {
            // html5gum 对 <style> 切 RAWTEXT（naively_switch_states），self_closing 标志不阻止——
            // <style/> 的 /> 被忽略，后续仍当 style 内容吞到 </style>（HTML5：style 非 void）。
            // 故 in_style 无条件置 true，让被吞的 raw text 进 style_texts（CSS 源）而非可见文本。
            self.in_style = true;
            return;
        }
        if name == "body" {
            self.in_body = true;
            // If head was left open (no explicit </head>), entering body
            // implicitly closes it so body text isn't silently dropped.
            self.in_head = false;
            return;
        }
        if name == "html" || name == "head" {
            if name == "head" {
                self.in_head = true;
            }
            return;
        }
        if is_shell_tag(&name) && !self.in_body {
            return;
        }

        let element = IrElement {
            tag: name.clone(),
            attributes,
            semantic: None,
        };
        let parent = self.current_parent();
        let id = self.tree.push_element(element, span, parent);

        let is_void = find_tag(&name).map(|s| s.void).unwrap_or(false);
        if !is_void && !self_closing {
            self.stack.push(id);
        }
    }

    fn handle_end_tag(&mut self, name: String, span: Span) {
        if name == "style" {
            self.in_style = false;
            return;
        }
        if name == "body" {
            self.in_body = false;
            return;
        }
        if name == "html" || name == "head" {
            if name == "head" {
                self.in_head = false;
            }
            return;
        }
        if is_shell_tag(&name) && !self.in_body {
            return;
        }

        let pos = self.stack.iter().rev().position(|&id| {
            self.tree
                .element(id)
                .map(|e| e.tag == name)
                .unwrap_or(false)
        });

        match pos {
            Some(depth_from_top) => {
                let stack_len = self.stack.len();
                let match_idx = stack_len - 1 - depth_from_top;
                for &id in &self.stack[match_idx + 1..] {
                    let el = self.tree.element(id);
                    let tag_name = el.map(|e| e.tag.as_str()).unwrap_or("unknown");
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::UnclosedTag,
                        format!(
                            "<{}> was not explicitly closed before </{}>",
                            tag_name, name
                        ),
                        self.loc(self.tree.nodes[id.0].span.start),
                    ));
                }
                self.stack.truncate(match_idx);
            }
            None => {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::UnclosedTag,
                    format!("stray </{}> with no matching open tag", name),
                    self.loc(span.start),
                ));
            }
        }
    }

    fn finish(mut self) -> (IrTree, Vec<Diagnostic>, Vec<String>) {
        for &id in self.stack.iter().rev() {
            let el = self.tree.element(id);
            let tag_name = el.map(|e| e.tag.as_str()).unwrap_or("unknown");
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::UnclosedTag,
                format!("<{}> was not closed before end of input", tag_name),
                self.loc(self.tree.nodes[id.0].span.start),
            ));
        }
        (self.tree, self.diagnostics, self.style_texts)
    }
}

/// Parse HTML source into an IR tree with diagnostics.
/// This is Stage 1 (Tokenize) + Stage 2 (Tree Build).
pub fn parse_html_to_ir(html: &str) -> (IrTree, Vec<Diagnostic>) {
    let (tree, diags, _style_texts) = parse_html_to_ir_named(html, "<inline>".to_string());
    (tree, diags)
}

/// Same as parse_html_to_ir but with a file name for diagnostics.
pub fn parse_html_to_ir_named(html: &str, file: String) -> (IrTree, Vec<Diagnostic>, Vec<String>) {
    // 文件首的 UTF-8 BOM (`\u{feff}`) 由 html5gum 当作可见文本产 String token，且
    // Rust `char::is_whitespace` 自 Unicode 3.2 不再认 BOM 为空白——BOM 顶层 Text
    // 节点漏过 ws filter，被 bridge 当成额外顶层根，破坏单根契约。源首一次性剥除。
    let html_stripped = html.strip_prefix('\u{feff}').unwrap_or(html);
    let tokens = tokenize(html_stripped);
    let mut builder = TreeBuilder::new(html_stripped, file);
    for token in tokens {
        builder.process_token(token);
    }
    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::IrNodeKind;

    #[test]
    fn simple_div_with_text() {
        let (tree, diags) = parse_html_to_ir(r#"<div>hello</div>"#);
        assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
        assert_eq!(tree.roots.len(), 1);
        let div = tree.roots[0];
        assert_eq!(tree.element(div).unwrap().tag, "div");
        assert_eq!(tree.nodes[div.0].children.len(), 1);
        match &tree.nodes[tree.nodes[div.0].children[0].0].kind {
            IrNodeKind::Text(t) => assert_eq!(t, "hello"),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn nested_structure() {
        let (tree, diags) = parse_html_to_ir(r#"<div><span>x</span></div>"#);
        assert!(diags.is_empty());
        let div = tree.roots[0];
        let span_id = tree.nodes[div.0].children[0];
        assert_eq!(tree.element(span_id).unwrap().tag, "span");
    }

    #[test]
    fn void_element_no_close_tag() {
        let (tree, diags) = parse_html_to_ir(r#"<div><img src="a.png"></div>"#);
        assert!(diags.is_empty(), "img is void -- no closing tag needed");
        let div = tree.roots[0];
        assert_eq!(tree.nodes[div.0].children.len(), 1);
    }

    #[test]
    fn attributes_preserve_order() {
        let (tree, _) = parse_html_to_ir(r#"<div id="x" class="y" data-z="w"></div>"#);
        let el = tree.element(tree.roots[0]).unwrap();
        assert_eq!(el.attributes.len(), 3);
        assert_eq!(el.attributes[0].name, "id");
        assert_eq!(el.attributes[1].name, "class");
        assert_eq!(el.attributes[2].name, "data-z");
    }

    #[test]
    fn unclosed_tag_produces_diagnostic() {
        let (tree, diags) = parse_html_to_ir("<div><span>text</div>");
        assert!(
            !diags.is_empty(),
            "unclosed <span> should produce a diagnostic"
        );
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::UnclosedTag));
        let _ = tree;
    }

    #[test]
    fn body_wrapper_extracted() {
        let (tree, diags) =
            parse_html_to_ir(r#"<html><head></head><body><div>hi</div></body></html>"#);
        assert!(diags.is_empty());
        assert_eq!(tree.roots.len(), 1);
        assert_eq!(tree.element(tree.roots[0]).unwrap().tag, "div");
    }

    #[test]
    fn rich_text_inline_mix() {
        let (tree, diags) =
            parse_html_to_ir(r#"<div>Hello <span style="font-weight:700">world</span>!</div>"#);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        let p = tree.roots[0];
        assert_eq!(tree.nodes[p.0].children.len(), 3);
    }

    #[test]
    fn style_text_is_captured_not_dropped() {
        let html = r#"<html><head><style>.foo { color: red }</style></head><body><div>hi</div></body></html>"#;
        let (tree, diags, style_texts) = parse_html_to_ir_named(html, "x.html".into());
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        // <style> 元素本身不进树（shell 标签）
        assert!(
            !tree
                .nodes
                .iter()
                .any(|n| matches!(&n.kind, crate::ir::IrNodeKind::Element(e) if e.tag == "style")),
            "<style> 不应进 IrTree"
        );
        // 但文本留下来了
        assert_eq!(style_texts, vec![".foo { color: red }".to_string()]);
    }

    #[test]
    fn style_in_body_also_captured() {
        let (tree, _diags, style_texts) = parse_html_to_ir_named(
            r#"<div><style>.a { width: 10px }</style></div>"#,
            "x.html".into(),
        );
        let _ = tree;
        assert_eq!(style_texts, vec![".a { width: 10px }".to_string()]);
    }

    #[test]
    fn top_level_whitespace_not_orphan_root() {
        // 多行 HTML：元素间换行/缩进不能变成孤立 Text root（曾破坏 bridge 单根契约，
        // 拒掉所有多行生产 HTML）。元素内空白仍保留。
        let html = "<style>.x{width:50px}</style>\n<div class=\"root\">\n  <div>hi</div>\n</div>\n";
        let (tree, diags) = parse_html_to_ir(html);
        assert!(diags.is_empty(), "unexpected diags: {:?}", diags);
        assert_eq!(
            tree.roots.len(),
            1,
            "expected single root (top-level ws must not be roots), got roots: {:?}",
            tree.roots
        );
        assert_eq!(tree.element(tree.roots[0]).unwrap().tag, "div");
        // 元素内空白（div 和 p 之间的换行）仍保留为 Text 子节点——只丢顶层。
        let div = tree.roots[0];
        let has_ws_text_child = tree.nodes[div.0]
            .children
            .iter()
            .any(|&c| matches!(&tree.nodes[c.0].kind, IrNodeKind::Text(_)));
        assert!(
            has_ws_text_child,
            "in-element whitespace Text should be preserved (only top-level ws dropped)"
        );
    }
}
