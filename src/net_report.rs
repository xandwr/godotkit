use serde::{Deserialize, Serialize};

pub const NET_REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NetReport {
    pub schema_version: u32,
    pub project: String,
    pub engine: NetEngine,
    pub coverage: NetCoverage,
    pub autoloads: Vec<NetAutoload>,
    pub rpc_endpoints: Vec<RpcEndpoint>,
    pub rpc_calls: Vec<RpcCall>,
    pub peer_constructions: Vec<SourceFinding>,
    pub peer_assignments: Vec<SourceFinding>,
    pub lifecycle: Vec<LifecycleFinding>,
    pub authority: Vec<SourceFinding>,
    pub replication_nodes: Vec<ReplicationNode>,
    pub unknowns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NetEngine {
    pub executable: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NetCoverage {
    pub scripts_scanned: usize,
    pub scenes_scanned: usize,
    pub languages: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NetAutoload {
    pub index: usize,
    pub name: String,
    pub path: String,
    pub resolved_path: Option<String>,
    pub singleton: bool,
    pub networked: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RpcEndpoint {
    pub method: String,
    pub signature: Option<String>,
    pub source: SourceLocation,
    pub rpc_mode: String,
    pub call: String,
    pub transfer_mode: String,
    pub channel: i64,
    pub inherited: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RpcCall {
    pub method: String,
    pub kind: String,
    pub target: Option<String>,
    pub source: SourceLocation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SourceFinding {
    pub value: String,
    pub source: SourceLocation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LifecycleFinding {
    pub signal: String,
    pub operation: String,
    pub source: SourceLocation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReplicationNode {
    pub kind: String,
    pub node_path: String,
    pub scene: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SourceLocation {
    pub path: String,
    pub line: usize,
}
