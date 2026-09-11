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
    #[command(about = "Query the configured engine's native API")]
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
    #[command(about = "Print a compact tree for a Godot text scene")]
    SceneTree(SceneTreeArgs),
}

#[derive(Debug, Args)]
pub struct ApiArgs {
    #[arg(value_name = "CLASS|search", help = "Native class name, or search")]
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
        conflicts_with = "fresh",
        conflicts_with_all = ["scene", "strict_methods", "smoke_frames", "smoke_timeout", "output"],
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
