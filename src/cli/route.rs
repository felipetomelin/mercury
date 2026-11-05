//! Route preview and one-shot interactive dispatch. Prompts are read from a
//! bounded file or stdin, not placed in an agent's launch arguments.

use std::io::Read;

use crate::api::schema::{Method, Request, RoutePreviewParams};

mod dispatch;

const MAX_PROMPT_BYTES: u64 = 128 * 1024;

#[derive(Debug, PartialEq, Eq)]
struct RouteArgs {
    file: Option<String>,
    context: Option<String>,
    stdin: bool,
    tags: Vec<String>,
    project: Option<String>,
    json: bool,
    wait: bool,
    timeout_ms: u64,
    worktree: bool,
}

fn parse_route_args(args: &[String], submit: bool) -> Result<RouteArgs, &'static str> {
    let mut parsed = RouteArgs {
        file: None,
        context: None,
        stdin: false,
        tags: Vec::new(),
        project: None,
        json: false,
        wait: false,
        timeout_ms: 300_000,
        worktree: false,
    };
    let mut index = 0;
    let mut timeout_set = false;
    while index < args.len() {
        match args[index].as_str() {
            "--context" if submit => {
                let value = args.get(index + 1).ok_or("missing --context value")?;
                if parsed.context.replace(value.clone()).is_some() {
                    return Err("--context may appear only once");
                }
                index += 2;
            }
            "--file" => {
                let value = args.get(index + 1).ok_or("missing --file value")?;
                if parsed.file.replace(value.clone()).is_some() {
                    return Err("--file may appear only once");
                }
                index += 2;
            }
            "--stdin" => {
                parsed.stdin = true;
                index += 1;
            }
            "--tag" => {
                let value = args.get(index + 1).ok_or("missing --tag value")?;
                parsed.tags.push(value.clone());
                index += 2;
            }
            "--project" => {
                let value = args.get(index + 1).ok_or("missing --project value")?;
                if parsed.project.replace(value.clone()).is_some() {
                    return Err("--project may appear only once");
                }
                index += 2;
            }
            "--json" => {
                parsed.json = true;
                index += 1;
            }
            "--wait" if submit => {
                parsed.wait = true;
                index += 1;
            }
            "--worktree" if submit => {
                parsed.worktree = true;
                index += 1;
            }
            "--timeout" if submit => {
                let value = args.get(index + 1).ok_or("missing --timeout value")?;
                parsed.timeout_ms = value.parse().map_err(|_| "invalid --timeout value")?;
                timeout_set = true;
                index += 2;
            }
            _ => return Err("unknown route option"),
        }
    }
    if parsed.stdin == parsed.file.is_some() {
        return Err("choose exactly one of --file or --stdin");
    }
    if parsed.tags.len() > 32
        || parsed
            .tags
            .iter()
            .any(|tag| tag.is_empty() || tag.len() > 128)
        || parsed
            .project
            .as_ref()
            .is_some_and(|project| project.len() > 512)
    {
        return Err("invalid route metadata");
    }
    if submit && !(1_000..=3_600_000).contains(&parsed.timeout_ms) {
        return Err("route timeout must be 1000–3600000 ms");
    }
    if timeout_set && !parsed.wait {
        return Err("--timeout requires --wait");
    }
    Ok(parsed)
}

fn read_prompt(parsed: &RouteArgs) -> std::io::Result<String> {
    let mut input: Box<dyn Read> = if let Some(path) = &parsed.file {
        Box::new(std::fs::File::open(path)?)
    } else {
        Box::new(std::io::stdin())
    };
    let mut prompt = String::new();
    input
        .by_ref()
        .take(MAX_PROMPT_BYTES + 1)
        .read_to_string(&mut prompt)?;
    if prompt.trim().is_empty() || prompt.len() as u64 > MAX_PROMPT_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "prompt must be 1–131072 UTF-8 bytes",
        ));
    }
    Ok(prompt)
}

pub(super) fn run_route_command(args: &[String]) -> std::io::Result<i32> {
    let (submit, options) = match args.split_first() {
        Some((command, options)) if command == "preview" => (false, options),
        Some((command, options)) if command == "submit" => (true, options),
        Some((command, _)) if command.starts_with('-') => (true, args),
        _ => {
            eprintln!("usage: mercury route [preview|submit] (--file PATH | --stdin) [--tag TAG] [--project PROJECT] [--worktree] [--wait] [--timeout MS] [--json]");
            return Ok(2);
        }
    };
    let parsed = match parse_route_args(options, submit) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("{error}");
            return Ok(2);
        }
    };
    let prompt = match read_prompt(&parsed) {
        Ok(prompt) => prompt,
        Err(error) => {
            eprintln!("cannot read route prompt: {error}");
            return Ok(2);
        }
    };
    if submit {
        let response = dispatch::dispatch(&prompt, &parsed, |method| {
            super::send_request(&Request {
                id: "cli:route:dispatch".into(),
                method,
            })
        })?;
        let code = super::print_response(&response)?;
        if matches!(
            response["result"]["status"].as_str(),
            Some("timed_out" | "indeterminate" | "needs_interaction")
        ) {
            return Ok(1);
        }
        return Ok(code);
    }
    let response = super::send_request(&Request {
        id: "cli:route:preview".into(),
        method: Method::RoutePreview(RoutePreviewParams {
            prompt,
            tags: parsed.tags,
            project: parsed.project,
            workspace_id: None,
            exclude: Vec::new(),
        }),
    })?;
    if parsed.json || response.get("error").is_some() {
        return super::print_response(&response);
    }
    let route = &response["result"]["route"];
    let provider = route["provider"].as_str().unwrap_or("unknown");
    let headroom = route["headroom"].as_str().unwrap_or("unknown");
    let fallback = route["fallback_from"].as_str();
    match fallback {
        Some(primary) => println!("{provider} (fallback from {primary}; headroom {headroom})"),
        None => println!("{provider} (headroom {headroom})"),
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_requires_one_non_argv_prompt_source() {
        assert!(parse_route_args(&[], false).is_err());
        assert!(parse_route_args(&["--stdin".into(), "--file".into(), "x".into()], false).is_err());
        assert!(parse_route_args(&["secret prompt".into()], false).is_err());
        let parsed = parse_route_args(
            &[
                "--stdin".into(),
                "--tag".into(),
                "review".into(),
                "--project".into(),
                "checkout".into(),
            ],
            false,
        )
        .unwrap();
        assert_eq!(parsed.tags, vec!["review"]);
        assert_eq!(parsed.project.as_deref(), Some("checkout"));
        assert!(parse_route_args(&["--stdin".into(), "--wait".into()], false).is_err());
        assert!(parse_route_args(&["--stdin".into(), "--worktree".into()], false).is_err());
        assert!(
            parse_route_args(&["--stdin".into(), "--wait".into()], true)
                .unwrap()
                .wait
        );
        assert!(
            parse_route_args(&["--stdin".into(), "--worktree".into()], true)
                .unwrap()
                .worktree
        );
    }
}
