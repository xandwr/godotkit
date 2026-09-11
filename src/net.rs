use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use gdkit::net_report::{
    LifecycleFinding, NET_REPORT_SCHEMA_VERSION, NetAutoload, NetCoverage, NetEngine, NetReport,
    ReplicationNode, RpcCall, RpcEndpoint, SourceFinding, SourceLocation,
};
use gdview::syntax::{
    SyntaxKind as K,
    ast::{AstNode, Function},
    parse,
};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::{
    cli::{NetArgs, NetOutput},
    engine, project_files,
};

const RESULT_PREFIX: &str = "GDKIT_NET_RESULT:";

#[derive(Deserialize)]
struct EngineIndex {
    version: String,
    scripts: Vec<EngineScript>,
    errors: Vec<String>,
    resolved_paths: Map<String, Value>,
}

#[derive(Deserialize)]
struct EngineScript {
    path: String,
    rpc_config: Map<String, Value>,
}

struct TemporaryFile(PathBuf);

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Clone)]
struct SourceFunction {
    signature: String,
    line: usize,
    rpc_line: Option<usize>,
}

#[derive(Default)]
struct SourceIndex {
    functions: HashMap<(String, String), SourceFunction>,
    rpc_calls: Vec<RpcCall>,
    peer_constructions: Vec<SourceFinding>,
    peer_assignments: Vec<SourceFinding>,
    lifecycle: Vec<LifecycleFinding>,
    authority: Vec<SourceFinding>,
    unknowns: Vec<String>,
}

fn temporary_file(
    stem: &str,
    extension: &str,
    contents: &[u8],
) -> Result<TemporaryFile, Box<dyn Error>> {
    for attempt in 0..100 {
        let path = std::env::temp_dir().join(format!(
            "gdkit-{stem}-{}-{attempt}.{extension}",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(contents)?;
                return Ok(TemporaryFile(path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(format!("could not create temporary {stem} file").into())
}

fn resource_path(project: &Path, path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(format!(
        "res://{}",
        path.strip_prefix(project)?
            .to_string_lossy()
            .replace('\\', "/")
    ))
}

fn query_engine(
    engine_path: &Path,
    project: &Path,
    scripts: &[PathBuf],
) -> Result<EngineIndex, Box<dyn Error>> {
    let script = temporary_file("net-query", "gd", include_bytes!("net.gd"))?;
    let paths: Vec<_> = scripts
        .iter()
        .map(|path| resource_path(project, path))
        .collect::<Result<_, _>>()?;
    let manifest = temporary_file("net-scripts", "json", &serde_json::to_vec(&paths)?)?;
    let output = Command::new(engine_path)
        .args(["--headless", "--no-header", "--path"])
        .arg(project)
        .arg("--script")
        .arg(&script.0)
        .arg("--")
        .arg(&manifest.0)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let index = stdout
        .lines()
        .find_map(|line| line.strip_prefix(RESULT_PREFIX))
        .map(serde_json::from_str::<EngineIndex>)
        .transpose()?;
    match index {
        Some(index) if output.status.success() => Ok(index),
        _ => Err(format!(
            "configured engine could not inspect project RPC metadata\n{}{}",
            stdout,
            String::from_utf8_lossy(&output.stderr)
        )
        .into()),
    }
}

fn source_line(source: &str, offset: usize) -> usize {
    source[..offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn node_line(source: &str, node: gdview::syntax::Node<'_>) -> usize {
    let offset = node
        .tokens()
        .find(|token| !token.kind.is_trivia() && !token.kind.is_synthetic_layout())
        .map_or(node.range().start, |token| token.range.start);
    source_line(source, offset)
}

fn compact(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn function_signature(function: Function<'_>) -> String {
    let syntax = function.syntax();
    let end = function
        .body()
        .map_or(syntax.range().end, |body| body.syntax().range().start);
    compact(
        syntax.text()[..end - syntax.range().start].trim_end_matches([':', ' ', '\t', '\r', '\n']),
    )
}

fn annotation_name(node: gdview::syntax::Node<'_>) -> Option<&str> {
    node.tokens()
        .find(|token| token.kind == K::Ident)
        .map(|token| {
            &node.text()
                [token.range.start - node.range().start..token.range.end - node.range().start]
        })
}

fn collect_functions(
    container: gdview::syntax::Node<'_>,
    source: &str,
    path: &str,
    functions: &mut HashMap<(String, String), SourceFunction>,
) {
    let mut rpc_line = None;
    for child in container.children() {
        match child.kind() {
            K::Annotation => {
                if annotation_name(child) == Some("rpc") {
                    rpc_line = Some(node_line(source, child));
                }
            }
            K::FuncDecl => {
                if let Some(function) = Function::cast(child)
                    && let Some(name) = function.name()
                {
                    functions.insert(
                        (path.to_owned(), name.to_owned()),
                        SourceFunction {
                            signature: function_signature(function),
                            line: node_line(source, child),
                            rpc_line,
                        },
                    );
                }
                rpc_line = None;
            }
            _ => rpc_line = None,
        }
    }
}

fn call_parts(node: gdview::syntax::Node<'_>) -> Option<(String, Vec<String>)> {
    let arguments = node.children().find(|child| child.kind() == K::ArgList)?;
    let offset = arguments.range().start.checked_sub(node.range().start)?;
    let callee = node.text()[..offset].trim().to_owned();
    let values = arguments
        .children()
        .map(|argument| compact(argument.text()))
        .collect();
    Some((callee, values))
}

fn rpc_call(callee: &str, arguments: &[String], source: SourceLocation) -> Option<RpcCall> {
    if callee == "multiplayer.rpc" {
        return Some(RpcCall {
            method: arguments
                .get(2)
                .cloned()
                .unwrap_or_else(|| "dynamic".into()),
            kind: "MultiplayerAPI.rpc".into(),
            target: arguments.first().cloned(),
            source,
        });
    }
    for (suffix, kind) in [(".rpc_id", "rpc_id"), (".rpc", "rpc")] {
        if let Some(receiver) = callee.strip_suffix(suffix) {
            return Some(RpcCall {
                method: receiver.rsplit('.').next().unwrap_or(receiver).to_owned(),
                kind: kind.into(),
                target: (kind == "rpc_id")
                    .then(|| arguments.first().cloned())
                    .flatten(),
                source,
            });
        }
    }
    None
}

fn scan_script(path: &str, source: &str, index: &mut SourceIndex) {
    let parsed = parse(source);
    if !parsed.is_valid() {
        index.unknowns.push(format!(
            "{path}: gdview reported {} syntax error(s); findings may be incomplete",
            parsed.errors().len()
        ));
    }
    collect_functions(parsed.root(), source, path, &mut index.functions);
    for node in parsed.root().descendants() {
        let location = SourceLocation {
            path: path.to_owned(),
            line: node_line(source, node),
        };
        if node.kind() == K::AssignExpr {
            let value = compact(node.text());
            let left = value
                .split_once('=')
                .map_or(value.as_str(), |(left, _)| left);
            if left.trim().ends_with("multiplayer_peer") {
                index.peer_assignments.push(SourceFinding {
                    value,
                    source: location,
                });
            }
            continue;
        }
        if node.kind() != K::CallExpr {
            continue;
        }
        let Some((callee, arguments)) = call_parts(node) else {
            continue;
        };
        if let Some(call) = rpc_call(&callee, &arguments, location.clone()) {
            index.rpc_calls.push(call);
        }
        if let Some(class) = callee.strip_suffix(".new")
            && class
                .rsplit('.')
                .next()
                .is_some_and(|name| name.ends_with("MultiplayerPeer"))
        {
            index.peer_constructions.push(SourceFinding {
                value: class.to_owned(),
                source: location.clone(),
            });
        }
        for signal in [
            "peer_connected",
            "peer_disconnected",
            "connected_to_server",
            "connection_failed",
            "server_disconnected",
        ] {
            let prefix = format!("multiplayer.{signal}.");
            if let Some(operation) = callee.strip_prefix(&prefix)
                && matches!(operation, "connect" | "disconnect" | "emit")
            {
                index.lifecycle.push(LifecycleFinding {
                    signal: signal.into(),
                    operation: operation.into(),
                    source: location.clone(),
                });
            }
        }
        if [
            "is_server",
            "get_remote_sender_id",
            "get_unique_id",
            "get_multiplayer_authority",
            "is_multiplayer_authority",
            "set_multiplayer_authority",
        ]
        .iter()
        .any(|method| callee == *method || callee.ends_with(&format!(".{method}")))
        {
            index.authority.push(SourceFinding {
                value: format!("{callee}({})", arguments.join(", ")),
                source: location,
            });
        }
    }
}

fn config_integer(config: &Map<String, Value>, name: &str, default: i64) -> i64 {
    config.get(name).and_then(Value::as_i64).unwrap_or(default)
}

fn config_boolean(config: &Map<String, Value>, name: &str, default: bool) -> bool {
    config.get(name).and_then(Value::as_bool).unwrap_or(default)
}

fn rpc_mode(value: i64) -> String {
    match value {
        0 => "disabled",
        1 => "any_peer",
        2 => "authority",
        _ => "unknown",
    }
    .into()
}

fn transfer_mode(value: i64) -> String {
    match value {
        0 => "unreliable",
        1 => "unreliable_ordered",
        2 => "reliable",
        _ => "unknown",
    }
    .into()
}

fn rpc_endpoints(engine_index: &EngineIndex, source: &SourceIndex) -> Vec<RpcEndpoint> {
    let mut endpoints = Vec::new();
    for script in &engine_index.scripts {
        for (method, value) in &script.rpc_config {
            let Some(config) = value.as_object() else {
                continue;
            };
            let source_function = source.functions.get(&(script.path.clone(), method.clone()));
            endpoints.push(RpcEndpoint {
                method: method.clone(),
                signature: source_function.map(|function| function.signature.clone()),
                source: SourceLocation {
                    path: script.path.clone(),
                    line: source_function
                        .map_or(0, |function| function.rpc_line.unwrap_or(function.line)),
                },
                rpc_mode: rpc_mode(config_integer(config, "rpc_mode", 2)),
                call: if config_boolean(config, "call_local", false) {
                    "call_local"
                } else {
                    "call_remote"
                }
                .into(),
                transfer_mode: transfer_mode(config_integer(config, "transfer_mode", 2)),
                channel: config_integer(config, "channel", 0),
                inherited: source_function.is_none_or(|function| function.rpc_line.is_none()),
            });
        }
    }
    endpoints.sort_by(|left, right| {
        left.source
            .path
            .cmp(&right.source.path)
            .then_with(|| left.method.cmp(&right.method))
    });
    endpoints
}

fn replication_nodes(
    project: &Path,
    scenes: &[PathBuf],
    unknowns: &mut Vec<String>,
) -> Vec<ReplicationNode> {
    let mut nodes = Vec::new();
    for path in scenes {
        let display = match resource_path(project, path) {
            Ok(path) => path,
            Err(error) => {
                unknowns.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => {
                unknowns.push(format!("{display}: {error}"));
                continue;
            }
        };
        let scene = match gdview::scene::parse(&source) {
            Ok(scene) => scene,
            Err(error) => {
                unknowns.push(format!("{display}: {error}"));
                continue;
            }
        };
        for node in scene.nodes {
            let Some(kind) = node.kind else {
                continue;
            };
            if !matches!(
                kind.as_str(),
                "MultiplayerSpawner" | "MultiplayerSynchronizer"
            ) {
                continue;
            }
            let node_path = match node.parent.as_deref() {
                None | Some(".") => node.name,
                Some(parent) => format!("{parent}/{}", node.name),
            };
            nodes.push(ReplicationNode {
                kind,
                node_path,
                scene: display.clone(),
            });
        }
    }
    nodes.sort_by(|left, right| {
        left.scene
            .cmp(&right.scene)
            .then_with(|| left.node_path.cmp(&right.node_path))
    });
    nodes
}

fn location_text(location: &SourceLocation) -> String {
    if location.line == 0 {
        location.path.clone()
    } else {
        format!("{}:{}", location.path, location.line)
    }
}

fn print_findings(label: &str, findings: &[SourceFinding]) {
    println!("{label} ({}):", findings.len());
    for finding in findings {
        println!("  {} [{}]", finding.value, location_text(&finding.source));
    }
}

fn print_human(report: &NetReport) {
    println!(
        "Engine {} ({})",
        report.engine.version, report.engine.executable
    );
    println!("Project {}", report.project);
    println!(
        "Coverage: {} GDScript files, {} text scenes",
        report.coverage.scripts_scanned, report.coverage.scenes_scanned
    );
    let networked: Vec<_> = report
        .autoloads
        .iter()
        .filter(|autoload| autoload.networked)
        .collect();
    println!("\nNetworked autoloads ({}):", networked.len());
    for autoload in networked {
        let resolved = autoload
            .resolved_path
            .as_deref()
            .map_or(String::new(), |path| format!(" (resolved: {path})"));
        println!(
            "  {}. {} -> {}{}{}",
            autoload.index,
            autoload.name,
            autoload.path,
            resolved,
            if autoload.singleton {
                " [singleton]"
            } else {
                ""
            }
        );
    }
    println!("\nRPC endpoints ({}):", report.rpc_endpoints.len());
    for endpoint in &report.rpc_endpoints {
        println!(
            "  {} [{}; {}, {}, {}, channel {}{}]",
            endpoint
                .signature
                .as_deref()
                .unwrap_or(endpoint.method.as_str()),
            location_text(&endpoint.source),
            endpoint.rpc_mode,
            endpoint.call,
            endpoint.transfer_mode,
            endpoint.channel,
            if endpoint.inherited {
                "; inherited"
            } else {
                ""
            }
        );
    }
    println!("\nRPC calls ({}):", report.rpc_calls.len());
    for call in &report.rpc_calls {
        let target = call
            .target
            .as_deref()
            .map_or(String::new(), |target| format!(" to {target}"));
        println!(
            "  {} via {}{} [{}]",
            call.method,
            call.kind,
            target,
            location_text(&call.source)
        );
    }
    println!();
    print_findings("Peer constructions", &report.peer_constructions);
    println!();
    print_findings("Multiplayer peer assignments", &report.peer_assignments);
    println!("\nConnection lifecycle uses ({}):", report.lifecycle.len());
    for finding in &report.lifecycle {
        println!(
            "  {}.{} [{}]",
            finding.signal,
            finding.operation,
            location_text(&finding.source)
        );
    }
    println!();
    print_findings("Authority and peer identity uses", &report.authority);
    println!(
        "\nScene replication nodes ({}):",
        report.replication_nodes.len()
    );
    for node in &report.replication_nodes {
        println!("  {} {} [{}]", node.kind, node.node_path, node.scene);
    }
    if !report.unknowns.is_empty() {
        println!("\nUnknown or incomplete ({}):", report.unknowns.len());
        for unknown in &report.unknowns {
            println!("  {unknown}");
        }
    }
}

fn sort_source_findings(findings: &mut [SourceFinding]) {
    findings.sort_by(|left, right| {
        left.source
            .path
            .cmp(&right.source.path)
            .then_with(|| left.source.line.cmp(&right.source.line))
            .then_with(|| left.value.cmp(&right.value))
    });
}

pub fn run(args: NetArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = engine::project_root(&args.project)?;
    let engine_path = engine::resolve(&project, args.godot.as_deref())?;
    let scripts = project_files::collect(&project, &["gd"])?;
    let scenes = project_files::collect(&project, &["tscn"])?;
    let csharp_scripts = project_files::collect(&project, &["cs"])?;
    let binary_scenes = project_files::collect(&project, &["scn"])?;
    let engine_index = query_engine(&engine_path, &project, &scripts)?;
    let mut source = SourceIndex::default();
    for path in &scripts {
        let display = resource_path(&project, path)?;
        let text =
            fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
        scan_script(&display, &text, &mut source);
    }
    if !csharp_scripts.is_empty() {
        source.unknowns.push(format!(
            "{} C# script(s) were not inspected for multiplayer declarations or calls",
            csharp_scripts.len()
        ));
    }
    if !binary_scenes.is_empty() {
        source.unknowns.push(format!(
            "{} binary scene(s) were not inspected for multiplayer replication nodes",
            binary_scenes.len()
        ));
    }
    source.unknowns.extend(engine_index.errors.iter().cloned());
    sort_source_findings(&mut source.peer_constructions);
    sort_source_findings(&mut source.peer_assignments);
    sort_source_findings(&mut source.authority);
    source.rpc_calls.sort_by(|left, right| {
        left.source
            .path
            .cmp(&right.source.path)
            .then_with(|| left.source.line.cmp(&right.source.line))
    });
    source.lifecycle.sort_by(|left, right| {
        left.source
            .path
            .cmp(&right.source.path)
            .then_with(|| left.source.line.cmp(&right.source.line))
    });
    let endpoints = rpc_endpoints(&engine_index, &source);
    let reflected_endpoints: HashSet<_> = endpoints
        .iter()
        .map(|endpoint| (endpoint.source.path.as_str(), endpoint.method.as_str()))
        .collect();
    for ((path, method), function) in &source.functions {
        if function.rpc_line.is_some()
            && !reflected_endpoints.contains(&(path.as_str(), method.as_str()))
        {
            source.unknowns.push(format!(
                "{path}:{}: @rpc method {method} was not returned by the configured engine",
                function.rpc_line.unwrap_or(function.line)
            ));
        }
    }
    let network_paths: HashSet<_> = endpoints
        .iter()
        .map(|endpoint| endpoint.source.path.as_str())
        .chain(
            source
                .rpc_calls
                .iter()
                .map(|call| call.source.path.as_str()),
        )
        .chain(
            source
                .peer_constructions
                .iter()
                .map(|finding| finding.source.path.as_str()),
        )
        .chain(
            source
                .peer_assignments
                .iter()
                .map(|finding| finding.source.path.as_str()),
        )
        .chain(
            source
                .lifecycle
                .iter()
                .map(|finding| finding.source.path.as_str()),
        )
        .chain(
            source
                .authority
                .iter()
                .map(|finding| finding.source.path.as_str()),
        )
        .collect();
    let autoloads = gdview::Project::open(&project)?
        .autoloads()?
        .entries()
        .iter()
        .map(|autoload| {
            let resolved = engine_index
                .resolved_paths
                .get(&autoload.path)
                .and_then(Value::as_str);
            NetAutoload {
                index: autoload.index,
                name: autoload.name.clone(),
                path: autoload.path.clone(),
                resolved_path: resolved.map(str::to_owned),
                singleton: autoload.singleton,
                networked: network_paths.contains(autoload.path.as_str())
                    || resolved.is_some_and(|path| network_paths.contains(path)),
            }
        })
        .collect();
    let replication_nodes = replication_nodes(&project, &scenes, &mut source.unknowns);
    let report = NetReport {
        schema_version: NET_REPORT_SCHEMA_VERSION,
        project: engine::display_path(&project),
        engine: NetEngine {
            executable: engine::display_path(&engine_path),
            version: engine_index.version,
        },
        coverage: NetCoverage {
            scripts_scanned: scripts.len(),
            scenes_scanned: scenes.len(),
            languages: vec!["GDScript".into()],
        },
        autoloads,
        rpc_endpoints: endpoints,
        rpc_calls: source.rpc_calls,
        peer_constructions: source.peer_constructions,
        peer_assignments: source.peer_assignments,
        lifecycle: source.lifecycle,
        authority: source.authority,
        replication_nodes,
        unknowns: source.unknowns,
    };
    match args.output {
        NetOutput::Human => print_human(&report),
        NetOutput::Json => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_rpc_calls_peer_lifecycle_and_authority_without_matching_strings() {
        let source = r#"extends Node

@rpc("any_peer", "call_remote", "reliable")
func request(value: int) -> void:
	print("fake.rpc_id(8)")
	if multiplayer.is_server():
		request.rpc_id(7, value)

func setup(peer: MultiplayerPeer) -> void:
	multiplayer.peer_connected.connect(_on_peer)
	multiplayer.multiplayer_peer = peer
	var offline := OfflineMultiplayerPeer.new()
"#;
        let mut index = SourceIndex::default();
        scan_script("res://network.gd", source, &mut index);
        let function = index
            .functions
            .get(&("res://network.gd".into(), "request".into()))
            .unwrap();
        assert_eq!(function.rpc_line, Some(3));
        assert_eq!(index.rpc_calls.len(), 1);
        assert_eq!(index.rpc_calls[0].method, "request");
        assert_eq!(index.rpc_calls[0].target.as_deref(), Some("7"));
        assert_eq!(index.lifecycle.len(), 1);
        assert_eq!(index.peer_assignments.len(), 1);
        assert_eq!(index.peer_constructions.len(), 1);
        assert_eq!(index.authority.len(), 1);
    }

    #[test]
    fn converts_engine_rpc_defaults_and_source_provenance() {
        let mut source = SourceIndex::default();
        source.functions.insert(
            ("res://network.gd".into(), "request".into()),
            SourceFunction {
                signature: "func request(value: int) -> void".into(),
                line: 4,
                rpc_line: Some(3),
            },
        );
        let engine = EngineIndex {
            version: "test".into(),
            scripts: vec![EngineScript {
                path: "res://network.gd".into(),
                rpc_config: Map::from_iter([(
                    "request".into(),
                    serde_json::json!({"rpc_mode": 1, "call_local": false, "transfer_mode": 2}),
                )]),
            }],
            errors: Vec::new(),
            resolved_paths: Map::new(),
        };
        let endpoints = rpc_endpoints(&engine, &source);
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].rpc_mode, "any_peer");
        assert_eq!(endpoints[0].call, "call_remote");
        assert_eq!(endpoints[0].transfer_mode, "reliable");
        assert_eq!(endpoints[0].source.line, 3);
        assert!(!endpoints[0].inherited);
    }
}
