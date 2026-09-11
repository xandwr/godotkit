use std::{
    collections::hash_map::RandomState,
    error::Error,
    fs::{self, File},
    hash::{BuildHasher, Hasher},
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitCode, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::cli::{InspectArgs, NetOutput};

const REQUEST_SCHEMA_VERSION: u32 = 1;
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProbeEndpoint {
    port: u16,
    token: String,
    debugger_port: u16,
    debugger_token: String,
    debugger_pid: u32,
    debugger_process_started: u64,
}

pub(crate) struct PreparedProbe {
    pub(crate) script: PathBuf,
    pub(crate) ready: PathBuf,
    pub(crate) token: String,
    generation: String,
    debugger_project: PathBuf,
    debugger_script: PathBuf,
    debugger_ready: PathBuf,
    debugger_token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    schema_version: u32,
    generation: String,
    port: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DebuggerReady {
    schema_version: u32,
    generation: String,
    debugger_port: u16,
    query_port: u16,
}

pub(crate) struct DebuggerProcess {
    pub(crate) child: Child,
    pub(crate) debugger_port: u16,
    query_port: u16,
    token: String,
    process_started: u64,
}

#[derive(Serialize)]
struct Request<'a> {
    schema_version: u32,
    request_id: String,
    generation: &'a str,
    token: &'a str,
    kind: &'a str,
    deadline_unix_ms: u64,
    max_response_bytes: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Response {
    schema_version: u32,
    request_id: String,
    #[serde(default)]
    session: String,
    generation: String,
    observation: NetworkObservation,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NetworkObservation {
    collected_at_unix_ms: u64,
    process_tick: u64,
    physics_tick: u64,
    multiplayer_roots: Vec<MultiplayerRoot>,
    node_authorities: Vec<NodeAuthority>,
    spawners: Vec<SpawnerState>,
    synchronizers: Vec<SynchronizerState>,
    recent_events: Vec<NetworkEvent>,
    truncated: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MultiplayerRoot {
    root: String,
    api_class: String,
    peer_class: String,
    connection_status: String,
    local_peer_id: i64,
    is_server: bool,
    connected_peers: Vec<i64>,
    authenticating_peers: Vec<i64>,
    configuration: SceneMultiplayerConfiguration,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SceneMultiplayerConfiguration {
    root_path: String,
    refusing_new_connections: Option<bool>,
    object_decoding_allowed: Option<bool>,
    server_relay_enabled: Option<bool>,
    auth_timeout: Option<f64>,
    max_sync_packet_size: Option<i64>,
    max_delta_packet_size: Option<i64>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NodeAuthority {
    path: String,
    authority: i64,
    local_authority: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SpawnerState {
    path: String,
    authority: i64,
    spawn_path: String,
    spawn_limit: i64,
    spawn_path_child_count: i64,
    spawnable_scenes: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SynchronizerState {
    path: String,
    authority: i64,
    root_path: String,
    replication_interval: f64,
    delta_interval: f64,
    visibility_update_mode: i64,
    public_visibility: bool,
    properties: Vec<ReplicationProperty>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReplicationProperty {
    path: String,
    spawn: bool,
    sync: bool,
    watch: bool,
    mode: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NetworkEvent {
    kind: String,
    collected_at_unix_ms: u64,
    process_tick: u64,
    api_instance_id: i64,
    peer_id: Option<i64>,
    direction: Option<String>,
    node_path: Option<String>,
    bytes: Option<i64>,
    count: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RpcResponse {
    schema_version: u32,
    request_id: String,
    generation: String,
    recent_events: Vec<NetworkEvent>,
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn token() -> String {
    format!(
        "{:016x}{:016x}",
        RandomState::new().build_hasher().finish(),
        RandomState::new().build_hasher().finish()
    )
}

pub(crate) fn prepare(project: &Path, generation: &str) -> Result<PreparedProbe, Box<dyn Error>> {
    let directory = project.join(".godot/gdkit/sessions/probe");
    fs::create_dir_all(&directory)?;
    let script = directory.join("runtime-probe.gd");
    fs::write(&script, include_str!("runtime_probe.gd"))?;
    let probe_token = token();
    let ready = directory.join(format!("ready-{generation}-{probe_token}.json"));
    let debugger_project = directory.join("debugger");
    fs::create_dir_all(&debugger_project)?;
    fs::write(debugger_project.join("project.godot"), "config_version=5\n")?;
    let debugger_script = debugger_project.join("debugger-bridge.gd");
    fs::write(&debugger_script, include_str!("debugger_bridge.gd"))?;
    let debugger_token = token();
    let debugger_ready =
        directory.join(format!("debugger-ready-{generation}-{debugger_token}.json"));
    Ok(PreparedProbe {
        script,
        ready,
        token: probe_token,
        generation: generation.to_owned(),
        debugger_project,
        debugger_script,
        debugger_ready,
        debugger_token,
    })
}

pub(crate) fn start_debugger(
    prepared: &PreparedProbe,
    engine: &Path,
    stdout: File,
    stderr: File,
) -> Result<DebuggerProcess, Box<dyn Error>> {
    let mut command = Command::new(engine);
    command
        .args(["--headless", "--no-header", "--quiet", "--path"])
        .arg(&prepared.debugger_project)
        .arg("--script")
        .arg(&prepared.debugger_script)
        .env("GDKIT_DEBUGGER_GENERATION", &prepared.generation)
        .env("GDKIT_DEBUGGER_TOKEN", &prepared.debugger_token)
        .env("GDKIT_DEBUGGER_READY", &prepared.debugger_ready)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = crate::session::spawn_session(&mut command)?;
    let began = Instant::now();
    loop {
        if let Ok(bytes) = fs::read(&prepared.debugger_ready)
            && let Ok(ready) = serde_json::from_slice::<DebuggerReady>(&bytes)
        {
            let _ = fs::remove_file(&prepared.debugger_ready);
            if ready.schema_version != REQUEST_SCHEMA_VERSION
                || ready.generation != prepared.generation
                || ready.debugger_port == 0
                || ready.query_port == 0
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err("runtime debugger returned an invalid readiness record".into());
            }
            let process_started = match crate::session::process_started(child.id()) {
                Ok(started) => started,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error.into());
                }
            };
            return Ok(DebuggerProcess {
                child,
                debugger_port: ready.debugger_port,
                query_port: ready.query_port,
                token: prepared.debugger_token.clone(),
                process_started,
            });
        }
        if child.try_wait()?.is_some() {
            let _ = fs::remove_file(&prepared.debugger_ready);
            return Err("runtime debugger exited before becoming ready".into());
        }
        if began.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&prepared.debugger_ready);
            return Err("runtime debugger startup timed out".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn await_ready(
    prepared: &PreparedProbe,
    child: &mut Child,
    debugger: &DebuggerProcess,
) -> Result<ProbeEndpoint, Box<dyn Error>> {
    let began = Instant::now();
    loop {
        if let Ok(bytes) = fs::read(&prepared.ready)
            && let Ok(ready) = serde_json::from_slice::<Ready>(&bytes)
        {
            let _ = fs::remove_file(&prepared.ready);
            if ready.schema_version != REQUEST_SCHEMA_VERSION
                || ready.generation != prepared.generation
                || ready.port == 0
            {
                return Err("runtime probe returned an invalid readiness record".into());
            }
            return Ok(ProbeEndpoint {
                port: ready.port,
                token: prepared.token.clone(),
                debugger_port: debugger.query_port,
                debugger_token: debugger.token.clone(),
                debugger_pid: debugger.child.id(),
                debugger_process_started: debugger.process_started,
            });
        }
        if child.try_wait()?.is_some() {
            let _ = fs::remove_file(&prepared.ready);
            return Err("runtime probe exited before becoming ready".into());
        }
        if began.elapsed() > Duration::from_secs(15) {
            let _ = fs::remove_file(&prepared.ready);
            return Err("runtime probe startup timed out".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn stop(endpoint: &ProbeEndpoint) -> Result<bool, Box<dyn Error>> {
    Ok(crate::session::terminate_process(
        endpoint.debugger_pid,
        endpoint.debugger_process_started,
    )?)
}

fn send_request<T: for<'de> Deserialize<'de>>(
    port: u16,
    endpoint_token: &str,
    generation: &str,
    kind: &str,
) -> Result<(String, T), Box<dyn Error>> {
    let request_id = token();
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(500))?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    serde_json::to_writer(
        &mut stream,
        &Request {
            schema_version: REQUEST_SCHEMA_VERSION,
            request_id: request_id.clone(),
            generation,
            token: endpoint_token,
            kind,
            deadline_unix_ms: timestamp() + 2500,
            max_response_bytes: MAX_RESPONSE_BYTES,
        },
    )?;
    stream.write_all(b"\n")?;
    let mut response = String::new();
    let read = BufReader::new(stream)
        .take(MAX_RESPONSE_BYTES + 1)
        .read_line(&mut response)?;
    if read == 0 {
        return Err("runtime probe closed without a response".into());
    }
    if read as u64 > MAX_RESPONSE_BYTES || !response.ends_with('\n') {
        return Err("runtime probe response exceeded its declared bound".into());
    }
    Ok((request_id, serde_json::from_str(&response)?))
}

fn request(endpoint: &ProbeEndpoint, generation: &str) -> Result<Response, Box<dyn Error>> {
    let (request_id, mut response): (String, Response) = send_request(
        endpoint.port,
        &endpoint.token,
        generation,
        "network_observation",
    )?;
    if response.schema_version != REQUEST_SCHEMA_VERSION
        || response.request_id != request_id
        || response.generation != generation
    {
        return Err("runtime probe response did not match the request".into());
    }
    let (rpc_request_id, rpc): (String, RpcResponse) = send_request(
        endpoint.debugger_port,
        &endpoint.debugger_token,
        generation,
        "rpc_events",
    )?;
    if rpc.schema_version != REQUEST_SCHEMA_VERSION
        || rpc.request_id != rpc_request_id
        || rpc.generation != generation
    {
        return Err("runtime debugger response did not match the request".into());
    }
    response.observation.recent_events.extend(rpc.recent_events);
    response
        .observation
        .recent_events
        .sort_by_key(|event| event.collected_at_unix_ms);
    if response.observation.recent_events.len() > 256 {
        response.observation.recent_events = response
            .observation
            .recent_events
            .split_off(response.observation.recent_events.len() - 256);
        response.observation.truncated = true;
    }
    Ok(response)
}

pub(crate) fn inspect(args: InspectArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let Some(record) = crate::session::select_record(&project, &args.session)? else {
        eprintln!("error: no session matches '{}'", args.session);
        return Ok(ExitCode::from(1));
    };
    if !crate::session::is_running(&record) {
        eprintln!(
            "error: {}@{} is not running",
            record.name, record.generation
        );
        return Ok(ExitCode::from(1));
    }
    let Some(endpoint) = &record.probe else {
        eprintln!(
            "error: {}@{} predates runtime probe support; restart it first",
            record.name, record.generation
        );
        return Ok(ExitCode::from(1));
    };
    let mut response = request(endpoint, &record.generation)?;
    response.session = record.name.clone();
    match args.output {
        NetOutput::Json => println!("{}", serde_json::to_string_pretty(&response)?),
        NetOutput::Human => print_human(&response),
    }
    Ok(ExitCode::SUCCESS)
}

fn print_human(response: &Response) {
    let observation = &response.observation;
    println!("session: {}@{}", response.session, response.generation);
    println!("collected: {} ms", observation.collected_at_unix_ms);
    println!(
        "ticks: process {} physics {}",
        observation.process_tick, observation.physics_tick
    );
    println!("multiplayer roots:");
    for root in &observation.multiplayer_roots {
        println!(
            "  {}: {} / {} ({}) local={} server={} peers={:?} authenticating={:?}",
            root.root,
            root.api_class,
            root.peer_class,
            root.connection_status,
            root.local_peer_id,
            root.is_server,
            root.connected_peers,
            root.authenticating_peers
        );
        println!(
            "    config: root={} refuse={:?} object_decoding={:?} relay={:?} auth_timeout={:?} packet_sizes={:?}/{:?}",
            root.configuration.root_path,
            root.configuration.refusing_new_connections,
            root.configuration.object_decoding_allowed,
            root.configuration.server_relay_enabled,
            root.configuration.auth_timeout,
            root.configuration.max_sync_packet_size,
            root.configuration.max_delta_packet_size
        );
    }
    println!("node authorities:");
    for node in &observation.node_authorities {
        println!(
            "  {}: {}{}",
            node.path,
            node.authority,
            if node.local_authority { " (local)" } else { "" }
        );
    }
    println!("spawners:");
    for spawner in &observation.spawners {
        println!(
            "  {}: authority={} spawn_path={} children={} limit={} scenes={:?}",
            spawner.path,
            spawner.authority,
            spawner.spawn_path,
            spawner.spawn_path_child_count,
            spawner.spawn_limit,
            spawner.spawnable_scenes
        );
    }
    println!("synchronizers:");
    for synchronizer in &observation.synchronizers {
        println!(
            "  {}: authority={} root={} interval={}/{} visibility={}/{} properties={}",
            synchronizer.path,
            synchronizer.authority,
            synchronizer.root_path,
            synchronizer.replication_interval,
            synchronizer.delta_interval,
            synchronizer.visibility_update_mode,
            synchronizer.public_visibility,
            synchronizer.properties.len()
        );
        for property in &synchronizer.properties {
            println!(
                "    {}: spawn={} sync={} watch={} mode={}",
                property.path, property.spawn, property.sync, property.watch, property.mode
            );
        }
    }
    println!("recent events:");
    for event in &observation.recent_events {
        println!(
            "  tick {} {} api={} peer={:?} direction={:?} node={:?} count={:?} bytes={:?}",
            event.process_tick,
            event.kind,
            event.api_instance_id,
            event.peer_id,
            event.direction,
            event.node_path,
            event.count,
            event.bytes
        );
    }
    if observation.truncated {
        println!("observation truncated to the requested response bound");
    }
}
