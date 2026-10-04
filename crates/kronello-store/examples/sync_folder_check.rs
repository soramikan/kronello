//! Operational check using real location detection and a separate process.
use std::{
    error::Error,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use kronello_store::{DetectedLocation, OpenMode, OpenOptions, ProjectStore};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn probe(path: &Path, mode: &str) -> String {
    let mode = match mode {
        "auto" => OpenMode::Auto,
        "normal" => OpenMode::ForceNormal,
        _ => unreachable!(),
    };
    match ProjectStore::open(path, OpenOptions { mode }) {
        Ok(store) => match store.close() {
            Ok(()) => "OPENED".into(),
            Err(error) => error.code().into(),
        },
        Err(error) => error.code().into(),
    }
}

struct ProbeChild(Option<Child>);
impl Drop for ProbeChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn second_process(path: &Path, mode: &str) -> Result<String> {
    let mut command = Command::new(std::env::current_exe()?);
    #[cfg(not(test))]
    command.arg("--probe").arg(path).arg(mode);
    #[cfg(test)]
    command
        .args(["--exact", "tests::probe_actor", "--nocapture"])
        .env("KRONELLO_SYNC_PROBE_PATH", path)
        .env("KRONELLO_SYNC_PROBE_MODE", mode);
    let mut child = ProbeChild(Some(
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?,
    ));
    let deadline = Instant::now() + Duration::from_secs(15);
    while child.0.as_mut().unwrap().try_wait()?.is_none() {
        if Instant::now() >= deadline {
            return Err("second process timed out".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.0.take().unwrap().wait_with_output()?;
    if !output.status.success() {
        return Err(format!("second process failed: {}", output.status).into());
    }
    let output = String::from_utf8(output.stdout)?;
    #[cfg(test)]
    return output
        .lines()
        .find_map(|line| line.strip_prefix("PROBE_RESULT="))
        .map(str::to_owned)
        .ok_or_else(|| "missing probe result".into());
    #[cfg(not(test))]
    Ok(output.trim().into())
}

fn check(directory: &Path, expected: &str) -> Result<Value> {
    let directory = directory.canonicalize()?;
    // Only this uniquely created subdirectory is removed; never touch an
    // existing project. All database handles and children close before cleanup.
    let scratch = tempfile::Builder::new()
        .prefix("kronello-store-003-")
        .tempdir_in(&directory)?;
    let path = scratch.path().join("probe.kronello");
    let store = ProjectStore::open(&path, OpenOptions::default())?;
    let location = match store.detected_location() {
        DetectedLocation::Local => "local",
        DetectedLocation::SyncFolder => "sync",
        DetectedLocation::NetworkFileSystem => "network",
    };
    if location != expected {
        return Err(format!("expected {expected}, detected {location}").into());
    }
    let safe = store.safe_mode();
    let journal = store.journal_mode()?;
    if safe != (location != "local") || journal != if safe { "delete" } else { "wal" } {
        return Err("Auto mode and journal do not match location".into());
    }
    let auto = second_process(&path, "auto")?;
    let normal = second_process(&path, "normal")?;
    let expected_outcome = if safe { "PROJECT_LOCKED" } else { "OPENED" };
    if auto != expected_outcome || normal != expected_outcome {
        return Err(format!("unexpected second process: auto={auto}, normal={normal}").into());
    }
    store.close()?;
    // Local control checks both normal sharing and safe-mode exclusion. The
    // reported Auto results above remain separate from this forced control.
    let forced_control = if !safe {
        let store = ProjectStore::open(
            &path,
            OpenOptions {
                mode: OpenMode::ForceSafe,
            },
        )?;
        let outcome = second_process(&path, "normal")?;
        store.close()?;
        if outcome != "PROJECT_LOCKED" {
            return Err(format!("forced safe control: {outcome}").into());
        }
        Some(outcome)
    } else {
        None
    };
    let reopened = ProjectStore::open(&path, OpenOptions::default())?;
    reopened.close()?;
    scratch.close()?;
    Ok(json!({
        "directory": directory, "detected_location": location,
        "auto_safe_mode": safe, "journal_mode": journal,
        "second_process_auto": auto, "second_process_force_normal": normal,
        "local_forced_safe_control": forced_control, "reopened": true,
        "cleaned_up": true,
    }))
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 3 && args[0] == "--probe" {
        println!(
            "{}",
            probe(Path::new(&args[1]), args[2].to_str().ok_or("invalid mode")?)
        );
        return Ok(());
    }
    if args.len() != 2 {
        return Err("usage: sync_folder_check <existing-directory> <local|sync|network>".into());
    }
    let expected = args[1].to_str().ok_or("invalid expected location")?;
    if !matches!(expected, "local" | "sync" | "network") {
        return Err("expected location must be local, sync or network".into());
    }
    println!("{}", check(Path::new(&args[0]), expected)?);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_actor() {
        if let Some(path) = std::env::var_os("KRONELLO_SYNC_PROBE_PATH") {
            let mode = std::env::var("KRONELLO_SYNC_PROBE_MODE").unwrap();
            println!("PROBE_RESULT={}", probe(Path::new(&path), &mode));
        }
    }

    #[test]
    fn local_directory_control_uses_real_detector_and_cleans_up() {
        let directory = tempfile::tempdir().unwrap();
        let report = check(directory.path(), "local").unwrap();
        assert_eq!(report["local_forced_safe_control"], "PROJECT_LOCKED");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        println!("{report}");
    }
}
