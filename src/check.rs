use std::{
    env,
    error::Error,
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Output},
};

use serde::Deserialize;

use crate::cli::CheckArgs;

const HARNESS: &str = include_str!("check.gd");
const RESULT_PREFIX: &str = "GDKIT_CHECK_RESULT:";

#[derive(Deserialize)]
struct HarnessResult {
    counts: Counts,
    failures: Vec<String>,
}

#[derive(Deserialize)]
struct Counts {
    scripts: usize,
    scenes: usize,
    resources: usize,
}

struct TemporaryScript(PathBuf);

impl TemporaryScript {
    fn create() -> io::Result<Self> {
        for attempt in 0..100 {
            let path =
                env::temp_dir().join(format!("gdkit-check-{}-{attempt}.gd", std::process::id()));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    file.write_all(HARNESS.as_bytes())?;
                    return Ok(Self(path));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create a temporary checker script",
        ))
    }
}

impl Drop for TemporaryScript {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn engine_output(engine: &Path, project: &Path, args: &[&OsStr]) -> io::Result<Output> {
    Command::new(engine)
        .args([OsStr::new("--headless"), OsStr::new("--no-header")])
        .arg("--path")
        .arg(project)
        .args(args)
        .output()
}

pub(crate) fn has_errors(output: &Output) -> bool {
    [output.stdout.as_slice(), output.stderr.as_slice()]
        .into_iter()
        .any(|bytes| {
            String::from_utf8_lossy(bytes).lines().any(|line| {
                let line = line.trim_start();
                line.starts_with("ERROR:") || line.starts_with("SCRIPT ERROR:")
            })
        })
}

fn write_diagnostics(output: &Output) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    for bytes in [&output.stdout, &output.stderr] {
        for line in String::from_utf8_lossy(bytes).lines() {
            if !line.starts_with(RESULT_PREFIX) && !line.is_empty() {
                writeln!(stderr, "{line}")?;
            }
        }
    }
    Ok(())
}

fn harness_result(output: &Output) -> Result<HarnessResult, Box<dyn Error>> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let result = stdout
        .lines()
        .find_map(|line| line.strip_prefix(RESULT_PREFIX))
        .ok_or("Godot checker did not return a result")?;
    Ok(serde_json::from_str(result)?)
}

pub fn run(args: CheckArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = crate::engine::project_root(&args.project)?;
    let engine = crate::engine::resolve(&project, args.godot.as_deref())?;
    let version = crate::engine::probe(&engine)?;
    eprintln!("engine: {} ({version})", engine.display());

    let import = engine_output(
        &engine,
        &project,
        &[OsStr::new("--import"), OsStr::new("--quiet")],
    )?;
    let mut failed = !import.status.success() || has_errors(&import);
    write_diagnostics(&import)?;

    let harness = TemporaryScript::create()?;
    let check = engine_output(
        &engine,
        &project,
        &[OsStr::new("--script"), harness.0.as_os_str()],
    )?;
    let check_failed = !check.status.success() || has_errors(&check);
    write_diagnostics(&check)?;
    let result = match harness_result(&check) {
        Ok(result) => result,
        Err(_) if check_failed => return Ok(ExitCode::from(1)),
        Err(error) => return Err(error),
    };
    failed |= check_failed || !result.failures.is_empty();
    for path in &result.failures {
        eprintln!("error: failed to load {path}");
    }

    println!(
        "checked {} scripts, {} scenes, {} resources",
        result.counts.scripts, result.counts.scenes, result.counts.resources
    );
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
