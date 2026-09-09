use std::{
    collections::HashMap,
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

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
    pub groups: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreeOptions {
    pub connections: bool,
    pub groups: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneConnection {
    pub signal: String,
    pub from: String,
    pub to: String,
    pub method: String,
    pub flags: Option<String>,
    pub binds: Option<String>,
    pub unbinds: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scene {
    pub nodes: Vec<SceneNode>,
    pub external_resources: Vec<ExternalResource>,
    pub connections: Vec<SceneConnection>,
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

#[derive(Clone, Copy)]
struct RenderPosition<'a> {
    prefix: &'a str,
    last: bool,
    root: bool,
    path: &'a str,
}

#[derive(Clone, Debug)]
struct InstanceReference {
    display: String,
    path: PathBuf,
}

#[derive(Debug)]
struct ExpandedNode {
    name: String,
    kind: Option<String>,
    instance: Option<InstanceReference>,
    script: Option<String>,
    unique_name_in_owner: bool,
    inherited: bool,
    inherited_parent: bool,
    origin: Option<String>,
    expanded: bool,
    children: Vec<ExpandedNode>,
    groups: Vec<String>,
    connections: Vec<SceneConnection>,
}

impl RenderNode {
    fn ensure_path(&mut self, path: &[&str]) {
        let Some((name, rest)) = path.split_first() else {
            return;
        };
        if *name == "." {
            self.ensure_path(rest);
            return;
        }
        let index = self
            .children
            .iter()
            .position(|child| child.name == *name)
            .unwrap_or_else(|| {
                self.children.push(RenderNode::new((*name).to_owned()));
                self.children.len() - 1
            });
        self.children[index].ensure_path(rest);
    }

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
    let mut connections = Vec::new();
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
                        groups: header
                            .attributes
                            .get("groups")
                            .map(|value| parse_groups(value, line_number))
                            .transpose()?
                            .unwrap_or_default(),
                    });
                    current_node = Some(nodes.len() - 1);
                }
                "connection" => connections.push(SceneConnection {
                    signal: required_attribute(&header, "signal", line_number)?,
                    from: required_attribute(&header, "from", line_number)?,
                    to: required_attribute(&header, "to", line_number)?,
                    method: required_attribute(&header, "method", line_number)?,
                    flags: header.attributes.get("flags").cloned(),
                    binds: header.attributes.get("binds").cloned(),
                    unbinds: header.attributes.get("unbinds").cloned(),
                }),
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
        connections,
    })
}

impl Scene {
    pub fn compact_tree(&self) -> Result<String, SceneError> {
        self.compact_tree_with_options(TreeOptions::default())
    }

    pub fn compact_tree_with_options(&self, options: TreeOptions) -> Result<String, SceneError> {
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
        if options.connections {
            for connection in &self.connections {
                root.ensure_path(&connection.from.split('/').collect::<Vec<_>>());
            }
        }
        let resources = self
            .external_resources
            .iter()
            .map(|resource| (resource.id.as_str(), resource.path.as_str()))
            .collect::<HashMap<_, _>>();
        let mut output = String::new();
        self.write_node(
            &root,
            &resources,
            options,
            RenderPosition {
                prefix: "",
                last: true,
                root: true,
                path: ".",
            },
            &mut output,
        );
        Ok(output)
    }

    fn write_node(
        &self,
        node: &RenderNode,
        resources: &HashMap<&str, &str>,
        options: TreeOptions,
        position: RenderPosition<'_>,
        output: &mut String,
    ) {
        if !position.root {
            output.push_str(position.prefix);
            output.push_str(if position.last { "\\- " } else { "|- " });
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
        let child_prefix = if position.root {
            String::new()
        } else {
            format!(
                "{}{}",
                position.prefix,
                if position.last { "   " } else { "|  " }
            )
        };
        let details_prefix = if position.root { "   " } else { &child_prefix };
        write_details(
            node.source
                .map(|index| self.nodes[index].groups.as_slice())
                .unwrap_or_default(),
            self.connections
                .iter()
                .filter(|connection| connection.from == position.path),
            options,
            details_prefix,
            |connection| self.connection_target(&connection.to),
            output,
        );
        for (index, child) in node.children.iter().enumerate() {
            let path = child_path(position.path, &child.name);
            self.write_node(
                child,
                resources,
                options,
                RenderPosition {
                    prefix: &child_prefix,
                    last: index + 1 == node.children.len(),
                    root: false,
                    path: &path,
                },
                output,
            );
        }
    }

    fn connection_target(&self, path: &str) -> String {
        self.nodes
            .iter()
            .find(|node| node_path(node) == path)
            .map_or_else(
                || path.to_owned(),
                |node| {
                    if node.unique_name_in_owner {
                        format!("%{}", node.name)
                    } else if path == "." {
                        node.name.clone()
                    } else {
                        path.to_owned()
                    }
                },
            )
    }
}

pub fn compact_tree_expanded(path: &Path, depth: usize) -> Result<String, SceneError> {
    compact_tree_expanded_with_options(path, depth, TreeOptions::default())
}

pub fn compact_tree_expanded_with_options(
    path: &Path,
    depth: usize,
    options: TreeOptions,
) -> Result<String, SceneError> {
    let path = fs::canonicalize(path)
        .map_err(|error| SceneError::new(format!("{}: {error}", path.display())))?;
    let project_root = find_project_root(&path);
    let mut root = load_expanded_scene(&path, None, project_root.as_deref(), options.connections)?;
    let mut stack = vec![path];
    expand_node(
        &mut root,
        depth,
        project_root.as_deref(),
        &mut stack,
        options.connections,
    )?;
    let mut output = String::new();
    let mut labels = HashMap::new();
    collect_labels(&root, ".", &mut labels);
    write_expanded_node(&root, "", true, true, (options, &labels, "."), &mut output);
    Ok(output)
}

fn find_project_root(path: &Path) -> Option<PathBuf> {
    path.parent()?
        .ancestors()
        .find(|directory| directory.join("project.godot").is_file())
        .map(Path::to_path_buf)
}

fn load_expanded_scene(
    path: &Path,
    origin: Option<&str>,
    project_root: Option<&Path>,
    connections: bool,
) -> Result<ExpandedNode, SceneError> {
    let source = fs::read_to_string(path)
        .map_err(|error| SceneError::new(format!("{}: {error}", path.display())))?;
    let scene =
        parse(&source).map_err(|error| SceneError::new(format!("{}: {error}", path.display())))?;
    let resources = scene
        .external_resources
        .iter()
        .map(|resource| (resource.id.as_str(), resource.path.as_str()))
        .collect::<HashMap<_, _>>();
    let root_index = scene
        .nodes
        .iter()
        .position(|node| node.parent.is_none())
        .ok_or_else(|| SceneError::new("scene contains no root node"))?;
    let mut render_root = RenderNode::new(scene.nodes[root_index].name.clone());
    render_root.source = Some(root_index);
    for (index, node) in scene.nodes.iter().enumerate() {
        if index == root_index {
            continue;
        }
        let parent = node
            .parent
            .as_deref()
            .ok_or_else(|| SceneError::new("scene contains multiple root nodes"))?;
        let node_path = if parent == "." {
            node.name.clone()
        } else {
            format!("{parent}/{}", node.name)
        };
        render_root.insert(&node_path.split('/').collect::<Vec<_>>(), index)?;
    }
    if connections {
        for connection in &scene.connections {
            render_root.ensure_path(&connection.from.split('/').collect::<Vec<_>>());
        }
    }
    materialize_expanded(
        &render_root,
        &scene,
        &resources,
        path,
        project_root,
        origin,
        ".",
    )
}

fn materialize_expanded(
    render: &RenderNode,
    scene: &Scene,
    resources: &HashMap<&str, &str>,
    scene_path: &Path,
    project_root: Option<&Path>,
    origin: Option<&str>,
    path: &str,
) -> Result<ExpandedNode, SceneError> {
    let source = render.source.map(|index| &scene.nodes[index]);
    let instance = source
        .and_then(|node| node.instance.as_deref())
        .map(|id| {
            let display = resources.get(id).copied().unwrap_or(id).to_owned();
            resolve_resource_path(scene_path, project_root, &display)
                .map(|path| InstanceReference { display, path })
        })
        .transpose()?;
    let script = source.and_then(|node| {
        node.script
            .as_deref()
            .map(|id| resources.get(id).copied().unwrap_or(id).to_owned())
    });
    let mut children = Vec::with_capacity(render.children.len());
    for child in &render.children {
        children.push(materialize_expanded(
            child,
            scene,
            resources,
            scene_path,
            project_root,
            origin,
            &child_path(path, &child.name),
        )?);
    }
    Ok(ExpandedNode {
        name: source.map_or_else(|| render.name.clone(), |node| node.name.clone()),
        kind: source.and_then(|node| node.kind.clone()),
        inherited: source.is_some_and(|node| node.kind.is_none() && node.instance.is_none()),
        inherited_parent: source.is_none(),
        instance,
        script,
        unique_name_in_owner: source.is_some_and(|node| node.unique_name_in_owner),
        origin: origin.map(str::to_owned),
        expanded: false,
        children,
        groups: source.map(|node| node.groups.clone()).unwrap_or_default(),
        connections: scene
            .connections
            .iter()
            .filter(|connection| connection.from == path)
            .cloned()
            .map(|mut connection| {
                connection.to = relative_target(path, &connection.to);
                connection
            })
            .collect(),
    })
}

fn resolve_resource_path(
    scene_path: &Path,
    project_root: Option<&Path>,
    resource: &str,
) -> Result<PathBuf, SceneError> {
    if let Some(relative) = resource.strip_prefix("res://") {
        return project_root.map(|root| root.join(relative)).ok_or_else(|| {
            SceneError::new(format!(
                "cannot resolve {resource}: no project.godot found above {}",
                scene_path.display()
            ))
        });
    }
    if resource.starts_with("uid://") {
        return Err(SceneError::new(format!(
            "cannot resolve scene resource {resource} without Godot"
        )));
    }
    let path = Path::new(resource);
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(scene_path.parent().unwrap_or(Path::new(".")).join(path))
    }
}

fn expand_node(
    node: &mut ExpandedNode,
    remaining: usize,
    project_root: Option<&Path>,
    stack: &mut Vec<PathBuf>,
    connections: bool,
) -> Result<usize, SceneError> {
    if remaining == 0 || node.instance.is_none() || node.expanded {
        for child in &mut node.children {
            expand_node(child, remaining, project_root, stack, connections)?;
        }
        return Ok(0);
    }
    let instance = node.instance.clone().unwrap();
    let target = fs::canonicalize(&instance.path)
        .map_err(|error| SceneError::new(format!("{}: {error}", instance.path.display())))?;
    if let Some(position) = stack.iter().position(|path| path == &target) {
        let mut cycle = stack[position..]
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();
        cycle.push(target.display().to_string());
        return Err(SceneError::new(format!(
            "scene instance cycle detected: {}",
            cycle.join(" -> ")
        )));
    }
    stack.push(target.clone());
    let mut base =
        load_expanded_scene(&target, Some(&instance.display), project_root, connections)?;
    let inherited_depth = expand_node(&mut base, remaining - 1, project_root, stack, connections)?;
    let child_depth = remaining.saturating_sub(inherited_depth + 1);
    for child in &mut node.children {
        expand_node(child, child_depth, project_root, stack, connections)?;
    }
    merge_instance(node, base);
    stack.pop();
    Ok(inherited_depth + 1)
}

fn merge_instance(node: &mut ExpandedNode, mut base: ExpandedNode) {
    merge_details(&mut base, &mut node.groups, &mut node.connections);
    let local_children = std::mem::take(&mut node.children);
    for child in local_children {
        merge_child(&mut base.children, child);
    }
    base.name = node.name.clone();
    if node.kind.is_some() {
        base.kind = node.kind.clone();
    }
    if node.script.is_some() {
        base.script = node.script.clone();
    }
    base.instance = node.instance.clone();
    base.unique_name_in_owner = node.unique_name_in_owner;
    base.inherited = false;
    base.inherited_parent = false;
    base.origin = node.origin.clone();
    base.expanded = true;
    *node = base;
}

fn merge_child(children: &mut Vec<ExpandedNode>, mut local: ExpandedNode) {
    let Some(index) = children.iter().position(|child| child.name == local.name) else {
        children.push(local);
        return;
    };
    if local.inherited_parent {
        merge_details(
            &mut children[index],
            &mut local.groups,
            &mut local.connections,
        );
        for child in local.children {
            merge_child(&mut children[index].children, child);
        }
        return;
    }
    if local.instance.is_some() {
        children[index] = local;
        return;
    }
    let base = &mut children[index];
    merge_details(base, &mut local.groups, &mut local.connections);
    if local.kind.is_some() {
        base.kind = local.kind;
        base.inherited = false;
    }
    if local.script.is_some() {
        base.script = local.script;
    }
    if local.unique_name_in_owner {
        base.unique_name_in_owner = true;
    }
    for child in local.children {
        merge_child(&mut base.children, child);
    }
}

fn write_expanded_node(
    node: &ExpandedNode,
    prefix: &str,
    last: bool,
    root: bool,
    details: (TreeOptions, &HashMap<String, String>, &str),
    output: &mut String,
) {
    if !root {
        output.push_str(prefix);
        output.push_str(if last { "\\- " } else { "|- " });
    }
    if node.unique_name_in_owner {
        output.push('%');
    }
    output.push_str(&node.name);
    if let Some(kind) = &node.kind {
        output.push_str(" : ");
        output.push_str(kind);
    } else if node.inherited {
        output.push_str(" [inherited]");
    } else if node.inherited_parent {
        output.push_str(" [inherited parent]");
    }
    if let Some(instance) = &node.instance {
        output.push_str(" [instance: ");
        output.push_str(&instance.display);
        output.push(']');
    }
    if let Some(script) = &node.script {
        output.push_str(" [script: ");
        output.push_str(script);
        output.push(']');
    }
    if let Some(origin) = &node.origin {
        output.push_str(" [origin: ");
        output.push_str(origin);
        output.push(']');
    }
    output.push('\n');
    let child_prefix = if root {
        String::new()
    } else {
        format!("{prefix}{}", if last { "   " } else { "|  " })
    };
    let (options, labels, path) = details;
    write_details(
        &node.groups,
        node.connections.iter(),
        options,
        if root { "   " } else { &child_prefix },
        |connection| {
            let target = resolve_target(path, &connection.to);
            labels.get(&target).cloned().unwrap_or(target)
        },
        output,
    );
    for (index, child) in node.children.iter().enumerate() {
        write_expanded_node(
            child,
            &child_prefix,
            index + 1 == node.children.len(),
            false,
            (options, labels, &child_path(path, &child.name)),
            output,
        );
    }
}

fn child_path(parent: &str, name: &str) -> String {
    if parent == "." {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

fn node_path(node: &SceneNode) -> String {
    node.parent
        .as_deref()
        .map_or_else(|| ".".to_owned(), |parent| child_path(parent, &node.name))
}

fn relative_target(from: &str, to: &str) -> String {
    let from = from
        .split('/')
        .filter(|part| *part != ".")
        .collect::<Vec<_>>();
    let to = to
        .split('/')
        .filter(|part| *part != ".")
        .collect::<Vec<_>>();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = vec![".."; from.len() - common];
    parts.extend_from_slice(&to[common..]);
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

fn resolve_target(from: &str, relative: &str) -> String {
    let mut parts = from
        .split('/')
        .filter(|part| *part != ".")
        .collect::<Vec<_>>();
    for part in relative.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

fn collect_labels(node: &ExpandedNode, path: &str, labels: &mut HashMap<String, String>) {
    let label = if node.unique_name_in_owner {
        format!("%{}", node.name)
    } else if path == "." {
        node.name.clone()
    } else {
        path.to_owned()
    };
    labels.insert(path.to_owned(), label);
    for child in &node.children {
        collect_labels(child, &child_path(path, &child.name), labels);
    }
}

fn merge_details(
    base: &mut ExpandedNode,
    groups: &mut Vec<String>,
    connections: &mut Vec<SceneConnection>,
) {
    for group in groups.drain(..) {
        if !base.groups.contains(&group) {
            base.groups.push(group);
        }
    }
    for connection in connections.drain(..) {
        if !base.connections.iter().any(|existing| {
            existing.signal == connection.signal
                && existing.to == connection.to
                && existing.method == connection.method
                && existing.binds == connection.binds
                && existing.unbinds == connection.unbinds
        }) {
            base.connections.push(connection);
        }
    }
}

fn write_details<'a>(
    groups: &[String],
    connections: impl Iterator<Item = &'a SceneConnection>,
    options: TreeOptions,
    prefix: &str,
    target: impl Fn(&SceneConnection) -> String,
    output: &mut String,
) {
    if options.connections {
        let mut heading = false;
        for connection in connections {
            if !heading {
                output.push_str(&format!("{prefix}signals:\n"));
                heading = true;
            }
            output.push_str(&format!(
                "{prefix}   {} -> {}.{}",
                connection.signal,
                target(connection),
                connection.method
            ));
            for (name, value) in [
                ("flags", &connection.flags),
                ("binds", &connection.binds),
                ("unbinds", &connection.unbinds),
            ] {
                if let Some(value) = value {
                    output.push_str(&format!(" [{name}: {value}]"));
                }
            }
            output.push('\n');
        }
    }
    if options.groups && !groups.is_empty() {
        output.push_str(&format!("{prefix}groups: {}\n", groups.join(", ")));
    }
}

fn parse_groups(value: &str, line: usize) -> Result<Vec<String>, SceneError> {
    let inner = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .or_else(|| {
            value
                .strip_prefix("PackedStringArray(")
                .and_then(|value| value.strip_suffix(')'))
        })
        .ok_or_else(|| SceneError::at(line, "invalid groups array"))?;
    let mut cursor = 0;
    let mut groups = Vec::new();
    loop {
        skip_whitespace(inner, &mut cursor);
        if cursor == inner.len() {
            return Ok(groups);
        }
        if inner[cursor..].starts_with('&') {
            cursor += 1;
        }
        if !inner[cursor..].starts_with('"') {
            return Err(SceneError::at(line, "expected quoted group name"));
        }
        groups.push(parse_quoted(inner, &mut cursor, line)?);
        skip_whitespace(inner, &mut cursor);
        if cursor == inner.len() {
            return Ok(groups);
        }
        if !inner[cursor..].starts_with(',') {
            return Err(SceneError::at(line, "expected comma between groups"));
        }
        cursor += 1;
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
