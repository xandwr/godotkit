use crate::{
    cli::{NetOutput, ResourceArgs, ResourceCommand, ResourceCreateArgs},
    engine, process,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    error::Error,
    fs,
    path::{Component, Path, PathBuf},
    process::{Command, ExitCode},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    class: Option<String>,
    script: Option<String>,
    properties: serde_json::Map<String, Value>,
}

struct Workspace(PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn local_path(project: &Path, value: &str) -> Result<PathBuf, Box<dyn Error>> {
    let relative = value
        .strip_prefix("res://")
        .ok_or("path must start with res://")?;
    if relative.is_empty()
        || relative.contains(['\\', ':'])
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || Path::new(relative)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("path must be a project-local resource path without traversal".into());
    }
    let path = project.join(relative);
    let mut ancestor = path.as_path();
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or("invalid resource path")?;
    }
    if !fs::canonicalize(ancestor)?.starts_with(project) {
        return Err("resource path resolves outside project".into());
    }
    Ok(path)
}

fn create(args: ResourceCreateArgs) -> Result<Value, Box<dyn Error>> {
    let project = engine::project_root(&args.project)?;
    let destination = local_path(&project, &args.out)?;
    if destination.extension().and_then(|s| s.to_str()) != Some("tres") {
        return Err("destination must end in .tres".into());
    }
    if fs::symlink_metadata(&destination).is_ok() {
        return Err("destination already exists".into());
    }
    let parent = destination.parent().ok_or("invalid destination")?;
    if !parent.is_dir() {
        return Err("destination parent directory must exist".into());
    }
    let text = fs::read_to_string(&args.spec)?;
    let spec: Spec = serde_json::from_str(&text)?;
    match (&spec.class, &spec.script) {
        (Some(name), None) if !name.is_empty() => {}
        (None, Some(path)) => {
            local_path(&project, path)?;
            if !path.ends_with(".gd") {
                return Err("script must be a .gd resource path".into());
            }
        }
        _ => return Err("spec requires exactly one nonempty class or script".into()),
    }
    for (field, value) in &spec.properties {
        if !matches!(value, Value::Bool(_) | Value::Number(_) | Value::String(_)) {
            return Ok(
                json!({"status": "error", "stage": "validate", "field": field,
                "message": "Only boolean, integer, float, and string values are supported"}),
            );
        }
        if let Some(number) = value.as_number()
            && (number.is_i64() || number.is_u64())
            && number
                .as_f64()
                .is_none_or(|v| v.abs() > 9_007_199_254_740_991.0)
        {
            return Ok(
                json!({"status": "error", "stage": "validate", "field": field,
                "message": "Integer exceeds exact JSON transport range"}),
            );
        }
    }
    let executable = engine::resolve(&project, args.godot.as_deref())?;
    engine::validated_version(&executable, &project)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let workspace =
        Workspace(parent.join(format!(".gdkit-resource-{}-{nonce}", std::process::id())));
    fs::create_dir(&workspace.0)?;
    let script = workspace.0.join("worker.gd");
    let request = workspace.0.join("request.json");
    let staged = workspace.0.join("resource.tres");
    fs::write(&script, include_bytes!("resource.gd"))?;
    fs::write(&request, text)?;
    let captured = process::run(
        Command::new(executable)
            .args(["--headless", "--no-header", "--path"])
            .arg(&project)
            .arg("--script")
            .arg(&script)
            .arg("--")
            .arg(&request)
            .arg(&staged),
        Some(Duration::from_secs(60)),
    )?;
    let stdout = String::from_utf8_lossy(&captured.output.stdout);
    let stderr = String::from_utf8_lossy(&captured.output.stderr);
    let mut result: Value = stdout
        .lines()
        .find_map(|line| line.strip_prefix("GDKIT_RESOURCE_RESULT:"))
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_else(|| {
            json!({"status": "error", "stage": "construct", "field": "",
            "message": format!("Resource worker did not complete\n{stdout}{stderr}")})
        });
    if captured.timed_out
        || !captured.output.status.success()
        || result["status"] != "created"
        || stderr.contains("SCRIPT ERROR:")
        || stderr.contains("ERROR:")
        || stderr.contains("WARNING:")
        || stdout.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("ERROR:")
                || line.starts_with("SCRIPT ERROR:")
                || line.starts_with("WARNING:")
        })
    {
        if result["status"] == "error" {
            if !stderr.trim().is_empty() {
                result["diagnostics"] = json!(stderr);
            }
            return Ok(result);
        }
        return Ok(json!({"status": "error", "stage": "verify", "field": "",
            "message": format!("Resource worker failed\n{stdout}{stderr}")}));
    }
    if !stderr.trim().is_empty() {
        eprint!("{stderr}");
    }
    if let Err(error) = fs::hard_link(&staged, &destination) {
        return Ok(json!({"status": "error", "stage": "publish", "field": "",
            "message": error.to_string()}));
    }
    result["path"] = json!(args.out);
    Ok(result)
}

pub(crate) fn run(args: ResourceArgs) -> Result<ExitCode, Box<dyn Error>> {
    let ResourceCommand::Create(args) = args.command;
    let output = args.output;
    let result = create(args).unwrap_or_else(|error| json!({"status": "error", "stage": "prepare", "field": "", "message": error.to_string()}));
    match output {
        NetOutput::Json => println!("{}", serde_json::to_string(&result)?),
        NetOutput::Human => {
            if result["status"] == "created" {
                println!(
                    "Created {} ({})\nVerified properties: {}",
                    result["path"].as_str().unwrap_or_default(),
                    result["type"].as_str().unwrap_or_default(),
                    result["properties"]
                );
                if let Some(script) = result["script"].as_str() {
                    println!("Script: {script}");
                }
            } else {
                eprintln!(
                    "Resource creation failed [{}] {}: {}",
                    result["stage"].as_str().unwrap_or_default(),
                    result["field"].as_str().unwrap_or_default(),
                    result["message"].as_str().unwrap_or_default()
                );
                if let Some(diagnostics) = result["diagnostics"].as_str() {
                    eprint!("{diagnostics}");
                }
            }
        }
    }
    Ok(if result["status"] == "created" {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}
