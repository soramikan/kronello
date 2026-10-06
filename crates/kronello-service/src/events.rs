//! CLI transport framing; document operations remain in the shared registry.
use crate::Response;
use serde::{Deserialize, Serialize};

pub const CLI_EVENT_VERSION: u32 = 1;
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]
pub enum CliEvent {
    Header {
        version: u32,
        sequence: u64,
    },
    Progress {
        sequence: u64,
        completed: u64,
        total: u64,
    },
    Terminal {
        sequence: u64,
        outcome: CliEventOutcome,
        response: Box<Response>,
    },
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CliEventOutcome {
    End,
    Cancelled,
    Error,
}
