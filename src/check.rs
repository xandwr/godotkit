use std::{
    collections::HashSet,
    env,
    error::Error,
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Output},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;

use crate::cli::CheckArgs;
use crate::process::{CapturedOutput, OutputStream};

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

struct CheckArtifacts {
    directory: PathBuf,
}

impl CheckArtifacts {
    fn create(project: &Path) -> io::Result<Self> {
        let root = project.join(".godot/gdkit/checks");
        fs::create_dir_all(&root)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        for attempt in 0..100 {
            let directory = root.join(format!("{timestamp}-{}-{attempt}", std::process::id()));
            match fs::create_dir(&directory) {
                Ok(()) => return Ok(Self { directory }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create a check artifact directory",
        ))
    }

    fn preserve(&self, phase: &str, output: &CapturedOutput) -> io::Result<()> {
        fs::write(
            self.directory.join(format!("{phase}.stdout.log")),
            &output.output.stdout,
        )?;
        fs::write(
            self.directory.join(format!("{phase}.stderr.log")),
            &output.output.stderr,
        )?;
        let mut events = Vec::new();
        for (sequence, line) in output.lines.iter().enumerate() {
            serde_json::to_writer(
                &mut events,
                &serde_json::json!({
                    "sequence": sequence,
                    "stream": match line.stream {
                        OutputStream::Stdout => "stdout",
                        OutputStream::Stderr => "stderr",
                    },
                    "observed_at_unix_ms": line.observed_at_unix_ms,
                    "text": String::from_utf8_lossy(&line.bytes)
                        .trim_end_matches(['\r', '\n']),
                }),
            )?;
            events.push(b'\n');
        }
        fs::write(self.directory.join(format!("{phase}.events.jsonl")), events)?;
        Ok(())
    }
}

fn engine_output(engine: &Path, project: &Path, args: &[&OsStr]) -> io::Result<CapturedOutput> {
    let mut command = Command::new(engine);
    command
        .args([OsStr::new("--headless"), OsStr::new("--no-header")])
        .arg("--path")
        .arg(project)
        .args(args);
    crate::process::run(&mut command, None)
}

fn smoke_output(
    engine: &Path,
    project: &Path,
    scene: &Path,
    frames: u32,
    timeout: u64,
) -> io::Result<CapturedOutput> {
    let mut command = Command::new(engine);
    command
        .args(["--headless", "--no-header", "--path"])
        .arg(project)
        .arg(scene)
        .arg("--quit-after")
        .arg(frames.to_string());
    crate::process::run(&mut command, Some(Duration::from_secs(timeout)))
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

fn filter_import_errors(
    output: &CapturedOutput,
    rules: &[crate::engine::ImportError],
) -> (CapturedOutput, usize) {
    let mut ignored = 0;
    let mut excluded = HashSet::new();
    for stream in [OutputStream::Stdout, OutputStream::Stderr] {
        let lines: Vec<_> = output
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.stream == stream)
            .map(|(global_index, line)| {
                (
                    global_index,
                    String::from_utf8_lossy(&line.bytes)
                        .trim_end_matches(['\r', '\n'])
                        .to_owned(),
                )
            })
            .collect();
        let mut index = 0;
        while index < lines.len() {
            let start = index;
            index += 1;
            let message = lines[start].1.trim();
            if message.starts_with("ERROR:") || message.starts_with("SCRIPT ERROR:") {
                while index < lines.len() {
                    let line = lines[index].1.trim();
                    let frame = line
                        .strip_prefix('[')
                        .and_then(|line| line.split_once(']'))
                        .is_some_and(|(number, _)| {
                            !number.is_empty() && number.chars().all(|c| c.is_ascii_digit())
                        });
                    if !(line.is_empty()
                        || line.starts_with("at:")
                        || line.starts_with("GDScript backtrace")
                        || frame)
                    {
                        break;
                    }
                    index += 1;
                }
                if rules.iter().any(|rule| {
                    message == rule.message
                        && lines[start + 1..index].iter().any(|(_, line)| {
                            line.contains(&format!("({}:", rule.source))
                                || line.contains(&format!("({})", rule.source))
                        })
                }) {
                    ignored += 1;
                    excluded.extend(
                        lines[start..index]
                            .iter()
                            .map(|(global_index, _)| *global_index),
                    );
                    continue;
                }
            }
        }
    }
    (
        output.retaining_lines(|index| !excluded.contains(&index)),
        ignored,
    )
}

#[derive(Default)]
struct Diagnostics {
    issues: Vec<DiagnosticMessage>,
    cleanup: Vec<DiagnosticMessage>,
}

struct DiagnosticMessage {
    text: String,
    occurrence_count: usize,
}

fn diagnostics(output: &CapturedOutput) -> Diagnostics {
    let mut result = Diagnostics::default();
    for event in &output.lines {
        let text = String::from_utf8_lossy(&event.bytes);
        let line = text.trim();
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
        if let Some(existing) = target.iter_mut().find(|existing| existing.text == line) {
            existing.occurrence_count += 1;
        } else {
            target.push(DiagnosticMessage {
                text: line.to_owned(),
                occurrence_count: 1,
            });
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
    output: &CapturedOutput,
    phase: &str,
    project: &Path,
    paths: &[String],
    verbose: bool,
) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    let report = diagnostics(output);
    if !report.issues.is_empty() {
        writeln!(stderr, "\n{phase} diagnostics:")?;
        for diagnostic in &report.issues {
            write!(stderr, "  {}", diagnostic.text)?;
            if diagnostic.occurrence_count > 1 {
                write!(stderr, " (repeated {} times)", diagnostic.occurrence_count)?;
            }
            writeln!(stderr)?;
            if let Some(uid) = unresolved_uid(&diagnostic.text) {
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
        for diagnostic in &report.cleanup {
            write!(stderr, "  {}", cleanup_message(&diagnostic.text))?;
            if diagnostic.occurrence_count > 1 {
                write!(stderr, " (repeated {} times)", diagnostic.occurrence_count)?;
            }
            writeln!(stderr)?;
        }
        writeln!(
            stderr,
            "  Use gdkit check --verbose for the original engine messages and stack traces."
        )?;
    }
    if !output.output.status.success() {
        writeln!(
            stderr,
            "\n{phase}: Godot process exited with {}.",
            output.output.status
        )?;
    }
    if verbose {
        writeln!(stderr, "\n{phase} full Godot output:")?;
        for bytes in [&output.output.stdout, &output.output.stderr] {
            for line in String::from_utf8_lossy(bytes).lines() {
                if !line.starts_with(RESULT_PREFIX) {
                    writeln!(stderr, "{line}")?;
                }
            }
        }
    }
    Ok(())
}

fn harness_result(output: &CapturedOutput) -> Result<HarnessResult, Box<dyn Error>> {
    let stdout = String::from_utf8_lossy(&output.output.stdout);
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
    let project = crate::engine::project_root(&args.project)?;
    if args.stop_worker {
        crate::import_worker::stop_project(&project)?;
        println!("import worker stopped");
        return Ok(ExitCode::SUCCESS);
    }
    let persistent_import = !args.fresh;
    let result = run_project(args, &project);
    let failed = match &result {
        Ok(code) => *code != ExitCode::SUCCESS,
        Err(_) => true,
    };
    if failed
        && persistent_import
        && let Err(error) = crate::import_worker::stop_project(&project)
    {
        eprintln!("warning: failed to stop import worker after check failure: {error}");
    }
    result
}

fn run_project(args: CheckArgs, project: &Path) -> Result<ExitCode, Box<dyn Error>> {
    let artifacts = CheckArtifacts::create(project)?;
    eprintln!(
        "artifacts: {}",
        crate::engine::display_path(&artifacts.directory)
    );
    let scan_timer = PhaseTimer::new("file scan", args.timings);
    let scenes = args
        .scene
        .iter()
        .map(|scene| {
            let path = project.join(scene.strip_prefix("res://").unwrap_or(scene));
            let path = fs::canonicalize(&path)
                .map_err(|error| format!("smoke scene {}: {error}", path.display()))?;
            if !path.starts_with(fs::canonicalize(project)?)
                || !path.is_file()
                || !path
                    .extension()
                    .is_some_and(|extension| extension == "tscn" || extension == "scn")
            {
                return Err("smoke scenes must be .tscn or .scn files inside the project".into());
            }
            Ok(path)
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let config = crate::engine::read_config(project)?;
    let strict_methods = args.strict_methods
        || config
            .as_ref()
            .is_some_and(|config| config.check.strict_methods);
    let rules = config.as_ref().map_or(&[][..], |config| {
        config.check.ignore_import_errors.as_slice()
    });
    let paths =
        crate::project_files::collect(project, &["gd", "tscn", "scn", "tres", "res", "gdshader"])?;
    let paths = paths
        .iter()
        .map(|path| {
            Ok(format!(
                "res://{}",
                path.strip_prefix(project)?
                    .to_str()
                    .ok_or("resource path is not valid UTF-8")?
                    .replace('\\', "/")
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let manifest = TemporaryScript::create(&serde_json::to_vec(&paths)?, "json")?;
    drop(scan_timer);
    let mut validation_timer = PhaseTimer::new("engine validation (probe)", args.timings);
    let engine = crate::engine::resolve(project, args.godot.as_deref())?;
    let (version, cached) = crate::engine::validated_version(&engine, project)?;
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
        crate::import_worker::stop_project(project)?;
        engine_output(
            &engine,
            project,
            &[OsStr::new("--import"), OsStr::new("--quiet")],
        )?
    } else {
        match crate::import_worker::import(project, &engine) {
            Ok((output, reused)) => {
                import_timer.name = if reused {
                    "import (warm worker)"
                } else {
                    "import (worker startup)"
                };
                CapturedOutput::from_output(output)
            }
            Err(error) => {
                crate::import_worker::stop_project(project)?;
                eprintln!("warning: {error}; falling back to a fresh import");
                engine_output(
                    &engine,
                    project,
                    &[OsStr::new("--import"), OsStr::new("--quiet")],
                )?
            }
        }
    };
    drop(import_timer);
    artifacts.preserve("import", &import)?;
    let (filtered_import, ignored) = filter_import_errors(&import, rules);
    let mut failed = !import.output.status.success() || has_errors(&filtered_import.output);
    if ignored > 0 {
        eprintln!(
            "Import: ignored {ignored} configured diagnostic(s); use --verbose for original output."
        );
    }
    write_diagnostics(
        if args.verbose {
            &import
        } else {
            &filtered_import
        },
        "Import",
        project,
        &paths,
        args.verbose,
    )?;

    let harness = TemporaryScript::create(HARNESS.as_bytes(), "gd")?;
    let loading_timer = PhaseTimer::new("resource loading", args.timings);
    let check = engine_output(
        &engine,
        project,
        &[
            OsStr::new("--script"),
            harness.0.as_os_str(),
            OsStr::new("--"),
            manifest.0.as_os_str(),
            OsStr::new(if strict_methods {
                "strict-methods"
            } else {
                "project-policy"
            }),
        ],
    )?;
    drop(loading_timer);
    artifacts.preserve("resource-loading", &check)?;
    let check_failed = !check.output.status.success() || has_errors(&check.output);
    write_diagnostics(&check, "Resource loading", project, &paths, args.verbose)?;
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

    let mut smoke_count = 0;
    if !failed {
        for (index, scene) in scenes.iter().enumerate() {
            let phase = format!("Scene smoke {}", scene.display());
            let output = smoke_output(
                &engine,
                project,
                scene,
                args.smoke_frames,
                args.smoke_timeout,
            )?;
            artifacts.preserve(&format!("scene-smoke-{}", index + 1), &output)?;
            write_diagnostics(&output, &phase, project, &paths, args.verbose)?;
            if output.timed_out {
                eprintln!("error: {phase} exceeded {} seconds", args.smoke_timeout);
            }
            failed |=
                output.timed_out || !output.output.status.success() || has_errors(&output.output);
            smoke_count += 1;
        }
    } else if !scenes.is_empty() {
        eprintln!("Scene smoke checks skipped because resource validation failed.");
    }
    print_summary(failed, Some(&result.counts))?;
    eprintln!(
        "coverage: resource loading{}; {smoke_count} scene smoke checks executed",
        if strict_methods {
            " and strict method validation"
        } else {
            " (project warning policy)"
        }
    );
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
    fn zero_exit_script_errors_fail_on_either_stream() {
        for stdout in [false, true] {
            let message = b"  SCRIPT ERROR: Invalid call. Nonexistent function 'push_tornado' in base 'RichTextLabel'.\n   at: _ready (res://lobby.gd:4)\n".to_vec();
            let output = Output {
                status: Default::default(),
                stdout: if stdout { message.clone() } else { Vec::new() },
                stderr: if stdout { Vec::new() } else { message },
            };
            let output = CapturedOutput::from_output(output);
            assert!(output.output.status.success());
            assert!(has_errors(&output.output));
            assert!(
                diagnostics(&output)
                    .issues
                    .iter()
                    .any(|diagnostic| diagnostic.text.contains("res://lobby.gd:4"))
            );
        }
    }

    #[test]
    fn import_exceptions_require_message_and_own_source_frame() {
        let rules = vec![crate::engine::ImportError {
            message: "ERROR: Known plugin problem".into(),
            source: "res://addons/plugin.gd".into(),
        }];
        let output = Output {
            status: Default::default(),
            stdout: Vec::new(),
            stderr: concat!(
                "ERROR: Known plugin problem\n",
                "   at: native (core/example.cpp:10)\n",
                "   GDScript backtrace (most recent call first):\n",
                "       [0] setup (res://addons/plugin.gd:67)\n",
                "\n",
                "GDScript backtrace (most recent call first):\n",
                "    [0] setup (res://addons/plugin.gd:67)\n",
                "ERROR: Unrelated problem\n",
                "       [0] setup (res://addons/plugin.gd:68)\n",
                "ERROR: Known plugin problem\n",
                "       [0] setup (res://game.gd:10)\n",
                "ERROR: Known plugin problem\n",
                "       [0] setup (res://addons/plugin.gd.backup:10)\n",
                "ERROR: Known plugin problem\n",
                "WARNING: Separate diagnostic\n",
                "       [0] setup (res://addons/plugin.gd:70)\n",
            )
            .as_bytes()
            .to_vec(),
        };
        let output = CapturedOutput::from_output(output);
        let (filtered, count) = filter_import_errors(&output, &rules);
        assert_eq!(count, 1);
        assert!(has_errors(&filtered.output));
        let remaining = String::from_utf8(filtered.output.stderr).unwrap();
        assert!(!remaining.contains("plugin.gd:67"));
        assert!(remaining.contains("Unrelated problem"));
        assert_eq!(remaining.matches("ERROR: Known plugin problem").count(), 3);
        assert_eq!(filter_import_errors(&output, &[]).1, 0);
        let known_only = Output {
            status: Default::default(),
            stdout: b"ERROR: Known plugin problem\n   at: setup (res://addons/plugin.gd:1)\n"
                .to_vec(),
            stderr: Vec::new(),
        };
        let known_only = CapturedOutput::from_output(known_only);
        assert!(!has_errors(
            &filter_import_errors(&known_only, &rules).0.output
        ));
        assert!(has_errors(&known_only.output));
    }

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
        let output = CapturedOutput::from_output(output);
        let report = diagnostics(&output);
        assert_eq!(report.cleanup.len(), 4);
        assert_eq!(
            cleanup_message(&report.cleanup[0].text),
            "ERROR: 12 headless renderer textures still allocated at shutdown."
        );
        assert_eq!(report.issues.len(), 4);
        assert_eq!(
            unresolved_uid(&report.issues[0].text),
            Some("uid://missing")
        );
        assert!(report.issues[2].text.contains("res://player.gd:3"));
        assert_eq!(report.issues[3].text, "ERROR: Unknown engine failure");
        assert!(has_errors(&output.output));
        let cleanup_only = Output {
            status: Default::default(),
            stdout: Vec::new(),
            stderr: report
                .cleanup
                .iter()
                .map(|diagnostic| diagnostic.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes(),
        };
        assert!(has_errors(&cleanup_only));
    }

    #[test]
    fn consolidates_repeated_diagnostics_without_reordering_first_occurrences() {
        let output = Output {
            status: Default::default(),
            stdout: b"WARNING: first\nERROR: second\nWARNING: first\n".to_vec(),
            stderr: b"ERROR: second\nWARNING: third\n".to_vec(),
        };
        let output = CapturedOutput::from_output(output);
        let report = diagnostics(&output);
        assert_eq!(report.issues.len(), 3);
        assert_eq!(report.issues[0].text, "WARNING: first");
        assert_eq!(report.issues[0].occurrence_count, 2);
        assert_eq!(report.issues[1].text, "ERROR: second");
        assert_eq!(report.issues[1].occurrence_count, 2);
        assert_eq!(report.issues[2].text, "WARNING: third");
        assert_eq!(report.issues[2].occurrence_count, 1);
    }

    #[test]
    fn preserves_raw_phase_streams_as_check_artifacts() {
        let directory = env::temp_dir().join(format!("gdkit-artifacts-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let artifacts = CheckArtifacts::create(&directory).unwrap();
        let output = Output {
            status: Default::default(),
            stdout: b"raw stdout\r\n".to_vec(),
            stderr: b"raw stderr\n".to_vec(),
        };
        let output = CapturedOutput::from_output(output);
        artifacts.preserve("resource-loading", &output).unwrap();
        assert_eq!(
            fs::read(artifacts.directory.join("resource-loading.stdout.log")).unwrap(),
            output.output.stdout
        );
        assert_eq!(
            fs::read(artifacts.directory.join("resource-loading.stderr.log")).unwrap(),
            output.output.stderr
        );
        let events =
            fs::read_to_string(artifacts.directory.join("resource-loading.events.jsonl")).unwrap();
        let events: Vec<serde_json::Value> = events
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["sequence"], 0);
        assert_eq!(events[0]["stream"], "stdout");
        assert_eq!(events[0]["text"], "raw stdout");
        assert_eq!(events[1]["sequence"], 1);
        assert_eq!(events[1]["stream"], "stderr");
        assert!(events[1]["observed_at_unix_ms"].as_u64().unwrap() > 0);
        fs::remove_dir_all(directory).unwrap();
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
