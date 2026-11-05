//! Search the server-owned private index; never read agent homes in the CLI.

use std::collections::HashMap;
use std::io::{self, Read};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::api::schema::{
    AgentStartParams, EmptyParams, HistoryResolveParams, HistorySearchParams, Method,
    PaneRightClickTarget, PaneSplitParams, Request, SplitDirection, WorkspaceCreateParams,
};

static NEXT_HISTORY_AGENT: AtomicU32 = AtomicU32::new(0);
const START_RETRY_WINDOW: Duration = Duration::from_secs(3);
const START_RETRY_DELAY: Duration = Duration::from_millis(100);

fn agent_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let nonce = NEXT_HISTORY_AGENT.fetch_add(1, Ordering::Relaxed);
    format!("history-{nanos:016x}-{:04x}", nonce & 0xffff)
}

pub(super) fn run_history_command(args: &[String]) -> io::Result<i32> {
    match args.split_first() {
        Some((command, options)) if command == "search" => search(options),
        Some((command, options)) if command == "open" => open(options),
        _ => {
            eprintln!("usage: mercury history search [--query TEXT | --stdin] [--provider claude|codex] [--cwd PATH] [--from YYYY-MM-DD] [--until YYYY-MM-DD] [--limit N] [--json]\n       mercury history open claude|codex SESSION_ID [--json]");
            Ok(2)
        }
    }
}

fn search(args: &[String]) -> io::Result<i32> {
    let mut params = HistorySearchParams {
        query: String::new(),
        provider: None,
        cwd: None,
        from: None,
        until: None,
        limit: 30,
    };
    let mut stdin = false;
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--query" | "--provider" | "--cwd" | "--from" | "--until" | "--limit" => {
                let option = args[index].as_str();
                let Some(value) = args.get(index + 1) else {
                    return Ok(usage_error("missing history option value"));
                };
                match option {
                    "--query" => params.query = value.clone(),
                    "--provider" => params.provider = Some(value.clone()),
                    "--cwd" => params.cwd = Some(value.clone()),
                    "--from" => params.from = Some(value.clone()),
                    "--until" => params.until = Some(value.clone()),
                    "--limit" => match value.parse() {
                        Ok(limit) => params.limit = limit,
                        Err(_) => return Ok(usage_error("invalid history limit")),
                    },
                    _ => unreachable!(),
                }
                index += 2;
            }
            "--stdin" => {
                stdin = true;
                index += 1;
            }
            "--json" => {
                json = true;
                index += 1;
            }
            _ => return Ok(usage_error("unknown history search option")),
        }
    }
    if stdin {
        if !params.query.is_empty() {
            return Ok(usage_error("choose --query or --stdin"));
        }
        io::stdin().take(513).read_to_string(&mut params.query)?;
        params.query = params.query.trim_end_matches(['\n', '\r']).to_owned();
    }
    let response = super::send_request(&Request {
        id: "cli:history:search".into(),
        method: Method::HistorySearch(params),
    })?;
    if json || response.get("error").is_some() {
        return super::print_response(&response);
    }
    if let Some(hits) = response["result"]["hits"].as_array() {
        for hit in hits {
            let provider = hit["provider"].as_str().unwrap_or("?");
            let session = hit["session_id"].as_str().unwrap_or("?");
            let date = hit["timestamp"].as_str().unwrap_or("?");
            let cwd = hit["cwd"].as_str().unwrap_or("?");
            let snippet = hit["snippet"].as_str().unwrap_or("");
            println!("{provider} {session}  {date}  {cwd}\n  {snippet}");
        }
        if hits.is_empty() {
            println!("Nenhuma conversa encontrada");
        }
        return Ok(0);
    }
    super::print_response(&response)
}

fn open(args: &[String]) -> io::Result<i32> {
    let (provider, session_id, json) = match args {
        [provider, session_id] => (provider, session_id, false),
        [provider, session_id, flag] if flag == "--json" => (provider, session_id, true),
        _ => {
            return Ok(usage_error(
                "usage: mercury history open claude|codex SESSION_ID [--json]",
            ))
        }
    };
    let response = dispatch_open(provider, session_id, |method| {
        super::send_request(&Request {
            id: "cli:history:open".into(),
            method,
        })
    })?;
    if json || response.get("error").is_some() {
        return super::print_response(&response);
    }
    let pane = response["result"]["pane_id"].as_str().unwrap_or("?");
    println!("Sessão reaberta no pane {pane}");
    Ok(0)
}

fn dispatch_open<F>(provider: &str, session_id: &str, mut send: F) -> io::Result<Value>
where
    F: FnMut(Method) -> io::Result<Value>,
{
    let resolved = send(Method::HistoryResolve(HistoryResolveParams {
        provider: provider.into(),
        session_id: session_id.into(),
    }))?;
    if resolved.get("error").is_some() {
        return Ok(resolved);
    }
    let target = &resolved["result"]["target"];
    let Some(cwd) = target["cwd"].as_str() else {
        return Ok(cli_error(
            "history_session_unavailable",
            "session or original project is unavailable",
        ));
    };
    let Some(id) = target["session_id"].as_str() else {
        return Ok(cli_error(
            "history_session_unavailable",
            "session identity is unavailable",
        ));
    };
    let Some(kind) = target["provider"].as_str() else {
        return Ok(cli_error(
            "history_session_unavailable",
            "provider is unavailable",
        ));
    };
    let Some(session_ref) = crate::agent_resume::AgentSessionRef::id(id) else {
        return Ok(cli_error(
            "history_session_unavailable",
            "session identity is invalid",
        ));
    };
    let source = format!("mercury:{kind}");
    let Some(plan) = crate::agent_resume::plan(&source, kind, &session_ref) else {
        return Ok(cli_error(
            "history_session_unavailable",
            "native resume is unavailable",
        ));
    };
    let listed = send(Method::WorkspaceList(EmptyParams::default()))?;
    if listed.get("error").is_some() {
        return Ok(listed);
    }
    let Some(workspaces) = listed["result"]["workspaces"].as_array() else {
        return Ok(cli_error(
            "history_workspace_unavailable",
            "workspace list is unavailable",
        ));
    };
    let focused: Vec<_> = workspaces
        .iter()
        .filter(|workspace| workspace["focused"].as_bool() == Some(true))
        .collect();
    let selected = if workspaces.is_empty() {
        None
    } else if focused.len() == 1 {
        focused.first().copied()
    } else if focused.is_empty() && workspaces.len() == 1 {
        workspaces.first()
    } else {
        return Ok(cli_error(
            "history_workspace_ambiguous",
            "select one focused workspace before reopening history",
        ));
    };
    let (workspace_id, pane_id) = if let Some(workspace) = selected {
        let Some(workspace_id) = workspace["workspace_id"].as_str() else {
            return Ok(cli_error(
                "history_workspace_unavailable",
                "selected workspace has no id",
            ));
        };
        let split = send(Method::PaneSplit(PaneSplitParams {
            workspace_id: Some(workspace_id.into()),
            target_pane_id: None,
            direction: SplitDirection::Right,
            ratio: None,
            cwd: Some(cwd.into()),
            focus: true,
            right_click: PaneRightClickTarget::Mercury,
            env: HashMap::new(),
        }))?;
        if split.get("error").is_some() {
            return Ok(split);
        }
        let Some(pane_id) = split["result"]["pane"]["pane_id"].as_str() else {
            return Ok(cli_error(
                "history_pane_indeterminate",
                "pane creation returned no id; inspect the workspace",
            ));
        };
        (workspace_id.to_owned(), pane_id.to_owned())
    } else {
        let created = send(Method::WorkspaceCreate(WorkspaceCreateParams {
            source_workspace_id: None,
            cwd: Some(cwd.into()),
            focus: true,
            label: None,
            env: HashMap::new(),
        }))?;
        if created.get("error").is_some() {
            return Ok(created);
        }
        let Some(workspace_id) = created["result"]["workspace"]["workspace_id"].as_str() else {
            return Ok(cli_error(
                "history_workspace_indeterminate",
                "workspace creation returned no id; inspect the workspace list",
            ));
        };
        let Some(pane_id) = created["result"]["root_pane"]["pane_id"].as_str() else {
            return Ok(cli_error(
                "history_workspace_indeterminate",
                "workspace creation returned no root pane; inspect the workspace list",
            ));
        };
        (workspace_id.to_owned(), pane_id.to_owned())
    };
    let start = AgentStartParams {
        name: agent_name(),
        kind: kind.into(),
        pane_id: pane_id.clone(),
        args: plan.argv.into_iter().skip(1).collect(),
        timeout_ms: Some(30_000),
    };
    let deadline = Instant::now() + START_RETRY_WINDOW;
    let started = loop {
        let response = send(Method::AgentStart(start.clone()))?;
        if response["error"]["code"].as_str() == Some("agent_pane_busy")
            && Instant::now() < deadline
        {
            std::thread::sleep(START_RETRY_DELAY);
            continue;
        }
        break response;
    };
    if started.get("error").is_some() {
        return Ok(json!({"id":"cli:history:open","error":{
            "code":"history_start_failed", "message":"native resume could not start; pane preserved for inspection"
        }, "pane_id":pane_id,"workspace_id":workspace_id}));
    }
    if started["result"]["type"].as_str() != Some("agent_started")
        || started["result"]["agent"]["terminal_id"].as_str().is_none()
    {
        return Ok(json!({"id":"cli:history:open","error":{
            "code":"history_start_indeterminate",
            "message":"native resume result is unknown; pane preserved for inspection"
        }, "pane_id":pane_id,"workspace_id":workspace_id}));
    }
    Ok(
        json!({"id":"cli:history:open","result":{"type":"history_open","provider":kind,
        "session_id":id,"pane_id":pane_id,"workspace_id":workspace_id,
        "cwd":cwd,"status":"started"}}),
    )
}

fn cli_error(code: &str, message: &str) -> Value {
    json!({"id":"cli:history:open","error":{"code":code,"message":message}})
}

fn usage_error(message: &str) -> i32 {
    eprintln!("{message}");
    2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_workspace() -> Value {
        json!({"result":{"workspaces":[{"workspace_id":"w1","focused":true}]}})
    }

    #[test]
    fn missing_session_does_not_create_pane() {
        let mut calls = Vec::new();
        let response = dispatch_open("codex", "session-1", |method| {
            calls.push(crate::api::api_method_name(&method));
            Ok(json!({"result":{"type":"history_resolve","target":null}}))
        })
        .unwrap();
        assert_eq!(calls, ["history.resolve"]);
        assert_eq!(response["error"]["code"], "history_session_unavailable");
    }

    #[test]
    fn open_uses_native_resume_in_new_pane() {
        let mut calls = Vec::new();
        let response = dispatch_open("codex", "session-1", |method| {
            calls.push(method.clone());
            Ok(match method {
                Method::HistoryResolve(_) => json!({"result":{"target":{"provider":"codex","session_id":"session-1","cwd":"/project"}}}),
                Method::WorkspaceList(_) => one_workspace(),
                Method::PaneSplit(_) => json!({"result":{"pane":{"pane_id":"p1"}}}),
                Method::AgentStart(_) => json!({"result":{"type":"agent_started","agent":{"terminal_id":"t1"}}}),
                _ => unreachable!(),
            })
        }).unwrap();
        assert_eq!(response["result"]["pane_id"], "p1");
        assert!(
            matches!(&calls[2], Method::PaneSplit(params) if params.cwd.as_deref() == Some("/project") && params.workspace_id.as_deref() == Some("w1"))
        );
        assert!(
            matches!(&calls[3], Method::AgentStart(params) if params.args == ["resume", "session-1"])
        );
    }

    #[test]
    fn open_retries_busy_new_pane_without_creating_another() {
        let mut starts = Vec::new();
        let mut splits = 0;
        let response = dispatch_open("claude", "session-1", |method| {
            Ok(match method {
                Method::HistoryResolve(_) => json!({"result":{"target":{"provider":"claude","session_id":"session-1","cwd":"/project"}}}),
                Method::WorkspaceList(_) => one_workspace(),
                Method::PaneSplit(_) => {
                    splits += 1;
                    json!({"result":{"pane":{"pane_id":"p1"}}})
                }
                Method::AgentStart(params) => {
                    starts.push(params);
                    if starts.len() == 1 {
                        json!({"error":{"code":"agent_pane_busy"}})
                    } else {
                        json!({"result":{"type":"agent_started","agent":{"terminal_id":"t1"}}})
                    }
                }
                _ => unreachable!(),
            })
        })
        .unwrap();
        assert_eq!(response["result"]["pane_id"], "p1");
        assert_eq!(splits, 1);
        assert_eq!(starts.len(), 2);
        assert_eq!(starts[0].name, starts[1].name);
        assert_eq!(starts[0].args, ["--resume", "session-1"]);
    }

    #[test]
    fn open_preserves_pane_and_does_not_retry_nonbusy_start_error() {
        let mut starts = 0;
        let response = dispatch_open("codex", "session-1", |method| {
            Ok(match method {
                Method::HistoryResolve(_) => json!({"result":{"target":{"provider":"codex","session_id":"session-1","cwd":"/project"}}}),
                Method::WorkspaceList(_) => one_workspace(),
                Method::PaneSplit(_) => json!({"result":{"pane":{"pane_id":"p1"}}}),
                Method::AgentStart(_) => {
                    starts += 1;
                    json!({"error":{"code":"agent_start_failed"}})
                }
                _ => unreachable!(),
            })
        })
        .unwrap();
        assert_eq!(starts, 1);
        assert_eq!(response["error"]["code"], "history_start_failed");
        assert_eq!(response["pane_id"], "p1");
    }

    #[test]
    fn open_does_not_claim_started_on_malformed_agent_response() {
        let response = dispatch_open("codex", "session-1", |method| {
            Ok(match method {
                Method::HistoryResolve(_) => json!({"result":{"target":{"provider":"codex","session_id":"session-1","cwd":"/project"}}}),
                Method::WorkspaceList(_) => one_workspace(),
                Method::PaneSplit(_) => json!({"result":{"pane":{"pane_id":"p1"}}}),
                Method::AgentStart(_) => json!({"result":{"type":"agent_started"}}),
                _ => unreachable!(),
            })
        })
        .unwrap();
        assert_eq!(response["error"]["code"], "history_start_indeterminate");
        assert_eq!(response["pane_id"], "p1");
    }

    #[test]
    fn open_creates_project_workspace_when_server_has_none() {
        let mut calls = Vec::new();
        let response = dispatch_open("codex", "session-1", |method| {
            calls.push(method.clone());
            Ok(match method {
                Method::HistoryResolve(_) => json!({"result":{"target":{"provider":"codex","session_id":"session-1","cwd":"/project"}}}),
                Method::WorkspaceList(_) => json!({"result":{"workspaces":[]}}),
                Method::WorkspaceCreate(params) => {
                    assert_eq!(params.cwd.as_deref(), Some("/project"));
                    assert!(params.focus);
                    json!({"result":{"workspace":{"workspace_id":"w1"},"root_pane":{"pane_id":"w1:p1"}}})
                }
                Method::AgentStart(params) => {
                    assert_eq!(params.pane_id, "w1:p1");
                    json!({"result":{"type":"agent_started","agent":{"terminal_id":"t1"}}})
                }
                _ => unreachable!(),
            })
        })
        .unwrap();
        assert_eq!(response["result"]["workspace_id"], "w1");
        assert_eq!(response["result"]["pane_id"], "w1:p1");
        assert_eq!(calls.len(), 4);
    }

    #[test]
    fn open_rejects_ambiguous_workspaces_before_creating_pane() {
        let mut calls = Vec::new();
        let response = dispatch_open("codex", "session-1", |method| {
            calls.push(crate::api::api_method_name(&method));
            Ok(match method {
                Method::HistoryResolve(_) => json!({"result":{"target":{"provider":"codex","session_id":"session-1","cwd":"/project"}}}),
                Method::WorkspaceList(_) => json!({"result":{"workspaces":[
                    {"workspace_id":"w1","focused":false},
                    {"workspace_id":"w2","focused":false}
                ]}}),
                _ => unreachable!(),
            })
        })
        .unwrap();
        assert_eq!(calls, ["history.resolve", "workspace.list"]);
        assert_eq!(response["error"]["code"], "history_workspace_ambiguous");
    }
}
