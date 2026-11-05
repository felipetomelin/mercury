//! Learn inspection through the server-owned local API.

use std::io;

use crate::api::schema::{
    EmptyParams, LearnChangeTarget, LearnListParams, LearnUndoDateParams, Method, Request,
};

pub(super) fn run_learn_command(args: &[String]) -> io::Result<i32> {
    let [command, options @ ..] = args else {
        return Ok(usage());
    };
    match command.as_str() {
        "list" => list(options),
        "status" => status(options),
        "show" => target_command("show", options),
        "diff" => target_command("diff", options),
        "undo" if options.first().is_some_and(|arg| arg == "--date") => undo_date(options),
        "undo" => target_command("undo", options),
        _ => Ok(usage()),
    }
}

fn status(options: &[String]) -> io::Result<i32> {
    let json = match options {
        [] => false,
        [flag] if flag == "--json" => true,
        _ => return Ok(usage()),
    };
    let response = super::send_request(&Request {
        id: "cli:learn:status".into(),
        method: Method::LearnStatus(EmptyParams::default()),
    })?;
    if json || response.get("error").is_some() {
        return super::print_response(&response);
    }
    let status = &response["result"]["status"];
    if status.is_null() {
        return super::print_response(&response);
    }
    let changes = &status["changes"];
    let triggers = &status["triggers"];
    println!(
        "ledger: {}",
        if status["ledger_present"] == true {
            "present"
        } else {
            "absent"
        }
    );
    println!(
        "changes: applied={} conflict={} failed={} undone={} prepared={} undo_prepared={}",
        changes["applied"].as_u64().unwrap_or_default(),
        changes["conflict"].as_u64().unwrap_or_default(),
        changes["failed"].as_u64().unwrap_or_default(),
        changes["undone"].as_u64().unwrap_or_default(),
        changes["prepared"].as_u64().unwrap_or_default(),
        changes["undo_prepared"].as_u64().unwrap_or_default(),
    );
    println!(
        "triggers: claimed={} succeeded={} failed={}",
        triggers["claimed"].as_u64().unwrap_or_default(),
        triggers["succeeded"].as_u64().unwrap_or_default(),
        triggers["failed"].as_u64().unwrap_or_default(),
    );
    Ok(0)
}

fn undo_date(options: &[String]) -> io::Result<i32> {
    let Some((date, json)) = parse_date_options(options) else {
        return Ok(usage());
    };
    let response = super::send_request(&Request {
        id: "cli:learn:undo-date".into(),
        method: Method::LearnUndoDate(LearnUndoDateParams { date }),
    })?;
    if response.get("error").is_some() {
        return super::print_response(&response);
    }
    let Some(outcomes) = response["result"]["outcomes"].as_array() else {
        return super::print_response(&response);
    };
    let remaining_count = response["result"]["remaining_count"]
        .as_u64()
        .unwrap_or_default();
    let failed = response["result"]["interrupted"].as_bool() == Some(true)
        || remaining_count > 0
        || outcomes.iter().any(|item| {
            matches!(
                item["outcome"].as_str(),
                Some("conflict" | "failed" | "uncertain")
            )
        });
    if json {
        super::print_response(&response)?;
    } else {
        for item in outcomes {
            println!(
                "{}  {}",
                item["change_id"].as_str().unwrap_or("?"),
                item["outcome"].as_str().unwrap_or("?")
            );
        }
        if outcomes.is_empty() {
            println!("Nenhuma alteração Learn encontrada nesta data");
        }
        if response["result"]["interrupted"].as_bool() == Some(true) {
            eprintln!("Undo interrompido; {remaining_count} item(ns) não tentados. Confira o ledger antes de tentar novamente");
        }
    }
    Ok(if failed { 1 } else { 0 })
}

fn parse_date_options(args: &[String]) -> Option<(String, bool)> {
    let (date, json) = match args {
        [flag, date] if flag == "--date" => (date, false),
        [flag, date, json] if flag == "--date" && json == "--json" => (date, true),
        _ => return None,
    };
    if date.len() != 10 {
        return None;
    }
    Some((date.clone(), json))
}

fn list(options: &[String]) -> io::Result<i32> {
    let (limit, json) = match parse_list_options(options) {
        Some(parsed) => parsed,
        None => return Ok(usage()),
    };
    let response = super::send_request(&Request {
        id: "cli:learn:list".into(),
        method: Method::LearnList(LearnListParams { limit: Some(limit) }),
    })?;
    if json || response.get("error").is_some() {
        return super::print_response(&response);
    }
    let Some(changes) = response["result"]["changes"].as_array() else {
        return super::print_response(&response);
    };
    if changes.is_empty() {
        println!("Nenhuma alteração Learn encontrada");
    }
    for change in changes {
        let id = change["id"].as_str().unwrap_or("?");
        let target = change["target_id"].as_str().unwrap_or("?");
        let status = change["status"].as_str().unwrap_or("?");
        let created = change["created_at_ms"].as_i64().unwrap_or_default();
        println!("{id}  {status}  {target}  {created}");
    }
    Ok(0)
}

fn target_command(command: &str, options: &[String]) -> io::Result<i32> {
    let Some((change_id, json)) = parse_target_options(options) else {
        return Ok(usage());
    };
    let target = LearnChangeTarget { change_id };
    let method = match command {
        "show" => Method::LearnShow(target),
        "diff" => Method::LearnDiff(target),
        "undo" => Method::LearnUndo(target),
        _ => return Ok(usage()),
    };
    let response = super::send_request(&Request {
        id: format!("cli:learn:{command}"),
        method,
    })?;
    if json || response.get("error").is_some() {
        return super::print_response(&response);
    }
    if command == "diff" {
        let diff = &response["result"]["diff"];
        let change = &diff["change"];
        let summary = &diff["summary"];
        if change.is_null() {
            return super::print_response(&response);
        }
        if summary.is_null() {
            println!(
                "{}  diff unavailable (after snapshot missing)",
                change["id"].as_str().unwrap_or("?")
            );
        } else {
            println!(
                "{}  -{} +{} lines (unchanged prefix {}, suffix {}; content redacted)",
                change["id"].as_str().unwrap_or("?"),
                summary["removed_lines"].as_u64().unwrap_or_default(),
                summary["added_lines"].as_u64().unwrap_or_default(),
                summary["unchanged_prefix_lines"]
                    .as_u64()
                    .unwrap_or_default(),
                summary["unchanged_suffix_lines"]
                    .as_u64()
                    .unwrap_or_default(),
            );
        }
        return Ok(0);
    }
    if command == "show" {
        let detail = &response["result"]["detail"];
        let change = &detail["change"];
        if change.is_null() {
            return super::print_response(&response);
        }
        println!(
            "{}  {}  {}  {}",
            change["id"].as_str().unwrap_or("?"),
            change["status"].as_str().unwrap_or("?"),
            change["target_id"].as_str().unwrap_or("?"),
            change["created_at_ms"].as_i64().unwrap_or_default()
        );
        println!("before: {}", detail["before_hash"].as_str().unwrap_or("?"));
        println!("after:  {}", detail["after_hash"].as_str().unwrap_or("?"));
        println!("reason: [redacted]");
        return Ok(0);
    }
    let change = &response["result"]["change"];
    if change.is_null() {
        return super::print_response(&response);
    }
    println!(
        "{}  {}",
        change["id"].as_str().unwrap_or("?"),
        change["status"].as_str().unwrap_or("?")
    );
    Ok(0)
}

fn parse_target_options(args: &[String]) -> Option<(String, bool)> {
    let (change_id, json) = match args {
        [change_id] => (change_id, false),
        [change_id, flag] if flag == "--json" => (change_id, true),
        _ => return None,
    };
    if change_id.is_empty() || change_id.len() > 64 {
        return None;
    }
    Some((change_id.clone(), json))
}

fn parse_list_options(args: &[String]) -> Option<(usize, bool)> {
    let mut limit = 30;
    let mut seen_limit = false;
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--limit" if !seen_limit => {
                limit = args.get(index + 1)?.parse().ok()?;
                if !(1..=100).contains(&limit) {
                    return None;
                }
                seen_limit = true;
                index += 2;
            }
            "--json" if !json => {
                json = true;
                index += 1;
            }
            _ => return None,
        }
    }
    Some((limit, json))
}

fn usage() -> i32 {
    eprintln!("usage: mercury learn list [--limit 1..100] [--json]\n       mercury learn status [--json]\n       mercury learn show CHANGE_ID [--json]\n       mercury learn diff CHANGE_ID [--json]\n       mercury learn undo CHANGE_ID [--json]\n       mercury learn undo --date YYYY-MM-DD [--json]");
    2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_options_enforce_server_limit_and_reject_ambiguity() {
        assert_eq!(parse_list_options(&[]), Some((30, false)));
        assert_eq!(
            parse_list_options(&["--json".into(), "--limit".into(), "100".into()]),
            Some((100, true))
        );
        assert!(parse_list_options(&["--limit".into(), "0".into()]).is_none());
        assert!(parse_list_options(&["--limit".into(), "101".into()]).is_none());
        assert!(parse_list_options(&["--limit".into()]).is_none());
        assert!(parse_list_options(&["--json".into(), "--json".into()]).is_none());
    }

    #[test]
    fn target_options_require_one_change_id_and_optional_json() {
        assert_eq!(
            parse_target_options(&["change-1".into(), "--json".into()]),
            Some(("change-1".into(), true))
        );
        assert!(parse_target_options(&[]).is_none());
        assert!(parse_target_options(&["".into()]).is_none());
        assert!(parse_target_options(&["change-1".into(), "--limit".into()]).is_none());
    }

    #[test]
    fn date_options_require_date_and_optional_json() {
        assert_eq!(
            parse_date_options(&["--date".into(), "2026-10-06".into(), "--json".into()]),
            Some(("2026-10-06".into(), true))
        );
        assert!(parse_date_options(&["--date".into()]).is_none());
        assert!(parse_date_options(&["--date".into(), "yesterday".into()]).is_none());
    }
}
