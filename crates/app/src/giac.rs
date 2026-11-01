//! The Giac computer algebra system, run as a separate worker process so a
//! long computation can be stopped by ending the process. The worker ships
//! inside the .exe and is written out next to the app's other data the
//! first time it is needed; it stays running between problems.
//!
//! The worker reads one Giac command per line and answers each with a line
//! starting `\x01`: `ok\t<giac text>\t<latex>` or `err\t<message>`.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use graph_expr::cas::Plan;

/// How long one problem may take before the worker is stopped.
const TIME_LIMIT: Duration = Duration::from_secs(30);

struct Worker {
    stdin: ChildStdin,
    lines: Receiver<String>,
}

/// The worker talked to. Held for a whole problem, so problems take turns.
static WORKER: Mutex<Option<Worker>> = Mutex::new(None);
/// The worker's process, kept apart so it can be ended while a problem
/// holds `WORKER`.
static PROCESS: Mutex<Option<Child>> = Mutex::new(None);

#[cfg(giac)]
static EMBEDDED: &[u8] = include_bytes!(env!("GIAC_WORKER_PATH"));

/// Whether Giac can be used: it is built into this .exe, or
/// `GRAPHCALC_GIAC` names a worker.
pub fn available() -> bool {
    std::env::var_os("GRAPHCALC_GIAC").is_some() || cfg!(giac)
}

/// Stops the problem running now, if any; it answers "Stopped".
pub fn cancel() {
    if let Some(mut child) = PROCESS.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Runs `plan` in Giac: its setup, then its command, whose answer comes
/// back as (Giac text, Giac's LaTeX).
pub fn run(plan: &Plan) -> Result<(String, String), String> {
    let mut slot = WORKER.lock().unwrap_or_else(|e| e.into_inner());
    let started = Instant::now();
    let result = (|| {
        if slot.is_none() || !alive() {
            *slot = Some(start()?);
        }
        let worker = slot.as_mut().expect("just started");
        for line in &plan.setup {
            ask(worker, line, started).map_err(|e| format!("{e} (in {line})"))?;
        }
        ask(worker, &plan.command, started)
    })();
    if result.is_err() && !alive() {
        // Stopped, timed out or crashed: the next problem starts afresh.
        *slot = None;
    }
    result
}

fn alive() -> bool {
    let mut process = PROCESS.lock().unwrap_or_else(|e| e.into_inner());
    match process.as_mut().map(|c| c.try_wait()) {
        Some(Ok(None)) => true,
        _ => {
            *process = None;
            false
        }
    }
}

fn ask(worker: &mut Worker, command: &str, started: Instant) -> Result<(String, String), String> {
    let line = command.replace(['\n', '\r'], " ");
    writeln!(worker.stdin, "{line}")
        .and_then(|_| worker.stdin.flush())
        .map_err(|_| "Stopped".to_owned())?;
    loop {
        let left = TIME_LIMIT.saturating_sub(started.elapsed());
        match worker.lines.recv_timeout(left) {
            Ok(reply) => {
                let Some(reply) = reply.strip_prefix('\x01') else {
                    continue;
                };
                let mut parts = reply.splitn(3, '\t');
                return match (parts.next(), parts.next(), parts.next()) {
                    (Some("ok"), Some(text), tex) => {
                        Ok((text.to_owned(), tex.unwrap_or_default().to_owned()))
                    }
                    (Some("err"), message, _) => Err(message.unwrap_or("error").trim().to_owned()),
                    _ => Err(format!("Giac sent something unexpected: {reply}")),
                };
            }
            Err(RecvTimeoutError::Timeout) => {
                cancel();
                return Err(format!(
                    "This took longer than {} seconds, so it was stopped",
                    TIME_LIMIT.as_secs()
                ));
            }
            Err(RecvTimeoutError::Disconnected) => return Err("Stopped".into()),
        }
    }
}

fn start() -> Result<Worker, String> {
    let path = worker_path()?;
    let mut command = Command::new(&path);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Couldn't start Giac ({}): {e}", path.display()))?;
    let stdin = child.stdin.take().ok_or("Giac has no input")?;
    let stdout = child.stdout.take().ok_or("Giac has no output")?;
    let (send, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if send.send(line).is_err() {
                break;
            }
        }
    });
    *PROCESS.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
    Ok(Worker { stdin, lines })
}

fn worker_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("GRAPHCALC_GIAC") {
        return Ok(path.into());
    }
    embedded_path()
}

#[cfg(giac)]
fn embedded_path() -> Result<PathBuf, String> {
    // Named by a hash of its bytes, so a new version never runs an old copy.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in EMBEDDED {
        hash = (hash ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("graphcalc");
    let path = dir.join(format!("giac-worker-{hash:016x}.exe"));
    let written = std::fs::metadata(&path).is_ok_and(|m| m.len() == EMBEDDED.len() as u64);
    if !written {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't set up Giac: {e}"))?;
        // Written under another name first, so a half-written file is never run.
        let partial = dir.join(format!(
            "giac-worker-{hash:016x}.{}.part",
            std::process::id()
        ));
        std::fs::write(&partial, EMBEDDED).map_err(|e| format!("Couldn't set up Giac: {e}"))?;
        if std::fs::rename(&partial, &path).is_err() {
            let _ = std::fs::remove_file(&partial);
        }
    }
    Ok(path)
}

#[cfg(not(giac))]
fn embedded_path() -> Result<PathBuf, String> {
    Err("Giac isn't part of this build".into())
}
