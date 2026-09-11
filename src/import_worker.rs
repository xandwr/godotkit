use std::{
    collections::{BTreeMap, hash_map::RandomState},
    error::Error,
    fs::{self, File, OpenOptions},
    hash::{BuildHasher, Hash, Hasher},
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::Path,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

const SCRIPT: &str = include_str!("import_worker.gd");

#[derive(Serialize, Deserialize)]
struct Worker {
    key: u64,
    port: u16,
    pid: u32,
    token: String,
    startup: String,
    scripts: BTreeMap<String, u64>,
    diagnostics: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkerState {
    None,
    Running,
    Stale,
    Unknown,
    Malformed,
}

pub(crate) struct WorkerInspection {
    pub(crate) state: WorkerState,
    pub(crate) pid: Option<u32>,
    pub(crate) current: Option<bool>,
    pub(crate) diagnostics: Vec<String>,
}

#[derive(Deserialize)]
struct Response {
    diagnostics: String,
}

fn hash(value: impl Hash) -> u64 {
    let mut hash = std::hash::DefaultHasher::new();
    value.hash(&mut hash);
    hash.finish()
}

fn inputs(project: &Path, engine: &Path) -> Result<(u64, BTreeMap<String, u64>), Box<dyn Error>> {
    let mut settings = vec![serde_json::to_vec(&crate::engine::probe_key(engine)?)?];
    settings.push(project.as_os_str().as_encoded_bytes().to_vec());
    settings.push(SCRIPT.as_bytes().to_vec());
    settings.push(include_bytes!("import_worker.rs").to_vec());
    let mut scripts = BTreeMap::new();
    let mut directories = vec![project.to_path_buf()];
    while let Some(directory) = directories.pop() {
        settings.push(
            directory
                .strip_prefix(project)?
                .as_os_str()
                .as_encoded_bytes()
                .to_vec(),
        );
        settings.push(vec![u8::from(directory.join(".gdignore").exists())]);
        if directory.join(".gdignore").exists() {
            continue;
        }
        let mut entries = fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                return Err(
                    "persistent importing does not support project symlinks; use --fresh".into(),
                );
            }
            if kind.is_dir() {
                directories.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(project)?
                .to_string_lossy()
                .replace('\\', "/");
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if name.to_string_lossy().starts_with('~')
                && ["dll", "so", "dylib"].contains(&extension.as_str())
            {
                continue;
            }
            if extension == "gd" {
                let bytes = fs::read(&path)?;
                scripts.insert(format!("res://{relative}"), hash(&bytes));
                if relative.starts_with("addons/")
                    || String::from_utf8_lossy(&bytes).contains("@tool")
                {
                    settings.push(relative.as_bytes().to_vec());
                    settings.push(bytes);
                }
            } else {
                settings.push(relative.as_bytes().to_vec());
                let metadata = entry.metadata()?;
                settings.push(serde_json::to_vec(&(metadata.len(), metadata.modified()?))?);
            }
        }
    }
    Ok((hash(settings), scripts))
}

#[cfg(windows)]
fn worker_is_running(pid: u32) -> Option<bool> {
    unsafe {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, WAIT_TIMEOUT},
            System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        };
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return Some(false);
        }
        let running = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
        CloseHandle(handle);
        Some(running)
    }
}

#[cfg(not(windows))]
fn worker_is_running(_pid: u32) -> Option<bool> {
    None
}

pub(crate) fn inspect(project: &Path, engine: &Path) -> WorkerInspection {
    let record = project.join(".godot/gdkit/import-worker.json");
    let bytes = match fs::read(record) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return WorkerInspection {
                state: WorkerState::None,
                pid: None,
                current: None,
                diagnostics: Vec::new(),
            };
        }
        Err(_) => {
            return WorkerInspection {
                state: WorkerState::Unknown,
                pid: None,
                current: None,
                diagnostics: Vec::new(),
            };
        }
    };
    let worker = match serde_json::from_slice::<Worker>(&bytes) {
        Ok(worker) => worker,
        Err(_) => {
            return WorkerInspection {
                state: WorkerState::Malformed,
                pid: None,
                current: None,
                diagnostics: Vec::new(),
            };
        }
    };
    let state = match worker_is_running(worker.pid) {
        Some(true) => WorkerState::Running,
        Some(false) => WorkerState::Stale,
        None => WorkerState::Unknown,
    };
    let current = inputs(project, engine)
        .ok()
        .map(|(key, _)| key == worker.key);
    let diagnostics = worker
        .diagnostics
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("ERROR:") || line.starts_with("SCRIPT ERROR:")
        })
        .map(str::to_owned)
        .collect();
    WorkerInspection {
        state,
        pid: Some(worker.pid),
        current,
        diagnostics,
    }
}

pub(crate) fn compatibility_issue(project: &Path, engine: &Path) -> Option<String> {
    inputs(project, engine).err().map(|error| error.to_string())
}

fn request(worker: &Worker, changed: &[String], stop: bool) -> Result<Response, Box<dyn Error>> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, worker.port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(250))?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(if stop { 5 } else { 120 })))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    serde_json::to_writer(
        &mut stream,
        &serde_json::json!({
            "token": worker.token, "changed_scripts": changed, "stop": stop
        }),
    )?;
    stream.write_all(b"\n")?;
    let mut response = String::new();
    BufReader::new(stream)
        .take(16 * 1024 * 1024)
        .read_line(&mut response)?;
    if stop {
        return Ok(Response {
            diagnostics: String::new(),
        });
    }
    Ok(serde_json::from_str(&response)?)
}

fn start(
    project: &Path,
    engine: &Path,
    directory: &Path,
    key: u64,
) -> Result<Worker, Box<dyn Error>> {
    let token = format!(
        "{:016x}{:016x}",
        RandomState::new().build_hasher().finish(),
        RandomState::new().build_hasher().finish()
    );
    let script = directory.join("import-worker.gd");
    let ready = directory.join(format!("ready-{token}.json"));
    let log = directory.join(format!("import-worker-{token}.log"));
    fs::write(&script, SCRIPT)?;
    let output = File::create(&log)?;
    let mut command = Command::new(engine);
    command
        .args(["--headless", "--no-header", "--editor", "--quiet", "--path"])
        .arg(project)
        .arg("--script")
        .arg(&script)
        .arg("--")
        .arg(&ready)
        .arg(&token)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = spawn_detached(&mut command)?;
    let began = Instant::now();
    let result = loop {
        if let Ok(bytes) = fs::read(&ready)
            && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
            && let Some(port) = value["port"]
                .as_u64()
                .and_then(|port| u16::try_from(port).ok())
            && let Some(pid) = value["pid"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
        {
            break Ok(Worker {
                key,
                port,
                pid,
                token,
                startup: fs::read_to_string(&log).unwrap_or_default(),
                scripts: BTreeMap::new(),
                diagnostics: String::new(),
            });
        }
        if child.try_wait()?.is_some() {
            break Err(format!(
                "import worker failed to start:\n{}",
                fs::read_to_string(&log).unwrap_or_default()
            )
            .into());
        }
        if began.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            let _ = child.wait();
            break Err("import worker startup timed out".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let _ = fs::remove_file(ready);
    if result.is_ok() {
        thread::spawn(move || {
            let _ = child.wait();
        });
    }
    result
}

pub fn import(project: &Path, engine: &Path) -> Result<(Output, bool), Box<dyn Error>> {
    let directory = project.join(".godot/gdkit");
    fs::create_dir_all(&directory)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("import-worker.lock"))?;
    lock.lock()?;
    let (key, scripts) = inputs(project, engine)?;
    let record = directory.join("import-worker.json");
    let previous = fs::read(&record)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Worker>(&bytes).ok());
    let mut worker = None;
    let mut response = None;
    if let Some(previous) = previous {
        if previous.key == key
            && (previous.scripts == scripts || !contains_errors(&previous.diagnostics))
        {
            let changed = changed_scripts(&previous.scripts, &scripts);
            if let Ok(result) = request(&previous, &changed, false) {
                response = Some(result);
                worker = Some(previous);
            } else {
                stop(&previous)?;
            }
        } else {
            stop(&previous)?;
        }
    }
    let reused = worker.is_some();
    let mut worker = match worker {
        Some(worker) => worker,
        None => start(project, engine, &directory, key)?,
    };
    let response = match response {
        Some(response) => response,
        None => match request(&worker, &[], false) {
            Ok(response) => response,
            Err(error) => {
                stop(&worker)?;
                return Err(error);
            }
        },
    };
    let diagnostics = format!("{}\n{}", worker.startup, response.diagnostics);
    let diagnostics = if reused && worker.scripts == scripts && contains_errors(&worker.diagnostics)
    {
        format!("{}\n{diagnostics}", worker.diagnostics)
    } else {
        diagnostics
    };
    let output = Output {
        status: Default::default(),
        stdout: Vec::new(),
        stderr: diagnostics.clone().into_bytes(),
    };
    worker.key = key;
    worker.scripts = scripts;
    let mut seen = std::collections::HashSet::new();
    worker.diagnostics = diagnostics
        .lines()
        .filter(|line| seen.insert(*line))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(record, serde_json::to_vec(&worker)?)?;
    Ok((output, reused))
}

fn contains_errors(text: &str) -> bool {
    text.lines().any(|line| {
        line.trim_start().starts_with("ERROR:") || line.trim_start().starts_with("SCRIPT ERROR:")
    })
}

fn spawn_detached(command: &mut Command) -> std::io::Result<std::process::Child> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::{GetHandleInformation, HANDLE_FLAG_INHERIT, SetHandleInformation},
            System::Console::{
                GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
            },
        };
        let mut handles = Vec::new();
        for stream in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let handle = GetStdHandle(stream);
            let mut flags = 0;
            if GetHandleInformation(handle, &mut flags) != 0 && flags & HANDLE_FLAG_INHERIT != 0 {
                if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) == 0 {
                    let error = std::io::Error::last_os_error();
                    for handle in handles {
                        SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
                    }
                    return Err(error);
                }
                handles.push(handle);
            }
        }
        let result = command.spawn();
        for handle in handles {
            SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
        }
        result
    }
    #[cfg(not(windows))]
    command.spawn()
}

fn stop(worker: &Worker) -> Result<(), Box<dyn Error>> {
    let graceful = request(worker, &[], true).is_ok();
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, WAIT_TIMEOUT},
            System::Threading::{
                OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
                WaitForSingleObject,
            },
        };
        let handle = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, 0, worker.pid);
        if !handle.is_null() {
            let mut result = WaitForSingleObject(handle, if graceful { 5000 } else { 0 });
            if result == WAIT_TIMEOUT {
                if TerminateProcess(handle, 1) == 0 {
                    let error = std::io::Error::last_os_error();
                    CloseHandle(handle);
                    return Err(error.into());
                }
                result = WaitForSingleObject(handle, 5000);
            }
            CloseHandle(handle);
            if result == WAIT_TIMEOUT {
                return Err("import worker did not terminate".into());
            }
        }
    }
    #[cfg(not(windows))]
    if !graceful {
        return Ok(());
    }
    Ok(())
}

pub fn stop_project(project: &Path) -> Result<(), Box<dyn Error>> {
    let record = project.join(".godot/gdkit/import-worker.json");
    if !record.exists() {
        return Ok(());
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(project.join(".godot/gdkit/import-worker.lock"))?;
    lock.lock()?;
    if let Ok(bytes) = fs::read(&record)
        && let Ok(worker) = serde_json::from_slice::<Worker>(&bytes)
    {
        stop(&worker)?;
        let _ = fs::remove_file(record);
    }
    Ok(())
}

fn changed_scripts(before: &BTreeMap<String, u64>, after: &BTreeMap<String, u64>) -> Vec<String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|path| before.get(*path) != after.get(*path))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}
