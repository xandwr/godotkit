mod cli;

use std::{
    error::Error,
    ffi::OsStr,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::Parser;
use godotkit::formatter::{Options, format_source};

use cli::{Cli, Command, FormatArgs, FormatProjectArgs};

fn replace_file(path: &Path, original: &str, formatted: &str) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::other(
            "input must be a regular file, not a symlink",
        ));
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut attempt = 0;
    let (temporary, mut file) = loop {
        let temporary = parent.join(format!(".godotkit-{}-{attempt}.tmp", std::process::id()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && attempt < 100 => {
                attempt += 1
            }
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.set_permissions(metadata.permissions())?;
        file.write_all(formatted.as_bytes())?;
        file.sync_all()?;
        if fs::read_to_string(path)? != original {
            return Err(io::Error::other("input changed during formatting"));
        }
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn format(args: FormatArgs) -> Result<ExitCode, Box<dyn Error>> {
    let stdin = args.path == Path::new("-");
    let mut source = String::new();
    if stdin {
        io::stdin().read_to_string(&mut source)?;
    } else {
        source = fs::read_to_string(&args.path)?;
    }
    let options = Options {
        line_width: usize::from(args.line_width),
        ..Options::default()
    };
    let formatted = format_source(&source, &options)?;
    if args.check {
        return Ok(if source == formatted {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        });
    }
    if stdin {
        io::stdout().lock().write_all(formatted.as_bytes())?;
    } else {
        if source != formatted {
            replace_file(&args.path, &source, &formatted)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn collect_gdscript_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut directories = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_dir() {
                directories.push(path);
            } else if file_type.is_file() && path.extension() == Some(OsStr::new("gd")) {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn format_project(args: FormatProjectArgs) -> Result<ExitCode, Box<dyn Error>> {
    let root = std::env::current_dir()?;
    let project_file = root.join("project.godot");
    if !project_file.is_file() {
        return Err(format!(
            "{} is not a Godot project root: project.godot is missing or not a file",
            root.display()
        )
        .into());
    }
    let options = Options {
        line_width: usize::from(args.line_width),
        ..Options::default()
    };
    let mut changes = Vec::new();
    for path in collect_gdscript_files(&root)? {
        let source =
            fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let formatted = format_source(&source, &options)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if source != formatted {
            changes.push((path, source, formatted));
        }
    }
    if args.check {
        return Ok(if changes.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        });
    }
    for (path, source, formatted) in changes {
        replace_file(&path, &source, &formatted)?;
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Format(args) => format(args),
        Command::FormatProject(args) => format_project(args),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}
