use super::{Span, SyntaxError, SyntaxKind, Token};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NodeId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ElementId {
    Node(NodeId),
    Token(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NodeData {
    pub kind: SyntaxKind,
    pub range: Span,
    pub children: Vec<ElementId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse<'s> {
    pub(super) source: &'s str,
    pub(super) tokens: Vec<Token>,
    pub(super) nodes: Vec<NodeData>,
    pub(super) root: NodeId,
    pub(super) errors: Vec<SyntaxError>,
}

impl Parse<'_> {
    pub fn root(&self) -> Node<'_> {
        Node {
            tree: self,
            id: self.root,
        }
    }

    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }

    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn source(&self) -> &str {
        self.source
    }

    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    pub fn debug_tree(&self) -> String {
        use fmt::Write;
        let mut output = String::new();
        let mut pending = vec![(Element::Node(self.root()), 0)];
        while let Some((element, depth)) = pending.pop() {
            let (kind, range) = (element.kind(), element.range());
            let _ = write!(
                output,
                "{:indent$}{kind:?}@{}..{}",
                "",
                range.start,
                range.end,
                indent = depth * 2
            );
            match element {
                Element::Node(node) => pending.extend(
                    node.children_with_tokens()
                        .rev()
                        .map(|child| (child, depth + 1)),
                ),
                Element::Token(token) => {
                    let _ = write!(output, " {:?}", &self.source[token.range]);
                }
            }
            output.push('\n');
        }
        output
    }
}

#[derive(Clone, Copy)]
pub struct Node<'a> {
    tree: &'a Parse<'a>,
    id: NodeId,
}

impl fmt::Debug for Node<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Node")
            .field("kind", &self.kind())
            .field("range", &self.range())
            .finish()
    }
}

impl PartialEq for Node<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.tree, other.tree) && self.id == other.id
    }
}

impl Eq for Node<'_> {}

impl<'a> Node<'a> {
    pub fn kind(self) -> SyntaxKind {
        self.tree.nodes[self.id.0].kind
    }

    pub fn range(self) -> Span {
        self.tree.nodes[self.id.0].range
    }

    pub fn text(self) -> &'a str {
        &self.tree.source[self.range()]
    }

    pub fn children_with_tokens(self) -> Children<'a> {
        Children {
            tree: self.tree,
            elements: self.tree.nodes[self.id.0].children.iter(),
        }
    }

    pub fn children(self) -> impl DoubleEndedIterator<Item = Node<'a>> + 'a {
        self.children_with_tokens()
            .filter_map(|element| match element {
                Element::Node(node) => Some(node),
                Element::Token(_) => None,
            })
    }

    pub fn descendants(self) -> Descendants<'a> {
        Descendants {
            pending: vec![self],
        }
    }

    pub fn tokens(self) -> Tokens<'a> {
        Tokens {
            pending: vec![self.children_with_tokens()],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Element<'a> {
    Node(Node<'a>),
    Token(&'a Token),
}

impl Element<'_> {
    pub fn kind(self) -> SyntaxKind {
        match self {
            Self::Node(node) => node.kind(),
            Self::Token(token) => token.kind,
        }
    }

    pub fn range(self) -> Span {
        match self {
            Self::Node(node) => node.range(),
            Self::Token(token) => token.range,
        }
    }
}

pub struct Children<'a> {
    tree: &'a Parse<'a>,
    elements: std::slice::Iter<'a, ElementId>,
}

impl<'a> Children<'a> {
    fn resolve(&self, element: ElementId) -> Element<'a> {
        match element {
            ElementId::Node(id) => Element::Node(Node {
                tree: self.tree,
                id,
            }),
            ElementId::Token(index) => Element::Token(&self.tree.tokens[index]),
        }
    }
}

impl<'a> Iterator for Children<'a> {
    type Item = Element<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let id = *self.elements.next()?;
        Some(self.resolve(id))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.elements.size_hint()
    }
}

impl DoubleEndedIterator for Children<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let id = *self.elements.next_back()?;
        Some(self.resolve(id))
    }
}

impl ExactSizeIterator for Children<'_> {}
impl std::iter::FusedIterator for Children<'_> {}

pub struct Descendants<'a> {
    pending: Vec<Node<'a>>,
}

impl<'a> Iterator for Descendants<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.pending.pop()?;
        self.pending.extend(node.children().rev());
        Some(node)
    }
}

impl std::iter::FusedIterator for Descendants<'_> {}

pub struct Tokens<'a> {
    pending: Vec<Children<'a>>,
}

impl<'a> Iterator for Tokens<'a> {
    type Item = &'a Token;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.pending.last_mut()?.next() {
                Some(Element::Token(token)) => return Some(token),
                Some(Element::Node(node)) => self.pending.push(node.children_with_tokens()),
                None => {
                    self.pending.pop();
                }
            }
        }
    }
}

impl std::iter::FusedIterator for Tokens<'_> {}
