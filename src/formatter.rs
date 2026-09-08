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
    let reordered = reorder_fields(source);
    let normalized = normalize_whitespace(&reordered)?;
    let source = normalized.as_str();
    let parsed = parse(source);
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
    normalize_whitespace(&output)
}

fn significant(kind: K) -> bool {
    !kind.is_trivia() && !kind.is_synthetic_layout() && kind != K::Eof
}

fn normalize_whitespace(source: &str) -> Result<String, SyntaxError> {
    let parsed = parse(source);
    let tokens = parsed.tokens();
    let mut starts = vec![0];
    for (at, byte) in source.bytes().enumerate() {
        if byte == b'\n' && at + 1 < source.len() {
            starts.push(at + 1);
        }
    }
    let line_index = |at: usize| {
        starts
            .partition_point(|start| *start <= at)
            .saturating_sub(1)
    };
    let mut lines: Vec<String> = source.split_inclusive('\n').map(str::to_owned).collect();
    let mut protected = vec![false; lines.len()];
    let mut protected_tail = vec![false; lines.len()];
    let mut columns = std::collections::BTreeMap::from([(0, 0)]);
    let mut line_columns = vec![columns.clone(); lines.len()];
    let mut seen = vec![false; lines.len()];
    let mut column_stack = Vec::new();
    let mut previous_indent = 0usize;
    let mut logical_start = true;
    for token in tokens {
        match token.kind {
            K::Indent => {
                column_stack.push(columns.clone());
                let prefix = &source[line_start(source, token.range.start)..token.range.start];
                let column = prefix
                    .bytes()
                    .map(|b| if b == b'\t' { 4 } else { 1 })
                    .sum::<usize>();
                columns.insert(column, previous_indent + 1);
            }
            K::Dedent => {
                columns = column_stack
                    .pop()
                    .unwrap_or_else(|| std::collections::BTreeMap::from([(0, 0)]));
            }
            _ => {}
        }
        if token.kind == K::Newline {
            logical_start = true;
        }
        if !token.kind.is_synthetic_layout()
            && token.kind != K::Eof
            && token.kind != K::Whitespace
            && token.kind != K::NewlinePhys
        {
            let index = line_index(token.range.start);
            if significant(token.kind) && (logical_start || token.kind == K::FuncKw) {
                logical_start = false;
                let prefix = &source[line_start(source, token.range.start)..token.range.start];
                let prefix = &prefix[..prefix.len() - prefix.trim_start_matches([' ', '\t']).len()];
                let column = prefix
                    .bytes()
                    .map(|b| if b == b'\t' { 4 } else { 1 })
                    .sum::<usize>();
                let (&base, &level) = columns.range(..=column).next_back().unwrap();
                let unit = columns.keys().copied().find(|col| *col > 0).unwrap_or(4);
                previous_indent = level + (column - base).div_ceil(unit);
            }
            if !seen[index] {
                line_columns[index] = columns.clone();
                seen[index] = true;
            }
        }
        if matches!(token.kind, K::String | K::StringName | K::NodePath) {
            let first = line_index(token.range.start);
            let last = line_index(token.range.end.saturating_sub(1));
            if last > first {
                protected_tail[first] = true;
            }
            protected[first + 1..last + 1].fill(true);
        }
    }
    for (index, line) in lines.iter_mut().enumerate() {
        let ending = if line.ends_with("\r\n") {
            "\r\n"
        } else if line.ends_with('\n') {
            "\n"
        } else {
            ""
        };
        let content = &line[..line.len() - ending.len()];
        if protected[index] {
            continue;
        }
        let content = if protected_tail[index] {
            content
        } else {
            content.trim_end_matches([' ', '\t'])
        };
        let body = content.trim_start_matches([' ', '\t']);
        let prefix = &content[..content.len() - body.len()];
        let column = prefix
            .bytes()
            .map(|b| if b == b'\t' { 4 } else { 1 })
            .sum::<usize>();
        let columns = &line_columns[index];
        let unit = columns.keys().copied().find(|col| *col > 0).unwrap_or(4);
        let (&base, &level) = columns.range(..=column).next_back().unwrap();
        let indent = if body.is_empty() {
            0
        } else {
            level + (column - base).div_ceil(unit)
        };
        *line = format!("{}{body}{ending}", "\t".repeat(indent));
    }
    let mut spacing = std::collections::BTreeMap::new();
    for scope in parsed
        .root()
        .descendants()
        .filter(|node| matches!(node.kind(), K::SourceFile | K::ClassBody))
    {
        type Category = (K, bool, bool, usize);
        let mut previous: Option<(K, usize, Category)> = None;
        let mut annotations = Vec::new();
        let mut annotation_start = None;
        for member in scope.children() {
            let Some(first) = member.tokens().find(|token| significant(token.kind)) else {
                continue;
            };
            let start = line_index(first.range.start);
            if member.kind() == K::Annotation {
                annotation_start.get_or_insert(start);
                if let Some(name) = member.tokens().find(|token| token.kind == K::Ident) {
                    annotations.push(source[name.range].to_owned());
                }
                continue;
            }
            let mut start = annotation_start.take().unwrap_or(start);
            let last = member
                .tokens()
                .filter(|token| significant(token.kind))
                .last()
                .unwrap();
            let end = line_index(last.range.end.saturating_sub(1));
            let private = member
                .children()
                .find(|node| node.kind() == K::Name)
                .is_some_and(|name| {
                    name.tokens()
                        .find(|token| token.kind == K::Ident)
                        .is_some_and(|token| source[token.range].starts_with('_'))
                });
            let is_static = first.kind == K::StaticKw;
            let rank = if annotations
                .iter()
                .any(|name: &String| name == "export" || name.starts_with("export_"))
            {
                1
            } else if annotations.iter().any(|name| name == "onready") {
                2
            } else {
                3
            };
            annotations.clear();
            let category = (member.kind(), private, is_static, rank);
            if let Some((previous_kind, previous_end, previous_category)) = &previous {
                while start > previous_end + 1 && lines[start - 1].trim_start().starts_with('#') {
                    start -= 1;
                }
                if start > *previous_end {
                    let fields = matches!(
                        member.kind(),
                        K::VarDecl | K::ConstDecl | K::SignalDecl | K::EnumDecl
                    );
                    let blanks = if member.kind() == K::FuncDecl || *previous_kind == K::FuncDecl {
                        2
                    } else if fields && category != *previous_category {
                        1
                    } else {
                        usize::from(
                            lines[previous_end + 1..start]
                                .iter()
                                .any(|line| line.trim().is_empty()),
                        )
                    };
                    spacing.insert(start, blanks);
                }
            }
            previous = Some((member.kind(), end, category));
        }
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut output = String::with_capacity(source.len());
    let mut pending = 0usize;
    for (index, line) in lines.iter().enumerate() {
        if !protected[index] && line.trim().is_empty() {
            pending += 1;
            continue;
        }
        if !output.is_empty() {
            let blanks = spacing.get(&index).copied().unwrap_or(pending.min(1));
            for _ in 0..blanks {
                output.push_str(newline);
            }
        }
        pending = 0;
        output.push_str(line);
    }
    let checked = parse(&output);
    if let Some(error) = checked.errors().first() {
        return Err(error.clone());
    }
    Ok(output)
}
fn reorder_fields(source: &str) -> String {
    let parsed = parse(source);
    let mut edits = Vec::new();
    for scope in parsed
        .root()
        .descendants()
        .filter(|node| matches!(node.kind(), K::SourceFile | K::ClassBody))
    {
        let mut fields = Vec::new();
        let mut annotation_start = None;
        let mut rank = 3;
        let flush = |fields: &mut Vec<(usize, usize, usize)>,
                     edits: &mut Vec<(usize, usize, String)>| {
            if fields.windows(2).any(|pair| pair[0].2 > pair[1].2) {
                let start = fields[0].0;
                let end = fields.last().unwrap().1;
                let mut sorted = fields.clone();
                sorted.sort_by_key(|field| field.2);
                let newline = if source.contains("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                };
                let mut replacement = String::new();
                for (index, (from, to, _)) in sorted.iter().enumerate() {
                    if index > 0 && !replacement.ends_with('\n') {
                        replacement.push_str(newline);
                    }
                    replacement.push_str(&source[*from..*to]);
                }
                if source[..end].ends_with('\n') && !replacement.ends_with('\n') {
                    replacement.push_str(newline);
                }
                edits.push((start, end, replacement));
            }
            fields.clear();
        };
        let mut previous_end = scope.range().start;
        for member in scope.children() {
            let Some(first) = member.tokens().find(|token| significant(token.kind)) else {
                continue;
            };
            if member.kind() == K::Annotation {
                annotation_start.get_or_insert(first.range.start);
                let name = member
                    .tokens()
                    .find(|token| token.kind == K::Ident)
                    .map(|token| &source[token.range])
                    .unwrap_or("");
                if name == "export" || name.starts_with("export_") {
                    rank = 1;
                } else if name == "onready" && rank != 1 {
                    rank = 2;
                }
                if matches!(name, "export_group" | "export_subgroup" | "export_category") {
                    flush(&mut fields, &mut edits);
                    annotation_start = None;
                    previous_end = member.range().end;
                    rank = 3;
                }
                continue;
            }
            let at = annotation_start.take().unwrap_or(first.range.start);
            let mut start = line_start(source, at);
            let last = member
                .tokens()
                .filter(|token| significant(token.kind))
                .last()
                .unwrap();
            let end = source[last.range.end..]
                .find('\n')
                .map_or(source.len(), |offset| last.range.end + offset + 1);
            if matches!(member.kind(), K::VarDecl | K::ConstDecl)
                && source[start..at].trim().is_empty()
                && !source[last.range.end..end].trim().starts_with(';')
            {
                while start > previous_end {
                    let previous_start = line_start(
                        source,
                        start
                            .saturating_sub(1)
                            .saturating_sub(usize::from(source[..start].ends_with("\r\n"))),
                    );
                    let line = source[previous_start..start].trim();
                    if previous_start < previous_end || !(line.is_empty() || line.starts_with('#'))
                    {
                        break;
                    }
                    start = previous_start;
                }
                fields.push((
                    start,
                    end,
                    if member.kind() == K::ConstDecl {
                        0
                    } else {
                        rank
                    },
                ));
            } else {
                flush(&mut fields, &mut edits);
            }
            rank = 3;
            previous_end = end;
        }
        flush(&mut fields, &mut edits);
    }
    let mut output = source.to_owned();
    edits.sort_by_key(|edit| edit.0);
    for (start, end, replacement) in edits.into_iter().rev() {
        output.replace_range(start..end, &replacement);
    }
    output
}
