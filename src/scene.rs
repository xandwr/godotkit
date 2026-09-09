use std::{collections::HashMap, error::Error, fmt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalResource {
    pub id: String,
    pub kind: Option<String>,
    pub path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneNode {
    pub name: String,
    pub kind: Option<String>,
    pub parent: Option<String>,
    pub instance: Option<String>,
    pub script: Option<String>,
    pub unique_name_in_owner: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scene {
    pub nodes: Vec<SceneNode>,
    pub external_resources: Vec<ExternalResource>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneError {
    line: Option<usize>,
    message: String,
}

impl SceneError {
    fn at(line: usize, message: impl Into<String>) -> Self {
        Self {
            line: Some(line),
            message: message.into(),
        }
    }

    fn new(message: impl Into<String>) -> Self {
        Self {
            line: None,
            message: message.into(),
        }
    }
}

impl fmt::Display for SceneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(formatter, "{} at line {line}", self.message)
        } else {
            formatter.write_str(&self.message)
        }
    }
}

impl Error for SceneError {}

#[derive(Debug)]
struct Header {
    kind: String,
    attributes: HashMap<String, String>,
}

#[derive(Debug)]
struct RenderNode {
    name: String,
    source: Option<usize>,
    children: Vec<RenderNode>,
}

impl RenderNode {
    fn new(name: String) -> Self {
        Self {
            name,
            source: None,
            children: Vec::new(),
        }
    }

    fn insert(&mut self, path: &[&str], source: usize) -> Result<(), SceneError> {
        if path.is_empty() {
            if self.source.replace(source).is_some() {
                return Err(SceneError::new("scene contains duplicate node paths"));
            }
            return Ok(());
        }
        let name = path[0];
        let index = if let Some(index) = self.children.iter().position(|child| child.name == name) {
            index
        } else {
            self.children.push(RenderNode::new(name.to_owned()));
            self.children.len() - 1
        };
        self.children[index].insert(&path[1..], source)
    }
}

pub fn parse(source: &str) -> Result<Scene, SceneError> {
    let mut nodes = Vec::new();
    let mut external_resources = Vec::new();
    let mut current_node = None;
    let mut has_scene_header = false;

    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        let section = line.trim_end();
        let starts_section = section.starts_with('[')
            && section[1..]
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphabetic() || character == '_');
        if starts_section {
            if !section.ends_with(']') {
                return Err(SceneError::at(line_number, "unterminated section header"));
            }
            let header = parse_header(section, line_number)?;
            current_node = None;
            match header.kind.as_str() {
                "gd_scene" => has_scene_header = true,
                "ext_resource" => {
                    let id = required_attribute(&header, "id", line_number)?;
                    let path = required_attribute(&header, "path", line_number)?;
                    external_resources.push(ExternalResource {
                        id,
                        kind: header.attributes.get("type").cloned(),
                        path,
                    });
                }
                "node" => {
                    let name = required_attribute(&header, "name", line_number)?;
                    let instance = header
                        .attributes
                        .get("instance")
                        .and_then(|value| reference_id(value, "ExtResource"));
                    nodes.push(SceneNode {
                        name,
                        kind: header.attributes.get("type").cloned(),
                        parent: header.attributes.get("parent").cloned(),
                        instance,
                        script: None,
                        unique_name_in_owner: false,
                    });
                    current_node = Some(nodes.len() - 1);
                }
                _ => {}
            }
            continue;
        }
        let Some(node_index) = current_node else {
            continue;
        };
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        match name.trim() {
            "unique_name_in_owner" => {
                nodes[node_index].unique_name_in_owner = value.trim() == "true";
            }
            "script" => {
                nodes[node_index].script = reference_id(value.trim(), "ExtResource");
            }
            _ => {}
        }
    }

    if !has_scene_header {
        return Err(SceneError::new("missing gd_scene header"));
    }
    if nodes.is_empty() {
        return Err(SceneError::new("scene contains no nodes"));
    }
    if nodes.iter().filter(|node| node.parent.is_none()).count() != 1 {
        return Err(SceneError::new("scene must contain exactly one root node"));
    }
    Ok(Scene {
        nodes,
        external_resources,
    })
}

impl Scene {
    pub fn compact_tree(&self) -> Result<String, SceneError> {
        let root_index = self
            .nodes
            .iter()
            .position(|node| node.parent.is_none())
            .ok_or_else(|| SceneError::new("scene contains no root node"))?;
        let mut root = RenderNode::new(self.nodes[root_index].name.clone());
        root.source = Some(root_index);
        for (index, node) in self.nodes.iter().enumerate() {
            if index == root_index {
                continue;
            }
            let parent = node
                .parent
                .as_deref()
                .ok_or_else(|| SceneError::new("scene contains multiple root nodes"))?;
            let path = if parent == "." {
                node.name.clone()
            } else {
                format!("{parent}/{}", node.name)
            };
            root.insert(&path.split('/').collect::<Vec<_>>(), index)?;
        }
        let resources = self
            .external_resources
            .iter()
            .map(|resource| (resource.id.as_str(), resource.path.as_str()))
            .collect::<HashMap<_, _>>();
        let mut output = String::new();
        self.write_node(&root, "", true, true, &resources, &mut output);
        Ok(output)
    }

    fn write_node(
        &self,
        node: &RenderNode,
        prefix: &str,
        last: bool,
        root: bool,
        resources: &HashMap<&str, &str>,
        output: &mut String,
    ) {
        if !root {
            output.push_str(prefix);
            output.push_str(if last { "\\- " } else { "|- " });
        }
        if let Some(index) = node.source {
            let source = &self.nodes[index];
            if source.unique_name_in_owner {
                output.push('%');
            }
            output.push_str(&source.name);
            if let Some(kind) = &source.kind {
                output.push_str(" : ");
                output.push_str(kind);
            } else if source.instance.is_none() {
                output.push_str(" [inherited]");
            }
            if let Some(instance) = &source.instance {
                output.push_str(" [instance: ");
                output.push_str(
                    resources
                        .get(instance.as_str())
                        .copied()
                        .unwrap_or(instance),
                );
                output.push(']');
            }
            if let Some(script) = &source.script {
                output.push_str(" [script: ");
                output.push_str(resources.get(script.as_str()).copied().unwrap_or(script));
                output.push(']');
            }
        } else {
            output.push_str(&node.name);
            output.push_str(" [inherited parent]");
        }
        output.push('\n');
        let child_prefix = if root {
            String::new()
        } else {
            format!("{prefix}{}", if last { "   " } else { "|  " })
        };
        for (index, child) in node.children.iter().enumerate() {
            self.write_node(
                child,
                &child_prefix,
                index + 1 == node.children.len(),
                false,
                resources,
                output,
            );
        }
    }
}

fn required_attribute(header: &Header, name: &str, line: usize) -> Result<String, SceneError> {
    header
        .attributes
        .get(name)
        .cloned()
        .ok_or_else(|| SceneError::at(line, format!("missing {name} attribute")))
}

fn parse_header(line: &str, line_number: usize) -> Result<Header, SceneError> {
    let inner = &line[1..line.len() - 1];
    let mut cursor = 0;
    skip_whitespace(inner, &mut cursor);
    let kind = take_while(inner, &mut cursor, |character| !character.is_whitespace());
    if kind.is_empty() {
        return Err(SceneError::at(line_number, "empty section header"));
    }
    let mut attributes = HashMap::new();
    while cursor < inner.len() {
        skip_whitespace(inner, &mut cursor);
        if cursor == inner.len() {
            break;
        }
        let name = take_while(inner, &mut cursor, |character| {
            !character.is_whitespace() && character != '='
        });
        skip_whitespace(inner, &mut cursor);
        if name.is_empty() || !inner[cursor..].starts_with('=') {
            return Err(SceneError::at(line_number, "invalid section attribute"));
        }
        cursor += 1;
        skip_whitespace(inner, &mut cursor);
        let value = parse_value(inner, &mut cursor, line_number)?;
        attributes.insert(name.to_owned(), value);
    }
    Ok(Header {
        kind: kind.to_owned(),
        attributes,
    })
}

fn parse_value(input: &str, cursor: &mut usize, line: usize) -> Result<String, SceneError> {
    if input[*cursor..].starts_with('"') {
        return parse_quoted(input, cursor, line);
    }
    let start = *cursor;
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, character) in input[start..].char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
            continue;
        }
        match character {
            '"' => quoted = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if character.is_whitespace() && depth == 0 => {
                *cursor = start + offset;
                return Ok(input[start..*cursor].to_owned());
            }
            _ => {}
        }
    }
    if quoted || depth != 0 {
        return Err(SceneError::at(line, "unterminated section attribute"));
    }
    *cursor = input.len();
    Ok(input[start..].to_owned())
}

fn parse_quoted(input: &str, cursor: &mut usize, line: usize) -> Result<String, SceneError> {
    *cursor += 1;
    let mut output = String::new();
    while *cursor < input.len() {
        let character = input[*cursor..].chars().next().unwrap();
        *cursor += character.len_utf8();
        match character {
            '"' => return Ok(output),
            '\\' => {
                let escaped = input[*cursor..]
                    .chars()
                    .next()
                    .ok_or_else(|| SceneError::at(line, "unterminated string escape"))?;
                *cursor += escaped.len_utf8();
                output.push(match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '"' => '"',
                    '\\' => '\\',
                    _ => escaped,
                });
            }
            _ => output.push(character),
        }
    }
    Err(SceneError::at(line, "unterminated quoted string"))
}

fn reference_id(value: &str, constructor: &str) -> Option<String> {
    let inner = value
        .strip_prefix(constructor)?
        .strip_prefix('(')?
        .strip_suffix(')')?
        .trim();
    let mut cursor = 0;
    parse_quoted(inner, &mut cursor, 0).ok()
}

fn skip_whitespace(input: &str, cursor: &mut usize) {
    while let Some(character) = input[*cursor..].chars().next() {
        if !character.is_whitespace() {
            break;
        }
        *cursor += character.len_utf8();
    }
}

fn take_while<'a>(input: &'a str, cursor: &mut usize, predicate: impl Fn(char) -> bool) -> &'a str {
    let start = *cursor;
    while let Some(character) = input[*cursor..].chars().next() {
        if !predicate(character) {
            break;
        }
        *cursor += character.len_utf8();
    }
    &input[start..*cursor]
}
