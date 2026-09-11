use std::{
    env,
    error::Error,
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Output},
    time::Instant,
};

use serde::Deserialize;

use crate::cli::CheckArgs;

const HARNESS: &str = include_str!("check.gd");
const RESULT_PREFIX: &str = "GDKIT_CHECK_RESULT:";

struct PhaseTimer {
    start: Instant,
    name: &'static str,
    enabled: bool,
}

impl PhaseTimer {
    fn new(name: &'static str, enabled: bool) -> Self {
        Self {
            start: Instant::now(),
            name,
            enabled,
        }
    }
}

impl Drop for PhaseTimer {
    fn drop(&mut self) {
        if self.enabled {
            let _ = writeln!(
                io::stderr().lock(),
                "timing: {} {:.1} ms",
                self.name,
                self.start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
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
    fn create(contents: &[u8], extension: &str) -> io::Result<Self> {
        for attempt in 0..100 {
            let path = env::temp_dir().join(format!(
                "gdkit-check-{}-{attempt}.{extension}",
                std::process::id()
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let temporary = Self(path);
                    file.write_all(contents)?;
                    return Ok(temporary);
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

#[derive(Default)]
struct Diagnostics {
    issues: Vec<String>,
    cleanup: Vec<String>,
}

fn diagnostics(output: &Output) -> Diagnostics {
    let mut result = Diagnostics::default();
    for bytes in [&output.stdout, &output.stderr] {
        for line in String::from_utf8_lossy(bytes).lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(RESULT_PREFIX) {
                continue;
            }
            if line.starts_with("at:") && !line.contains("res://") {
                continue;
            }
            let cleanup = (line.contains("RID allocations of type")
                && line.ends_with("were leaked at exit."))
                || (line.contains("RIDs of type") && line.ends_with("were leaked."))
                || (line.starts_with("WARNING: ObjectDB instances leaked at exit"))
                || (line.contains("ObjectDB instances were leaked at exit"))
                || (line.starts_with("ERROR:") && line.contains("resources still in use at exit"));
            let target = if cleanup {
                &mut result.cleanup
            } else {
                &mut result.issues
            };
            if !target.iter().any(|existing| existing == line) {
                target.push(line.to_owned());
            }
        }
    }
    result
}

fn unresolved_uid(line: &str) -> Option<&str> {
    let rest = line.split_once("Unrecognized UID: \"")?.1;
    rest.split_once('"').map(|(uid, _)| uid)
}

fn cleanup_message(line: &str) -> String {
    let severity = line.split_once(':').map_or("WARNING", |(value, _)| value);
    let count = line.split_whitespace().nth(1).unwrap_or("");
    let kind = if line.contains("DummyTexture") {
        Some("headless renderer textures")
    } else if line.contains("ShapedTextData") {
        Some("text layout buffers")
    } else if line.contains("FontLinkedVariation") {
        Some("font variations")
    } else if line.contains("Font") && line.contains("RID allocations") {
        Some("fonts")
    } else if line.contains("CanvasItem") {
        Some("2D canvas items")
    } else if line.contains("resources still in use at exit") {
        Some("resources")
    } else if line.contains("ObjectDB") && count.parse::<usize>().is_ok() {
        Some("objects")
    } else {
        None
    };
    match kind {
        Some(kind) => format!("{severity}: {count} {kind} still allocated at shutdown."),
        None if line.contains("ObjectDB") => format!(
            "{severity}: Objects still allocated at shutdown (Godot did not report a count)."
        ),
        None => line.to_owned(),
    }
}

fn uid_references(project: &Path, paths: &[String], uid: &str) -> Vec<String> {
    let mut references = Vec::new();
    for path in std::iter::once("res://project.godot").chain(paths.iter().map(String::as_str)) {
        let local = project.join(path.trim_start_matches("res://"));
        let Ok(source) = fs::read_to_string(local) else {
            continue;
        };
        for (index, line) in source.lines().enumerate() {
            if line.match_indices(uid).any(|(offset, _)| {
                !line[offset + uid.len()..].starts_with(|c: char| c.is_ascii_alphanumeric())
            }) {
                references.push(format!("{path}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    references
}

fn write_diagnostics(
    output: &Output,
    phase: &str,
    project: &Path,
    paths: &[String],
    verbose: bool,
) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    let report = diagnostics(output);
    if !report.issues.is_empty() {
        writeln!(stderr, "\n{phase} diagnostics:")?;
        for line in &report.issues {
            writeln!(stderr, "  {line}")?;
            if let Some(uid) = unresolved_uid(line) {
                writeln!(
                    stderr,
                    "    Godot cannot resolve this resource ID to a file."
                )?;
                let references = uid_references(project, paths, uid);
                if references.is_empty() {
                    writeln!(
                        stderr,
                        "    No reference found in checked text files or project.godot; it may come from a dependency or binary resource."
                    )?;
                } else {
                    writeln!(stderr, "    References (verify which one needs repair):")?;
                    for reference in references {
                        writeln!(stderr, "      {reference}")?;
                    }
                }
                writeln!(
                    stderr,
                    "    Reassign stale references to the intended resource in Godot and save the affected file."
                )?;
            }
        }
    }
    if !report.cleanup.is_empty() {
        writeln!(stderr, "\n{phase} shutdown cleanup diagnostics:")?;
        writeln!(
            stderr,
            "  Godot reported objects or resources still allocated when the headless process exited."
        )?;
        writeln!(
            stderr,
            "  These messages do not identify a source file; ERROR entries still fail this check."
        )?;
        for line in &report.cleanup {
            writeln!(stderr, "  {}", cleanup_message(line))?;
        }
        writeln!(
            stderr,
            "  Use gdkit check --verbose for the original engine messages and stack traces."
        )?;
    }
    if !output.status.success() {
        writeln!(
            stderr,
            "\n{phase}: Godot process exited with {}.",
            output.status
        )?;
    }
    if verbose {
        writeln!(stderr, "\n{phase} full Godot output:")?;
        for bytes in [&output.stdout, &output.stderr] {
            for line in String::from_utf8_lossy(bytes).lines() {
                if !line.starts_with(RESULT_PREFIX) {
                    writeln!(stderr, "{line}")?;
                }
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

fn print_summary(failed: bool, counts: Option<&Counts>) -> io::Result<()> {
    let (color, status) = if failed {
        (31, "check failed")
    } else {
        (32, "check passed")
    };
    let mut output = anstream::stdout();
    write!(output, "\x1b[{color}m{status}")?;
    if let Some(counts) = counts {
        write!(
            output,
            ": checked {} scripts, {} scenes, {} resources",
            counts.scripts, counts.scenes, counts.resources
        )?;
    }
    writeln!(output, "\x1b[0m")
}

pub fn run(args: CheckArgs) -> Result<ExitCode, Box<dyn Error>> {
    let _total = PhaseTimer::new("total", args.timings);
    let scan_timer = PhaseTimer::new("file scan", args.timings);
    let project = crate::engine::project_root(&args.project)?;
    if args.stop_worker {
        crate::import_worker::stop_project(&project)?;
        println!("import worker stopped");
        return Ok(ExitCode::SUCCESS);
    }
    let paths =
        crate::project_files::collect(&project, &["gd", "tscn", "scn", "tres", "res", "gdshader"])?;
    let paths = paths
        .iter()
        .map(|path| {
            Ok(format!(
                "res://{}",
                path.strip_prefix(&project)?
                    .to_str()
                    .ok_or("resource path is not valid UTF-8")?
                    .replace('\\', "/")
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let manifest = TemporaryScript::create(&serde_json::to_vec(&paths)?, "json")?;
    drop(scan_timer);
    let mut validation_timer = PhaseTimer::new("engine validation (probe)", args.timings);
    let engine = crate::engine::resolve(&project, args.godot.as_deref())?;
    let (version, cached) = crate::engine::validated_version(&engine, &project)?;
    if cached {
        validation_timer.name = "engine validation (cached)";
    }
    drop(validation_timer);
    eprintln!(
        "engine: {} ({version})",
        crate::engine::display_path(&engine)
    );

    let mut import_timer = PhaseTimer::new("import", args.timings);
    let import = if args.fresh {
        crate::import_worker::stop_project(&project)?;
        engine_output(
            &engine,
            &project,
            &[OsStr::new("--import"), OsStr::new("--quiet")],
        )?
    } else {
        match crate::import_worker::import(&project, &engine) {
            Ok((output, reused)) => {
                import_timer.name = if reused {
                    "import (warm worker)"
                } else {
                    "import (worker startup)"
                };
                output
            }
            Err(error) => {
                crate::import_worker::stop_project(&project)?;
                eprintln!("warning: {error}; falling back to a fresh import");
                engine_output(
                    &engine,
                    &project,
                    &[OsStr::new("--import"), OsStr::new("--quiet")],
                )?
            }
        }
    };
    drop(import_timer);
    let mut failed = !import.status.success() || has_errors(&import);
    write_diagnostics(&import, "Import", &project, &paths, args.verbose)?;

    let harness = TemporaryScript::create(HARNESS.as_bytes(), "gd")?;
    let loading_timer = PhaseTimer::new("resource loading", args.timings);
    let check = engine_output(
        &engine,
        &project,
        &[
            OsStr::new("--script"),
            harness.0.as_os_str(),
            OsStr::new("--"),
            manifest.0.as_os_str(),
        ],
    )?;
    drop(loading_timer);
    let check_failed = !check.status.success() || has_errors(&check);
    write_diagnostics(&check, "Resource loading", &project, &paths, args.verbose)?;
    let result = match harness_result(&check) {
        Ok(result) => result,
        Err(error) if check_failed => {
            eprintln!("error: {error}; resource checks did not complete");
            print_summary(true, None)?;
            return Ok(ExitCode::from(1));
        }
        Err(error) => return Err(error),
    };
    failed |= check_failed || !result.failures.is_empty();
    for path in &result.failures {
        eprintln!("error: failed to load {path}");
    }

    print_summary(failed, Some(&result.counts))?;
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_cleanup_without_hiding_errors_or_project_locations() {
        let output = Output {
            status: Default::default(),
            stdout: b"GDKIT_CHECK_RESULT:{}\n".to_vec(),
            stderr: concat!(
                "ERROR: 12 RID allocations of type 'DummyTexture' were leaked at exit.\n",
                "ERROR: Unrecognized UID: \"uid://missing\".\n",
                "   at: ResourceUID::get_id_path (core/io/resource_uid.cpp:214)\n",
                "WARNING: 21 RIDs of type \"CanvasItem\" were leaked.\n",
                "WARNING: ObjectDB instances leaked at exit (run with --verbose for details).\n",
                "ERROR: 5 resources still in use at exit (run with --verbose for details).\n",
                "SCRIPT ERROR: Parse Error: Invalid type.\n",
                "   at: GDScript::reload (res://player.gd:3)\n",
                "ERROR: Unknown engine failure\n",
            )
            .as_bytes()
            .to_vec(),
        };
        let report = diagnostics(&output);
        assert_eq!(report.cleanup.len(), 4);
        assert_eq!(
            cleanup_message(&report.cleanup[0]),
            "ERROR: 12 headless renderer textures still allocated at shutdown."
        );
        assert_eq!(report.issues.len(), 4);
        assert_eq!(unresolved_uid(&report.issues[0]), Some("uid://missing"));
        assert!(report.issues[2].contains("res://player.gd:3"));
        assert_eq!(report.issues[3], "ERROR: Unknown engine failure");
        assert!(has_errors(&output));
        let cleanup_only = Output {
            status: Default::default(),
            stdout: Vec::new(),
            stderr: report.cleanup.join("\n").into_bytes(),
        };
        assert!(has_errors(&cleanup_only));
    }

    #[test]
    fn finds_uid_references_with_line_numbers_in_project_and_scripts() {
        let directory = env::temp_dir().join(format!("gdkit-uid-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        fs::write(
            directory.join("project.godot"),
            "[application]\nrun/main_scene=\"uid://abc\"\n",
        )
        .unwrap();
        fs::write(
            directory.join("player.gd"),
            "var a = preload(\"uid://abcdef\")\nvar b = preload(\"uid://abc\")\n",
        )
        .unwrap();
        let references = uid_references(&directory, &["res://player.gd".into()], "uid://abc");
        assert_eq!(references.len(), 2);
        assert!(references[0].starts_with("res://project.godot:2:"));
        assert!(references[1].starts_with("res://player.gd:2:"));
        assert!(uid_references(&directory, &[], "uid://absent").is_empty());
        fs::remove_dir_all(directory).unwrap();
    }
}
