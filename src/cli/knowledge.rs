use crate::api::schema::{KnowledgeAction as Action, KnowledgeParams, Method, Request};
use serde_json::{json, Value};
use std::{
    io::{self, Read},
    time::{Duration, Instant},
};

pub(super) fn run(args: &[String], context: bool) -> io::Result<i32> {
    let Some((command, options)) = args.split_first() else {
        eprintln!("usage: mercury knowledge status|doctor|index|search|related|ask|capture|review|proposals|jobs; mercury context build");
        return Ok(2);
    };
    let action = if context {
        match command.as_str() {
            "build" => Action::Context,
            _ => return Ok(2),
        }
    } else {
        match command.as_str() {
            "status" => Action::Status,
            "doctor" => Action::Doctor,
            "index" => Action::Index,
            "search" => Action::Search,
            "related" => Action::Related,
            "ask" => Action::Ask,
            "capture" => Action::Capture,
            "review" => Action::Review,
            "proposals" => Action::Proposals,
            "jobs" => Action::JobShow,
            _ => return Ok(2),
        }
    };
    let mut p = KnowledgeParams::new(action);
    let mut is_json = false;
    let mut stdin = false;
    let mut i = 0;
    if command == "jobs" {
        match options.first().map(String::as_str) {
            Some("show") => p.action = Action::JobShow,
            Some("cancel") => p.action = Action::JobCancel,
            _ => return Ok(2),
        }
        i = 1;
    }
    while i < options.len() {
        let arg = options[i].as_str();
        match arg {
            "--json" => is_json = true,
            "--graph" => p.graph = true,
            "--rebuild" => p.rebuild = true,
            "--completed" => p.completed = true,
            "--stdin" => stdin = true,
            "--query" | "--project" | "--kind" | "--status" | "--service" | "--id" | "--mode"
            | "--provider" | "--title" => {
                let Some(v) = options.get(i + 1) else {
                    eprintln!("missing {arg} value");
                    return Ok(2);
                };
                match arg {
                    "--query" => p.query = v.clone(),
                    "--project" => p.project = Some(v.clone()),
                    "--kind" => p.kind = Some(v.clone()),
                    "--status" => p.status = Some(v.clone()),
                    "--service" => p.service = Some(v.clone()),
                    "--id" => p.id = v.clone(),
                    "--mode" => p.mode = v.clone(),
                    "--provider" => p.provider = v.clone(),
                    "--title" => p.title = v.clone(),
                    _ => unreachable!(),
                }
                i += 1;
            }
            _ if !arg.starts_with('-')
                && p.id.is_empty()
                && matches!(
                    p.action,
                    Action::Related | Action::JobShow | Action::JobCancel | Action::Review
                ) =>
            {
                p.id = arg.into()
            }
            _ => {
                eprintln!("unknown knowledge option");
                return Ok(2);
            }
        }
        i += 1;
    }
    if stdin {
        if !p.query.is_empty() {
            return Ok(2);
        }
        io::stdin().take(2049).read_to_string(&mut p.query)?;
        p.query = p.query.trim_end().to_owned();
    }
    let wait = matches!(p.action, Action::Ask | Action::Doctor | Action::Capture)
        || p.action == Action::Search && p.mode == "hybrid";
    let mut response = send(p)?;
    if wait {
        if let Some(job) = response["result"]["data"]["job_id"]
            .as_str()
            .map(str::to_owned)
        {
            response = wait_job(&job)?;
        }
    }
    if is_json || response.get("error").is_some() {
        return super::print_response(&response);
    }
    let data = &response["result"]["data"];
    // Pretty JSON preserves IDs, source paths and revision for human inspection.
    println!("{}", serde_json::to_string_pretty(data)?);
    Ok(0)
}
pub(super) fn send(params: KnowledgeParams) -> io::Result<Value> {
    super::send_request(&Request {
        id: "cli:knowledge".into(),
        method: Method::Knowledge(params),
    })
}
fn wait_job(id: &str) -> io::Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(3600);
    loop {
        let mut p = KnowledgeParams::new(Action::JobShow);
        p.id = id.into();
        let v = send(p)?;
        if v.get("error").is_some() {
            return Ok(v);
        }
        let data = &v["result"]["data"];
        match data["state"].as_str() {
            Some("completed") => {
                return Ok(
                    json!({"id":"cli:knowledge","result":{"type":"knowledge","data":data["result"]}}),
                )
            }
            Some("failed" | "cancelled" | "interrupted") => {
                return Ok(
                    json!({"id":"cli:knowledge","error":{"code":"knowledge_job_failed","message":data["error"].as_str().unwrap_or("knowledge job interrupted")}}),
                )
            }
            _ => {}
        }
        if Instant::now() > deadline {
            return Ok(
                json!({"id":"cli:knowledge","error":{"code":"knowledge_wait_timeout","message":"job still running; inspect knowledge jobs show or cancel","job_id":id}}),
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
