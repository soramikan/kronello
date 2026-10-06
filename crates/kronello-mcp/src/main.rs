//! MCP entry point; all diagnostics and listener addresses go to stderr.
use kronello_service::BackendSelection;

#[tokio::main(worker_threads = 3)]
async fn main() -> std::process::ExitCode {
    if let Some(exit) = kronello_service::worker_entry() {
        return exit;
    }
    let mut backend = BackendSelection::Gpu;
    let mut http = false;
    let mut config = kronello_mcp::http::HttpConfig::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--http" => http = true,
            "--bind" => {
                config.bind = match args.next().and_then(|value| value.parse().ok()) {
                    Some(bind) => bind,
                    None => {
                        eprintln!("INVALID_REQUEST: --bind requires an IP:port socket address");
                        return std::process::ExitCode::FAILURE;
                    }
                };
                http = true;
            }
            "--auth-token-env" => {
                config.bearer_token = match args.next().and_then(|name| std::env::var(name).ok()) {
                    Some(token) => Some(token),
                    None => {
                        eprintln!(
                            "INVALID_REQUEST: --auth-token-env requires a configured environment variable"
                        );
                        return std::process::ExitCode::FAILURE;
                    }
                };
                http = true;
            }
            "--backend" => {
                backend = match args.next().as_deref() {
                    Some("gpu") => BackendSelection::Gpu,
                    Some("cpu-reference") => BackendSelection::CpuReference,
                    Some("gpu-resident-bgra8") => BackendSelection::GpuResidentBgra8,
                    Some("gpu-resident-nv12") => BackendSelection::GpuResidentNv12,
                    _ => {
                        eprintln!(
                            "INVALID_REQUEST: --backend requires gpu, cpu-reference, gpu-resident-bgra8 or gpu-resident-nv12"
                        );
                        return std::process::ExitCode::FAILURE;
                    }
                };
            }
            "--help" | "-h" => {
                eprintln!(
                    "kronello-mcp [--backend gpu|cpu-reference|gpu-resident-bgra8|gpu-resident-nv12] [--http] [--bind IP:port] [--auth-token-env NAME]\nDefault: stdio. HTTP default: 127.0.0.1:8765/mcp. Non-loopback requires authentication."
                );
                return std::process::ExitCode::SUCCESS;
            }
            _ => {
                eprintln!("INVALID_REQUEST: unknown option: {arg}");
                return std::process::ExitCode::FAILURE;
            }
        }
    }
    let outcome = if http {
        kronello_mcp::http::serve_http(config, backend).await
    } else {
        kronello_mcp::serve_stdio(backend).await
    };
    match outcome {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("IO_ERROR: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
