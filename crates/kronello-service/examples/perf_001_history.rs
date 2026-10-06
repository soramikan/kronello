//! Release-only shared-API editing and historical restoration benchmark.
use kronello_model::{DocumentObject, Project};
use kronello_service::{BackendSelection, Response, Service};
use kronello_store::{OpenMode, OpenOptions, ProjectStore};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    time::Instant,
};
use uuid::Uuid;

fn request(service: &Service<'_>, value: Value) -> Value {
    let response = service.execute_json(&value.to_string());
    assert!(matches!(response, Response::Success { .. }), "{response:?}");
    serde_json::to_value(response).unwrap()["result"]["value"].clone()
}
fn handshake(kind: &str, name: &str) {
    println!("{}", json!({"event":kind,"phase":name}));
    io::stdout().flush().unwrap();
    let mut line = String::new();
    io::stdin().read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "continue");
}
fn open(path: &Path) -> ProjectStore {
    ProjectStore::open(
        path,
        OpenOptions {
            mode: OpenMode::ForceNormal,
        },
    )
    .unwrap()
}
fn elapsed(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap()
}
fn summary(mut samples: Vec<u64>) -> Value {
    samples.sort_unstable();
    let n = samples.len();
    json!({"n":n,"percentile_method":"nearest_rank","p50_ns":samples[n.div_ceil(2)-1],
        "p95_ns":samples[(95*n).div_ceil(100)-1],"samples_ns_sorted":samples})
}
fn main() {
    if cfg!(debug_assertions) {
        panic!("use cargo build --release");
    }
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "source.kronello output-directory");
    let source = PathBuf::from(&args[1]);
    let directory = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("history.kronello");
    assert!(!path.exists(), "each run requires a fresh directory");
    let service = Service::new(BackendSelection::CpuReference);
    let exported = if source
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        json!({"document":serde_json::from_slice::<Value>(&std::fs::read(&source).unwrap()).unwrap()})
    } else {
        request(
            &service,
            json!({"operation":"project.export","project":source}),
        )
    };
    let document: Project = serde_json::from_value(exported["document"].clone()).unwrap();
    let bytes = serde_json::to_vec(&serde_json::to_value(&document).unwrap()).unwrap();
    let instance = exported["document"]["template_instances"][0]["id"].clone();
    let mut opacity = exported["document"]["compositions"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|composition| composition["nodes"].as_array().unwrap())
        .flat_map(|node| node["properties"].as_array().unwrap())
        .find(|property| property["descriptor"]["key"] == "kronello.opacity")
        .unwrap()
        .clone();
    let opacity_id = Uuid::new_v4();
    opacity["id"] = json!(opacity_id);

    let DocumentObject::Known(composition) = &document.compositions[0] else {
        panic!()
    };
    let node = composition.nodes[0].id;
    let composition_id = composition.id;
    handshake("phase_start", "editing");
    request(
        &service,
        json!({"operation":"project.create","project":path,"document":document}),
    );
    let session = Uuid::new_v4();
    let mut revision = 1_u64;
    let mut references = vec![(revision, open(&path).snapshot().unwrap().document)];
    let insert = json!({"node_property_insert":{"composition":composition_id,"node":node,"property":opacity}});
    let plan = request(
        &service,
        json!({"operation":"edit.plan","project":path,"base_revision":revision.to_string(),"commands":[insert]}),
    );
    request(
        &service,
        json!({"operation":"edit.apply","project":path,"base_revision":revision.to_string(),"commands":[insert],"plan_hash":plan["plan_hash"],"session_id":session,"idempotency_key":"insert-opacity"}),
    );
    revision += 1;
    references.push((revision, open(&path).snapshot().unwrap().document));
    let mut edit_times = vec![];
    let mut template_times = vec![];
    let mut undo_times = vec![];
    let mut undo_calls = 0;
    for index in 0..128 {
        let command = match index % 4 {
            0 => {
                json!({"node_rename":{"composition":composition_id,"node":node,"name":format!("lower-third take {}",index/3)}})
            }
            1 => {
                json!({"property_source_set":{"object":node,"property":opacity_id,"source":{"kind":"constant","value":{"kind":"scalar","value":0.5+f64::from(index%5)/10.0}}}})
            }
            _ => {
                json!({"node_tags_set":{"composition":composition_id,"node":node,"tags":[format!("review-{}",index%5)]}})
            }
        };
        let start = Instant::now();
        let event = if index % 4 == 3 {
            let event = request(
                &service,
                json!({"operation":"template.set_input","project":path,"base_revision":revision.to_string(),"session_id":session,"idempotency_key":format!("template-{index}"),"instance":instance,"name":"headline","value":{"kind":"string","value":format!("日本語の字幕\n編集案 {}",index/4)}}),
            );
            template_times.push(elapsed(start));
            event
        } else {
            let plan = request(
                &service,
                json!({"operation":"edit.plan","project":path,"base_revision":revision.to_string(),"commands":[command]}),
            );
            let event = request(
                &service,
                json!({"operation":"edit.apply","project":path,"base_revision":revision.to_string(),"commands":[command],"plan_hash":plan["plan_hash"],"session_id":session,"idempotency_key":format!("edit-{index}")}),
            );
            edit_times.push(elapsed(start));
            event
        };
        revision += 1;
        references.push((revision, open(&path).snapshot().unwrap().document));
        if index % 4 == 3 {
            let start = Instant::now();
            request(
                &service,
                json!({"operation":"edit.undo","project":path,"base_revision":revision.to_string(),"session_id":session,"idempotency_key":format!("undo-{index}"),"event_id":event["id"]}),
            );
            revision += 1;
            undo_times.push(elapsed(start));
            undo_calls += 1;
            references.push((revision, open(&path).snapshot().unwrap().document));
        }
        request(
            &service,
            json!({"operation":"history.list","project":path,"since_revision":"0","limit":1000}),
        );
    }
    handshake("phase_end", "editing");
    let mut cases = serde_json::Map::new();
    cases.insert("edit_plan_apply".into(), summary(edit_times));
    cases.insert("template_set_input".into(), summary(template_times));
    cases.insert("selective_undo".into(), summary(undo_times));
    for cold in [true, false] {
        let name = if cold {
            "restore_connection_cold"
        } else {
            "restore_connection_warm"
        };
        handshake("phase_start", name);
        let shared = if cold { None } else { Some(open(&path)) };
        let mut samples = vec![];
        let mut targets = vec![];
        for round in 0..21 {
            let (target, expected) = &references[(round * 7) % references.len()];
            targets.push(*target);
            let start = Instant::now();
            let snapshot = if cold {
                open(&path).snapshot_at(*target).unwrap()
            } else {
                shared.as_ref().unwrap().snapshot_at(*target).unwrap()
            };
            samples.push(elapsed(start));
            assert_eq!(
                serde_json::to_value(snapshot.document).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
        }
        handshake("phase_end", name);
        let mut measurements = summary(samples);
        measurements["target_revisions"] = json!(targets);
        cases.insert(name.into(), measurements);
    }
    let db_bytes = std::fs::metadata(&path).unwrap().len();
    let wal_bytes = std::fs::metadata(format!("{}-wal", path.display())).map_or(0, |m| m.len());
    println!(
        "{}",
        json!({"event":"report","schema_version":1,"build_profile":"release",
        "compiled_source_id":option_env!("KRONELLO_PERF_SOURCE_ID").unwrap_or("unrecorded"),
        "source_project":source,"source_document_sha256":format!("{:x}",Sha256::digest(bytes)),
        "workload":"scripted rename/tag/opacity/template-headline edits and selective undo of actual lower-third; no human telemetry",
        "edits":129,"normal_property_edits":32,"template_input_edits":32,"undo_calls":undo_calls,"history_queries":128,"reference_snapshot_calls":references.len(),
        "historical_restore_calls":42,"final_revision":revision,"cases":cases,
        "frequency_scope":"42 scripted benchmark restores; reference_snapshot_calls are correctness validation, not editing workflow telemetry",
        "database_file_bytes":db_bytes,"wal_file_bytes_at_end":wal_bytes,
        "os_disk_cache":"not purged","restore_correctness":"all measured revisions match saved documents"})
    );
    io::stdout().flush().unwrap();
    handshake("complete", "complete");
}
