//! Bounded, server-owned Learn ledger inspection. Never expose target paths,
//! backup bytes, transcript text, or model-supplied reasons in list responses.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};

use mercury_learn::metrics::{ComparisonState, RecentObservedMetric, TaskOutcome};
use mercury_learn::storage::{
    ChangeRecord, ChangeStatus, LedgerStatus, Storage, StorageError, TriggerRecord, TriggerStatus,
};
use mercury_learn::transcript::Provider;
use mercury_learn::{ChangeError, Changes};
use sha2::{Digest, Sha256};
use time::{Date, Month};

use crate::api::schema::{
    ErrorBody, ErrorResponse, LearnChangeCounts, LearnChangeDetail, LearnChangeInfo,
    LearnComparisonInfo, LearnDiffInfo, LearnDiffSummary, LearnObservedMetricInfo, LearnStatusInfo,
    LearnTriggerCounts, LearnTriggerInfo, LearnUndoOutcome, LearnUndoOutcomeKind, Method, Request,
    ResponseResult, SuccessResponse,
};

const MAX_PENDING: usize = 8;
static PENDING: AtomicUsize = AtomicUsize::new(0);
static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

pub(super) fn dispatch(
    request: Request,
    respond_to: mpsc::Sender<String>,
    app_events: tokio::sync::mpsc::Sender<crate::events::AppEvent>,
) {
    if PENDING.fetch_add(1, Ordering::AcqRel) >= MAX_PENDING {
        PENDING.fetch_sub(1, Ordering::AcqRel);
        let _ = respond_to.send(error(
            request.id,
            "learn_busy",
            "Learn is busy; retry shortly",
        ));
        return;
    }
    tokio::task::spawn_blocking(move || {
        struct PendingGuard;
        impl Drop for PendingGuard {
            fn drop(&mut self) {
                PENDING.fetch_sub(1, Ordering::AcqRel);
            }
        }
        let _pending = PendingGuard;
        let response = match SERIAL.get_or_init(|| Mutex::new(())).lock() {
            Ok(_guard) => handle_at_with_events(
                &crate::session::data_dir().join("learn"),
                request,
                Some(&app_events),
            ),
            Err(_) => error(
                request.id,
                "learn_unavailable",
                "Learn ledger is unavailable",
            ),
        };
        let _ = respond_to.send(response);
    });
}

#[cfg(test)]
fn handle_at(root: &Path, request: Request) -> String {
    handle_at_with_events(root, request, None)
}

fn notify_target_reload(
    app_events: Option<&tokio::sync::mpsc::Sender<crate::events::AppEvent>>,
    path: &Path,
) {
    let Some(events) = app_events else { return };
    let event = match path.file_name().and_then(|name| name.to_str()) {
        Some("config.toml") => crate::events::AppEvent::LearnRouteChanged,
        Some("jobs.toml") => crate::events::AppEvent::LearnCronChanged,
        _ => return,
    };
    if events.try_send(event).is_err() {
        tracing::warn!("Learn undo completed but runtime reload could not be queued");
    }
}

fn handle_at_with_events(
    root: &Path,
    request: Request,
    app_events: Option<&tokio::sync::mpsc::Sender<crate::events::AppEvent>>,
) -> String {
    let id = request.id;
    let method = request.method;
    if let Method::LearnList(params) = &method {
        let limit = params.limit.unwrap_or(30);
        if !(1..=100).contains(&limit) {
            return error(
                id,
                "invalid_learn_limit",
                "Learn limit must be between 1 and 100",
            );
        }
    }
    let target_id = match &method {
        Method::LearnShow(params) | Method::LearnDiff(params) | Method::LearnUndo(params) => {
            Some(params.change_id.as_str())
        }
        _ => None,
    };
    if target_id.is_some_and(|change_id| change_id.len() > 64 || change_id.is_empty()) {
        return error(id, "invalid_learn_id", "invalid Learn change ID");
    }
    let date_range = if let Method::LearnUndoDate(params) = &method {
        match utc_date_range(&params.date) {
            Some(range) => Some(range),
            None => return error(id, "invalid_learn_date", "invalid UTC date"),
        }
    } else {
        None
    };
    if !root.exists() {
        return match method {
            Method::LearnList(_) => success(
                id,
                ResponseResult::LearnList {
                    changes: Vec::new(),
                },
            ),
            Method::LearnStatus(_) => success(
                id,
                ResponseResult::LearnStatus {
                    status: to_status(LedgerStatus::default(), false, Vec::new(), Vec::new()),
                },
            ),
            Method::LearnShow(_) | Method::LearnDiff(_) | Method::LearnUndo(_) => {
                error(id, "learn_not_found", "Learn change was not found")
            }
            Method::LearnUndoDate(params) => success(
                id,
                ResponseResult::LearnUndoDate {
                    date: params.date,
                    outcomes: Vec::new(),
                    interrupted: false,
                    remaining_count: 0,
                },
            ),
            _ => error(id, "invalid_method", "invalid Learn method"),
        };
    }
    let storage = match Storage::open(root) {
        Ok(storage) => storage,
        Err(_) => return error(id, "learn_unavailable", "Learn ledger is unavailable"),
    };
    match method {
        Method::LearnStatus(_) => match (
            storage.status(),
            storage.recent_triggers(16),
            storage.recent_observed_metrics(16),
        ) {
            (Ok(status), Ok(triggers), Ok(metrics)) => {
                let recent_metrics = metrics
                    .iter()
                    .map(|metric| to_observed_metric_info(&storage, metric))
                    .collect::<Result<Vec<_>, _>>();
                match recent_metrics {
                    Ok(recent_metrics) => success(
                        id,
                        ResponseResult::LearnStatus {
                            status: to_status(
                                status,
                                true,
                                triggers.iter().map(to_trigger_info).collect(),
                                recent_metrics,
                            ),
                        },
                    ),
                    Err(_) => error(id, "learn_unavailable", "Learn metric comparison failed"),
                }
            }
            _ => error(id, "learn_unavailable", "Learn ledger status failed"),
        },
        Method::LearnList(params) => match storage.list_recent(params.limit.unwrap_or(30)) {
            Ok(records) => success(
                id,
                ResponseResult::LearnList {
                    changes: records.into_iter().map(to_info).collect(),
                },
            ),
            Err(_) => error(id, "learn_unavailable", "Learn ledger query failed"),
        },
        Method::LearnShow(params) => match storage.get(&params.change_id) {
            Ok(Some(record)) => success(
                id,
                ResponseResult::LearnShow {
                    detail: LearnChangeDetail {
                        before_hash: record.before_hash.clone(),
                        after_hash: record.after_hash.clone(),
                        backup_available: Some(storage.read_backup(&record).is_ok()),
                        change: to_info(record),
                        reason_redacted: true,
                    },
                },
            ),
            Ok(None) => error(id, "learn_not_found", "Learn change was not found"),
            Err(StorageError::InvalidInput) => {
                error(id, "invalid_learn_id", "invalid Learn change ID")
            }
            Err(_) => error(id, "learn_unavailable", "Learn ledger query failed"),
        },
        Method::LearnDiff(params) => match storage.get(&params.change_id) {
            Ok(Some(record)) => {
                let before = match storage.read_backup(&record) {
                    Ok(bytes) => bytes,
                    Err(_) => return error(id, "learn_unavailable", "Learn backup is unavailable"),
                };
                let summary = match storage.read_after_snapshot(&record) {
                    Ok(after) => Some(diff_summary(&before, &after)),
                    Err(StorageError::Io(error))
                        if error.kind() == std::io::ErrorKind::NotFound =>
                    {
                        None
                    }
                    Err(_) => {
                        return error(id, "learn_unavailable", "Learn after snapshot is invalid")
                    }
                };
                success(
                    id,
                    ResponseResult::LearnDiff {
                        diff: LearnDiffInfo {
                            change: to_info(record),
                            summary,
                            content_redacted: true,
                        },
                    },
                )
            }
            Ok(None) => error(id, "learn_not_found", "Learn change was not found"),
            Err(StorageError::InvalidInput) => {
                error(id, "invalid_learn_id", "invalid Learn change ID")
            }
            Err(_) => error(id, "learn_unavailable", "Learn ledger query failed"),
        },
        Method::LearnUndo(params) => {
            let original = match storage.get(&params.change_id) {
                Ok(Some(record)) => record,
                Ok(None) => return error(id, "learn_not_found", "Learn change was not found"),
                Err(StorageError::InvalidInput) => {
                    return error(id, "invalid_learn_id", "invalid Learn change ID")
                }
                Err(_) => return error(id, "learn_unavailable", "Learn ledger query failed"),
            };
            let mut changes = Changes::new(storage);
            match changes.undo(&params.change_id) {
                Ok(record) => {
                    notify_target_reload(app_events, &original.target_path);
                    success(
                        id,
                        ResponseResult::LearnUndo {
                            change: to_info(record),
                        },
                    )
                }
                Err(ChangeError::Conflict) => error(
                    id,
                    "learn_conflict",
                    "target changed; original bytes preserved",
                ),
                Err(ChangeError::InvalidState) => error(
                    id,
                    "learn_invalid_state",
                    "change cannot be undone in its current state",
                ),
                Err(ChangeError::Missing) | Err(ChangeError::Storage(StorageError::Missing)) => {
                    error(id, "learn_not_found", "Learn change was not found")
                }
                Err(ChangeError::Uncertain) => error(
                    id,
                    "learn_uncertain",
                    "undo outcome is uncertain; inspect the ledger",
                ),
                Err(_) => error(id, "learn_unavailable", "Learn undo failed"),
            }
        }
        Method::LearnUndoDate(params) => {
            let (start_ms, end_ms) = date_range.expect("date validated above");
            let records = match storage.list_created_between(start_ms, end_ms, 1000) {
                Ok(records) => records,
                Err(StorageError::TooLarge) => {
                    return error(
                        id,
                        "learn_date_too_many",
                        "too many changes on this date; none were undone",
                    )
                }
                Err(_) => return error(id, "learn_unavailable", "Learn ledger query failed"),
            };
            let mut changes = Changes::new(storage);
            let total = records.len();
            let mut outcomes = Vec::with_capacity(records.len());
            let mut interrupted = false;
            let mut route_undone = false;
            let mut cron_undone = false;
            for record in records {
                let outcome = if matches!(
                    record.status,
                    ChangeStatus::Applied | ChangeStatus::Conflict
                ) {
                    match changes.undo(&record.id) {
                        Ok(_) => {
                            route_undone |= record
                                .target_path
                                .file_name()
                                .is_some_and(|name| name == "config.toml");
                            cron_undone |= record
                                .target_path
                                .file_name()
                                .is_some_and(|name| name == "jobs.toml");
                            LearnUndoOutcomeKind::Undone
                        }
                        Err(ChangeError::Conflict) => LearnUndoOutcomeKind::Conflict,
                        Err(ChangeError::InvalidState) => LearnUndoOutcomeKind::Skipped,
                        Err(ChangeError::Uncertain) => {
                            interrupted = true;
                            LearnUndoOutcomeKind::Uncertain
                        }
                        Err(_) => {
                            interrupted = true;
                            LearnUndoOutcomeKind::Failed
                        }
                    }
                } else {
                    LearnUndoOutcomeKind::Skipped
                };
                outcomes.push(LearnUndoOutcome {
                    change_id: record.id,
                    outcome,
                });
                if interrupted {
                    break;
                }
            }
            if route_undone {
                notify_target_reload(app_events, Path::new("config.toml"));
            }
            if cron_undone {
                notify_target_reload(app_events, Path::new("jobs.toml"));
            }
            let remaining_count = total - outcomes.len();
            success(
                id,
                ResponseResult::LearnUndoDate {
                    date: params.date,
                    outcomes,
                    interrupted,
                    remaining_count,
                },
            )
        }
        _ => error(id, "invalid_method", "invalid Learn method"),
    }
}

fn to_status(
    status: LedgerStatus,
    ledger_present: bool,
    recent_triggers: Vec<LearnTriggerInfo>,
    recent_metrics: Vec<LearnObservedMetricInfo>,
) -> LearnStatusInfo {
    LearnStatusInfo {
        ledger_present,
        changes: LearnChangeCounts {
            prepared: status.changes.prepared,
            applied: status.changes.applied,
            undo_prepared: status.changes.undo_prepared,
            conflict: status.changes.conflict,
            failed: status.changes.failed,
            undone: status.changes.undone,
        },
        triggers: LearnTriggerCounts {
            claimed: status.triggers.claimed,
            succeeded: status.triggers.succeeded,
            failed: status.triggers.failed,
        },
        recent_triggers,
        recent_metrics,
    }
}

fn to_observed_metric_info(
    storage: &Storage,
    metric: &RecentObservedMetric,
) -> Result<LearnObservedMetricInfo, StorageError> {
    let comparison = storage.task_comparison(&metric.task_key, metric.provider)?;
    let comparison =
        (comparison.state == ComparisonState::ComparableObserved).then(|| LearnComparisonInfo {
            baseline_runs: comparison.baseline.samples,
            after_runs: comparison.after.samples,
            baseline_average_tokens: (comparison.baseline.input_tokens
                + comparison.baseline.output_tokens)
                / comparison.baseline.samples,
            after_average_tokens: (comparison.after.input_tokens + comparison.after.output_tokens)
                / comparison.after.samples,
            baseline_succeeded: comparison.baseline.succeeded,
            after_succeeded: comparison.after.succeeded,
        });
    Ok(LearnObservedMetricInfo {
        task_key: metric.task_key.clone(),
        provider: match metric.provider {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
        }
        .into(),
        input_tokens: metric.input_tokens,
        output_tokens: metric.output_tokens,
        cost_microusd: metric.cost_microusd,
        outcome: metric
            .outcome
            .map(|outcome| match outcome {
                TaskOutcome::Succeeded => "succeeded",
                TaskOutcome::Failed => "failed",
            })
            .map(str::to_owned),
        observed_at_ms: metric.observed_at_ms,
        comparison,
    })
}

fn to_trigger_info(record: &TriggerRecord) -> LearnTriggerInfo {
    let key = record.key.as_str();
    let provider = if key.starts_with("claude:") {
        "claude"
    } else if key.starts_with("codex:") {
        "codex"
    } else {
        "internal"
    };
    let kind = if key.starts_with("curation:weekly:") {
        "curation_weekly"
    } else if key.starts_with("weekly:") {
        "weekly"
    } else if key.ends_with(":end") {
        "session_end"
    } else if key.contains(":turn:") {
        "turn_milestone"
    } else {
        "other"
    };
    let digest = Sha256::digest(key.as_bytes());
    let fingerprint = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    LearnTriggerInfo {
        fingerprint,
        provider: provider.to_owned(),
        kind: kind.to_owned(),
        status: match record.status {
            TriggerStatus::Claimed => "claimed",
            TriggerStatus::Succeeded => "succeeded",
            TriggerStatus::Failed => "failed",
        }
        .to_owned(),
        created_at_ms: record.created_at_ms,
        finished_at_ms: record.finished_at_ms,
    }
}

#[cfg(test)]
mod curation_trigger_tests {
    use super::*;

    #[test]
    fn weekly_curator_status_exposes_type_only_not_private_key() {
        let record = TriggerRecord {
            key: "curation:weekly:2026-10-05".into(),
            status: TriggerStatus::Succeeded,
            created_at_ms: 1,
            finished_at_ms: Some(2),
        };
        let visible = to_trigger_info(&record);
        assert_eq!(visible.kind, "curation_weekly");
        assert_eq!(visible.provider, "internal");
        assert_eq!(visible.fingerprint.len(), 16);
        assert!(!visible.fingerprint.contains("2026"));
    }
}

/// One coarse hunk after trimming equal leading/trailing LF-delimited lines.
/// No source bytes are ever serialized into the public response.
fn diff_summary(before: &[u8], after: &[u8]) -> LearnDiffSummary {
    let before_lines: Vec<_> = before.split_inclusive(|byte| *byte == b'\n').collect();
    let after_lines: Vec<_> = after.split_inclusive(|byte| *byte == b'\n').collect();
    let before_len = if before.is_empty() {
        0
    } else {
        before_lines.len()
    };
    let after_len = if after.is_empty() {
        0
    } else {
        after_lines.len()
    };
    let prefix = before_lines
        .iter()
        .zip(after_lines.iter())
        .take_while(|(left, right)| left == right)
        .count();
    let suffix = before_lines[..before_len]
        .iter()
        .rev()
        .zip(after_lines[..after_len].iter().rev())
        .take_while(|(left, right)| left == right)
        .count()
        .min(before_len.saturating_sub(prefix))
        .min(after_len.saturating_sub(prefix));
    LearnDiffSummary {
        before_lines: before_len,
        after_lines: after_len,
        unchanged_prefix_lines: prefix,
        unchanged_suffix_lines: suffix,
        removed_lines: before_len - prefix - suffix,
        added_lines: after_len - prefix - suffix,
    }
}

fn utc_date_range(value: &str) -> Option<(i64, i64)> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
    {
        return None;
    }
    let year: i32 = value[..4].parse().ok()?;
    if !(1..=9999).contains(&year) {
        return None;
    }
    let month = Month::try_from(value[5..7].parse::<u8>().ok()?).ok()?;
    let day = value[8..].parse::<u8>().ok()?;
    let date = Date::from_calendar_date(year, month, day).ok()?;
    let next = date.next_day()?;
    Some((
        date.midnight().assume_utc().unix_timestamp() * 1000,
        next.midnight().assume_utc().unix_timestamp() * 1000,
    ))
}

fn to_info(record: ChangeRecord) -> LearnChangeInfo {
    LearnChangeInfo {
        id: record.id,
        target_id: record.target_id,
        status: status_name(record.status).into(),
        created_at_ms: record.created_at_ms,
    }
}

fn status_name(status: ChangeStatus) -> &'static str {
    match status {
        ChangeStatus::Prepared => "prepared",
        ChangeStatus::Applied => "applied",
        ChangeStatus::UndoPrepared => "undo_prepared",
        ChangeStatus::Conflict => "conflict",
        ChangeStatus::Failed => "failed",
        ChangeStatus::Undone => "undone",
    }
}

fn success(id: String, result: ResponseResult) -> String {
    serde_json::to_string(&SuccessResponse { id, result }).unwrap_or_else(|_| "{}".into())
}

fn error(id: String, code: &str, message: &str) -> String {
    serde_json::to_string(&ErrorResponse {
        id,
        error: ErrorBody {
            code: code.into(),
            message: message.into(),
        },
    })
    .unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mercury_learn::metrics::{MetricPhase, TaskObservation};
    use mercury_learn::policy::{Policy, Proposal, TargetKind, TargetPolicy};
    use mercury_learn::storage::hash;
    use mercury_learn::storage::{PrepareChange, Storage};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "mercury-learn-api-{}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn data(&self) -> PathBuf {
            self.0.join("learn")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn request(limit: Option<usize>) -> Request {
        Request {
            id: "list".into(),
            method: Method::LearnList(crate::api::schema::LearnListParams { limit }),
        }
    }

    fn status_request() -> Request {
        Request {
            id: "status".into(),
            method: Method::LearnStatus(crate::api::schema::EmptyParams::default()),
        }
    }

    #[test]
    fn status_returns_bounded_observed_metrics_without_task_text() {
        let fixture = Fixture::new();
        let mut storage = Storage::open(&fixture.data()).unwrap();
        storage
            .record_task_observation(TaskObservation {
                observation_id: "run-1",
                task_key: "cron:job-1",
                provider: Provider::Claude,
                phase: MetricPhase::Observed,
                input_tokens: 120,
                output_tokens: 30,
                cost_microusd: Some(125),
                outcome: Some(TaskOutcome::Succeeded),
                observed_at_ms: 1_000,
            })
            .unwrap();
        drop(storage);
        let response = handle_at(&fixture.data(), status_request());
        let value: serde_json::Value = serde_json::from_str(&response).unwrap();
        let rows = value["result"]["status"]["recent_metrics"]
            .as_array()
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["task_key"], "cron:job-1");
        assert_eq!(rows[0]["input_tokens"], 120);
        assert_eq!(rows[0]["cost_microusd"], 125);
        assert!(!response.contains("prompt"));
        assert!(!response.contains("run-1"));
        assert!(!response.contains(fixture.0.to_str().unwrap()));
    }

    #[test]
    fn status_exposes_descriptive_comparison_only_for_verified_target_change() {
        let fixture = Fixture::new();
        let target = fixture.0.join("AGENTS.md");
        fs::write(&target, b"Before\n").unwrap();
        let policy = Policy::new(vec![TargetPolicy {
            id: "project".into(),
            path: target.clone(),
            kind: TargetKind::Instructions,
            max_bytes: 4096,
        }])
        .unwrap();
        let key = format!(
            "cron:123e4567-e89b-12d3-a456-426614174000:target:project:{}",
            mercury_learn::metrics::target_path_fingerprint(&target).unwrap()
        );
        let now_ms = time::OffsetDateTime::now_utc().unix_timestamp() * 1000;
        let mut storage = Storage::open(&fixture.data()).unwrap();
        storage
            .record_task_observation(TaskObservation {
                observation_id: "baseline-run",
                task_key: &key,
                provider: Provider::Claude,
                phase: MetricPhase::Observed,
                input_tokens: 100,
                output_tokens: 20,
                cost_microusd: Some(400),
                outcome: Some(TaskOutcome::Succeeded),
                observed_at_ms: now_ms - 1000,
            })
            .unwrap();
        drop(storage);
        let snapshot = mercury_learn::review_snapshots(&policy, true)
            .unwrap()
            .remove(0);
        let mut changes = Changes::new(Storage::open(&fixture.data()).unwrap());
        changes
            .apply(
                &policy,
                true,
                Proposal {
                    target_id: "project".into(),
                    content: "After\n".into(),
                    reason: "confirmed preference".into(),
                    source: "claude:session-1".into(),
                },
                &snapshot.before_hash,
            )
            .unwrap();
        drop(changes);
        let mut storage = Storage::open(&fixture.data()).unwrap();
        storage
            .record_task_observation(TaskObservation {
                observation_id: "after-run",
                task_key: &key,
                provider: Provider::Claude,
                phase: MetricPhase::Observed,
                input_tokens: 50,
                output_tokens: 10,
                cost_microusd: Some(200),
                outcome: Some(TaskOutcome::Succeeded),
                observed_at_ms: now_ms + 3000,
            })
            .unwrap();
        drop(storage);
        let response = handle_at(&fixture.data(), status_request());
        let value: serde_json::Value = serde_json::from_str(&response).unwrap();
        let comparison = &value["result"]["status"]["recent_metrics"][0]["comparison"];
        assert_eq!(comparison["baseline_runs"], 1);
        assert_eq!(comparison["after_runs"], 1);
        assert_eq!(comparison["baseline_average_tokens"], 120);
        assert_eq!(comparison["after_average_tokens"], 60);
        assert!(!response.contains("Before"));
        assert!(!response.contains("After"));
    }

    fn target_request(method: &str, change_id: &str) -> Request {
        let target = crate::api::schema::LearnChangeTarget {
            change_id: change_id.into(),
        };
        Request {
            id: method.into(),
            method: match method {
                "show" => Method::LearnShow(target),
                "diff" => Method::LearnDiff(target),
                "undo" => Method::LearnUndo(target),
                _ => panic!("unsupported fixture method"),
            },
        }
    }

    fn date_request(date: &str) -> Request {
        Request {
            id: "undo-date".into(),
            method: Method::LearnUndoDate(crate::api::schema::LearnUndoDateParams {
                date: date.into(),
            }),
        }
    }

    fn applied_change(fixture: &Fixture) -> (PathBuf, String) {
        let target = fixture.0.join("AGENTS.md");
        fs::write(&target, b"original bytes").unwrap();
        let policy = Policy::new(vec![TargetPolicy {
            id: "instructions".into(),
            path: target.clone(),
            kind: TargetKind::Instructions,
            max_bytes: 1024,
        }])
        .unwrap();
        let storage = Storage::open(&fixture.data()).unwrap();
        let mut changes = Changes::new(storage);
        let record = changes
            .apply(
                &policy,
                true,
                Proposal {
                    target_id: "instructions".into(),
                    content: "learned bytes".into(),
                    reason: "private reason from model".into(),
                    source: "codex:session-1".into(),
                },
                &hash(b"original bytes"),
            )
            .unwrap();
        (target, record.id)
    }

    #[test]
    fn list_returns_bounded_metadata_without_model_text_or_paths() {
        let fixture = Fixture::new();
        let mut storage = Storage::open(&fixture.data()).unwrap();
        let target = fixture.0.join("AGENTS.md");
        let record = storage
            .prepare(PrepareChange {
                target_id: "instructions",
                target_path: &target,
                source: "codex:session-1",
                reason: "secret phrase from model",
                before_bytes: b"private before",
                after_bytes: b"private after",
            })
            .unwrap();
        drop(storage);

        let response: serde_json::Value =
            serde_json::from_str(&handle_at(&fixture.data(), request(Some(1)))).unwrap();
        let changes = response["result"]["changes"].as_array().unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0]["id"], record.id);
        assert_eq!(changes[0]["target_id"], "instructions");
        assert_eq!(changes[0]["status"], "prepared");
        assert!(!response.to_string().contains("secret phrase"));
        assert!(!response.to_string().contains("private before"));
        assert!(!response.to_string().contains("AGENTS.md"));
    }

    #[test]
    fn list_rejects_unbounded_limit_and_handles_empty_ledger() {
        let fixture = Fixture::new();
        let invalid: serde_json::Value =
            serde_json::from_str(&handle_at(&fixture.data(), request(Some(101)))).unwrap();
        assert_eq!(invalid["error"]["code"], "invalid_learn_limit");
        assert!(!fixture.data().exists());

        let empty: serde_json::Value =
            serde_json::from_str(&handle_at(&fixture.data(), request(None))).unwrap();
        assert_eq!(empty["result"]["changes"].as_array().unwrap().len(), 0);
        assert!(!fixture.data().exists());
    }

    #[test]
    fn status_counts_full_ledger_without_exposing_record_content() {
        let fixture = Fixture::new();
        let empty: serde_json::Value =
            serde_json::from_str(&handle_at(&fixture.data(), status_request())).unwrap();
        assert_eq!(empty["result"]["status"]["ledger_present"], false);
        let (target, _) = applied_change(&fixture);
        let mut storage = Storage::open(&fixture.data()).unwrap();
        storage.claim_trigger("codex:session-1:end").unwrap();
        drop(storage);

        let response: serde_json::Value =
            serde_json::from_str(&handle_at(&fixture.data(), status_request())).unwrap();
        assert_eq!(response["result"]["status"]["ledger_present"], true);
        assert_eq!(response["result"]["status"]["changes"]["applied"], 1);
        assert_eq!(response["result"]["status"]["triggers"]["claimed"], 1);
        let triggers = response["result"]["status"]["recent_triggers"]
            .as_array()
            .unwrap();
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0]["provider"], "codex");
        assert_eq!(triggers[0]["kind"], "session_end");
        assert_eq!(triggers[0]["status"], "claimed");
        assert_eq!(triggers[0]["fingerprint"].as_str().unwrap().len(), 16);
        assert!(!response.to_string().contains("session-1"));
        assert!(!response.to_string().contains(target.to_str().unwrap()));
    }

    #[test]
    fn diff_remains_redacted_after_undo_and_missing_snapshot_is_explicit() {
        let fixture = Fixture::new();
        let (target, change_id) = applied_change(&fixture);
        let shown: serde_json::Value = serde_json::from_str(&handle_at(
            &fixture.data(),
            target_request("diff", &change_id),
        ))
        .unwrap();
        let diff = &shown["result"]["diff"];
        assert_eq!(diff["summary"]["removed_lines"], 1);
        assert_eq!(diff["summary"]["added_lines"], 1);
        assert_eq!(diff["content_redacted"], true);
        assert!(!shown.to_string().contains("original bytes"));
        assert!(!shown.to_string().contains("learned bytes"));
        assert!(!shown.to_string().contains(target.to_str().unwrap()));

        let undone: serde_json::Value = serde_json::from_str(&handle_at(
            &fixture.data(),
            target_request("undo", &change_id),
        ))
        .unwrap();
        assert_eq!(undone["result"]["change"]["status"], "undone");
        let after_undo: serde_json::Value = serde_json::from_str(&handle_at(
            &fixture.data(),
            target_request("diff", &change_id),
        ))
        .unwrap();
        assert_eq!(after_undo["result"]["diff"]["summary"], diff["summary"]);

        fs::remove_file(
            fixture
                .data()
                .join("snapshots")
                .join(format!("{change_id}.after")),
        )
        .unwrap();
        let legacy: serde_json::Value = serde_json::from_str(&handle_at(
            &fixture.data(),
            target_request("diff", &change_id),
        ))
        .unwrap();
        assert!(legacy["result"]["diff"]["summary"].is_null());
    }

    #[test]
    fn diff_summary_trims_equal_lines_without_overlapping_regions() {
        let summary = diff_summary(b"same\nold\nlast", b"same\nnew\nlast");
        assert_eq!(summary.before_lines, 3);
        assert_eq!(summary.after_lines, 3);
        assert_eq!(summary.unchanged_prefix_lines, 1);
        assert_eq!(summary.unchanged_suffix_lines, 1);
        assert_eq!(summary.removed_lines, 1);
        assert_eq!(summary.added_lines, 1);
        let equal = diff_summary(b"one\ntwo", b"one\ntwo");
        assert_eq!(equal.removed_lines, 0);
        assert_eq!(equal.added_lines, 0);
        assert_eq!(diff_summary(b"", b"new").before_lines, 0);
    }

    #[test]
    fn show_redacts_reason_and_undo_restores_exact_bytes() {
        let fixture = Fixture::new();
        let (target, change_id) = applied_change(&fixture);
        assert_eq!(fs::read(&target).unwrap(), b"learned bytes");

        let shown: serde_json::Value = serde_json::from_str(&handle_at(
            &fixture.data(),
            target_request("show", &change_id),
        ))
        .unwrap();
        assert_eq!(shown["result"]["detail"]["change"]["status"], "applied");
        assert_eq!(shown["result"]["detail"]["reason_redacted"], true);
        assert_eq!(shown["result"]["detail"]["backup_available"], true);
        assert!(!shown.to_string().contains("private reason"));
        assert!(!shown.to_string().contains("AGENTS.md"));

        let undone: serde_json::Value = serde_json::from_str(&handle_at(
            &fixture.data(),
            target_request("undo", &change_id),
        ))
        .unwrap();
        assert_eq!(undone["result"]["change"]["status"], "undone");
        assert_eq!(fs::read(&target).unwrap(), b"original bytes");
        let repeat: serde_json::Value = serde_json::from_str(&handle_at(
            &fixture.data(),
            target_request("undo", &change_id),
        ))
        .unwrap();
        assert_eq!(repeat["error"]["code"], "learn_invalid_state");
    }

    #[test]
    fn route_undo_queues_reload_after_restoring_original_bytes() {
        let fixture = Fixture::new();
        let target = fixture.0.join("config.toml");
        fs::write(&target, b"[route]\nenabled = false\n").unwrap();
        let policy = Policy::new(vec![TargetPolicy {
            id: "routing".into(),
            path: target.clone(),
            kind: TargetKind::ConfigSection,
            max_bytes: 4096,
        }])
        .unwrap();
        let mut changes = Changes::new(Storage::open(&fixture.data()).unwrap());
        let record = changes
            .apply(
                &policy,
                true,
                Proposal {
                    target_id: "routing".into(),
                    content: "[route]\nenabled = true\n".into(),
                    reason: "confirmed preference".into(),
                    source: "claude:session-1".into(),
                },
                &hash(b"[route]\nenabled = false\n"),
            )
            .unwrap();
        drop(changes);
        let (events, mut receiver) = tokio::sync::mpsc::channel(2);
        let response = handle_at_with_events(
            &fixture.data(),
            target_request("undo", &record.id),
            Some(&events),
        );
        assert!(response.contains("\"undone\""));
        assert!(matches!(
            receiver.try_recv().unwrap(),
            crate::events::AppEvent::LearnRouteChanged
        ));
        assert_eq!(fs::read(&target).unwrap(), b"[route]\nenabled = false\n");
        notify_target_reload(Some(&events), Path::new("jobs.toml"));
        assert!(matches!(
            receiver.try_recv().unwrap(),
            crate::events::AppEvent::LearnCronChanged
        ));
    }

    #[test]
    fn show_marks_corrupt_backup_unavailable_without_returning_bytes() {
        let fixture = Fixture::new();
        let (_, change_id) = applied_change(&fixture);
        let storage = Storage::open(&fixture.data()).unwrap();
        let record = storage.get(&change_id).unwrap().unwrap();
        fs::write(
            fixture.data().join("backups").join(&record.backup_name),
            b"tampered",
        )
        .unwrap();

        let response: serde_json::Value = serde_json::from_str(&handle_at(
            &fixture.data(),
            target_request("show", &change_id),
        ))
        .unwrap();
        assert_eq!(response["result"]["detail"]["backup_available"], false);
        assert!(!response.to_string().contains("tampered"));
        assert!(!response.to_string().contains(&record.backup_name));
    }

    #[test]
    fn undo_conflict_keeps_human_edit_and_invalid_id_is_rejected() {
        let fixture = Fixture::new();
        let (target, change_id) = applied_change(&fixture);
        fs::write(&target, b"human edit").unwrap();

        let response: serde_json::Value = serde_json::from_str(&handle_at(
            &fixture.data(),
            target_request("undo", &change_id),
        ))
        .unwrap();
        assert_eq!(response["error"]["code"], "learn_conflict");
        assert_eq!(fs::read(&target).unwrap(), b"human edit");
        let invalid: serde_json::Value = serde_json::from_str(&handle_at(
            &fixture.data(),
            target_request("show", "../escape"),
        ))
        .unwrap();
        assert_eq!(invalid["error"]["code"], "invalid_learn_id");
    }

    #[test]
    fn undo_date_reverts_newest_changes_first_and_reports_each_outcome() {
        let fixture = Fixture::new();
        let (target, first_id) = applied_change(&fixture);
        let mut changes = Changes::new(Storage::open(&fixture.data()).unwrap());
        let policy = Policy::new(vec![TargetPolicy {
            id: "instructions".into(),
            path: target.clone(),
            kind: TargetKind::Instructions,
            max_bytes: 1024,
        }])
        .unwrap();
        let second = changes
            .apply(
                &policy,
                true,
                Proposal {
                    target_id: "instructions".into(),
                    content: "second lesson".into(),
                    reason: "second private reason".into(),
                    source: "codex:session-2".into(),
                },
                &hash(b"learned bytes"),
            )
            .unwrap();
        let date = time::OffsetDateTime::from_unix_timestamp(second.created_at_ms / 1000)
            .unwrap()
            .date()
            .to_string();
        drop(changes);

        let response: serde_json::Value =
            serde_json::from_str(&handle_at(&fixture.data(), date_request(&date))).unwrap();
        let outcomes = response["result"]["outcomes"].as_array().unwrap();
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0]["change_id"], second.id);
        assert_eq!(outcomes[0]["outcome"], "undone");
        assert_eq!(outcomes[1]["change_id"], first_id);
        assert_eq!(outcomes[1]["outcome"], "undone");
        assert_eq!(response["result"]["interrupted"], false);
        assert_eq!(response["result"]["remaining_count"], 0);
        assert_eq!(fs::read(&target).unwrap(), b"original bytes");
    }

    #[test]
    fn undo_date_rejects_invalid_date_and_preserves_human_edit() {
        let fixture = Fixture::new();
        let (target, change_id) = applied_change(&fixture);
        let invalid: serde_json::Value =
            serde_json::from_str(&handle_at(&fixture.data(), date_request("2026-02-30"))).unwrap();
        assert_eq!(invalid["error"]["code"], "invalid_learn_date");

        let record = Storage::open(&fixture.data())
            .unwrap()
            .get(&change_id)
            .unwrap()
            .unwrap();
        let date = time::OffsetDateTime::from_unix_timestamp(record.created_at_ms / 1000)
            .unwrap()
            .date()
            .to_string();
        fs::write(&target, b"human edit").unwrap();
        let response: serde_json::Value =
            serde_json::from_str(&handle_at(&fixture.data(), date_request(&date))).unwrap();
        assert_eq!(response["result"]["outcomes"][0]["outcome"], "conflict");
        assert_eq!(response["result"]["interrupted"], false);
        assert_eq!(response["result"]["remaining_count"], 0);
        assert_eq!(fs::read(&target).unwrap(), b"human edit");
    }

    #[test]
    fn undo_date_reports_unattempted_items_after_missing_backup() {
        let fixture = Fixture::new();
        let (target, first_id) = applied_change(&fixture);
        let policy = Policy::new(vec![TargetPolicy {
            id: "instructions".into(),
            path: target.clone(),
            kind: TargetKind::Instructions,
            max_bytes: 1024,
        }])
        .unwrap();
        let mut changes = Changes::new(Storage::open(&fixture.data()).unwrap());
        let second = changes
            .apply(
                &policy,
                true,
                Proposal {
                    target_id: "instructions".into(),
                    content: "second lesson".into(),
                    reason: "second private reason".into(),
                    source: "codex:session-2".into(),
                },
                &hash(b"learned bytes"),
            )
            .unwrap();
        let third = changes
            .apply(
                &policy,
                true,
                Proposal {
                    target_id: "instructions".into(),
                    content: "third lesson".into(),
                    reason: "third private reason".into(),
                    source: "codex:session-3".into(),
                },
                &hash(b"second lesson"),
            )
            .unwrap();
        let date = time::OffsetDateTime::from_unix_timestamp(third.created_at_ms / 1000)
            .unwrap()
            .date()
            .to_string();
        drop(changes);
        fs::remove_file(fixture.data().join("backups").join(&second.backup_name)).unwrap();

        let response: serde_json::Value =
            serde_json::from_str(&handle_at(&fixture.data(), date_request(&date))).unwrap();
        let outcomes = response["result"]["outcomes"].as_array().unwrap();
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0]["change_id"], third.id);
        assert_eq!(outcomes[0]["outcome"], "undone");
        assert_eq!(outcomes[1]["change_id"], second.id);
        assert_eq!(outcomes[1]["outcome"], "failed");
        assert_eq!(response["result"]["interrupted"], true);
        assert_eq!(response["result"]["remaining_count"], 1);
        assert_eq!(fs::read(&target).unwrap(), b"second lesson");
        assert_eq!(
            Storage::open(&fixture.data())
                .unwrap()
                .get(&first_id)
                .unwrap()
                .unwrap()
                .status,
            ChangeStatus::Applied
        );
    }
}
