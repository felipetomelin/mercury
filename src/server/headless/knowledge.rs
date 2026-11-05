//! Long jobs never block the UI; cancellation/status bypass the serial model worker.
use crate::api::schema::{KnowledgeAction as Action, KnowledgeParams, Method, Request};
use mercury_knowledge::{
    backend::{Model, NativeModel},
    Config, Error, Filter, Index, Result,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
static PENDING: AtomicUsize = AtomicUsize::new(0);
static CONTROL_PENDING: AtomicUsize = AtomicUsize::new(0);
static SERIAL: Mutex<()> = Mutex::new(());
static CANCELS: OnceLock<Mutex<BTreeMap<String, Arc<AtomicBool>>>> = OnceLock::new();
fn cancels() -> &'static Mutex<BTreeMap<String, Arc<AtomicBool>>> {
    CANCELS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
fn encode(id: &str, result: Result<Value>) -> String {
    match result {
        Ok(data) => json!({"id":id,"result":{"type":"knowledge","data":data}}).to_string(),
        Err(e) => {
            json!({"id":id,"error":{"code":"knowledge_error","message":e.to_string()}}).to_string()
        }
    }
}
fn root() -> std::path::PathBuf {
    crate::session::data_dir().join("knowledge")
}
fn save_job(
    root: &Path,
    id: &str,
    state: &str,
    result: Option<Value>,
    error: Option<String>,
) -> Result<Value> {
    let value = json!({"id":id,"state":state,"result":result,"error":error,"updated":mercury_knowledge::now()});
    let index = Index::open(root)?;
    index.save_job(id, &value)?;
    Ok(value)
}
pub(super) fn dispatch(
    request: Request,
    respond: mpsc::Sender<String>,
    config: Config,
    route: mercury_router::RouteConfig,
    limits: Vec<mercury_usage::ProviderLimits>,
) {
    let Method::Knowledge(params) = request.method else {
        return;
    };
    let id = request.id;
    let counter = if matches!(params.action, Action::JobShow | Action::JobCancel) {
        &CONTROL_PENDING
    } else {
        &PENDING
    };
    if counter.fetch_add(1, Ordering::AcqRel) >= 8 {
        counter.fetch_sub(1, Ordering::AcqRel);
        let _ = respond.send(encode(&id, Err(Error::Invalid("knowledge queue full"))));
        return;
    }
    tokio::task::spawn_blocking(move || {
        struct Guard(&'static AtomicUsize);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::AcqRel);
            }
        }
        let _guard = Guard(counter);
        if matches!(params.action, Action::JobShow | Action::JobCancel) {
            let result = (|| {
                if params.action == Action::JobCancel {
                    if let Some(c) = cancels()
                        .lock()
                        .map_err(|_| Error::Unavailable)?
                        .get(&params.id)
                    {
                        c.store(true, Ordering::Relaxed);
                    }
                }
                let index = Index::open(&root())?;
                let mut job = index
                    .job(&params.id)?
                    .ok_or(Error::Invalid("unknown knowledge job"))?;
                if [Some("queued"), Some("running")].contains(&job["state"].as_str())
                    && !cancels()
                        .lock()
                        .map_err(|_| Error::Unavailable)?
                        .contains_key(&params.id)
                {
                    job["state"] = json!("interrupted");
                }
                Ok(job)
            })();
            let _ = respond.send(encode(&id, result));
            return;
        }
        if !config.enabled {
            let _ = respond.send(encode(
                &id,
                if params.action == Action::Status {
                    Ok(json!({"enabled":false}))
                } else {
                    Err(Error::Invalid(
                        "knowledge is disabled; configure [knowledge]",
                    ))
                },
            ));
            return;
        }
        if params.query.len() > 2048
            || params.id.len() > 512
            || params.title.len() > 256
            || params.project.as_ref().is_some_and(|p| p.len() > 128)
            || params.kind.as_ref().is_some_and(|v| v.len() > 64)
            || params.status.as_ref().is_some_and(|v| v.len() > 32)
            || params.service.as_ref().is_some_and(|v| v.len() > 256)
            || params.mode.len() > 16
            || params.provider.len() > 16
        {
            let _ = respond.send(encode(
                &id,
                Err(Error::Invalid("knowledge input exceeds limit")),
            ));
            return;
        }
        let long = matches!(
            params.action,
            Action::Index | Action::Ask | Action::Doctor | Action::Capture
        ) || params.action == Action::Search && params.mode == "hybrid";
        if long {
            let job_id = mercury_knowledge::hash(
                format!(
                    "{}:{}:{}",
                    std::process::id(),
                    mercury_knowledge::now(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                )
                .as_bytes(),
            );
            let cancel = Arc::new(AtomicBool::new(false));
            let deadline_cancel = cancel.clone();
            let deadline_task = tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                deadline_cancel.store(true, Ordering::Relaxed);
            });
            let result = save_job(&root(), &job_id, "queued", None, None);
            if result.is_err() {
                deadline_task.abort();
                let _ = respond.send(encode(&id, result));
                return;
            }
            if let Ok(mut c) = cancels().lock() {
                c.insert(job_id.clone(), cancel.clone());
            }
            let _ = respond.send(encode(&id, Ok(json!({"job_id":job_id}))));
            let result = (|| {
                let _serial = SERIAL.lock().map_err(|_| Error::Unavailable)?;
                if cancel.load(Ordering::Relaxed) {
                    return Err(Error::Cancelled);
                }
                save_job(&root(), &job_id, "running", None, None)?;
                handle(&root(), &config, &params, &cancel, &route, &limits)
            })();
            match result {
                Ok(value) => {
                    let _ = save_job(&root(), &job_id, "completed", Some(value), None);
                }
                Err(error) => {
                    let state = if matches!(error, Error::Cancelled) {
                        "cancelled"
                    } else {
                        "failed"
                    };
                    let _ = save_job(&root(), &job_id, state, None, Some(error.to_string()));
                }
            }
            deadline_task.abort();
            if let Ok(mut c) = cancels().lock() {
                c.remove(&job_id);
            }
        } else {
            let result = (|| {
                if params.action == Action::Status {
                    return handle(
                        &root(),
                        &config,
                        &params,
                        &AtomicBool::new(false),
                        &route,
                        &limits,
                    );
                }
                let _serial = SERIAL
                    .try_lock()
                    .map_err(|_| Error::Invalid("knowledge worker busy; retry shortly"))?;
                handle(
                    &root(),
                    &config,
                    &params,
                    &AtomicBool::new(false),
                    &route,
                    &limits,
                )
            })();
            let _ = respond.send(encode(&id, result));
        }
    });
}
fn handle(
    root: &Path,
    config: &Config,
    p: &KnowledgeParams,
    cancel: &AtomicBool,
    route: &mercury_router::RouteConfig,
    limits: &[mercury_usage::ProviderLimits],
) -> Result<Value> {
    config.validate()?;
    let mut index = Index::open(root)?;
    if matches!(
        p.action,
        Action::Search | Action::Related | Action::Ask | Action::Context | Action::RouteContext
    ) {
        if let Some(snapshot) = index.snapshot()? {
            mercury_knowledge::capture::verify_vault(config, &snapshot)?;
        }
    }
    let filter = Filter {
        project: p.project.clone(),
        kind: p.kind.clone(),
        status: p.status.clone(),
        service: p.service.clone(),
    };
    match p.action {
        Action::Status => Ok(
            json!({"enabled":true,"index":index.status()?,"pending":PENDING.load(Ordering::Relaxed)}),
        ),
        Action::Doctor => NativeModel::new(
            config,
            &root.join("model"),
            route,
            limits,
            p.project.as_deref(),
            cancel,
        )?
        .doctor(),
        Action::Index => {
            let mut model_metadata = Value::Null;
            let status = if p.rebuild {
                index.rebuild(config, cancel)?
            } else {
                index.sync(config, cancel)?
            };
            if p.graph {
                let model = NativeModel::new(
                    config,
                    &root.join("model"),
                    route,
                    limits,
                    p.project.as_deref(),
                    cancel,
                )?;
                let derived = index.build_graph_verified(&model, cancel, config)?;
                model_metadata = derived.model_metadata;
                mercury_knowledge::capture::verify_vault(
                    config,
                    &index.snapshot()?.ok_or(Error::Conflict)?,
                )?;
            }
            Ok(
                json!({"index":index.status()?,"revision":status.revision,"model":model_metadata,"diagnostics":index.snapshot()?.map(|s|s.diagnostics)}),
            )
        }
        Action::Search => {
            let hits = if p.mode == "hybrid" {
                index.hybrid(
                    &p.query,
                    &filter,
                    30,
                    &NativeModel::new(
                        config,
                        &root.join("model"),
                        route,
                        limits,
                        p.project.as_deref(),
                        cancel,
                    )?,
                )?
            } else {
                index.search(&p.query, &filter, 30)?
            };
            Ok(json!({"hits":hits,"revision":index.status()?.revision}))
        }
        Action::Related => {
            Ok(json!({"relations":index.related(&p.id,2)?,"revision":index.status()?.revision}))
        }
        Action::Ask => {
            let model = NativeModel::new(
                config,
                &root.join("model"),
                route,
                limits,
                p.project.as_deref(),
                cancel,
            )?;
            let answer = index.ask(
                &p.query,
                if p.mode.is_empty() { "local" } else { &p.mode },
                &filter,
                &model,
                cancel,
                config.max_context_chars,
            )?;
            mercury_knowledge::capture::verify_vault(
                config,
                &index.snapshot()?.ok_or(Error::Conflict)?,
            )?;
            let mut result = serde_json::to_value(answer)?;
            result["usage"] = serde_json::to_value(model.observations())?;
            Ok(result)
        }
        Action::Context => Ok(serde_json::to_value(
            index.context(&p.query, p.project.as_deref().unwrap_or(""))?,
        )?),
        Action::RouteContext => {
            if !config.route_destinations.contains(&p.provider) {
                return Err(Error::Invalid(
                    "route destination is not allowed for knowledge context",
                ));
            }
            Ok(serde_json::to_value(index.route_bundle(
                &p.id,
                p.project.as_deref().unwrap_or(""),
                config.allow_route_external,
            )?)?)
        }
        Action::Proposals => Ok(serde_json::to_value(index.proposals()?)?),
        Action::Review => Ok(serde_json::to_value(index.review(config, &p.id, &p.mode)?)?),
        Action::Capture => capture(&index, config, p, cancel, route, limits),
        _ => Err(Error::Invalid("invalid knowledge action")),
    }
}
fn capture(
    index: &Index,
    config: &Config,
    p: &KnowledgeParams,
    cancel: &AtomicBool,
    route: &mercury_router::RouteConfig,
    limits: &[mercury_usage::ProviderLimits],
) -> Result<Value> {
    use mercury_learn::{
        storage::{transcript_fingerprint, Storage},
        transcript::{self, Provider},
    };
    let provider = match p.provider.as_str() {
        "claude" => Provider::Claude,
        "codex" => Provider::Codex,
        _ => return Err(Error::Invalid("capture provider must be claude or codex")),
    };
    // Only a trusted completed-session ledger entry authorizes transcript reading.
    if !mercury_usage::locate::is_safe_session_id(&p.id) {
        return Err(Error::Invalid("invalid capture session"));
    }
    let (path, fingerprint, completion) = if p.completed {
        let path = match provider {
            Provider::Claude => mercury_usage::locate::default_claude_home()
                .and_then(|h| mercury_usage::locate::claude_transcript_path(&h, None, &p.id)),
            Provider::Codex => mercury_usage::locate::default_codex_home()
                .and_then(|h| mercury_usage::locate::codex_rollout_path(&h, &p.id, 36500)),
        }
        .ok_or(Error::Invalid(
            "capture transcript not found in configured provider home",
        ))?;
        let fingerprint = transcript_fingerprint(&path).map_err(|_| Error::Conflict)?;
        (path, fingerprint, "user-attested")
    } else {
        let storage = Storage::open(&crate::session::data_dir().join("learn"))
            .map_err(|_| Error::Unavailable)?;
        let session = storage
            .recent_completed_sessions(64)
            .map_err(|_| Error::Unavailable)?
            .into_iter()
            .find(|s| s.session_id == p.id && s.provider == provider)
            .ok_or(Error::Invalid(
                "completion not recorded; use --completed only after ending the session",
            ))?;
        (
            session.transcript_path,
            session.fingerprint.ok_or(Error::Conflict)?,
            "recorded",
        )
    };
    if transcript_fingerprint(&path).map_err(|_| Error::Conflict)? != fingerprint {
        return Err(Error::Conflict);
    }
    let snapshot = transcript::read(
        &path,
        provider,
        &p.id,
        transcript::Limits {
            total_text_bytes: 16 * 1024,
            ..Default::default()
        },
    )
    .map_err(|_| Error::Conflict)?;
    let messages: Vec<_> = snapshot
        .messages
        .iter()
        .map(|m| json!({"role":format!("{:?}",m.role),"text":m.text}))
        .collect();
    let model = NativeModel::new(
        config,
        &root().join("model"),
        route,
        limits,
        p.project.as_deref(),
        cancel,
    )?;
    let summary=model.structured("propose reusable engineering learning; preserve uncertainty and cite session, do not emit a transcript",&json!({"messages":messages,"truncated":snapshot.truncated}),&json!({"type":"object","properties":{"markdown":{"type":"string"}},"required":["markdown"],"additionalProperties":false}))?;
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    if transcript_fingerprint(&path).map_err(|_| Error::Conflict)? != fingerprint {
        return Err(Error::Conflict);
    }
    let markdown = with_confirmed_links(
        index,
        p.project.as_deref().unwrap_or(""),
        summary["markdown"]
            .as_str()
            .ok_or(Error::Invalid("invalid capture output"))?,
    )?;
    Ok(serde_json::to_value(index.propose(
        &format!(
            "{}:{}:{}:{}:{}",
            completion,
            p.provider,
            p.id,
            fingerprint,
            model.fingerprint()
        ),
        &p.title,
        p.project.as_deref().unwrap_or(""),
        &markdown,
    )?)?)
}

fn with_confirmed_links(index: &Index, project: &str, markdown: &str) -> Result<String> {
    let mut output = markdown.to_owned();
    let Some(snapshot) = index.snapshot()? else {
        return Ok(output);
    };
    let lower = markdown.to_lowercase();
    let mut lines = Vec::new();
    for relation in &snapshot.relations {
        let source = snapshot
            .notes
            .iter()
            .find(|n| n.id == relation.source && n.project == project && n.status == "validated");
        let target = snapshot
            .notes
            .iter()
            .find(|n| n.id == relation.target && n.project == project);
        if let (Some(source), Some(target)) = (source, target) {
            if !lower.contains(&source.title.to_lowercase())
                || !lower.contains(&target.title.to_lowercase())
            {
                continue;
            }
            let clean = |p: &str| {
                !p.chars()
                    .any(|c| c.is_control() || ['[', ']', '|', '#'].contains(&c))
            };
            if !clean(&source.path) || !clean(&target.path) {
                continue;
            }
            let line = format!(
                "- [[{}]] — {} → [[{}]] (origem: nota validada).",
                source.path.trim_end_matches(".md"),
                relation.kind,
                target.path.trim_end_matches(".md")
            );
            if output.len() + lines.iter().map(String::len).sum::<usize>() + line.len() > 30_000
                || lines.len() >= 8
            {
                break;
            }
            lines.push(line);
        }
    }
    if !lines.is_empty() {
        output.push_str("\n\n## Relações explícitas relacionadas\n");
        output.push_str(&lines.join("\n"));
    }
    Ok(output)
}
