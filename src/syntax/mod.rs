mod kind;
mod layout;
mod lexer;
mod parser;
mod tree;

pub mod ast;

pub use kind::SyntaxKind;
pub use lexer::tokenize;
pub use parser::parse;
pub use tree::{Children, Descendants, Element, Node, Parse, Tokens};

use std::{
    fmt,
    ops::{Index, Range},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub const fn empty(at: usize) -> Self {
        Self::new(at, at)
    }

    pub const fn start(self) -> usize {
        self.start
    }

    pub const fn end(self) -> usize {
        self.end
    }

    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub const fn range(self) -> Range<usize> {
        self.start..self.end
    }
}

impl Index<Span> for str {
    type Output = str;

    fn index(&self, span: Span) -> &str {
        &self[span.range()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: SyntaxKind,
    pub range: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub range: Span,
    pub message: String,
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.range.start)
    }
}

impl std::error::Error for SyntaxError {}
