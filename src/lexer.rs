use std::{fmt, ops::Range};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Space,
    Newline,
    Comment,
    String,
    Word,
    Symbol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub offset: usize,
    pub message: &'static str,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for LexError {}

pub fn tokenize(source: &str) -> Result<Vec<Token>, LexError> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let start = cursor;
        let current = bytes[cursor];
        let kind = match current {
            b' ' | b'\t' => {
                cursor += 1;
                while cursor < bytes.len() && matches!(bytes[cursor], b' ' | b'\t') {
                    cursor += 1;
                }
                Kind::Space
            }
            b'\r' | b'\n' => {
                cursor += 1;
                if current == b'\r' && bytes.get(cursor) == Some(&b'\n') {
                    cursor += 1;
                }
                Kind::Newline
            }
            b'#' => {
                while cursor < bytes.len() && !matches!(bytes[cursor], b'\r' | b'\n') {
                    cursor += 1;
                }
                Kind::Comment
            }
            b'\'' | b'"' => {
                let triple = bytes.get(cursor..cursor + 3) == Some(&[current; 3]);
                let width = if triple { 3 } else { 1 };
                cursor += width;
                loop {
                    if cursor >= bytes.len() {
                        return Err(LexError {
                            offset: start,
                            message: "unterminated string",
                        });
                    }
                    if bytes[cursor] == b'\\' {
                        cursor += 1;
                        if cursor < bytes.len() {
                            cursor += source[cursor..].chars().next().unwrap().len_utf8();
                        }
                    } else if bytes.get(cursor..cursor + width) == Some(&[current; 3][..width]) {
                        cursor += width;
                        break;
                    } else {
                        cursor += source[cursor..].chars().next().unwrap().len_utf8();
                    }
                }
                Kind::String
            }
            _ => {
                let ch = source[cursor..].chars().next().unwrap();
                cursor += ch.len_utf8();
                if ch.is_alphanumeric() || ch == '_' {
                    while cursor < bytes.len() {
                        let next = source[cursor..].chars().next().unwrap();
                        if !next.is_alphanumeric() && next != '_' {
                            break;
                        }
                        cursor += next.len_utf8();
                    }
                    Kind::Word
                } else {
                    Kind::Symbol
                }
            }
        };
        tokens.push(Token {
            kind,
            span: start..cursor,
        });
    }
    Ok(tokens)
}
