//! Stdio entry point; all diagnostics go to stderr.
use kronello_service::BackendSelection;

fn main() -> std::process::ExitCode {
    if let Some(exit) = kronello_service::worker_entry() {
        return exit;
    }
    let mut backend = BackendSelection::Gpu;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--backend" => {
                backend = match args.next().as_deref() {
                    Some("gpu") => BackendSelection::Gpu,
                    Some("cpu-reference") => BackendSelection::CpuReference,
                    _ => {
                        eprintln!("INVALID_REQUEST: --backend requires gpu or cpu-reference");
                        return std::process::ExitCode::FAILURE;
                    }
                };
            }
            "--help" | "-h" => {
                eprintln!("kronello-mcp [--backend gpu|cpu-reference] (JSON-RPC 2.0 over stdio)");
                return std::process::ExitCode::SUCCESS;
            }
            _ => {
                eprintln!("INVALID_REQUEST: unknown option: {arg}");
                return std::process::ExitCode::FAILURE;
            }
        }
    }
    match kronello_mcp::serve(std::io::stdin().lock(), std::io::stdout().lock(), backend) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("IO_ERROR: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
