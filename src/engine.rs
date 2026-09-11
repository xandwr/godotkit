use std::{
    env,
    error::Error,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use serde::{Deserialize, Serialize};

use crate::cli::InitArgs;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    engine: EngineConfig,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EngineConfig {
    executable: PathBuf,
}

#[derive(Deserialize)]
struct ProbeResult {
    compatible: bool,
    version: String,
}

struct ProbeDirectory(PathBuf);

impl Drop for ProbeDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn project_root(path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let root = fs::canonicalize(path)?;
    if !root.join("project.godot").is_file() {
        return Err(format!(
            "no project.godot in {}\nRun from the directory containing project.godot, or use gdkit check <project-directory> (for example: gdkit check game).",
            display_path(&root)
        ).into());
    }
    gdview::Project::open(&root)?;
    fs::read_to_string(root.join("project.godot"))?;
    Ok(root)
}

pub(crate) fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
    }
}

pub fn resolve(project: &Path, explicit: Option<&Path>) -> Result<PathBuf, Box<dyn Error>> {
    let path = if let Some(path) = explicit {
        path.to_owned()
    } else if let Some(path) = env::var_os("GDKIT_GODOT") {
        PathBuf::from(path)
    } else {
        let config_path = project.join("gdkit.toml");
        if !config_path.exists() {
            return Err("no engine configured; run gdkit init --godot <path> in the project, or supply --godot or GDKIT_GODOT".into());
        }
        let config: Config = toml::from_str(&fs::read_to_string(&config_path)?)
            .map_err(|error| format!("{}: {error}", config_path.display()))?;
        project.join(config.engine.executable)
    };
    if !path.is_file() {
        return Err(format!("Godot executable not found: {}", path.display()).into());
    }
    Ok(fs::canonicalize(path)?)
}

pub fn probe(engine: &Path) -> Result<String, Box<dyn Error>> {
    let help = Command::new(engine).arg("--help").output()?;
    let help_text = String::from_utf8_lossy(&help.stdout);
    if !help.status.success()
        || ![
            "--headless",
            "--import",
            "--script",
            "--check-only",
            "--no-header",
        ]
        .iter()
        .all(|flag| help_text.contains(flag))
    {
        return Err("engine lacks required headless editor command-line support".into());
    }
    let mut directory = None;
    for attempt in 0..100 {
        let path = env::temp_dir().join(format!("gdkit-probe-{}-{attempt}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => {
                directory = Some(ProbeDirectory(path));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let directory = directory.ok_or("could not create engine probe directory")?;
    fs::write(directory.0.join("project.godot"), "config_version=5\n")?;
    fs::write(directory.0.join("probe.gd"), include_str!("probe.gd"))?;
    fs::write(
        directory.0.join("probe.tres"),
        "[gd_resource type=\"Resource\" format=3]\n[resource]\nresource_name = \"probe\"\n",
    )?;
    fs::write(directory.0.join("check.gd"), include_str!("check.gd"))?;
    let output = Command::new(engine)
        .args(["--headless", "--path"])
        .arg(&directory.0)
        .args(["--script", "probe.gd"])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let result = stdout
        .lines()
        .find_map(|line| line.strip_prefix("GDKIT_PROBE_RESULT:"))
        .and_then(|line| serde_json::from_str::<ProbeResult>(line).ok());
    let result = match result {
        Some(result) if output.status.success() && result.compatible && !crate::check::has_errors(&output) => result,
        _ => return Err(format!("engine compatibility probe failed; requires a Godot 4 editor with GDScript and resource loading\n{}{}", stdout, String::from_utf8_lossy(&output.stderr)).into()),
    };
    let syntax = Command::new(engine)
        .args(["--headless", "--path"])
        .arg(&directory.0)
        .args(["--script", "check.gd", "--check-only"])
        .output()?;
    if !syntax.status.success() || crate::check::has_errors(&syntax) {
        return Err(format!(
            "engine cannot parse the checker harness\n{}{}",
            String::from_utf8_lossy(&syntax.stdout),
            String::from_utf8_lossy(&syntax.stderr)
        )
        .into());
    }
    Ok(result.version)
}

pub fn init(args: InitArgs) -> Result<ExitCode, Box<dyn Error>> {
    let project = project_root(&env::current_dir()?)?;
    let config_path = project.join("gdkit.toml");
    if config_path.exists() {
        return Err(
            "gdkit.toml already exists; edit its engine.executable to change engines".into(),
        );
    }
    let engine = resolve(&project, args.godot.as_deref())?;
    let version = probe(&engine)?;
    let relative = pathdiff::diff_paths(&engine, &project).unwrap_or_else(|| engine.clone());
    let config = Config {
        engine: EngineConfig {
            executable: relative,
        },
    };
    let text = toml::to_string_pretty(&config)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&config_path)?;
    file.write_all(text.as_bytes())?;
    println!("initialized {}", config_path.display());
    eprintln!("engine: {} ({version})", engine.display());
    Ok(ExitCode::SUCCESS)
}
