/// Human confirmation schema for `kill_process` (rmcp elicitation).
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct KillProcessHumanConfirm {
    /// Set to true only if a human operator explicitly approved terminating this process.
    pub confirm_terminate: bool,
}

rmcp::elicit_safe!(KillProcessHumanConfirm);
