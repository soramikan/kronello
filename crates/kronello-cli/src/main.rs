//! Machine transport adapter. All document and execution policy lives in service.
use std::io::{Read, Write};

use kronello_service::{BackendSelection, Request, Response, Service, ServiceError};

const USAGE: &str = "kronello [--backend gpu|cpu-reference] [--request-json JSON] [project create|import|export|info|collect | asset relink | render frame|sequence|submit|explain | node explain | job get|list|cancel|prune | edit plan|apply|undo | history list | scene query | property sample | capabilities get | sequence create|query | clip place|trim|stretch | instance retime | template_instance retime | template define|instantiate|set_input|set_duration|preview|migration_plan] | worker --job <id>; otherwise read a tagged service Request from stdin";
fn run() -> Result<Response, ServiceError> {
    let mut selection = BackendSelection::Gpu;
    let mut literal = None;
    let mut command = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--backend" => {
                selection = match args.next().as_deref() {
                    Some("gpu") => BackendSelection::Gpu,
                    Some("cpu-reference") => BackendSelection::CpuReference,
                    _ => {
                        return Err(ServiceError::invalid(
                            "--backend requires gpu or cpu-reference",
                        ));
                    }
                }
            }
            "--request-json" if literal.is_none() => {
                literal = Some(
                    args.next()
                        .ok_or_else(|| ServiceError::invalid("--request-json requires JSON"))?,
                )
            }
            "--help" | "-h" => return Err(ServiceError::new("USAGE", USAGE)),
            _ if arg.starts_with('-') => {
                return Err(ServiceError::invalid(format!("unknown option: {arg}")));
            }
            _ => command.push(arg),
        }
    }
    let operation = match command
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => None,
        ["project", verb @ ("create" | "import" | "export" | "info")] => {
            Some(format!("project.{verb}"))
        }
        ["sequence", verb @ ("create" | "query")] => Some(format!("sequence.{verb}")),
        ["clip", verb @ ("place" | "trim" | "stretch")] => Some(format!("clip.{verb}")),
        [kind @ ("instance" | "template_instance"), "retime"] => Some(format!("{kind}.retime")),
        ["render", verb @ ("frame" | "sequence" | "submit")] => Some(format!("render.{verb}")),
        ["edit", verb @ ("plan" | "apply" | "undo")] => Some(format!("edit.{verb}")),
        [
            "template",
            verb @ ("define" | "instantiate" | "set_input" | "set_duration" | "preview"
            | "migration_plan"),
        ] => Some(format!("template.{verb}")),
        ["job", verb @ ("get" | "list" | "cancel" | "prune")] => Some(format!("job.{verb}")),
        ["history", "list"] => Some("history.list".into()),
        ["scene", "query"] => Some("scene.query".into()),
        ["node", "explain"] => Some("node.explain".into()),
        ["render", "explain"] => Some("render.explain".into()),
        ["property", "sample"] => Some("property.sample".into()),
        ["asset", "relink"] => Some("asset.relink".into()),
        ["project", "collect"] => Some("project.collect".into()),
        ["capabilities", "get"] => Some("capabilities.get".into()),
        _ => return Err(ServiceError::invalid(USAGE)),
    };
    let json = if let Some(json) = literal {
        json
    } else {
        let mut json = String::new();
        std::io::stdin()
            .take(16 * 1024 * 1024 + 1)
            .read_to_string(&mut json)?;
        json
    };
    if json.len() > 16 * 1024 * 1024 {
        return Err(ServiceError::invalid("request exceeds 16 MiB"));
    }
    let request: Request = if let Some(operation) = operation {
        let tail = json
            .trim_start()
            .strip_prefix('{')
            .ok_or_else(|| ServiceError::invalid("request must be an object"))?;
        // Prefix the transport tag without a Value round trip, so duplicate
        // payload fields remain visible to strict typed deserialization.
        let separator = if tail.trim_start().starts_with('}') {
            ""
        } else {
            ","
        };
        serde_json::from_str(&format!("{{\"operation\":\"{operation}\"{separator}{tail}"))?
    } else {
        serde_json::from_str(&json)?
    };
    Ok(Service::new(selection).execute(request))
}
fn main() -> std::process::ExitCode {
    if let Some(exit) = kronello_service::worker_entry() {
        return exit;
    }
    let response = run().unwrap_or_else(|error| Response::Error { error });
    let failed = matches!(&response, Response::Error { .. });
    if let Response::Error { error } = &response {
        eprintln!("{error}");
    }
    let mut stdout = std::io::stdout().lock();
    if let Err(error) = serde_json::to_writer(&mut stdout, &response)
        .map_err(std::io::Error::other)
        .and_then(|()| stdout.write_all(b"\n"))
        .and_then(|()| stdout.flush())
    {
        eprintln!("OUTPUT_IO_ERROR: {error}");
        return std::process::ExitCode::FAILURE;
    }
    if failed {
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}
