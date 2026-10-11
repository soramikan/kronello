//! Release service preview preparation probe; no GPU decode/presentation.
use kronello_service::{BackendSelection, FrameRenderRequest, ProjectSession, Service};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Instant};

fn main() {
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let directory = PathBuf::from(std::env::args().nth(1).expect("output directory"));
    let stateless = std::env::args()
        .nth(2)
        .is_some_and(|mode| mode == "stateless");
    std::fs::create_dir_all(&directory).unwrap();
    let mut cases = Vec::new();
    for count in [100, 1000, 5000] {
        let mut document: serde_json::Value =
            serde_json::from_str(include_str!("../../../examples/nle-001.project.json")).unwrap();
        let sequence = document["sequences"][0]["id"].clone();
        let original = document["sequences"][0]["tracks"][0]["clips"][0].clone();
        document["sequences"][0]["tracks"]
            .as_array_mut()
            .unwrap()
            .truncate(1);
        document["sequences"][0]["tracks"][0]["clips"] = (0..count)
            .map(|i| {
                let mut clip = original.clone();
                clip["id"] = json!(uuid::Uuid::from_u128(i + 1));
                clip["timeline_range"] = json!({"start":{"num":i.to_string(),"den":"1"},
                    "end":{"num":(i+1).to_string(),"den":"1"}});
                clip
            })
            .collect();
        let path = directory.join(format!("prepare-{count}.kronello"));
        let service = Service::new(BackendSelection::CpuReference);
        let result = serde_json::to_value(service.execute_json(
            &json!({"operation":"project.create","project":path,"document":document}).to_string(),
        ))
        .unwrap();
        assert_eq!(result["status"], "success", "{result}");
        let session = if stateless {
            None
        } else {
            Some(ProjectSession::open(&path).unwrap())
        };
        let mut samples = Vec::new();
        let mut revisions = Vec::new();
        let mut dag_hashes = Vec::new();
        for i in 0..22 {
            let request: FrameRenderRequest = serde_json::from_value(json!({
                "input":{"project":path,"target":{"kind":"sequence","sequence":sequence},
                    "region":{"origin":[0,0],"extent":[64,32],"pixels":[64,32]}},
                "time":{"num":i.to_string(),"den":"24"}
            }))
            .unwrap();
            let start = Instant::now();
            let run = || service.preview_dag_unresolved(&request);
            let (revision, dag) = match &session {
                Some(session) => session.scope(run),
                None => run(),
            }
            .unwrap();
            std::hint::black_box(&dag);
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
            revisions.push(revision);
            dag_hashes.push(format!("{:x}", Sha256::digest(format!("{dag:?}"))));
        }
        let mut warm = samples[1..].to_vec();
        warm.sort_by(f64::total_cmp);
        let stats = session
            .as_ref()
            .map(ProjectSession::preparation_stats)
            .unwrap_or_default();
        cases.push(
            json!({"clips":count,"cold_ms":samples[0],"warm_p50_ms":warm[10],
            "warm_p95_ms":warm[19],"samples_ms":samples,"revisions":revisions,
            "dag_hashes":dag_hashes,"preparations":stats.preparations,"cache_hits":stats.hits}),
        );
    }
    println!(
        "{}",
        json!({"path":"shared service preview preparation; no GPU", "stateless":stateless, "cases":cases})
    );
}
