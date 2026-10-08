//! Helper-process entry. This is the only path that reaches `abi`, i.e. the
//! only code that ever loads plugin bundles. It reads one JSON request from
//! stdin, arms a self-watchdog, executes the operation, and writes one JSON
//! response to stdout. A successful protocol exchange exits 0 even when the
//! operation reports a typed error; only crashes/hangs exit abnormally.
use std::io::Read;
use std::process::ExitCode;
use std::time::Duration;

use crate::abi;
use crate::{
    HELPER_PROTOCOL_VERSION, HelperOp, HelperRequest, HelperResponse, PluginError, PluginReport,
    verify_spec_pin,
};

/// Hard budget for the helper's own watchdog when the request carries none.
const FALLBACK_DEADLINE_MS: u64 = 120_000;

/// Entry point for embedding binaries: `program plugin-helper` runs the
/// helper protocol on stdin/stdout and returns the process exit code.
pub fn helper_entry() -> Option<ExitCode> {
    let is_helper = std::env::args()
        .nth(1)
        .is_some_and(|arg| arg == "plugin-helper");
    is_helper.then(plugin_host_main)
}
/// Unconditional helper main used by the standalone `kronello-plugin-host`.
pub fn plugin_host_main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}
fn respond(response: &HelperResponse) -> Result<(), ExitCode> {
    let body = serde_json::to_vec(response).map_err(|_| ExitCode::from(2))?;
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&body).map_err(|_| ExitCode::from(2))?;
    stdout.flush().map_err(|_| ExitCode::from(2))
}
fn run() -> Result<(), ExitCode> {
    let mut body = Vec::new();
    std::io::stdin()
        .lock()
        .read_to_end(&mut body)
        .map_err(|_| ExitCode::from(2))?;
    let request: HelperRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(e) => {
            return respond(&HelperResponse::error(
                "PLUGIN_PROTOCOL",
                format!("helper request is not valid protocol JSON: {e}"),
            ));
        }
    };
    if request.schema_version != HELPER_PROTOCOL_VERSION {
        return respond(&HelperResponse::error(
            "PLUGIN_PROTOCOL",
            format!(
                "helper protocol version {} != {HELPER_PROTOCOL_VERSION}",
                request.schema_version
            ),
        ));
    }
    // Self-watchdog: if plugin code hangs, this process dies even though the
    // worker would also kill us. abort() is unconditional (no locks held).
    let deadline = Duration::from_millis(
        if request.deadline_ms == 0 {
            FALLBACK_DEADLINE_MS
        } else {
            request.deadline_ms
        }
        .max(1_000),
    );
    std::thread::spawn(move || {
        std::thread::sleep(deadline);
        std::process::abort();
    });

    let response = match std::panic::catch_unwind(|| execute(&request)) {
        Ok(Ok((report, frames))) => HelperResponse::ok(report, frames),
        Ok(Err(e)) => HelperResponse::error(e.code(), e.to_string()),
        Err(_) => HelperResponse::error("PLUGIN_FAILED", "plugin operation panicked"),
    };
    respond(&response)
}
fn execute(request: &HelperRequest) -> Result<(PluginReport, Option<u64>), PluginError> {
    request.spec.validate()?;
    // Re-verify the content pin inside the helper, immediately before any
    // plugin byte reaches dlopen.
    verify_spec_pin(&request.spec)?;
    match request.op {
        HelperOp::Describe => Ok((abi::describe(&request.spec)?, None)),
        HelperOp::Process => {
            let io = request.io.as_ref().ok_or_else(|| {
                PluginError::Protocol("process request carries no io contract".into())
            })?;
            let (report, frames) = abi::process(&request.spec, io)?;
            Ok((report, Some(frames)))
        }
    }
}
