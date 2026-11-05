//! Cron API methods. Mutations are serialized on the App/server-owner thread.

use mercury_cron::job::parse_utc_offset;
use mercury_cron::storage::RunRow;
use mercury_cron::{Delivery, Job, JobTarget, Repeat};
use std::io::Read;
use time::{OffsetDateTime, UtcOffset};

use crate::api::schema::{
    CronAddParams, CronEditParams, CronJobDefinition, CronJobInfo, CronJobTarget, CronLogParams,
    CronPauseParams, CronRunInfo, CronRunsParams, ResponseResult,
};
use crate::app::App;

use super::responses::{encode_error, encode_success};

fn job_info(job: &Job) -> CronJobInfo {
    let target_kind = match &job.target {
        JobTarget::Headless { .. } => "headless",
        JobTarget::Pane { .. } => "pane",
        JobTarget::Script { .. } => "script",
    };
    CronJobInfo {
        id: job.id.clone(),
        name: job.name.clone(),
        schedule: job.schedule.clone(),
        next_run_at: job.next_run_at,
        enabled: job.enabled,
        target_kind: target_kind.into(),
        repeat_completed: job.repeat.as_ref().map(|repeat| repeat.completed),
        repeat_times: job.repeat.as_ref().map(|repeat| repeat.times),
        last_status: None,
    }
}

fn job_definition(job: &Job) -> CronJobDefinition {
    CronJobDefinition {
        name: job.name.clone(),
        schedule: job.schedule.clone(),
        utc_offset: Some(job.utc_offset.clone()),
        target: job.target.clone(),
        workdir: job.workdir.clone(),
        timeout_seconds: Some(job.timeout_seconds),
        enabled: Some(job.enabled),
        catch_up: job.catch_up,
        repeat_times: job.repeat.as_ref().map(|repeat| repeat.times),
        deliver: job.deliver.clone(),
    }
}

fn run_info(row: RunRow) -> CronRunInfo {
    CronRunInfo {
        id: row.id,
        job_id: row.job_id,
        trigger: row.trigger,
        occurrence_at: row.occurrence_at,
        status: row.status,
        started_at: row.started_at,
        finished_at: row.finished_at,
        error_class: row.error_class,
        output_path: row.output_path,
    }
}

fn build_job(
    definition: CronJobDefinition,
    id: Option<&str>,
    enabled: bool,
) -> Result<Job, String> {
    let offset = match definition.utc_offset.as_deref() {
        Some(value) => parse_utc_offset(value)?,
        None => UtcOffset::current_local_offset()
            .map_err(|_| "cannot determine local UTC offset; specify utc_offset")?,
    };
    let mut job = Job::new(
        definition.name,
        &definition.schedule,
        offset,
        OffsetDateTime::now_utc(),
        definition.target,
    )?;
    if let Some(id) = id {
        job.id = id.to_owned();
    }
    job.enabled = definition.enabled.unwrap_or(enabled);
    job.workdir = definition.workdir;
    if let Some(timeout) = definition.timeout_seconds {
        job.timeout_seconds = timeout;
    }
    job.catch_up = definition.catch_up;
    job.repeat = definition.repeat_times.map(|times| Repeat {
        times,
        completed: 0,
    });
    job.deliver = definition.deliver;
    job.validate()?;
    Ok(job)
}

fn executable_exists(program: &str, workdir: Option<&std::path::Path>) -> bool {
    fn executable(path: &std::path::Path) -> bool {
        if !path.is_file() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
        }
        #[cfg(not(unix))]
        {
            true
        }
    }
    let path = std::path::Path::new(program);
    if path.is_absolute() {
        return executable(path);
    }
    if path.components().count() > 1 {
        return workdir.is_some_and(|directory| executable(&directory.join(path)))
            || executable(path);
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| executable(&directory.join(program)))
    })
}

impl App {
    pub(super) fn handle_cron_preview(
        &self,
        id: String,
        params: crate::api::schema::CronPreviewParams,
    ) -> String {
        let result = (|| -> Result<_, String> {
            if params.schedule.len() > 256 {
                return Err("Agenda longa demais".into());
            }
            let offset = match params.utc_offset.as_deref() {
                Some(value) => parse_utc_offset(value)?,
                None => UtcOffset::current_local_offset()
                    .map_err(|_| "Informe o offset UTC do servidor")?,
            };
            let now = OffsetDateTime::now_utc();
            let schedule = mercury_cron::Schedule::parse(&params.schedule, now, offset)?;
            let mut previous = now;
            let mut occurrences = Vec::new();
            for _ in 0..3 {
                let Some(next) = schedule.next_after(previous) else {
                    break;
                };
                occurrences.push(next.unix_timestamp());
                previous = next;
            }
            if occurrences.is_empty() {
                return Err("Agenda sem próxima ocorrência".into());
            }
            Ok(ResponseResult::CronPreview {
                utc_offset: format!(
                    "{}{:02}:{:02}",
                    if offset.is_negative() { "-" } else { "+" },
                    offset.whole_hours().unsigned_abs(),
                    offset.minutes_past_hour().unsigned_abs()
                ),
                occurrences,
            })
        })();
        match result {
            Ok(result) => encode_success(id, result),
            Err(e) => encode_error(id, "invalid_schedule", e),
        }
    }

    pub(super) fn handle_cron_add(&mut self, id: String, params: CronAddParams) -> String {
        let job = match build_job(params.definition, None, true) {
            Ok(job) => job,
            Err(error) => return encode_error(id, "invalid_cron_job", error),
        };
        let Some(cron) = self.cron.as_mut() else {
            return encode_error(
                id,
                "cron_unavailable",
                "cron scheduler is not active on this server",
            );
        };
        if let Err(error) = cron.scheduler.add(job.clone()) {
            return encode_error(id, "cron_add_failed", error);
        }
        encode_success(
            id,
            ResponseResult::CronJob {
                job: job_info(&job),
            },
        )
    }

    pub(super) fn handle_cron_list(&mut self, id: String) -> String {
        let Some(cron) = self.cron.as_ref() else {
            return encode_error(
                id,
                "cron_unavailable",
                "cron scheduler is not active on this server",
            );
        };
        let mut jobs = Vec::new();
        for job in cron.scheduler.jobs() {
            let mut info = job_info(job);
            match cron.scheduler.ledger().list_runs(&job.id, 1) {
                Ok(runs) => info.last_status = runs.first().map(|run| run.status.clone()),
                Err(_) => return encode_error(id, "cron_list_failed", "cannot read run ledger"),
            }
            jobs.push(info);
        }
        encode_success(id, ResponseResult::CronList { jobs })
    }

    pub(super) fn handle_cron_get(&mut self, id: String, params: CronJobTarget) -> String {
        let Some(cron) = self.cron.as_ref() else {
            return encode_error(
                id,
                "cron_unavailable",
                "cron scheduler is not active on this server",
            );
        };
        let Some(job) = cron
            .scheduler
            .jobs()
            .iter()
            .find(|job| job.id == params.job_id)
        else {
            return encode_error(id, "cron_not_found", "job not found");
        };
        encode_success(
            id,
            ResponseResult::CronDefinition {
                job_id: job.id.clone(),
                definition: job_definition(job),
            },
        )
    }

    pub(super) fn handle_cron_edit(&mut self, id: String, params: CronEditParams) -> String {
        let Some(cron) = self.cron.as_mut() else {
            return encode_error(
                id,
                "cron_unavailable",
                "cron scheduler is not active on this server",
            );
        };
        let Some(old) = cron
            .scheduler
            .jobs()
            .iter()
            .find(|job| job.id == params.job_id)
        else {
            return encode_error(id, "cron_not_found", "job not found");
        };
        let job = match build_job(params.definition, Some(&params.job_id), old.enabled) {
            Ok(job) => job,
            Err(error) => return encode_error(id, "invalid_cron_job", error),
        };
        if let Err(error) = cron.scheduler.edit(job.clone()) {
            return encode_error(id, "cron_edit_failed", error);
        }
        let updated = cron
            .scheduler
            .jobs()
            .iter()
            .find(|entry| entry.id == job.id)
            .unwrap();
        encode_success(
            id,
            ResponseResult::CronJob {
                job: job_info(updated),
            },
        )
    }

    pub(super) fn handle_cron_pause(&mut self, id: String, params: CronPauseParams) -> String {
        let Some(cron) = self.cron.as_mut() else {
            return encode_error(
                id,
                "cron_unavailable",
                "cron scheduler is not active on this server",
            );
        };
        if let Err(error) = cron.scheduler.pause(&params.job_id, params.paused) {
            return encode_error(id, "cron_pause_failed", error);
        }
        let job = cron
            .scheduler
            .jobs()
            .iter()
            .find(|entry| entry.id == params.job_id)
            .unwrap();
        encode_success(id, ResponseResult::CronJob { job: job_info(job) })
    }

    pub(super) fn handle_cron_run(&mut self, id: String, params: CronJobTarget) -> String {
        let Some(cron) = self.cron.as_mut() else {
            return encode_error(
                id,
                "cron_unavailable",
                "cron scheduler is not active on this server",
            );
        };
        let run = match cron
            .scheduler
            .run_now(&params.job_id, OffsetDateTime::now_utc())
        {
            Ok(run) => run,
            Err(error) => return encode_error(id, "cron_run_failed", error),
        };
        let run_id = run.id.clone();
        self.dispatch_cron(run);
        encode_success(id, ResponseResult::CronRun { run_id })
    }

    pub(super) fn handle_cron_remove(&mut self, id: String, params: CronJobTarget) -> String {
        let Some(cron) = self.cron.as_mut() else {
            return encode_error(
                id,
                "cron_unavailable",
                "cron scheduler is not active on this server",
            );
        };
        if let Err(error) = cron.scheduler.remove(&params.job_id) {
            return encode_error(id, "cron_remove_failed", error);
        }
        encode_success(id, ResponseResult::Ok {})
    }

    pub(super) fn handle_cron_runs(&mut self, id: String, params: CronRunsParams) -> String {
        let Some(cron) = self.cron.as_ref() else {
            return encode_error(
                id,
                "cron_unavailable",
                "cron scheduler is not active on this server",
            );
        };
        let runs = match cron
            .scheduler
            .ledger()
            .list_runs(&params.job_id, params.limit.unwrap_or(20))
        {
            Ok(runs) => runs.into_iter().map(run_info).collect(),
            Err(error) => return encode_error(id, "cron_runs_failed", error),
        };
        encode_success(id, ResponseResult::CronRuns { runs })
    }

    pub(super) fn handle_cron_log(&mut self, id: String, params: CronLogParams) -> String {
        let Some(cron) = self.cron.as_ref() else {
            return encode_error(
                id,
                "cron_unavailable",
                "cron scheduler is not active on this server",
            );
        };
        if params.run_id.len() != 36
            || !params.run_id.bytes().enumerate().all(|(index, byte)| {
                if [8, 13, 18, 23].contains(&index) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            })
        {
            return encode_error(id, "invalid_run_id", "invalid run id");
        }
        let run = match cron.scheduler.ledger().get_run(&params.run_id) {
            Ok(Some(run)) => run,
            Ok(None) => return encode_error(id, "cron_run_not_found", "run not found"),
            Err(_) => return encode_error(id, "cron_log_failed", "cannot read run ledger"),
        };
        let expected = cron.output_dir.join(format!("{}.log", run.id));
        if run.output_path.as_ref() != Some(&expected) {
            return encode_error(id, "cron_log_unavailable", "run has no local log");
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = match options.open(expected) {
            Ok(file) => file,
            Err(_) => return encode_error(id, "cron_log_unavailable", "run log is unavailable"),
        };
        const MAX_LOG_BYTES: u64 = 512 * 1024;
        let metadata = match file.metadata() {
            Ok(metadata) if metadata.len() <= MAX_LOG_BYTES => metadata,
            _ => return encode_error(id, "cron_log_unavailable", "run log is unavailable"),
        };
        if !metadata.is_file() {
            return encode_error(id, "cron_log_unavailable", "run log is unavailable");
        }
        let mut text = String::new();
        if file
            .take(MAX_LOG_BYTES + 1)
            .read_to_string(&mut text)
            .is_err()
            || text.len() as u64 > MAX_LOG_BYTES
        {
            return encode_error(id, "cron_log_unavailable", "run log is unavailable");
        }
        encode_success(
            id,
            ResponseResult::CronLog {
                run_id: run.id,
                text,
            },
        )
    }

    pub(super) fn handle_cron_doctor(&mut self, id: String) -> String {
        let Some(cron) = self.cron.as_ref() else {
            return encode_error(
                id,
                "cron_unavailable",
                "cron scheduler is not active on this server",
            );
        };
        let mut findings = Vec::new();
        for job in cron.scheduler.jobs() {
            if let Err(error) = job.validate() {
                findings.push(format!("{}: {error}", job.id));
            }
            if job.workdir.as_ref().is_some_and(|path| !path.is_dir()) {
                findings.push(format!("{}: workdir does not exist", job.id));
            }
            let binary = match &job.target {
                JobTarget::Headless { provider, .. } => Some(provider.as_str()),
                JobTarget::Script { program, .. } => Some(program.as_str()),
                JobTarget::Pane { .. } => None,
            };
            if binary.is_some_and(|program| !executable_exists(program, job.workdir.as_deref())) {
                findings.push(format!("{}: target binary not found", job.id));
            }
            if let JobTarget::Headless { prompt_files, .. } = &job.target {
                if prompt_files.iter().any(|path| !path.is_file()) {
                    findings.push(format!("{}: prompt file not found", job.id));
                }
            }
            if matches!(&job.target, JobTarget::Headless { provider, .. } if provider == "codex")
                && !mercury_cron::executor::codex_workdir_is_git_repo(job.workdir.as_deref())
            {
                findings.push(format!("{}: codex workdir is not a Git repository", job.id));
            }
            for destination in &job.deliver {
                let names: Vec<&str> = match destination {
                    Delivery::Teams { webhook_env } => {
                        if let Ok(url) = std::env::var(webhook_env) {
                            if crate::app::cron::validate_teams_url(&url, false).is_err() {
                                findings.push(format!("{}: invalid Teams webhook URL", job.id));
                            }
                        }
                        vec![webhook_env]
                    }
                    Delivery::Pane { .. } | Delivery::Local { notify: true } => Vec::new(),
                    _ => Vec::new(),
                };
                for name in names {
                    if std::env::var_os(name).is_none_or(|value| value.is_empty()) {
                        findings.push(format!(
                            "{}: destination env {name} is unset or empty",
                            job.id
                        ));
                    }
                }
            }
        }
        encode_success(id, ResponseResult::CronDoctor { findings })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::api::schema::{EmptyParams, Method, Request};
    use crate::app::AppPolicy;
    use crate::config::Config;
    use crate::events::AppEvent;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    fn test_root() -> PathBuf {
        static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "mercury-cron-api-{}-{sequence}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn app(root: &std::path::Path) -> App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.enable_cron_at(root).unwrap();
        app
    }

    fn script_definition() -> CronJobDefinition {
        CronJobDefinition {
            name: "check".into(),
            schedule: "every 1h".into(),
            utc_offset: Some("+00:00".into()),
            target: JobTarget::Script {
                program: "/bin/echo".into(),
                args: vec!["api-cron".into()],
            },
            workdir: None,
            timeout_seconds: Some(5),
            enabled: None,
            catch_up: mercury_cron::CatchUp::Skip,
            repeat_times: None,
            deliver: Vec::new(),
        }
    }

    #[test]
    fn preview_uses_server_engine_and_fixed_offset_without_creating_a_job() {
        let root = test_root();
        let mut app = app(&root);
        let read = call(
            &mut app,
            Method::CronPreview(crate::api::schema::CronPreviewParams {
                schedule: "every 1h".into(),
                utc_offset: Some("-00:30".into()),
            }),
        );
        assert_eq!(read["result"]["utc_offset"], "-00:30");
        let times = read["result"]["occurrences"].as_array().unwrap();
        assert_eq!(times.len(), 3);
        assert_eq!(
            times[1].as_i64().unwrap() - times[0].as_i64().unwrap(),
            3600
        );
        let once = (OffsetDateTime::now_utc() + time::Duration::days(1))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        let read = call(
            &mut app,
            Method::CronPreview(crate::api::schema::CronPreviewParams {
                schedule: once,
                utc_offset: Some("-03:00".into()),
            }),
        );
        assert_eq!(read["result"]["occurrences"].as_array().unwrap().len(), 1);
        let invalid = call(
            &mut app,
            Method::CronPreview(crate::api::schema::CronPreviewParams {
                schedule: "0 9 31 2 *".into(),
                utc_offset: Some("-03:00".into()),
            }),
        );
        assert_eq!(invalid["error"]["code"], "invalid_schedule");
        assert!(app.cron.as_ref().unwrap().scheduler.jobs().is_empty());
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn call(app: &mut App, method: Method) -> serde_json::Value {
        serde_json::from_str(&app.handle_api_request(Request {
            id: "test".into(),
            method,
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn api_add_list_run_and_history_share_persisted_scheduler() {
        let root = test_root();
        let mut app = app(&root);
        let added = call(
            &mut app,
            Method::CronAdd(CronAddParams {
                definition: script_definition(),
            }),
        );
        let job_id = added["result"]["job"]["id"].as_str().unwrap().to_owned();
        let listed = call(&mut app, Method::CronList(EmptyParams::default()));
        assert_eq!(listed["result"]["jobs"][0]["id"], job_id);
        assert!(listed.to_string().find("api-cron").is_none());
        let loaded = call(
            &mut app,
            Method::CronGet(CronJobTarget {
                job_id: job_id.clone(),
            }),
        );
        assert_eq!(loaded["result"]["definition"]["name"], "check");
        assert_eq!(loaded["result"]["definition"]["target"]["kind"], "script");
        let doctor = call(&mut app, Method::CronDoctor(EmptyParams::default()));
        assert!(doctor["result"]["findings"].as_array().unwrap().is_empty());
        let launched = call(
            &mut app,
            Method::CronRun(CronJobTarget {
                job_id: job_id.clone(),
            }),
        );
        let run_id = launched["result"]["run_id"].as_str().unwrap().to_owned();
        let event = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let event = app.event_rx.recv().await.unwrap();
                if matches!(event, AppEvent::CronCompleted { .. }) {
                    break event;
                }
            }
        })
        .await
        .unwrap();
        app.handle_internal_event_with_render_impact(event);
        let history = call(
            &mut app,
            Method::CronRuns(CronRunsParams {
                job_id: job_id.clone(),
                limit: Some(10),
            }),
        );
        assert_eq!(history["result"]["runs"][0]["id"], run_id);
        assert_eq!(history["result"]["runs"][0]["status"], "success");
        let listed = call(&mut app, Method::CronList(EmptyParams::default()));
        assert_eq!(listed["result"]["jobs"][0]["last_status"], "success");
        let log = call(
            &mut app,
            Method::CronLog(CronLogParams {
                run_id: run_id.clone(),
            }),
        );
        assert_eq!(log["result"]["run_id"], run_id);
        assert!(log["result"]["text"].as_str().unwrap().contains("api-cron"));
        let removed = call(
            &mut app,
            Method::CronRemove(CronJobTarget {
                job_id: job_id.clone(),
            }),
        );
        assert_eq!(removed["result"]["type"], "ok");
        let retained = call(
            &mut app,
            Method::CronRuns(CronRunsParams {
                job_id,
                limit: Some(10),
            }),
        );
        assert_eq!(retained["result"]["runs"][0]["id"], run_id);
        let retained_log = call(
            &mut app,
            Method::CronLog(CronLogParams {
                run_id: run_id.clone(),
            }),
        );
        assert_eq!(retained_log["result"]["text"], log["result"]["text"]);
        let empty = call(&mut app, Method::CronList(EmptyParams::default()));
        assert!(empty["result"]["jobs"].as_array().unwrap().is_empty());

        let rejected = call(
            &mut app,
            Method::CronLog(CronLogParams {
                run_id: "../jobs.toml".into(),
            }),
        );
        assert_eq!(rejected["error"]["code"], "invalid_run_id");
        let output_path = root.join("output").join(format!("{run_id}.log"));
        std::fs::remove_file(&output_path).unwrap();
        std::os::unix::fs::symlink(root.join("jobs.toml"), &output_path).unwrap();
        let rejected = call(&mut app, Method::CronLog(CronLogParams { run_id }));
        assert_eq!(rejected["error"]["code"], "cron_log_unavailable");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn api_rejects_invalid_job_and_supports_edit_pause_remove() {
        let root = test_root();
        let mut app = app(&root);
        let mut definition = script_definition();
        definition.schedule = "bad schedule".into();
        let rejected = call(&mut app, Method::CronAdd(CronAddParams { definition }));
        assert_eq!(rejected["error"]["code"], "invalid_cron_job");
        let added = call(
            &mut app,
            Method::CronAdd(CronAddParams {
                definition: script_definition(),
            }),
        );
        let job_id = added["result"]["job"]["id"].as_str().unwrap().to_owned();
        let paused = call(
            &mut app,
            Method::CronPause(CronPauseParams {
                job_id: job_id.clone(),
                paused: true,
            }),
        );
        assert_eq!(paused["result"]["job"]["enabled"], false);
        let mut edited = script_definition();
        edited.name = "renamed".into();
        let changed = call(
            &mut app,
            Method::CronEdit(CronEditParams {
                job_id: job_id.clone(),
                definition: edited,
            }),
        );
        assert_eq!(changed["result"]["job"]["name"], "renamed");
        assert_eq!(changed["result"]["job"]["enabled"], false);
        let resumed = call(
            &mut app,
            Method::CronPause(CronPauseParams {
                job_id: job_id.clone(),
                paused: false,
            }),
        );
        assert_eq!(resumed["result"]["job"]["enabled"], true);
        let removed = call(&mut app, Method::CronRemove(CronJobTarget { job_id }));
        assert_eq!(removed["result"]["type"], "ok");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn api_added_job_survives_scheduler_restart() {
        let root = test_root();
        let mut first = app(&root);
        let added = call(
            &mut first,
            Method::CronAdd(CronAddParams {
                definition: script_definition(),
            }),
        );
        let job_id = added["result"]["job"]["id"].as_str().unwrap().to_owned();
        drop(first);
        let mut second = app(&root);
        let listed = call(&mut second, Method::CronList(EmptyParams::default()));
        assert_eq!(listed["result"]["jobs"][0]["id"], job_id);
        drop(second);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn api_add_accepts_wizard_definition_without_explicit_utc_offset() {
        let root = test_root();
        let mut app = app(&root);
        let mut definition = script_definition();
        definition.utc_offset = None;
        let added = call(&mut app, Method::CronAdd(CronAddParams { definition }));
        assert!(added.get("error").is_none(), "{added}");
        let listed = call(&mut app, Method::CronList(EmptyParams::default()));
        assert_eq!(listed["result"]["jobs"].as_array().unwrap().len(), 1);
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn doctor_and_run_reject_codex_workdir_outside_git_before_process_start() {
        let root = test_root();
        let mut app = app(&root);
        let mut definition = script_definition();
        definition.workdir = Some(root.clone());
        definition.target = JobTarget::Headless {
            provider: "codex".into(),
            prompt: "check".into(),
            prompt_files: Vec::new(),
            model: None,
            effort: None,
            skills: Vec::new(),
        };
        let added = call(&mut app, Method::CronAdd(CronAddParams { definition }));
        let job_id = added["result"]["job"]["id"].as_str().unwrap().to_owned();
        let doctor = call(&mut app, Method::CronDoctor(EmptyParams::default()));
        assert!(doctor["result"]["findings"][0]
            .as_str()
            .unwrap()
            .contains("codex workdir is not a Git repository"));
        let _ = call(
            &mut app,
            Method::CronRun(CronJobTarget {
                job_id: job_id.clone(),
            }),
        );
        let runs = call(
            &mut app,
            Method::CronRuns(CronRunsParams {
                job_id,
                limit: Some(1),
            }),
        );
        assert_eq!(runs["result"]["runs"][0]["status"], "failure");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}
