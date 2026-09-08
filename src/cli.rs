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
    #[command(about = "Format GDScript source files")]
    Format(FormatArgs),
    #[command(about = "Format every GDScript file in the current Godot project")]
    FormatProject(FormatProjectArgs),
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
