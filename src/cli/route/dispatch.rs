//! One-shot orchestration across existing typed API methods. Only a failure
//! before `agent.prompt` may select another provider; a prompt call is never
//! retried because its transport result can be indeterminate.

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::api::schema::{
    AgentPromptParams, AgentPromptWaitOptions, AgentReadParams, AgentStartParams, AgentStatus,
    AgentTarget, EmptyParams, Method, PaneRightClickTarget, PaneSplitParams, PaneTarget,
    ReadFormat, ReadSource, RoutePreviewParams, SplitDirection, WorktreeCreateParams,
    WorktreeRemoveParams,
};

use super::RouteArgs;

const START_TIMEOUT: Duration = Duration::from_secs(30);
const START_RETRY_DELAY: Duration = Duration::from_millis(100);
const MAX_RESULT_BYTES: usize = 16 * 1024;
static NEXT_ROUTE_NAME: AtomicU32 = AtomicU32::new(0);

fn route_error(code: &str, message: &str) -> Value {
    json!({"id":"cli:route:dispatch","error":{"code":code,"message":message}})
}

fn route_state(
    status: &str,
    provider: &str,
    destination: &Destination,
    agent_name: &str,
    summary: Option<&str>,
    residual_branches: &[String],
) -> Value {
    json!({
        "id":"cli:route:dispatch",
        "result":{
            "type":"route_dispatch",
            "status":status,
            "provider":provider,
            "pane_id":destination.pane_id,
            "workspace_id":destination.workspace_id,
            "worktree_path":destination.worktree_path,
            "agent_name":agent_name,
            "summary":summary,
            "residual_branches":residual_branches,
        }
    })
}

struct Destination {
    workspace_id: String,
    pane_id: String,
    worktree_path: Option<String>,
    worktree_branch: Option<String>,
}

fn route_error_at(code: &str, message: &str, destination: &Destination) -> Value {
    json!({
        "id":"cli:route:dispatch",
        "error":{"code":code,"message":message},
        "workspace_id":destination.workspace_id,
        "pane_id":destination.pane_id,
        "worktree_path":destination.worktree_path,
    })
}

fn with_residual_branches(mut response: Value, branches: &[String]) -> Value {
    if !branches.is_empty() {
        response["residual_branches"] = json!(branches);
    }
    response
}

fn agent_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let nonce = NEXT_ROUTE_NAME.fetch_add(1, Ordering::Relaxed);
    format!("route-{nanos:016x}-{:04x}", nonce & 0xffff)
}

fn workspace(args: &RouteArgs, response: &Value) -> Result<(String, String), Value> {
    let Some(workspaces) = response["result"]["workspaces"].as_array() else {
        return Err(route_error(
            "route_workspace_unavailable",
            "cannot list workspaces",
        ));
    };
    let candidates: Vec<&Value> = workspaces
        .iter()
        .filter(|workspace| match args.project.as_deref() {
            Some(project) => {
                workspace["workspace_id"].as_str() == Some(project)
                    || workspace["label"].as_str() == Some(project)
            }
            None => workspace["focused"].as_bool() == Some(true),
        })
        .collect();
    if candidates.len() != 1 {
        return Err(route_error(
            "route_workspace_ambiguous",
            "select exactly one existing workspace by id or label",
        ));
    }
    let selected = candidates[0];
    let Some(id) = selected["workspace_id"].as_str() else {
        return Err(route_error(
            "route_workspace_unavailable",
            "workspace has no id",
        ));
    };
    let Some(label) = selected["label"].as_str() else {
        return Err(route_error(
            "route_workspace_unavailable",
            "workspace has no label",
        ));
    };
    Ok((id.to_owned(), label.to_owned()))
}

fn agent_args(provider: &str, model: Option<&str>, effort: Option<&str>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(model) = model {
        args.extend(["--model".into(), model.into()]);
    }
    if let Some(effort) = effort {
        if provider == "claude" {
            args.extend(["--effort".into(), effort.into()]);
        } else {
            args.extend([
                "--config".into(),
                format!("model_reasoning_effort={effort}"),
            ]);
        }
    }
    args
}

fn close_created_pane<F>(send: &mut F, pane_id: &str) -> io::Result<bool>
where
    F: FnMut(Method) -> io::Result<Value>,
{
    let response = send(Method::PaneClose(PaneTarget {
        pane_id: pane_id.to_owned(),
    }))?;
    Ok(response.get("error").is_none())
}

fn create_destination<F>(
    send: &mut F,
    workspace_id: &str,
    worktree: bool,
) -> io::Result<Result<Destination, Value>>
where
    F: FnMut(Method) -> io::Result<Value>,
{
    if worktree {
        let response = send(Method::WorktreeCreate(WorktreeCreateParams {
            workspace_id: Some(workspace_id.to_owned()),
            focus: false,
            trust_repository: false,
            ..WorktreeCreateParams::default()
        }))?;
        if response.get("error").is_some() {
            return Ok(Err(response));
        }
        let Some(created_workspace_id) = response["result"]["workspace"]["workspace_id"].as_str()
        else {
            return Ok(Err(route_error(
                "route_worktree_indeterminate",
                "worktree creation returned no workspace id; inspect the worktree list before retrying",
            )));
        };
        let Some(pane_id) = response["result"]["root_pane"]["pane_id"].as_str() else {
            return Ok(Err(route_error(
                "route_worktree_indeterminate",
                "worktree creation returned no root pane; inspect the new workspace before retrying",
            )));
        };
        let Some(path) = response["result"]["worktree"]["path"].as_str() else {
            return Ok(Err(route_error(
                "route_worktree_indeterminate",
                "worktree creation returned no checkout path; inspect the new workspace before retrying",
            )));
        };
        return Ok(Ok(Destination {
            workspace_id: created_workspace_id.to_owned(),
            pane_id: pane_id.to_owned(),
            worktree_path: Some(path.to_owned()),
            worktree_branch: response["result"]["worktree"]["branch"]
                .as_str()
                .map(str::to_owned),
        }));
    }
    let split = send(Method::PaneSplit(PaneSplitParams {
        workspace_id: Some(workspace_id.to_owned()),
        target_pane_id: None,
        direction: SplitDirection::Right,
        ratio: None,
        cwd: None,
        focus: true,
        right_click: PaneRightClickTarget::Mercury,
        env: HashMap::new(),
    }))?;
    if split.get("error").is_some() {
        return Ok(Err(split));
    }
    let Some(pane_id) = split["result"]["pane"]["pane_id"].as_str() else {
        return Ok(Err(route_error(
            "route_pane_failed",
            "pane split returned no pane id",
        )));
    };
    Ok(Ok(Destination {
        workspace_id: workspace_id.to_owned(),
        pane_id: pane_id.to_owned(),
        worktree_path: None,
        worktree_branch: None,
    }))
}

fn cleanup_destination<F>(send: &mut F, destination: &Destination) -> io::Result<bool>
where
    F: FnMut(Method) -> io::Result<Value>,
{
    if destination.worktree_path.is_some() {
        let response = send(Method::WorktreeRemove(WorktreeRemoveParams {
            workspace_id: destination.workspace_id.clone(),
            force: false,
            trust_repository: false,
        }))?;
        Ok(response["result"]["type"].as_str() == Some("worktree_removed"))
    } else {
        close_created_pane(send, &destination.pane_id)
    }
}

fn start_and_wait<F>(
    send: &mut F,
    pane_id: &str,
    provider: &str,
    name: &str,
    args: &[String],
) -> io::Result<Result<(), String>>
where
    F: FnMut(Method) -> io::Result<Value>,
{
    let deadline = Instant::now() + START_TIMEOUT;
    let terminal_id = loop {
        let response = send(Method::AgentStart(AgentStartParams {
            name: name.to_owned(),
            kind: provider.to_owned(),
            pane_id: pane_id.to_owned(),
            args: args.to_vec(),
            timeout_ms: Some(START_TIMEOUT.as_millis() as u64),
        }))?;
        if let Some(terminal_id) = response["result"]["agent"]["terminal_id"].as_str() {
            break terminal_id.to_owned();
        }
        if response["error"]["code"].as_str() == Some("agent_pane_busy")
            && Instant::now() < deadline
        {
            std::thread::sleep(START_RETRY_DELAY);
            continue;
        }
        return Ok(Err("agent could not start before prompt submission".into()));
    };
    let mut polls = 0;
    while Instant::now() < deadline {
        let response = send(Method::AgentGet(AgentTarget {
            target: pane_id.to_owned(),
        }))?;
        let agent = &response["result"]["agent"];
        if agent["terminal_id"].as_str() != Some(&terminal_id) {
            return Ok(Err("route pane changed while agent was starting".into()));
        }
        if agent["name"].as_str() != Some(name) {
            return Ok(Err("route agent name was lost during startup".into()));
        }
        if agent["agent_status"].as_str() == Some("blocked") {
            return Ok(Err("route agent blocked before prompt submission".into()));
        }
        let detected = agent["agent"].as_str();
        if detected.is_some_and(|detected| detected != provider) {
            return Ok(Err("route agent kind changed during startup".into()));
        }
        if agent["interactive_ready"].as_bool() == Some(true)
            && agent["launch_pending"].as_bool() != Some(true)
            && (detected == Some(provider) || (provider == "codex" && detected.is_none()))
        {
            return Ok(Ok(()));
        }
        if agent["launch_pending"].as_bool() == Some(false)
            && matches!(agent["agent_status"].as_str(), Some("idle" | "done"))
        {
            return Ok(Err("route agent exited before becoming interactive".into()));
        }
        polls += 1;
        if provider == "codex" && polls % 10 == 0 && waiting_for_folder_trust(send, pane_id) {
            return Ok(Err("route agent is waiting for folder trust".into()));
        }
        std::thread::sleep(START_RETRY_DELAY);
    }
    Ok(Err(
        "route agent did not become interactive before timeout".into()
    ))
}

fn bounded_tail(text: &str) -> &str {
    if text.len() <= MAX_RESULT_BYTES {
        return text;
    }
    let mut start = text.len() - MAX_RESULT_BYTES;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

fn waiting_for_folder_trust<F>(send: &mut F, pane_id: &str) -> bool
where
    F: FnMut(Method) -> io::Result<Value>,
{
    let read = send(Method::AgentRead(AgentReadParams {
        target: pane_id.to_owned(),
        source: ReadSource::Visible,
        lines: None,
        format: ReadFormat::Text,
        strip_ansi: true,
    }));
    read.ok()
        .and_then(|response| {
            response["result"]["read"]["text"]
                .as_str()
                .map(str::to_owned)
        })
        .is_some_and(|text| text.contains("Trust this folder?"))
}

pub(super) fn dispatch<F>(prompt: &str, args: &RouteArgs, mut send: F) -> io::Result<Value>
where
    F: FnMut(Method) -> io::Result<Value>,
{
    let listed = send(Method::WorkspaceList(EmptyParams::default()))?;
    if listed.get("error").is_some() {
        return Ok(listed);
    }
    let (workspace_id, project_label) = match workspace(args, &listed) {
        Ok(workspace) => workspace,
        Err(response) => return Ok(response),
    };
    let mut excluded = Vec::<String>::new();
    let mut last_start_error = None;
    let mut residual_branches = Vec::<String>::new();
    for _ in 0..2 {
        let preview = send(Method::RoutePreview(RoutePreviewParams {
            prompt: prompt.to_owned(),
            tags: args.tags.clone(),
            project: Some(project_label.clone()),
            workspace_id: Some(workspace_id.clone()),
            exclude: excluded.clone(),
        }))?;
        if preview.get("error").is_some() {
            return Ok(with_residual_branches(
                last_start_error.unwrap_or(preview),
                &residual_branches,
            ));
        }
        let route = &preview["result"]["route"];
        let Some(provider @ ("claude" | "codex")) = route["provider"].as_str() else {
            return Ok(route_error(
                "route_invalid_decision",
                "route preview returned an invalid provider",
            ));
        };
        let context = if let Some(id) = &args.context {
            let mut params = crate::api::schema::KnowledgeParams::new(
                crate::api::schema::KnowledgeAction::RouteContext,
            );
            params.id = id.clone();
            params.project = Some(project_label.clone());
            params.provider = provider.into();
            let result = send(Method::Knowledge(params))?;
            if result.get("error").is_some() {
                return Ok(result);
            }
            Some(result["result"]["data"].clone())
        } else {
            None
        };
        let destination = match create_destination(&mut send, &workspace_id, args.worktree) {
            Ok(Ok(destination)) => destination,
            Ok(Err(response)) => return Ok(with_residual_branches(response, &residual_branches)),
            Err(_) => {
                return Ok(with_residual_branches(
                    route_error(
                        "route_destination_indeterminate",
                        "destination creation result is unknown; inspect panes or worktrees before retrying",
                    ),
                    &residual_branches,
                ))
            }
        };
        let name = agent_name();
        let launch_args = agent_args(provider, route["model"].as_str(), route["effort"].as_str());
        let started = start_and_wait(
            &mut send,
            &destination.pane_id,
            provider,
            &name,
            &launch_args,
        );
        match started {
            Ok(Ok(())) => {}
            Ok(Err(reason)) => {
                if (reason == "route agent is waiting for folder trust"
                    || reason == "route agent did not become interactive before timeout")
                    && provider == "codex"
                    && (reason == "route agent is waiting for folder trust"
                        || waiting_for_folder_trust(&mut send, &destination.pane_id))
                {
                    return Ok(route_state(
                        "needs_interaction",
                        provider,
                        &destination,
                        &name,
                        Some(
                            "Codex is waiting for folder trust in this pane; no task was submitted",
                        ),
                        &residual_branches,
                    ));
                }
                if !matches!(cleanup_destination(&mut send, &destination), Ok(true)) {
                    return Ok(with_residual_branches(route_error_at(
                        "route_cleanup_failed",
                        "route destination could not be removed safely after startup failure; no fallback was attempted",
                        &destination,
                    ), &residual_branches));
                }
                if let Some(branch) = destination.worktree_branch {
                    residual_branches.push(branch);
                }
                excluded.push(provider.to_owned());
                last_start_error = Some(route_error("route_start_failed", &reason));
                continue;
            }
            Err(_) => {
                return Ok(route_state(
                    "indeterminate",
                    provider,
                    &destination,
                    &name,
                    Some("agent startup result is unknown; no prompt or fallback was attempted"),
                    &residual_branches,
                ));
            }
        }
        let prompt_with_context = if let Some(context) = context {
            let mut params = crate::api::schema::KnowledgeParams::new(
                crate::api::schema::KnowledgeAction::RouteContext,
            );
            params.id = args.context.clone().unwrap_or_default();
            params.project = Some(project_label.clone());
            params.provider = provider.into();
            let checked = send(Method::Knowledge(params))?;
            if checked.get("error").is_some() {
                return Ok(checked);
            }
            format!(
                "{prompt}\n\n<untrusted_project_context>\n{}\n</untrusted_project_context>",
                serde_json::to_string(&context)?
            )
        } else {
            prompt.to_owned()
        };
        let prompted = send(Method::AgentPrompt(AgentPromptParams {
            target: destination.pane_id.clone(),
            text: prompt_with_context,
            wait: args.wait.then_some(AgentPromptWaitOptions {
                until: vec![AgentStatus::Idle, AgentStatus::Done],
                timeout_ms: Some(args.timeout_ms),
                submission_deadline: None,
            }),
        }));
        let prompted = match prompted {
            Ok(prompted) => prompted,
            Err(_) => {
                return Ok(route_state(
                    "indeterminate",
                    provider,
                    &destination,
                    &name,
                    None,
                    &residual_branches,
                ));
            }
        };
        if prompted.get("error").is_some() {
            let status = match prompted["error"]["code"].as_str() {
                Some("timeout" | "agent_prompt_stalled") => "timed_out",
                _ => "indeterminate",
            };
            return Ok(route_state(
                status,
                provider,
                &destination,
                &name,
                None,
                &residual_branches,
            ));
        }
        if prompted["result"]["type"].as_str() != Some("agent_prompted") {
            return Ok(route_state(
                "indeterminate",
                provider,
                &destination,
                &name,
                None,
                &residual_branches,
            ));
        }
        if !args.wait {
            return Ok(route_state(
                "submitted",
                provider,
                &destination,
                &name,
                None,
                &residual_branches,
            ));
        }
        let read = send(Method::AgentRead(AgentReadParams {
            target: destination.pane_id.clone(),
            source: ReadSource::RecentUnwrapped,
            lines: Some(120),
            format: ReadFormat::Text,
            strip_ansi: true,
        }));
        let summary = read
            .ok()
            .filter(|read| read.get("error").is_none())
            .and_then(|read| read["result"]["read"]["text"].as_str().map(str::to_owned))
            .map(|text| text.replace(prompt.trim_end(), "[task redacted]"));
        return Ok(route_state(
            "completed",
            provider,
            &destination,
            &name,
            summary.as_deref().map(bounded_tail),
            &residual_branches,
        ));
    }
    Ok(with_residual_branches(
        last_start_error.unwrap_or_else(|| {
            route_error("route_unavailable", "no route provider could be started")
        }),
        &residual_branches,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(wait: bool) -> RouteArgs {
        RouteArgs {
            file: None,
            context: None,
            stdin: true,
            tags: Vec::new(),
            project: None,
            json: true,
            wait,
            timeout_ms: 10_000,
            worktree: false,
        }
    }

    #[test]
    fn dispatch_sends_prompt_once_and_never_in_launch_args() {
        let mut prompts = 0;
        let mut launch_args = Vec::new();
        let mut started_name = String::new();
        let response = dispatch("private task", &options(true), |method| {
            Ok(match method {
                Method::WorkspaceList(_) => json!({"result":{"workspaces":[{"workspace_id":"w1","label":"project","focused":true}]}}),
                Method::RoutePreview(params) => {
                    assert_eq!(params.project.as_deref(), Some("project"));
                    assert_eq!(params.workspace_id.as_deref(), Some("w1"));
                    json!({"result":{"route":{"provider":"codex","model":"gpt-test","effort":"high"}}})
                }
                Method::PaneSplit(_) => json!({"result":{"pane":{"pane_id":"w1:p2"}}}),
                Method::AgentStart(params) => {
                    launch_args = params.args;
                    started_name = params.name;
                    json!({"result":{"agent":{"terminal_id":"terminal-1"}}})
                }
                Method::AgentGet(_) => json!({"result":{"agent":{"terminal_id":"terminal-1","name": started_name,"agent":"codex","agent_status":"idle","interactive_ready":true,"launch_pending":false}}}),
                Method::AgentPrompt(params) => {
                    prompts += 1;
                    assert_eq!(params.text, "private task");
                    json!({"result":{"type":"agent_prompted"}})
                }
                Method::AgentRead(_) => json!({"result":{"read":{"text":"private task\ndone"}}}),
                other => panic!("unexpected method: {other:?}"),
            })
        })
        .unwrap();
        assert_eq!(response["result"]["status"], "completed");
        assert_eq!(response["result"]["summary"], "[task redacted]\ndone");
        assert_eq!(prompts, 1);
        assert!(!launch_args.join(" ").contains("private task"));
    }

    #[test]
    fn startup_failure_closes_own_pane_then_falls_back_before_prompt() {
        let mut split_count = 0;
        let mut closed = Vec::new();
        let mut started_name = String::new();
        let mut prompts = 0;
        let result = dispatch("task", &options(false), |method| {
            Ok(match method {
                Method::WorkspaceList(_) => json!({"result":{"workspaces":[{"workspace_id":"w1","label":"project","focused":true}]}}),
                Method::RoutePreview(params) if params.exclude.is_empty() => {
                    json!({"result":{"route":{"provider":"claude"}}})
                }
                Method::RoutePreview(params) => {
                    assert_eq!(params.exclude, vec!["claude".to_owned()]);
                    json!({"result":{"route":{"provider":"codex"}}})
                }
                Method::PaneSplit(_) => {
                    split_count += 1;
                    json!({"result":{"pane":{"pane_id":format!("w1:p{}", split_count + 1)}}})
                }
                Method::AgentStart(params) if params.kind == "claude" => {
                    json!({"error":{"code":"agent_start_failed","message":"not ready"}})
                }
                Method::AgentStart(params) => {
                    started_name = params.name;
                    json!({"result":{"agent":{"terminal_id":"terminal-2"}}})
                }
                Method::PaneClose(target) => {
                    closed.push(target.pane_id);
                    json!({"result":{"type":"ok"}})
                }
                Method::AgentGet(_) => json!({"result":{"agent":{"terminal_id":"terminal-2","name":started_name,"agent":"codex","agent_status":"idle","interactive_ready":true,"launch_pending":false}}}),
                Method::AgentPrompt(params) => {
                    prompts += 1;
                    assert_eq!(params.target, "w1:p3");
                    json!({"result":{"type":"agent_prompted"}})
                }
                other => panic!("unexpected method: {other:?}"),
            })
        })
        .unwrap();
        assert_eq!(closed, ["w1:p2"]);
        assert_eq!(prompts, 1);
        assert_eq!(result["result"]["provider"], "codex");
        assert_eq!(result["result"]["status"], "submitted");
    }

    #[test]
    fn uncertain_prompt_result_never_falls_back_or_sends_again() {
        let mut previews = 0;
        let mut prompts = 0;
        let mut started_name = String::new();
        let result = dispatch("task", &options(false), |method| {
            match method {
                Method::WorkspaceList(_) => Ok(json!({"result":{"workspaces":[{"workspace_id":"w1","label":"project","focused":true}]}})),
                Method::RoutePreview(_) => {
                    previews += 1;
                    Ok(json!({"result":{"route":{"provider":"codex"}}}))
                }
                Method::PaneSplit(_) => Ok(json!({"result":{"pane":{"pane_id":"w1:p2"}}})),
                Method::AgentStart(params) => {
                    started_name = params.name;
                    Ok(json!({"result":{"agent":{"terminal_id":"terminal-1"}}}))
                }
                Method::AgentGet(_) => Ok(json!({"result":{"agent":{"terminal_id":"terminal-1","name":started_name,"agent":"codex","agent_status":"idle","interactive_ready":true,"launch_pending":false}}})),
                Method::AgentPrompt(_) => {
                    prompts += 1;
                    Err(io::Error::other("connection lost after submission"))
                }
                other => panic!("unexpected method: {other:?}"),
            }
        })
        .unwrap();
        assert_eq!(previews, 1);
        assert_eq!(prompts, 1);
        assert_eq!(result["result"]["status"], "indeterminate");
    }

    #[test]
    fn folder_trust_wait_preserves_pane_and_does_not_submit() {
        let mut started_name = String::new();
        let mut previews = 0;
        let result = dispatch("task", &options(false), |method| {
            Ok(match method {
                Method::WorkspaceList(_) => json!({"result":{"workspaces":[{"workspace_id":"w1","label":"project","focused":true}]}}),
                Method::RoutePreview(_) => {
                    previews += 1;
                    json!({"result":{"route":{"provider":"codex"}}})
                }
                Method::PaneSplit(_) => json!({"result":{"pane":{"pane_id":"w1:p2"}}}),
                Method::AgentStart(params) => {
                    started_name = params.name;
                    json!({"result":{"agent":{"terminal_id":"terminal-1"}}})
                }
                Method::AgentGet(_) => json!({"result":{"agent":{"terminal_id":"terminal-1","name":started_name,"agent":"codex","agent_status":"unknown","interactive_ready":false,"launch_pending":false}}}),
                Method::AgentRead(_) => json!({"result":{"read":{"text":"Trust this folder?"}}}),
                other => panic!("unexpected method: {other:?}"),
            })
        })
        .unwrap();
        assert_eq!(previews, 1);
        assert_eq!(result["result"]["status"], "needs_interaction");
        assert_eq!(result["result"]["pane_id"], "w1:p2");
    }

    #[test]
    fn worktree_route_uses_created_root_pane_and_returns_checkout() {
        let mut args = options(false);
        args.worktree = true;
        let mut started_name = String::new();
        let result = dispatch("task", &args, |method| {
            Ok(match method {
                Method::WorkspaceList(_) => json!({"result":{"workspaces":[{"workspace_id":"w1","label":"project","focused":true}]}}),
                Method::RoutePreview(_) => json!({"result":{"route":{"provider":"codex"}}}),
                Method::WorktreeCreate(params) => {
                    assert_eq!(params.workspace_id.as_deref(), Some("w1"));
                    assert!(!params.trust_repository);
                    assert!(params.branch.is_none());
                    json!({"result":{"type":"worktree_created","workspace":{"workspace_id":"w2"},"root_pane":{"pane_id":"w2:p1"},"worktree":{"path":"/worktrees/task","branch":"worktree/task"}}})
                }
                Method::AgentStart(params) => {
                    assert_eq!(params.pane_id, "w2:p1");
                    started_name = params.name;
                    json!({"result":{"agent":{"terminal_id":"terminal-2"}}})
                }
                Method::AgentGet(_) => json!({"result":{"agent":{"terminal_id":"terminal-2","name":started_name,"agent":"codex","agent_status":"idle","interactive_ready":true,"launch_pending":false}}}),
                Method::AgentPrompt(params) => {
                    assert_eq!(params.target, "w2:p1");
                    json!({"result":{"type":"agent_prompted"}})
                }
                other => panic!("unexpected method: {other:?}"),
            })
        })
        .unwrap();
        assert_eq!(result["result"]["status"], "submitted");
        assert_eq!(result["result"]["workspace_id"], "w2");
        assert_eq!(result["result"]["worktree_path"], "/worktrees/task");
    }

    #[test]
    fn worktree_fallback_requires_clean_remove_and_reports_residual_branch() {
        let mut args = options(false);
        args.worktree = true;
        let mut created = 0;
        let mut removed = 0;
        let mut started_name = String::new();
        let result = dispatch("task", &args, |method| {
            Ok(match method {
                Method::WorkspaceList(_) => json!({"result":{"workspaces":[{"workspace_id":"w1","label":"project","focused":true}]}}),
                Method::RoutePreview(params) if params.exclude.is_empty() => json!({"result":{"route":{"provider":"claude"}}}),
                Method::RoutePreview(params) => {
                    assert_eq!(params.exclude, ["claude"]);
                    json!({"result":{"route":{"provider":"codex"}}})
                }
                Method::WorktreeCreate(_) => {
                    created += 1;
                    json!({"result":{"type":"worktree_created","workspace":{"workspace_id":format!("w{}",created+1)},"root_pane":{"pane_id":format!("w{}:p1",created+1)},"worktree":{"path":format!("/worktrees/task-{created}"),"branch":format!("worktree/task-{created}")}}})
                }
                Method::AgentStart(params) if params.kind == "claude" => json!({"error":{"code":"agent_start_failed"}}),
                Method::WorktreeRemove(params) => {
                    removed += 1;
                    assert!(!params.force);
                    assert_eq!(params.workspace_id, "w2");
                    json!({"result":{"type":"worktree_removed"}})
                }
                Method::AgentStart(params) => {
                    started_name = params.name;
                    json!({"result":{"agent":{"terminal_id":"terminal-3"}}})
                }
                Method::AgentGet(_) => json!({"result":{"agent":{"terminal_id":"terminal-3","name":started_name,"agent":"codex","agent_status":"idle","interactive_ready":true,"launch_pending":false}}}),
                Method::AgentPrompt(_) => json!({"result":{"type":"agent_prompted"}}),
                other => panic!("unexpected method: {other:?}"),
            })
        })
        .unwrap();
        assert_eq!(created, 2);
        assert_eq!(removed, 1);
        assert_eq!(result["result"]["workspace_id"], "w3");
        assert_eq!(
            result["result"]["residual_branches"],
            json!(["worktree/task-1"])
        );
    }

    #[test]
    fn dirty_worktree_cleanup_stops_fallback_and_preserves_location() {
        let mut args = options(false);
        args.worktree = true;
        let result = dispatch("task", &args, |method| {
            Ok(match method {
                Method::WorkspaceList(_) => json!({"result":{"workspaces":[{"workspace_id":"w1","label":"project","focused":true}]}}),
                Method::RoutePreview(_) => json!({"result":{"route":{"provider":"claude"}}}),
                Method::WorktreeCreate(_) => json!({"result":{"type":"worktree_created","workspace":{"workspace_id":"w2"},"root_pane":{"pane_id":"w2:p1"},"worktree":{"path":"/worktrees/task","branch":"worktree/task"}}}),
                Method::AgentStart(_) => json!({"error":{"code":"agent_start_failed"}}),
                Method::WorktreeRemove(params) => {
                    assert!(!params.force);
                    json!({"error":{"code":"dirty_worktree_requires_force"}})
                }
                other => panic!("unexpected method: {other:?}"),
            })
        })
        .unwrap();
        assert_eq!(result["error"]["code"], "route_cleanup_failed");
        assert_eq!(result["workspace_id"], "w2");
        assert_eq!(result["worktree_path"], "/worktrees/task");
    }
    #[test]
    fn context_egress_denial_happens_before_pane_creation_or_prompt() {
        let mut args = options(false);
        args.context = Some("bundle".into());
        let mut checked = false;
        let result=dispatch("task",&args,|method|Ok(match method {
            Method::WorkspaceList(_)=>json!({"result":{"workspaces":[{"workspace_id":"w1","label":"PV","focused":true}]}}),
            Method::RoutePreview(_)=>json!({"result":{"route":{"provider":"codex"}}}),
            Method::Knowledge(p)=>{checked=true;assert_eq!(p.provider,"codex");json!({"error":{"code":"context_egress_denied","message":"blocked"}})},
            _=>panic!("no pane, agent or prompt may start after context denial"),
        })).unwrap();
        assert!(checked);
        assert_eq!(result["error"]["code"], "context_egress_denied");
    }
}
