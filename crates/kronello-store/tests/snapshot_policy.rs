//! Reproducible policy experiment; the candidate is confined to this test DB.
use std::{collections::BTreeSet, fs, hint::black_box, time::Instant};

use kronello_model::{
    Composition, CompositionId, CompositionInstanceId, DesignExtent, DocumentObject, Project,
    TemplateConstraints, TemplateDefinition, TemplateDurationPolicy, TemplateInstance,
    TemplateMiddleMode,
};
use kronello_store::{ApplyRequest, Mutation, OpenMode, OpenOptions, ProjectStore};
use kronello_time::{Duration, FrameRate, Time};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use uuid::Uuid;

fn should_snapshot(revision: u64, cumulative_patch_bytes: usize, document_bytes: usize) -> bool {
    revision.is_multiple_of(64) || cumulative_patch_bytes > document_bytes
}

#[test]
fn candidate_threshold_is_strict_and_periodic_checkpoint_is_preserved() {
    assert!(!should_snapshot(63, 100, 100));
    assert!(should_snapshot(63, 101, 100));
    assert!(should_snapshot(64, 0, 100));
    // Inverse bytes are deliberately excluded; a smaller current document
    // can trigger the rule even when earlier patches were small.
    assert!(should_snapshot(1, 100, 99));
}

enum Edit {
    Patch(Vec<Mutation>),
    Import(Box<Project>),
}

fn template_document() -> Project {
    let duration = Duration::new(Time::new(5, 1).unwrap()).unwrap();
    let composition = Composition {
        id: CompositionId::new(),
        duration,
        design_extent: DesignExtent::new(1920.0, 1080.0).unwrap(),
        edit_rate: FrameRate::new(30, 1).unwrap(),
        root_nodes: vec![],
        nodes: vec![],
        properties: vec![],
    };
    let definitions: Vec<_> = (0..32)
        .map(|_| TemplateDefinition {
            id: Uuid::new_v4(),
            template_id: Uuid::new_v4(),
            version: "1.0.0".into(),
            composition_ref: composition.id,
            public_inputs: Default::default(),
            variants: Default::default(),
            duration_policy: TemplateDurationPolicy {
                intro: Duration::new(Time::new(1, 1).unwrap()).unwrap(),
                outro: Duration::new(Time::new(1, 1).unwrap()).unwrap(),
                minimum_middle: Duration::new(Time::new(1, 1).unwrap()).unwrap(),
                middle_mode: TemplateMiddleMode::Stretch,
            },
            constraints: TemplateConstraints::default(),
            content_hash: "a".repeat(64),
        })
        .collect();
    let instances = (0..128)
        .map(|index| {
            DocumentObject::Known(TemplateInstance {
                id: CompositionInstanceId::new(),
                definition_ref: definitions[index % definitions.len()].id,
                version: "1.0.0".into(),
                duration,
                variant: None,
                inputs: Default::default(),
            })
        })
        .collect();
    Project {
        compositions: vec![DocumentObject::Known(composition)],
        templates: definitions.into_iter().map(DocumentObject::Known).collect(),
        template_instances: instances,
        ..Project::default()
    }
}

fn histories() -> Vec<(&'static str, Vec<Edit>)> {
    let small = (0..256)
        .map(|revision| {
            Edit::Patch(vec![Mutation::Set {
                path: vec!["name".into()],
                value: json!(format!("edit {revision:04}")),
            }])
        })
        .collect();
    let mut large = Project::default();
    let huge = (0..12)
        .map(|revision| {
            large.name = format!("{revision:04}{}", "字幕".repeat(32 * 1024));
            Edit::Import(Box::new(large.clone()))
        })
        .collect();
    let mut template = template_document();
    let mut templates = vec![Edit::Import(Box::new(template.clone()))];
    // Keep the opt-in experiment short; the small-edit history separately
    // exercises multiple periodic checkpoints.
    for revision in 1..32 {
        for instance in &mut template.template_instances {
            let DocumentObject::Known(instance) = instance else {
                unreachable!()
            };
            instance.duration = Duration::new(Time::new(5 + revision % 3, 1).unwrap()).unwrap();
        }
        templates.push(Edit::Patch(vec![Mutation::Set {
            path: vec!["template_instances".into()],
            value: serde_json::to_value(&template.template_instances).unwrap(),
        }]));
    }
    vec![
        ("many_small", small),
        ("few_huge", huge),
        ("template_heavy", templates),
    ]
}

fn evaluate(name: &str, edits: &[Edit], adaptive: bool) -> Value {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("evaluation.kronello");
    let options = OpenOptions {
        mode: OpenMode::ForceNormal,
    };
    let mut store = ProjectStore::open(&path, options.clone()).unwrap();
    let mut reference = vec![store.snapshot().unwrap().document];
    let initial_bytes = serde_json::to_vec(&reference[0]).unwrap().len();
    let mut logical_write_bytes = 2 * initial_bytes;
    let mut cumulative = 0;
    for (base, edit) in edits.iter().enumerate() {
        let event = match edit {
            Edit::Patch(mutations) => store.apply(ApplyRequest {
                base_revision: base as u64,
                session_id: Uuid::new_v4(),
                mutations: mutations.clone(),
                changed_keys: BTreeSet::new(),
                idempotency_key: None,
                undo_of: None,
            }),
            Edit::Import(project) => store.import_json(
                base as u64,
                Uuid::new_v4(),
                &serde_json::to_string(project).unwrap(),
            ),
        }
        .unwrap();
        let document = store.snapshot().unwrap().document;
        let encoded = serde_json::to_string(&document).unwrap();
        let patch_bytes = serde_json::to_vec(&event.mutations).unwrap().len();
        cumulative += patch_bytes;
        // Logical payload written, including repeated current-document updates,
        // event JSON columns and UUID/revision fields. Not physical disk I/O.
        logical_write_bytes += encoded.len()
            + patch_bytes
            + serde_json::to_vec(&event.inverse).unwrap().len()
            + serde_json::to_vec(&event.changed_keys).unwrap().len()
            + 80;
        let periodic = event.revision.is_multiple_of(64);
        let checkpoint =
            periodic || (adaptive && should_snapshot(event.revision, cumulative, encoded.len()));
        if checkpoint {
            if !periodic {
                // Experimental layout only. The separate transaction makes
                // write latency unsuitable for comparison; it is not measured.
                store
                    .migrate_schema(1, |tx| {
                        tx.execute(
                            "INSERT INTO snapshots VALUES(?1,?2)",
                            params![event.revision as i64, encoded],
                        )?;
                        Ok(())
                    })
                    .unwrap();
            }
            logical_write_bytes += encoded.len();
            cumulative = 0;
        }
        reference.push(document);
    }
    store.close().unwrap();
    let file_bytes = fs::metadata(&path).unwrap().len();
    let store = ProjectStore::open(&path, options).unwrap();
    let sql = Connection::open(&path).unwrap();
    let checkpoints: Vec<u64> = sql
        .prepare("SELECT revision FROM snapshots ORDER BY revision")
        .unwrap()
        .query_map([], |row| Ok(row.get::<_, i64>(0)? as u64))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();
    let snapshot_bytes: u64 = sql
        .query_row(
            "SELECT sum(length(CAST(document AS BLOB))) FROM snapshots",
            [],
            |row| Ok(row.get::<_, i64>(0)? as u64),
        )
        .unwrap();
    let patches: Vec<_> = (1..reference.len() as u64)
        .map(|revision| {
            revision
                - checkpoints
                    .iter()
                    .copied()
                    .filter(|base| *base <= revision)
                    .max()
                    .unwrap()
        })
        .collect();
    // Warm both layouts before timing; compare each revision to an independent
    // full-history document retained after each apply, outside the timer.
    for (revision, expected) in reference.iter().enumerate() {
        assert_eq!(
            &store.snapshot_at(revision as u64).unwrap().document,
            expected
        );
    }
    let mut rounds = Vec::new();
    for _ in 0..3 {
        let mut nanos = 0;
        for (revision, expected) in reference.iter().enumerate().skip(1) {
            let start = Instant::now();
            let restored = black_box(store.snapshot_at(revision as u64).unwrap());
            nanos += start.elapsed().as_nanos();
            assert_eq!(&restored.document, expected);
        }
        rounds.push(nanos as f64 / edits.len() as f64 / 1000.0);
    }
    rounds.sort_by(f64::total_cmp);
    let report = json!({
        "history": name, "policy": if adaptive { "adaptive" } else { "fixed64" },
        "edits": edits.len(), "snapshot_count": checkpoints.len(),
        "snapshot_payload_bytes": snapshot_bytes, "file_bytes": file_bytes,
        "logical_write_bytes": logical_write_bytes,
        "max_replay_patches": patches.iter().max().unwrap(),
        "mean_replay_patches": patches.iter().sum::<u64>() as f64 / edits.len() as f64,
        "restore_mean_us_median_of_3": rounds[1],
        "verified_revisions": reference.len(),
    });
    drop(sql);
    store.close().unwrap();
    report
}

#[test]
#[ignore = "policy measurement; run explicitly with --ignored --nocapture"]
fn snapshot_policy_evaluation() {
    for (name, edits) in histories() {
        for adaptive in [false, true] {
            println!("{}", evaluate(name, &edits, adaptive));
        }
    }
}
