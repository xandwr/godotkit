use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Associate the current Godot project with an engine")]
    Init(InitArgs),
    #[command(about = "Check a Godot project with its configured engine")]
    Check(CheckArgs),
    #[command(about = "Format a GDScript source file")]
    Format(FormatArgs),
    #[command(about = "Format every GDScript file in the current Godot project")]
    FormatProject(FormatProjectArgs),
    #[command(about = "Print a compact tree for a Godot text scene")]
    SceneTree(SceneTreeArgs),
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
    #[arg(default_value = ".", help = "Godot project directory")]
    pub project: PathBuf,
    #[arg(long, value_name = "PATH", help = "Use this Godot executable")]
    pub godot: Option<PathBuf>,
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
