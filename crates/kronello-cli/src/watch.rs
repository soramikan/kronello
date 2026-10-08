//! FLOW-003 watch folder (ADR-0130): a long-running deterministic poller,
//! never a resident service feature. New regular files become eligible once
//! their size and content hash repeat across two consecutive polls; each
//! stable file submits one `export.batch` item resolved server-side from the
//! stored preset. Detection order is the lexical path order, failures are
//! logged as typed JSON lines and never silently dropped.
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use kronello_service::{
    BatchFailurePolicy, ExportBatchItem, ExportBatchOutcome, ExportBatchRequest, ProjectRequest,
    Request, Response, ResultData, Service, ServiceError, preset_output_extension,
};

/// Parsed `kronello watch` arguments. The request-side knobs --poll-ms and
/// --once keep the loop deterministic and testable.
pub struct WatchArgs {
    pub project: PathBuf,
    pub directory: PathBuf,
    /// Preset id (UUID) or name, resolved once against the project document.
    pub preset: String,
    pub output: PathBuf,
    pub poll_ms: u64,
    pub once: bool,
}
fn usage() -> ServiceError {
    ServiceError::invalid(
        "kronello watch --project P --directory D --preset ID|NAME --output DIR [--poll-ms N] [--once]",
    )
}
/// Lexical normalization for non-existent paths: resolve `.`/`..` components
/// without touching the filesystem (destination may not exist yet).
fn normalize_lexical(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}
/// Canonical form of a possibly-non-existent destination: the deepest existing
/// ancestor resolves symlinks, then the remaining suffix is reattached. A
/// purely lexical form would miss `/var` vs `/private/var`-style aliases when
/// compared against a canonicalized watch directory.
fn normalize_output(path: &Path) -> PathBuf {
    let absolute = normalize_lexical(path);
    let mut probe = absolute.clone();
    let mut tail = Vec::new();
    loop {
        if let Ok(canonical) = probe.canonicalize() {
            let mut out = canonical;
            for part in tail.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match probe.file_name() {
            Some(name) => {
                tail.push(name.to_os_string());
                probe.pop();
            }
            None => return absolute,
        }
    }
}
pub fn parse(args: &[String]) -> Result<WatchArgs, ServiceError> {
    let mut project = None;
    let mut directory = None;
    let mut preset = None;
    let mut output = None;
    let mut poll_ms = 1000_u64;
    let mut once = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" => project = Some(PathBuf::from(args.next().ok_or_else(usage)?)),
            "--directory" => directory = Some(PathBuf::from(args.next().ok_or_else(usage)?)),
            "--preset" => preset = Some(args.next().ok_or_else(usage)?.to_owned()),
            "--output" => output = Some(PathBuf::from(args.next().ok_or_else(usage)?)),
            "--poll-ms" => {
                poll_ms =
                    args.next().ok_or_else(usage)?.parse().map_err(|_| {
                        ServiceError::invalid("--poll-ms requires an unsigned integer")
                    })?;
            }
            "--once" => once = true,
            _ => return Err(usage()),
        }
    }
    let (project, directory, preset, output) = match (project, directory, preset, output) {
        (Some(project), Some(directory), Some(preset), Some(output)) => {
            (project, directory, preset, output)
        }
        _ => return Err(usage()),
    };
    if poll_ms == 0 {
        return Err(ServiceError::invalid("--poll-ms must be positive"));
    }
    // Canonicalize early: scanned paths share the canonical directory prefix,
    // so the project-file exclusion and the output-inside check compare like
    // with like.
    let project = project.canonicalize().map_err(|e| {
        ServiceError::new(
            if e.kind() == std::io::ErrorKind::NotFound {
                "PROJECT_NOT_FOUND"
            } else {
                "IO_ERROR"
            },
            e.to_string(),
        )
    })?;
    let directory = directory.canonicalize().map_err(|e| {
        ServiceError::new(
            if e.kind() == std::io::ErrorKind::NotFound {
                "WATCH_DIRECTORY_MISSING"
            } else {
                "IO_ERROR"
            },
            e.to_string(),
        )
    })?;
    // Exporting into the watched tree would feed results back as new inputs.
    let normalized_output = normalize_output(&output);
    if normalized_output.starts_with(&directory) {
        return Err(ServiceError::invalid(
            "watch output must not be inside the watched directory",
        ));
    }
    Ok(WatchArgs {
        project,
        directory,
        preset,
        output,
        poll_ms,
        once,
    })
}
/// Sorted top-level regular files of the watched directory. Hidden names are
/// inert data, never watched inputs.
fn scan(directory: &Path, project: &Path) -> Result<Vec<PathBuf>, ServiceError> {
    let mut entries = std::fs::read_dir(directory)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|e| e.path())
        .filter(|path| {
            path.is_file()
                && path != project
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| !n.starts_with('.'))
        })
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries)
}
#[derive(Debug, Clone, PartialEq)]
enum FileState {
    /// Seen once; awaiting a second identical size before hashing.
    Seen(u64),
    /// Size+hash matched once already; the next identical poll submits.
    Hashed { size: u64, hash: String },
    /// A terminal batch outcome was recorded; never resubmitted by this run.
    Processed,
}
fn log_line(value: &serde_json::Value) {
    let mut stderr = std::io::stderr().lock();
    let _ = serde_json::to_writer(&mut stderr, value);
    let _ = stderr.write_all(b"\n");
    let _ = stderr.flush();
}
fn sanitize_stem(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("input");
    let sanitized: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if sanitized.is_empty() {
        "input".into()
    } else {
        sanitized
    }
}
/// Submit one stable batch and report each item deterministically. A file is
/// marked processed for every terminal outcome — including failure — so a
/// persistent error is logged once instead of retried every poll.
fn submit_stable(
    service: &Service,
    args: &WatchArgs,
    preset_id: kronello_model::ExportPresetId,
    extension: Option<&str>,
    files: &[PathBuf],
    hashes: &BTreeMap<PathBuf, String>,
    states: &mut BTreeMap<PathBuf, FileState>,
) -> bool {
    let items: Vec<ExportBatchItem> = files
        .iter()
        .map(|file| {
            let hash = &hashes[file];
            let suffix = extension.map_or(String::new(), |e| format!(".{e}"));
            ExportBatchItem {
                idempotency_key: None,
                submission: None,
                preset: Some(preset_id),
                project: Some(args.project.clone()),
                destination: Some(args.output.join(format!(
                    "{}-{}{}",
                    sanitize_stem(file),
                    &hash[..12],
                    suffix
                ))),
            }
        })
        .collect();
    let response = service.execute(Request::ExportBatch(ExportBatchRequest {
        items,
        // Watch is a deduplicated trigger: one failure never blocks the rest.
        failure_policy: BatchFailurePolicy::Continue,
    }));
    let mut ok = true;
    match response {
        Response::Success {
            result: ResultData::Batch(batch),
        } => {
            let mut stdout = std::io::stdout().lock();
            for (file, item) in files.iter().zip(batch.items.iter()) {
                states.insert(file.clone(), FileState::Processed);
                let line = serde_json::json!({
                    "watch": match item.outcome {
                        ExportBatchOutcome::Submitted => "submitted",
                        ExportBatchOutcome::Replayed => "replayed",
                        ExportBatchOutcome::Skipped => "skipped",
                        ExportBatchOutcome::Failed => "failed",
                    },
                    "file": file,
                    "idempotency_key": item.idempotency_key,
                    "job": item.job.as_ref().map(|j| &j.id),
                    "error": item.error,
                });
                let _ = serde_json::to_writer(&mut stdout, &line);
                let _ = stdout.write_all(b"\n");
                if item.outcome == ExportBatchOutcome::Failed {
                    ok = false;
                }
            }
            let _ = stdout.flush();
        }
        other => {
            ok = false;
            let error = match &other {
                Response::Error { error } => error.clone(),
                _ => ServiceError::new("INTERNAL_ERROR", "unexpected export.batch response"),
            };
            for file in files {
                states.insert(file.clone(), FileState::Processed);
            }
            log_line(&serde_json::json!({
                "watch": "batch_failed",
                "files": files,
                "error": error,
            }));
        }
    }
    ok
}
/// Run the watch loop. `--once` performs exactly one scan+submit pass and
/// reports an error exit code when any item failed; continuous mode polls
/// forever and keeps running across per-file failures.
pub fn run(
    args: &[String],
    selection: kronello_service::BackendSelection,
) -> Result<Response, ServiceError> {
    let args = parse(args)?;
    let service = Service::new(selection);
    // Resolve the preset once so a typo fails fast instead of per poll.
    let document = match service.execute(Request::ProjectExport(ProjectRequest {
        project: args.project.clone(),
    })) {
        Response::Success {
            result: ResultData::Export(export),
        } => export.document,
        Response::Error { error } => return Err(error),
        _ => {
            return Err(ServiceError::new(
                "INTERNAL_ERROR",
                "unexpected project.export response",
            ));
        }
    };
    let preset = document
        .export_presets
        .iter()
        .find(|p| p.id.to_string() == args.preset || p.name == args.preset)
        .ok_or_else(|| ServiceError::new("PRESET_MISSING", "export preset not found"))?;
    let extension = preset_output_extension(preset)?;
    let preset_id = preset.id;
    let mut states: BTreeMap<PathBuf, FileState> = BTreeMap::new();
    let mut ok = true;
    loop {
        let files = scan(&args.directory, &args.project)?;
        let present: std::collections::BTreeSet<_> = files.iter().cloned().collect();
        states.retain(|path, _| present.contains(path));
        let mut ready = Vec::new();
        let mut hashes = BTreeMap::new();
        for file in &files {
            if states.get(file) == Some(&FileState::Processed) {
                continue;
            }
            let size = match std::fs::metadata(file) {
                Ok(metadata) => metadata.len(),
                Err(_) => continue,
            };
            // --once treats a first successful hash as stable immediately:
            // a one-shot pass has no earlier observation to confirm against.
            let next = match states.get(file) {
                None if args.once => match kronello_media::content_hash(file) {
                    Ok(hash) => {
                        ready.push(file.clone());
                        hashes.insert(file.clone(), hash);
                        FileState::Seen(size)
                    }
                    Err(_) => FileState::Seen(size),
                },
                None => FileState::Seen(size),
                Some(FileState::Seen(previous)) if *previous == size => {
                    match kronello_media::content_hash(file) {
                        Ok(hash) => FileState::Hashed { size, hash },
                        Err(_) => FileState::Seen(size),
                    }
                }
                Some(FileState::Seen(_)) => FileState::Seen(size),
                Some(FileState::Hashed {
                    size: previous,
                    hash,
                }) if *previous == size => match kronello_media::content_hash(file) {
                    Ok(now) if now == *hash => {
                        ready.push(file.clone());
                        hashes.insert(file.clone(), now);
                        FileState::Processed
                    }
                    Ok(now) => FileState::Hashed { size, hash: now },
                    Err(_) => FileState::Seen(size),
                },
                Some(FileState::Hashed { .. }) => FileState::Seen(size),
                Some(FileState::Processed) => FileState::Processed,
            };
            states.insert(file.clone(), next);
        }
        if !ready.is_empty() {
            ok &= submit_stable(
                &service,
                &args,
                preset_id,
                extension,
                &ready,
                &hashes,
                &mut states,
            );
        }
        if args.once {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(args.poll_ms));
    }
    if ok {
        Ok(Response::Success {
            result: ResultData::Batch(Box::new(kronello_service::ExportBatchResult {
                items: Vec::new(),
            })),
        })
    } else {
        Err(ServiceError::new(
            "WATCH_SUBMIT_FAILED",
            "one or more watch submissions failed; see stderr log lines",
        ))
    }
}
