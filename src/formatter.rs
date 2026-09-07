use crate::syntax::{Element, Node, Span, SyntaxError, SyntaxKind as K, parse};

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

fn line_start(source: &str, offset: usize) -> usize {
    source[..offset].rfind(['\r', '\n']).map_or(0, |at| at + 1)
}

fn guard_edit(node: Node<'_>, source: &str, options: &Options) -> Option<Span> {
    let block = node.children().find(|child| child.kind() == K::Block)?;
    let mut statements = block.children();
    let statement = statements.next()?;
    if statement.kind() != K::ReturnStmt
        || statements.next().is_some()
        || statement.children().next().is_some()
    {
        return None;
    }
    let keyword = node
        .children_with_tokens()
        .find_map(|element| match element {
            Element::Token(token) if token.kind == K::IfKw => Some(token),
            _ => None,
        })?;
    let colon = node
        .children_with_tokens()
        .find_map(|element| match element {
            Element::Token(token) if token.kind == K::Colon => Some(token),
            _ => None,
        })?;
    let returned = statement.tokens().find(|token| token.kind == K::ReturnKw)?;
    let start = line_start(source, keyword.range.start);
    let indent = &source[start..keyword.range.start];
    let header = &source[start..colon.range.end];
    if !indent.bytes().all(|byte| matches!(byte, b' ' | b'\t')) || header.contains(['\r', '\n']) {
        return None;
    }
    let gap = &source[colon.range.end..returned.range.start];
    let gap = gap.trim_start_matches([' ', '\t']);
    let indentation = gap
        .strip_prefix("\r\n")
        .or_else(|| gap.strip_prefix('\n'))?;
    if indentation.is_empty() || !indentation.bytes().all(|byte| matches!(byte, b' ' | b'\t')) {
        return None;
    }
    let tail = &source[returned.range.end..];
    let tail = &tail[..tail.find(['\r', '\n']).unwrap_or(tail.len())];
    if !tail.bytes().all(|byte| matches!(byte, b' ' | b'\t')) {
        return None;
    }
    for token in block.tokens() {
        if token.kind == K::Semicolon {
            return None;
        }
        if matches!(
            token.kind,
            K::LineComment | K::DocComment | K::RegionComment | K::EndRegionComment
        ) {
            let comment_indent = &source[line_start(source, token.range.start)..token.range.start];
            if token.range.start < returned.range.end || comment_indent.len() > indent.len() {
                return None;
            }
        }
    }
    let tab_width = options.tab_width.max(1);
    let width = header
        .chars()
        .chain(" return".chars())
        .fold(0, |column, ch| {
            if ch == '\t' {
                column + tab_width - column % tab_width
            } else {
                column + 1
            }
        });
    (width <= options.line_width).then_some(Span::new(colon.range.end, returned.range.start))
}

pub fn format_source(source: &str, options: &Options) -> Result<String, SyntaxError> {
    let parsed = parse(source);
    if let Some(error) = parsed.errors().first() {
        return Err(error.clone());
    }
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    for node in parsed
        .root()
        .descendants()
        .filter(|node| node.kind() == K::IfStmt)
    {
        if let Some(edit) = guard_edit(node, source, options) {
            output.push_str(&source[cursor..edit.start]);
            output.push(' ');
            cursor = edit.end;
        }
    }
    output.push_str(&source[cursor..]);
    Ok(output)
}
