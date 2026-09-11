use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Query the configured engine and project's API")]
    Api(ApiArgs),
    #[command(about = "Print autoload initialization order")]
    Autoloads(AutoloadsArgs),
    #[command(about = "Associate the current Godot project with an engine")]
    Init(InitArgs),
    #[command(about = "Check a Godot project with its configured engine")]
    Check(CheckArgs),
    #[command(about = "Explain a Godot project's gdkit environment")]
    Doctor(DoctorArgs),
    #[command(about = "Format a GDScript source file")]
    Format(FormatArgs),
    #[command(about = "Format every GDScript file in the current Godot project")]
    FormatProject(FormatProjectArgs),
    #[command(about = "Inspect a Godot project's multiplayer topology")]
    Net(NetArgs),
    #[command(about = "Print a compact tree for a Godot text scene")]
    SceneTree(SceneTreeArgs),
    #[command(about = "Launch a durable named Godot session")]
    Run(RunArgs),
    #[command(about = "List named Godot sessions")]
    Sessions(SessionsArgs),
    #[command(about = "Print a named session's log")]
    Logs(SessionArgs),
    #[command(about = "Stop a named Godot session")]
    Stop(SessionArgs),
    #[command(about = "Restart a named Godot session")]
    Restart(SessionArgs),
}

#[derive(Debug, Args)]
pub struct NetArgs {
    #[command(subcommand)]
    pub command: Option<NetCommand>,
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value = "human",
        help = "Select human or JSON result output"
    )]
    pub output: NetOutput,
}

#[derive(Debug, Subcommand)]
pub enum NetCommand {
    #[command(about = "Explain matching RPC and replication contracts")]
    Explain(NetExplainArgs),
}

#[derive(Debug, Args)]
pub struct NetExplainArgs {
    #[arg(help = "RPC method, receiver.method, scene, or replication node")]
    pub query: String,
    #[arg(long, default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value = "human",
        help = "Select human or JSON result output"
    )]
    pub output: NetOutput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum NetOutput {
    Human,
    Json,
}

#[derive(Debug, Args)]
pub struct ApiArgs {
    #[arg(
        value_name = "CLASS|search",
        help = "Native or project class name, or search"
    )]
    pub query: String,
    #[arg(value_name = "MEMBER|TERM", help = "Member name, or search term")]
    pub member: Option<String>,
    #[arg(long, default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct AutoloadsArgs {
    #[arg(default_value = ".", help = "Godot project or a path inside it")]
    pub path: PathBuf,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Godot editor executable to associate"
    )]
    pub godot: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    #[arg(
        long,
        value_enum,
        default_value = "human",
        help = "Select human or JSON result output"
    )]
    pub output: CheckOutput,
    #[arg(long, help = "Reject method calls not guaranteed by the receiver type")]
    pub strict_methods: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Smoke-test this scene after validation (repeatable; executes gameplay)"
    )]
    pub scene: Vec<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Run this project-owned SceneTree script after validation (repeatable)"
    )]
    pub script: Vec<String>,
    #[arg(long, requires = "script", default_value = "30", value_parser = clap::value_parser!(u64).range(1..=3600), help = "Wall-clock seconds allowed per project script")]
    pub script_timeout: u64,
    #[arg(long, requires = "scene", default_value = "2", value_parser = clap::value_parser!(u32).range(1..), help = "Process frames per smoke scene")]
    pub smoke_frames: u32,
    #[arg(long, requires = "scene", default_value = "30", value_parser = clap::value_parser!(u64).range(1..=3600), help = "Wall-clock seconds allowed per smoke scene")]
    pub smoke_timeout: u64,
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
    #[arg(long, help = "Show full Godot output, including engine stack traces")]
    pub verbose: bool,
    #[arg(
        long,
        help = "Show file scan, engine validation, import, resource loading, and total times"
    )]
    pub timings: bool,
    #[arg(
        long,
        help = "Use fresh Godot processes, including a full editor import"
    )]
    pub fresh: bool,
    #[arg(
        long,
        help = "Copy the project to a temporary directory and perform a fresh check there"
    )]
    pub isolated: bool,
    #[arg(
        long,
        conflicts_with = "fresh",
        conflicts_with_all = ["scene", "script", "strict_methods", "smoke_frames", "smoke_timeout", "script_timeout", "isolated", "output"],
        help = "Stop this project's background import editor without checking"
    )]
    pub stop_worker: bool,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum CheckOutput {
    Human,
    Json,
}

#[derive(Debug, Args)]
pub struct SceneTreeArgs {
    #[arg(help = "Godot text scene to inspect")]
    pub path: PathBuf,
    #[arg(long, help = "Show outgoing signal connections under each node")]
    pub connections: bool,
    #[arg(long, help = "Show saved group memberships under each node")]
    pub groups: bool,
    #[arg(
        long,
        conflicts_with = "expand_depth",
        help = "Recursively expand scene instances"
    )]
    pub expand: bool,
    #[arg(long, value_name = "DEPTH", value_parser = clap::value_parser!(u8).range(1..=64), help = "Expand scene instances up to this depth")]
    pub expand_depth: Option<u8>,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, help = "Durable session name")]
    pub name: String,
    #[arg(long, help = "Run without a window")]
    pub headless: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Scene to launch instead of the project's main scene"
    )]
    pub scene: Option<String>,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
    #[arg(
        last = true,
        allow_hyphen_values = true,
        help = "Arguments passed to the project after --"
    )]
    pub arguments: Vec<String>,
}

#[derive(Debug, Args)]
pub struct SessionsArgs {
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, help = "Include superseded session generations")]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct SessionArgs {
    #[arg(help = "Session name, or name@generation")]
    pub session: String,
    #[arg(long, default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
}

#[derive(Debug, Args)]
pub struct FormatArgs {
    #[arg(default_value = "-", help = "Input file, or - for stdin")]
    pub path: PathBuf,
    #[arg(long, help = "Exit 1 if formatting would change the input")]
    pub check: bool,
    #[arg(long, default_value = "100", value_parser = clap::value_parser!(u16).range(1..), help = "Maximum width of compact guards")]
    pub line_width: u16,
}

#[derive(Debug, Args)]
pub struct FormatProjectArgs {
    #[arg(long, help = "Exit 1 if formatting would change any file")]
    pub check: bool,
    #[arg(long, default_value = "100", value_parser = clap::value_parser!(u16).range(1..), help = "Maximum width of compact guards")]
    pub line_width: u16,
}
