//! VST3/AU plugin hosting trust boundary (ADR-0131, AUDIO-011).
//!
//! Plugin code is never loaded into UI, service or render worker processes.
//! The only process that calls into a plugin bundle is the detached
//! `kronello-plugin-host` helper (or the `plugin-helper` entry point of a
//! binary embedding it), spawned by a `kronello-jobs` worker. This library
//! carries:
//!
//! - [`PluginSpec`] and related hash-pinned plugin identity types shared by
//!   the service API, the fixed job input and the helper protocol;
//! - the safe [`run_helper`] orchestration a worker calls;
//! - the audited `abi` module (VST3 COM-compatible ABI and macOS
//!   AudioToolbox FFI) reachable only through [`plugin_host_main`].
#![deny(unsafe_code)]

mod error;
pub use error::PluginError;
mod spec;
pub use spec::*;
mod protocol;
pub use protocol::*;
mod host;
pub use host::*;
mod helper;
pub use helper::{helper_entry, plugin_host_main};
#[allow(unsafe_code)]
mod abi;
#[cfg(feature = "test-support")]
pub mod test_support;
