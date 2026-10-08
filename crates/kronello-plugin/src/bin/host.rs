//! Standalone detached plugin-host binary (ADR-0131). Spawned only by a
//! `kronello-jobs` worker; speaks the helper JSON protocol on stdin/stdout
//! and is the only process that loads plugin code.
fn main() -> std::process::ExitCode {
    kronello_plugin::plugin_host_main()
}
