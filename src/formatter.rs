use crate::lexer::{Kind, LexError, Token, tokenize};

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub line_width: usize,
    pub tab_width: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            line_width: 100,
            tab_width: 4,
        }
    }
}

struct Line<'a> {
    start: usize,
    end: usize,
    tokens: Vec<&'a Token>,
}

impl Line<'_> {
    fn indent<'a>(&self, source: &'a str) -> &'a str {
        let text = &source[self.start..self.end];
        &text[..text.len() - text.trim_start_matches([' ', '\t']).len()]
    }
}

fn suite_ends(lines: &[Line<'_>], source: &str, indent: &str) -> bool {
    for line in lines {
        if line.tokens.is_empty() {
            continue;
        }
        if !indent.starts_with(line.indent(source)) {
            return false;
        }
        if line.tokens.iter().any(|token| token.kind != Kind::Comment) {
            return true;
        }
    }
    true
}

pub fn format_source(source: &str, options: &Options) -> Result<String, LexError> {
    let tokens = tokenize(source)?;
    let mut lines = Vec::new();
    let mut line = Line {
        start: 0,
        end: 0,
        tokens: Vec::new(),
    };
    let mut brackets = Vec::new();
    for token in &tokens {
        let text = &source[token.span.clone()];
        if token.kind == Kind::Symbol {
            match text {
                "(" | "[" | "{" => brackets.push(text),
                ")" | "]" | "}" => {
                    let expected = match text {
                        ")" => "(",
                        "]" => "[",
                        _ => "{",
                    };
                    if brackets.pop() != Some(expected) {
                        return Err(LexError {
                            offset: token.span.start,
                            message: "unmatched closing delimiter",
                        });
                    }
                }
                _ => {}
            }
        }
        line.end = token.span.end;
        let continued = line
            .tokens
            .last()
            .is_some_and(|last| &source[last.span.clone()] == "\\");
        if token.kind == Kind::Newline && brackets.is_empty() && !continued {
            lines.push(line);
            line = Line {
                start: token.span.end,
                end: token.span.end,
                tokens: Vec::new(),
            };
        } else if !matches!(token.kind, Kind::Space | Kind::Newline) {
            line.tokens.push(token);
        }
    }
    if !brackets.is_empty() {
        return Err(LexError {
            offset: source.len(),
            message: "unclosed delimiter",
        });
    }
    if line.end > line.start {
        lines.push(line);
    }
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    let mut index = 0;
    while index + 1 < lines.len() {
        let header = &lines[index];
        let body = &lines[index + 1];
        let header_indent = header.indent(source);
        let body_indent = body.indent(source);
        let header_text = source[header.start..header.end].trim_end_matches(['\r', '\n']);
        let guard = header.tokens.len() >= 3
            && &source[header.tokens[0].span.clone()] == "if"
            && &source[header.tokens.last().unwrap().span.clone()] == ":"
            && !header
                .tokens
                .iter()
                .any(|token| token.kind == Kind::Comment)
            && !header_text.contains(['\r', '\n'])
            && body.tokens.len() == 1
            && &source[body.tokens[0].span.clone()] == "return"
            && body_indent.starts_with(header_indent)
            && body_indent.len() > header_indent.len();
        if guard && suite_ends(&lines[index + 2..], source, header_indent) {
            let compact = format!("{} return", header_text.trim_end_matches([' ', '\t']));
            let width = compact.chars().fold(0, |column, ch| {
                if ch == '\t' {
                    column + options.tab_width.max(1) - column % options.tab_width.max(1)
                } else {
                    column + 1
                }
            });
            if width <= options.line_width {
                output.push_str(&source[cursor..header.start]);
                output.push_str(&compact);
                let return_end = body.tokens[0].span.end;
                output.push_str(&source[return_end..body.end]);
                cursor = body.end;
                index += 2;
                continue;
            }
        }
        index += 1;
    }
    output.push_str(&source[cursor..]);
    Ok(output)
}
