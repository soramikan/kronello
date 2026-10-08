//! Machine transport adapter. All document and execution policy lives in service.
use std::io::{Read, Write};
mod events;
mod watch;

use kronello_service::{BackendSelection, Request, Response, Service, ServiceError};

const USAGE: &str = "kronello [--events ndjson] [--backend gpu|cpu-reference|gpu-resident-bgra8|gpu-resident-nv12] [--request-json JSON] [project create|create_plan|import|import_plan|export|info|collect | asset relink|thumbnail | media query | export batch | render frame|sequence|export|submit|explain | node explain | job get|list|cancel|prune | track analyze | audio analyze|loudness|normalize|plugin_probe|plugin_process | proxy generate|status|clear | scene detect|apply | edit plan|apply|undo|insert|overwrite | expression format | history list | scene query | property sample | capabilities get | sequence create|query | clip place|trim|stretch|angle_switch | multicam create | instance retime | template_instance retime | template define|instantiate|set_input|set_duration|preview|migration_plan | captions import_plan|import|export] | watch --project P --directory D --preset ID|NAME --output DIR [--poll-ms N] [--once] | worker --job <id> | plugin-helper; otherwise read a tagged service Request from stdin";
fn run(stream: Option<&events::Stream>) -> Result<Response, ServiceError> {
    let mut selection = BackendSelection::Gpu;
    let mut literal = None;
    let mut command = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--events" => {
                if args.next().as_deref() != Some("ndjson") {
                    return Err(ServiceError::invalid("--events requires ndjson"));
                }
            }
            "--backend" => {
                selection = match args.next().as_deref() {
                    Some("gpu") => BackendSelection::Gpu,
                    Some("cpu-reference") => BackendSelection::CpuReference,
                    Some("gpu-resident-bgra8") => BackendSelection::GpuResidentBgra8,
                    Some("gpu-resident-nv12") => BackendSelection::GpuResidentNv12,
                    _ => {
                        return Err(ServiceError::invalid(
                            "--backend requires gpu, cpu-reference, gpu-resident-bgra8 or gpu-resident-nv12",
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
            // `watch` owns its flag tail; values are data, including ones
            // that look like global options.
            _ if command.first().map(String::as_str) == Some("watch") => command.push(arg),
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
        [
            "project",
            verb @ ("create" | "create_plan" | "import" | "import_plan" | "export" | "info"),
        ] => Some(format!("project.{verb}")),
        ["sequence", verb @ ("create" | "query")] => Some(format!("sequence.{verb}")),
        [
            "clip",
            verb @ ("place" | "trim" | "stretch" | "angle_switch"),
        ] => Some(format!("clip.{verb}")),
        ["multicam", verb @ "create"] => Some(format!("multicam.{verb}")),
        [kind @ ("instance" | "template_instance"), "retime"] => Some(format!("{kind}.retime")),
        [
            "render",
            verb @ ("frame" | "sequence" | "export" | "submit"),
        ] => Some(format!("render.{verb}")),
        [
            "edit",
            verb @ ("plan" | "apply" | "undo" | "insert" | "overwrite"),
        ] => Some(format!("edit.{verb}")),
        ["expression", "format"] => Some("expression.format".into()),
        [
            "template",
            verb @ ("define" | "instantiate" | "set_input" | "set_duration" | "preview"
            | "migration_plan"),
        ] => Some(format!("template.{verb}")),
        ["job", verb @ ("get" | "list" | "cancel" | "prune")] => Some(format!("job.{verb}")),
        ["track", "analyze"] => Some("track.analyze".into()),
        [
            "audio",
            verb @ ("analyze" | "loudness" | "normalize" | "plugin_probe" | "plugin_process"),
        ] => Some(format!("audio.{verb}")),
        ["proxy", verb @ ("generate" | "status" | "clear")] => Some(format!("proxy.{verb}")),
        ["history", "list"] => Some("history.list".into()),
        ["scene", verb @ ("query" | "detect" | "apply")] => Some(format!("scene.{verb}")),
        ["node", "explain"] => Some("node.explain".into()),
        ["render", "explain"] => Some("render.explain".into()),
        ["property", "sample"] => Some("property.sample".into()),
        ["asset", "relink"] => Some("asset.relink".into()),
        ["asset", "thumbnail"] => Some("asset.thumbnail".into()),
        ["media", "query"] => Some("media.query".into()),
        ["export", "batch"] => Some("export.batch".into()),
        ["project", "collect"] => Some("project.collect".into()),
        ["capabilities", "get"] => Some("capabilities.get".into()),
        ["captions", verb @ ("import_plan" | "import" | "export")] => {
            Some(format!("captions.{verb}"))
        }
        ["watch", ..] => return watch::run(&command[1..], selection),
        _ => return Err(ServiceError::invalid(USAGE)),
    };
    let json = if let Some(json) = literal {
        json
    } else if let Some(stream) = stream {
        stream.read_request()?
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
    Ok(if let Some(stream) = stream {
        Service::new(selection).execute_json_with_control(&serde_json::to_string(&request)?, stream)
    } else {
        Service::new(selection).execute(request)
    })
}
fn main() -> std::process::ExitCode {
    if let Some(exit) = kronello_service::worker_entry() {
        return exit;
    }
    // AUDIO-011: `kronello plugin-helper` is the detached plugin host entry.
    if let Some(exit) = kronello_service::plugin_helper_entry() {
        return exit;
    }
    // Option values are data, including malformed JSON equal to a flag name.
    let mut args = std::env::args().skip(1);
    let mut wants_events = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--backend" | "--request-json" => {
                args.next();
            }
            "--events" => {
                wants_events = true;
                break;
            }
            _ => {}
        }
    }
    let stream = if wants_events {
        match events::Stream::start() {
            Ok(stream) => Some(stream),
            Err(error) => {
                eprintln!("{error}");
                return std::process::ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    let response = run(stream.as_ref()).unwrap_or_else(|error| Response::Error { error });
    let failed = matches!(&response, Response::Error { .. });
    if let Response::Error { error } = &response {
        eprintln!("{error}");
    }
    if let Some(stream) = stream {
        return if stream.finish(response) {
            std::process::ExitCode::FAILURE
        } else {
            std::process::ExitCode::SUCCESS
        };
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
