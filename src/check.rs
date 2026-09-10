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
const LOCK: &str = include_str!("../godot.lock.json");
const RESULT_PREFIX: &str = "GDKIT_CHECK_RESULT:";

#[derive(Deserialize)]
struct Lock {
    version: String,
    artifacts: Artifacts,
}

#[derive(Deserialize)]
struct Artifacts {
    #[serde(rename = "windows-x86_64")]
    windows_x86_64: Artifact,
}

#[derive(Deserialize)]
struct Artifact {
    executable: String,
    version_prefix: String,
}

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

fn pinned_artifact(lock: &Lock) -> Result<&Artifact, Box<dyn Error>> {
    if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        return Err("the pinned Godot artifact only supports Windows x86_64".into());
    }
    Ok(&lock.artifacts.windows_x86_64)
}

fn find_lock_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|path| path.join("godot.lock.json").is_file())
        .map(Path::to_path_buf)
}

fn resolve_engine(explicit: Option<&Path>, lock: &Lock) -> Result<PathBuf, Box<dyn Error>> {
    let artifact = pinned_artifact(lock)?;
    let mut candidates = Vec::new();
    if let Some(path) = explicit {
        if !path.is_file() {
            return Err(format!("Godot executable not found: {}", path.display()).into());
        }
        candidates.push(path.to_path_buf());
    } else if let Some(path) = env::var_os("GDKIT_GODOT") {
        let path = PathBuf::from(path);
        if !path.is_file() {
            return Err(format!("Godot executable not found: {}", path.display()).into());
        }
        candidates.push(path);
    } else {
        if let Ok(executable) = env::current_exe()
            && let Some(directory) = executable.parent()
        {
            candidates.push(directory.join("runtime/godot").join(&artifact.executable));
            candidates.push(directory.join(&artifact.executable));
        }
        if let Ok(current) = env::current_dir()
            && let Some(root) = find_lock_root(&current)
        {
            candidates.push(
                root.join(".tools/godot")
                    .join(&lock.version)
                    .join("windows-x86_64")
                    .join(&artifact.executable),
            );
        }
        candidates.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(".tools/godot")
                .join(&lock.version)
                .join("windows-x86_64")
                .join(&artifact.executable),
        );
    }

    let path = candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or("pinned Godot is not provisioned; run scripts/provision-godot.ps1")?;
    let version = Command::new(&path).arg("--version").output()?;
    if !version.status.success() {
        return Err(format!("failed to run Godot at {}", path.display()).into());
    }
    let actual = String::from_utf8_lossy(&version.stdout);
    if !actual.starts_with(&artifact.version_prefix) {
        return Err(format!(
            "Godot version mismatch: expected {}, got {}",
            artifact.version_prefix,
            actual.trim()
        )
        .into());
    }
    Ok(path)
}

fn engine_output(engine: &Path, project: &Path, args: &[&OsStr]) -> io::Result<Output> {
    Command::new(engine)
        .args([OsStr::new("--headless"), OsStr::new("--no-header")])
        .arg("--path")
        .arg(project)
        .args(args)
        .output()
}

fn has_errors(output: &Output) -> bool {
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
    let project = fs::canonicalize(&args.project)?;
    if !project.join("project.godot").is_file() {
        return Err(format!(
            "{} is not a Godot project root: project.godot is missing",
            project.display()
        )
        .into());
    }
    let lock: Lock = serde_json::from_str(LOCK)?;
    let engine = resolve_engine(args.godot.as_deref(), &lock)?;

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
