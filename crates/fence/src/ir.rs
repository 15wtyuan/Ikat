/// Byte offset range in source text (start inclusive, end exclusive).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IrNodeId(pub usize);

#[derive(Debug, Clone, Default)]
pub struct IrTree {
    pub nodes: Vec<IrNode>,
    pub roots: Vec<IrNodeId>,
}

#[derive(Debug, Clone)]
pub struct IrNode {
    pub kind: IrNodeKind,
    pub span: Span,
    pub parent: Option<IrNodeId>,
    pub children: Vec<IrNodeId>,
}

#[derive(Debug, Clone)]
pub enum IrNodeKind {
    Element(IrElement),
    Text(String),
    Comment(String),
    Doctype { force_quirks: bool },
}

#[derive(Debug, Clone)]
pub struct IrElement {
    pub tag: String,
    pub attributes: Vec<IrAttribute>,
    pub semantic: Option<crate::schema::tag::SemanticKind>,
}

#[derive(Debug, Clone)]
pub struct IrAttribute {
    pub name: String,
    pub value: String,
    pub span: Span,
}

impl IrTree {
    pub fn push_element(
        &mut self,
        element: IrElement,
        span: Span,
        parent: Option<IrNodeId>,
    ) -> IrNodeId {
        self.push_node(IrNode {
            kind: IrNodeKind::Element(element),
            span,
            parent,
            children: Vec::new(),
        })
    }

    pub fn push_text(&mut self, text: String, span: Span, parent: Option<IrNodeId>) -> IrNodeId {
        self.push_node(IrNode {
            kind: IrNodeKind::Text(text),
            span,
            parent,
            children: Vec::new(),
        })
    }

    fn push_node(&mut self, node: IrNode) -> IrNodeId {
        let id = IrNodeId(self.nodes.len());
        let parent = node.parent;
        self.nodes.push(node);
        if let Some(pid) = parent {
            self.nodes[pid.0].children.push(id);
        } else {
            self.roots.push(id);
        }
        id
    }

    pub fn element(&self, id: IrNodeId) -> Option<&IrElement> {
        match &self.nodes[id.0].kind {
            IrNodeKind::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn element_mut(&mut self, id: IrNodeId) -> Option<&mut IrElement> {
        match &mut self.nodes[id.0].kind {
            IrNodeKind::Element(e) => Some(e),
            _ => None,
        }
    }

    /// Iterate all element node IDs (depth-first, roots first).
    /// Used by Stages 3 (Fence Gate), 5 (Structural), and 6 (Annotate).
    pub fn all_element_ids(&self) -> Vec<IrNodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<IrNodeId> = self.roots.to_vec();
        while let Some(id) = stack.pop() {
            if matches!(self.nodes[id.0].kind, IrNodeKind::Element(_)) {
                out.push(id);
            }
            for child in self.nodes[id.0].children.iter().rev() {
                stack.push(*child);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_simple_tree() {
        let mut tree = IrTree::default();
        let div = tree.push_element(
            IrElement {
                tag: "div".into(),
                attributes: vec![IrAttribute {
                    name: "class".into(),
                    value: "panel".into(),
                    span: Span { start: 0, end: 20 },
                }],
                semantic: None,
            },
            Span { start: 0, end: 30 },
            None,
        );
        let txt = tree.push_text("hello".into(), Span { start: 5, end: 10 }, Some(div));
        assert_eq!(tree.nodes.len(), 2);
        assert_eq!(tree.roots, vec![div]);
        assert_eq!(tree.nodes[div.0].children, vec![txt]);
        assert_eq!(tree.nodes[txt.0].parent, Some(div));
    }

    #[test]
    fn text_is_first_class_child() {
        let mut tree = IrTree::default();
        let p = tree.push_element(
            IrElement {
                tag: "p".into(),
                attributes: vec![],
                semantic: None,
            },
            Span::default(),
            None,
        );
        let _t1 = tree.push_text("Hello ".into(), Span::default(), Some(p));
        let strong = tree.push_element(
            IrElement {
                tag: "strong".into(),
                attributes: vec![],
                semantic: None,
            },
            Span::default(),
            Some(p),
        );
        let _t2 = tree.push_text("world".into(), Span::default(), Some(strong));
        let _t3 = tree.push_text("!".into(), Span::default(), Some(p));
        assert_eq!(tree.nodes[p.0].children.len(), 3);
        assert_eq!(tree.nodes[strong.0].children.len(), 1);
    }
}
