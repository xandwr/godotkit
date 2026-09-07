mod cli;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Format => godotkit::formatter::run(),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
