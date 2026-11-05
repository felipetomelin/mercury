//! Cron API contracts. Commands never accept a persisted job id or next-run
//! timestamp on creation; the server remains the only scheduler writer.

use std::path::PathBuf;

use mercury_cron::{CatchUp, Delivery, JobTarget};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CronPreviewParams {
    pub schedule: String,
    pub utc_offset: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CronJobDefinition {
    pub name: String,
    pub schedule: String,
    pub utc_offset: Option<String>,
    pub target: JobTarget,
    pub workdir: Option<PathBuf>,
    pub timeout_seconds: Option<u64>,
    pub enabled: Option<bool>,
    #[serde(default)]
    pub catch_up: CatchUp,
    pub repeat_times: Option<u32>,
    #[serde(default)]
    pub deliver: Vec<Delivery>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CronAddParams {
    pub definition: CronJobDefinition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CronEditParams {
    pub job_id: String,
    pub definition: CronJobDefinition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CronJobTarget {
    pub job_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CronPauseParams {
    pub job_id: String,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CronRunsParams {
    pub job_id: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CronLogParams {
    pub run_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CronJobInfo {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub next_run_at: Option<i64>,
    pub enabled: bool,
    pub target_kind: String,
    pub repeat_completed: Option<u32>,
    pub repeat_times: Option<u32>,
    #[serde(default)]
    pub last_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CronRunInfo {
    pub id: String,
    pub job_id: String,
    pub trigger: String,
    pub occurrence_at: i64,
    pub status: String,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub error_class: Option<String>,
    pub output_path: Option<PathBuf>,
}
