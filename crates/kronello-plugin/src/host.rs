//! Worker-side helper orchestration: spawn the detached plugin host, push
//! the serialized request through stdin, and map every outcome (success,
//! typed helper error, crash, timeout, protocol violation) to a
//! [`PluginError`]. Nothing here loads plugin code — the helper is the only
//! process that ever calls `dlopen`.
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{HelperRequest, HelperResponse, PluginError};

/// Environment override selecting an explicit helper executable (test and
/// deployment escape hatch; ADR-0131 Q11).
pub const HELPER_PATH_ENV: &str = "KRONELLO_PLUGIN_HELPER";
/// Environment override for the whole-exchange deadline, milliseconds.
pub const HELPER_TIMEOUT_ENV: &str = "KRONELLO_PLUGIN_TIMEOUT_MS";
pub const DEFAULT_HELPER_DEADLINE_MS: u64 = 120_000;

/// Executable + argv (+ optional extra env) used to enter the plugin host.
/// The helper always inherits the worker's environment; `envs` overlays
/// deterministic per-job settings (e.g. test fixture hooks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub envs: Vec<(String, String)>,
}
impl HelperCommand {
    /// Resolution order: `$KRONELLO_PLUGIN_HELPER` → a sibling
    /// `kronello-plugin-host` executable next to the current binary → the
    /// current binary re-entered as `plugin-helper`.
    pub fn resolve() -> Result<Self, PluginError> {
        if let Ok(path) = std::env::var(HELPER_PATH_ENV)
            && !path.is_empty()
        {
            return Ok(Self {
                program: PathBuf::from(path),
                args: Vec::new(),
                envs: Vec::new(),
            });
        }
        let exe = std::env::current_exe()?;
        let dir = exe.parent().map(PathBuf::from).unwrap_or_default();
        let standalone = dir.join(format!(
            "kronello-plugin-host{}",
            std::env::consts::EXE_SUFFIX
        ));
        if standalone.is_file() {
            return Ok(Self {
                program: standalone,
                args: Vec::new(),
                envs: Vec::new(),
            });
        }
        Ok(Self {
            program: exe,
            args: vec!["plugin-helper".into()],
            envs: Vec::new(),
        })
    }
    pub fn with_env(mut self, key: &str, value: impl Into<String>) -> Self {
        self.envs.push((key.into(), value.into()));
        self
    }
}

/// Spawn the helper, write the request, wait for the JSON response.
/// `timeout` covers the entire exchange; `request.deadline_ms` is the
/// helper's own watchdog and should stay below `timeout`.
pub fn run_helper(
    command: &HelperCommand,
    request: &HelperRequest,
    timeout: Duration,
) -> Result<HelperResponse, PluginError> {
    let mut child = Command::new(&command.program)
        .args(&command.args)
        .envs(command.envs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            PluginError::Failed(format!(
                "spawn plugin helper {}: {e}",
                command.program.display()
            ))
        })?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| PluginError::Failed("plugin helper stdout pipe missing".into()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| PluginError::Failed("plugin helper stderr pipe missing".into()))?;
    // Drain pipes on threads so a noisy plugin can never deadlock the wait.
    let out_thread = std::thread::spawn(move || {
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut stdout, &mut data).ok();
        data
    });
    let err_thread = std::thread::spawn(move || {
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(&mut stderr, 256 * 1024), &mut data)
            .ok();
        data
    });
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| PluginError::Failed("plugin helper stdin pipe missing".into()))?;
    let body = serde_json::to_vec(request)?;
    let _ = stdin.write_all(&body);
    drop(stdin);

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out_thread.join();
                let _ = err_thread.join();
                return Err(PluginError::Timeout(format!(
                    "plugin helper exceeded {} ms deadline and was killed",
                    timeout.as_millis()
                )));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => {
                let _ = child.kill();
                return Err(PluginError::Io(e));
            }
        }
    };
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    let stderr_text = String::from_utf8_lossy(&stderr);
    let tail: String = stderr_text
        .rsplit('\n')
        .filter(|line| !line.trim().is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" | ");

    if !status.success() {
        return Err(PluginError::Failed(format!(
            "plugin helper terminated abnormally ({status}){tail}",
            tail = if tail.is_empty() {
                String::new()
            } else {
                format!("; helper stderr: {tail}")
            }
        )));
    }
    let response: HelperResponse = serde_json::from_slice(&stdout).map_err(|e| {
        PluginError::Protocol(format!(
            "plugin helper response is not valid protocol JSON: {e}"
        ))
    })?;
    match response.status.as_str() {
        "ok" => Ok(response),
        "error" => Err(PluginError::from_code(
            response.code.as_deref().unwrap_or("PLUGIN_FAILED"),
            response
                .message
                .as_deref()
                .unwrap_or("plugin helper failed"),
        )),
        other => Err(PluginError::Protocol(format!(
            "plugin helper response status {other:?} is not ok|error"
        ))),
    }
}
