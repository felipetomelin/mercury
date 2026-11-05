//! Cron CLI. Prompts come from files, never command-line arguments.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use mercury_cron::{CatchUp, Delivery, JobTarget};

use crate::api::schema::{
    CronAddParams, CronEditParams, CronJobDefinition, CronJobTarget, CronLogParams,
    CronPauseParams, CronRunsParams, EmptyParams, Method, Request,
};

const MAX_DEFINITION_BYTES: u64 = 128 * 1024;
const MAX_PROMPT_BYTES: u64 = 512 * 1024;

fn read_bounded(path: &Path, limit: u64) -> Result<String, String> {
    let mut content = String::new();
    File::open(path)
        .map_err(|_| "cannot open input file")?
        .take(limit + 1)
        .read_to_string(&mut content)
        .map_err(|_| "cannot read UTF-8 input file")?;
    if content.len() as u64 > limit {
        return Err("input file exceeds size limit".into());
    }
    Ok(content)
}

fn definition_file(path: &str) -> Result<CronJobDefinition, String> {
    let content = read_bounded(Path::new(path), MAX_DEFINITION_BYTES)?;
    toml::from_str(&content).map_err(|_| "invalid job TOML".into())
}

fn request(name: &str, method: Method) -> std::io::Result<i32> {
    super::print_response(&super::send_request(&Request {
        id: format!("cli:cron:{name}"),
        method,
    })?)
}

fn required_arg<'a>(args: &'a [String], usage: &str) -> Option<&'a str> {
    if let [arg] = args {
        Some(arg)
    } else {
        eprintln!("{usage}");
        None
    }
}

pub(super) fn run_cron_command(args: &[String]) -> std::io::Result<i32> {
    let Some(command) = args.first().map(String::as_str) else {
        print_help();
        return Ok(2);
    };
    match command {
        "list" if args.len() == 1 => request("list", Method::CronList(EmptyParams::default())),
        "doctor" if args.len() == 1 => {
            let response = super::send_request(&Request {
                id: "cli:cron:doctor".into(),
                method: Method::CronDoctor(EmptyParams::default()),
            })?;
            let code = super::print_response(&response)?;
            Ok(
                if code == 0
                    && response["result"]["findings"]
                        .as_array()
                        .is_some_and(|findings| !findings.is_empty())
                {
                    1
                } else {
                    code
                },
            )
        }
        "add" => match parse_add(&args[1..]) {
            Ok(definition) => request("add", Method::CronAdd(CronAddParams { definition })),
            Err(error) => {
                eprintln!("{error}");
                Ok(2)
            }
        },
        "edit" => {
            let [job_id, flag, path] = &args[1..] else {
                eprintln!("usage: mercury cron edit JOB_ID --file JOB.toml");
                return Ok(2);
            };
            if flag != "--file" {
                eprintln!("usage: mercury cron edit JOB_ID --file JOB.toml");
                return Ok(2);
            }
            match definition_file(path) {
                Ok(definition) => request(
                    "edit",
                    Method::CronEdit(CronEditParams {
                        job_id: job_id.clone(),
                        definition,
                    }),
                ),
                Err(error) => {
                    eprintln!("{error}");
                    Ok(2)
                }
            }
        }
        "pause" | "resume" => {
            let Some(job_id) = required_arg(&args[1..], "usage: mercury cron pause|resume JOB_ID")
            else {
                return Ok(2);
            };
            request(
                command,
                Method::CronPause(CronPauseParams {
                    job_id: job_id.into(),
                    paused: command == "pause",
                }),
            )
        }
        "run" | "rm" => {
            let Some(job_id) = required_arg(&args[1..], "usage: mercury cron run|rm JOB_ID") else {
                return Ok(2);
            };
            let target = CronJobTarget {
                job_id: job_id.into(),
            };
            request(
                command,
                if command == "run" {
                    Method::CronRun(target)
                } else {
                    Method::CronRemove(target)
                },
            )
        }
        "runs" => {
            let [job_id] = &args[1..] else {
                eprintln!("usage: mercury cron runs JOB_ID");
                return Ok(2);
            };
            request(
                "runs",
                Method::CronRuns(CronRunsParams {
                    job_id: job_id.clone(),
                    limit: Some(20),
                }),
            )
        }
        "log" => {
            let Some(run_id) = required_arg(&args[1..], "usage: mercury cron log RUN_ID") else {
                return Ok(2);
            };
            request(
                "log",
                Method::CronLog(CronLogParams {
                    run_id: run_id.into(),
                }),
            )
        }
        "help" | "--help" | "-h" => {
            print_help();
            Ok(0)
        }
        _ => {
            print_help();
            Ok(2)
        }
    }
}

fn parse_add(args: &[String]) -> Result<CronJobDefinition, String> {
    if let [flag, path] = args {
        if flag == "--file" {
            return definition_file(path);
        }
    }
    let mut name = None;
    let mut schedule = None;
    let mut script = None;
    let mut provider = None;
    let mut pane = None;
    let mut prompt_file = None;
    let mut script_args = Vec::new();
    let mut workdir = None;
    let mut timeout_seconds = None;
    let mut utc_offset = None;
    let mut repeat_times = None;
    let mut index = 0;
    while index < args.len() {
        let option = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("missing value for {option}"))?;
        match option {
            "--name" => name = Some(value.clone()),
            "--schedule" => schedule = Some(value.clone()),
            "--script" => script = Some(value.clone()),
            "--provider" => provider = Some(value.clone()),
            "--pane" => pane = Some(value.clone()),
            "--prompt-file" => prompt_file = Some(value.clone()),
            "--arg" => script_args.push(value.clone()),
            "--workdir" => workdir = Some(PathBuf::from(value)),
            "--timeout" => timeout_seconds = Some(value.parse().map_err(|_| "invalid timeout")?),
            "--utc-offset" => utc_offset = Some(value.clone()),
            "--repeat" => repeat_times = Some(value.parse().map_err(|_| "invalid repeat count")?),
            _ => return Err(format!("unknown option {option}")),
        }
        index += 2;
    }
    let name = name.ok_or("missing --name")?;
    let schedule = schedule.ok_or("missing --schedule")?;
    let count = usize::from(script.is_some())
        + usize::from(provider.is_some())
        + usize::from(pane.is_some());
    if count != 1 {
        return Err("choose exactly one of --script, --provider, or --pane".into());
    }
    let target = if let Some(program) = script {
        if prompt_file.is_some() {
            return Err("--prompt-file is not valid for scripts".into());
        }
        JobTarget::Script {
            program,
            args: script_args,
        }
    } else {
        if !script_args.is_empty() {
            return Err("--arg is only valid for scripts".into());
        }
        let path = prompt_file.ok_or("headless/pane jobs require --prompt-file")?;
        let prompt = read_bounded(Path::new(&path), MAX_PROMPT_BYTES)?;
        if let Some(provider) = provider {
            JobTarget::Headless {
                provider,
                prompt,
                prompt_files: Vec::new(),
                model: None,
                effort: None,
                skills: Vec::new(),
            }
        } else {
            JobTarget::Pane {
                pane_id: pane.unwrap(),
                prompt,
            }
        }
    };
    Ok(CronJobDefinition {
        name,
        schedule,
        utc_offset,
        target,
        workdir,
        timeout_seconds,
        enabled: None,
        catch_up: CatchUp::Skip,
        repeat_times,
        deliver: vec![Delivery::Local { notify: true }],
    })
}

fn print_help() {
    eprintln!("mercury cron add --file JOB.toml | --name NAME --schedule EXPR (--script PROGRAM [--arg ARG]... | --provider claude|codex --prompt-file PATH | --pane PANE --prompt-file PATH)");
    eprintln!("mercury cron list | edit JOB_ID --file JOB.toml | pause JOB_ID | resume JOB_ID | run JOB_ID | rm JOB_ID | runs JOB_ID | log RUN_ID | doctor");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_script_flags_build_definition_without_shell() {
        let args = [
            "--name",
            "backup",
            "--schedule",
            "every 2h",
            "--script",
            "/bin/echo",
            "--arg",
            "hello",
        ];
        let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
        let definition = parse_add(&args).unwrap();
        assert!(matches!(definition.target, JobTarget::Script { .. }));
        assert_eq!(definition.schedule, "every 2h");
    }

    #[test]
    fn simple_flags_reject_ambiguous_targets_and_inline_prompt() {
        let args = [
            "--name",
            "x",
            "--schedule",
            "30m",
            "--script",
            "echo",
            "--provider",
            "codex",
        ];
        let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
        assert!(parse_add(&args).is_err());
        let args = [
            "--name",
            "x",
            "--schedule",
            "30m",
            "--provider",
            "codex",
            "--prompt",
            "secret",
        ];
        let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
        assert!(parse_add(&args).is_err());
    }

    #[test]
    fn job_toml_parses_tagged_target_and_optional_fields() {
        let definition: CronJobDefinition = toml::from_str(
            "name = 'nightly'\nschedule = '0 9 * * *'\nutc_offset = '-03:00'\n[target]\nkind = 'headless'\nprovider = 'codex'\nprompt = 'summarize changes'\n"
        ).unwrap();
        assert_eq!(definition.name, "nightly");
        assert!(matches!(definition.target, JobTarget::Headless { .. }));
        assert_eq!(definition.timeout_seconds, None);
    }
}
