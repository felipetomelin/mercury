//! Integration tests for thin client mode.

#![cfg(unix)]

pub mod support;
#[path = "support/terminal_screen.rs"]
mod terminal_screen;

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mercury_learn::policy::{Policy, Proposal, TargetKind, TargetPolicy};
use mercury_learn::storage::{hash, Storage};
use mercury_learn::Changes;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde_json::Value;
use support::{
    cleanup_test_base, client_shell_handshake, read_server_message, register_runtime_dir,
    register_spawned_mercury_pid, unregister_spawned_mercury_pid, wait_for_client_shell_bootstrap,
    wait_for_message_variant, wait_for_message_variants, wait_for_socket, wait_until,
    CURRENT_ENDPOINT_PROTOCOL_GENERATION as CURRENT_PROTOCOL, SERVER_MESSAGE_PANE_SURFACE,
    SERVER_MESSAGE_PANE_SURFACE_PATCH, SERVER_MESSAGE_SEMANTIC_NOTIFICATION,
    SERVER_MESSAGE_SERVER_SHUTDOWN,
};

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    PathBuf::from(format!(
        "/tmp/mercury-client-test-{}-{nanos}",
        std::process::id()
    ))
}

struct SpawnedMercury {
    _master: Option<Box<dyn MasterPty + Send>>,
    child: Box<dyn Child + Send + Sync>,
}

impl SpawnedMercury {
    fn close_master(&mut self) {
        drop(self._master.take());
    }
}

impl Drop for SpawnedMercury {
    fn drop(&mut self) {
        let pid = self.child.process_id();
        let _ = self.child.kill();
        self.close_master();

        if let Some(pid) = pid {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                let mut status = 0;
                let result =
                    unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) };
                if result == pid as libc::pid_t || result == -1 {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }

            unregister_spawned_mercury_pid(Some(pid));
        }
    }
}

fn cleanup_spawned_mercury(spawned: SpawnedMercury, base: PathBuf) {
    drop(spawned);
    cleanup_test_base(&base);
}

fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn spawn_client_process(
    config_home: &PathBuf,
    runtime_dir: &PathBuf,
    api_socket_path: &PathBuf,
) -> SpawnedMercury {
    spawn_client_process_with_args(config_home, runtime_dir, api_socket_path, &["client"])
}

fn spawn_client_shell_process(
    config_home: &PathBuf,
    runtime_dir: &PathBuf,
    api_socket_path: &PathBuf,
) -> SpawnedMercury {
    spawn_client_process_with_args(config_home, runtime_dir, api_socket_path, &["client"])
}

fn spawn_client_process_with_args(
    config_home: &PathBuf,
    runtime_dir: &PathBuf,
    api_socket_path: &PathBuf,
    args: &[&str],
) -> SpawnedMercury {
    spawn_client_process_with_args_and_env(config_home, runtime_dir, api_socket_path, args, &[])
}

fn spawn_client_process_with_args_and_env(
    config_home: &PathBuf,
    runtime_dir: &PathBuf,
    api_socket_path: &PathBuf,
    args: &[&str],
    extra_env: &[(&str, &str)],
) -> SpawnedMercury {
    register_runtime_dir(runtime_dir);
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();

    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_mercury"));
    cmd.args(args);
    cmd.env("MERCURY_DISABLE_SOUND", "1");
    cmd.env("XDG_STATE_HOME", runtime_dir.join("state"));
    cmd.env("XDG_CONFIG_HOME", config_home);
    cmd.env("CLAUDE_CONFIG_DIR", config_home.join("fixture-claude"));
    cmd.env("CODEX_HOME", config_home.join("fixture-codex"));
    cmd.env("XDG_RUNTIME_DIR", runtime_dir);
    cmd.env("XDG_DATA_HOME", runtime_dir.join("data"));
    cmd.env("MERCURY_SOCKET_PATH", api_socket_path);
    cmd.env_remove("MERCURY_CLIENT_SOCKET_PATH");
    cmd.env("SHELL", "/bin/sh");
    cmd.env_remove("MERCURY_ENV");
    for (key, value) in extra_env {
        cmd.env(key, value);
    }

    let child = pair.slave.spawn_command(cmd).unwrap();
    register_spawned_mercury_pid(child.process_id());
    drop(pair.slave);

    SpawnedMercury {
        _master: Some(pair.master),
        child,
    }
}

fn spawn_server(
    config_home: &PathBuf,
    runtime_dir: &PathBuf,
    api_socket_path: &PathBuf,
    client_socket_path: &PathBuf,
) -> SpawnedMercury {
    spawn_server_with_config(
        config_home,
        runtime_dir,
        api_socket_path,
        client_socket_path,
        "onboarding = false\n",
    )
}

fn spawn_server_with_config(
    config_home: &PathBuf,
    runtime_dir: &PathBuf,
    api_socket_path: &PathBuf,
    _client_socket_path: &PathBuf,
    config: &str,
) -> SpawnedMercury {
    spawn_server_with_config_and_env(
        config_home,
        runtime_dir,
        api_socket_path,
        _client_socket_path,
        config,
        &[],
    )
}

fn spawn_server_with_config_and_env(
    config_home: &PathBuf,
    runtime_dir: &PathBuf,
    api_socket_path: &PathBuf,
    _client_socket_path: &PathBuf,
    config: &str,
    extra_env: &[(&str, &str)],
) -> SpawnedMercury {
    fs::create_dir_all(config_home.join(app_dir_name())).unwrap();
    fs::create_dir_all(runtime_dir).unwrap();
    register_runtime_dir(runtime_dir);
    fs::write(config_home.join(app_dir_name()).join("config.toml"), config).unwrap();

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();

    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_mercury"));
    cmd.arg("server");
    cmd.env("XDG_CONFIG_HOME", config_home);
    cmd.env("CLAUDE_CONFIG_DIR", config_home.join("fixture-claude"));
    cmd.env("CODEX_HOME", config_home.join("fixture-codex"));
    cmd.env("XDG_RUNTIME_DIR", runtime_dir);
    cmd.env("XDG_DATA_HOME", runtime_dir.join("data"));
    cmd.env("MERCURY_SOCKET_PATH", api_socket_path);
    cmd.env_remove("MERCURY_CLIENT_SOCKET_PATH");
    cmd.env("SHELL", "/bin/sh");
    cmd.env_remove("MERCURY_ENV");
    for (key, value) in extra_env {
        cmd.env(key, value);
    }

    let child = pair.slave.spawn_command(cmd).unwrap();
    register_spawned_mercury_pid(child.process_id());
    drop(pair.slave);

    SpawnedMercury {
        _master: Some(pair.master),
        child,
    }
}

fn ping_socket(socket_path: &PathBuf) -> String {
    let mut stream = UnixStream::connect(socket_path).expect("should connect to API socket");

    let request = r#"{"id":"1","method":"ping","params":{}}"#;
    writeln!(stream, "{}", request).unwrap();

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).unwrap();
    response.trim().to_string()
}

fn send_json_request(socket_path: &PathBuf, request: &str) -> Value {
    let mut stream = UnixStream::connect(socket_path).expect("should connect to API socket");
    writeln!(stream, "{}", request).unwrap();

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).unwrap();
    serde_json::from_str(&response).expect("response should be valid JSON")
}

fn first_pane_id_in_workspace(socket_path: &PathBuf, workspace_id: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let request = format!(
            r#"{{"id":"pane_list","method":"pane.list","params":{{"workspace_id":"{workspace_id}"}}}}"#
        );
        let panes = send_json_request(socket_path, &request);
        if let Some(pane_id) = panes["result"]["panes"]
            .as_array()
            .and_then(|panes| panes.first())
            .and_then(|pane| pane["pane_id"].as_str())
        {
            return pane_id.to_string();
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("pane.list did not return a pane for workspace {workspace_id} before timeout");
}

fn app_dir_name() -> &'static str {
    if cfg!(debug_assertions) {
        "mercury-dev"
    } else {
        "mercury"
    }
}

#[test]
fn cron_created_in_client_wizard_is_persisted_and_listed() {
    let _lock = test_lock();
    let logical_base = unique_test_dir();
    let base = logical_base
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap()
        .join(logical_base.file_name().unwrap());
    fs::create_dir_all(&base).unwrap();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");
    let server = spawn_server(&config_home, &runtime_dir, &api_socket, &client_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));
    let client = spawn_client_shell_process(&config_home, &runtime_dir, &api_socket);
    let master = client._master.as_ref().unwrap();
    let output = spawn_pty_drain(master.try_clone_reader().unwrap());
    let screen = || {
        let bytes = output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .bytes
            .clone();
        terminal_screen::text(&bytes, 80, 24)
    };
    assert!(wait_until(
        Duration::from_secs(8),
        Duration::from_millis(25),
        || { screen().contains("crons") }
    ));
    let mut input = master.take_writer().unwrap();
    input.write_all(b"\x02C").unwrap();
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(25), || {
            screen().contains("Crons ")
        }),
        "{}",
        screen()
    );
    input.write_all(b"a").unwrap();
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(25), || {
            screen().contains("novo agendamento")
        }),
        "{}",
        screen()
    );
    input
        .write_all(b"fixture-job\t\x1b[C\x1b[C\x1b[C\x1b[C\t60\t\t\t\x1b[C\x1b[C\t/bin/echo")
        .unwrap();
    input.write_all(&[0x13]).unwrap();
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(25), || {
            screen().contains("revisar antes de salvar")
        }),
        "{}",
        screen()
    );
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(25), || {
            screen().contains("Próxima:")
        }),
        "{}",
        screen()
    );
    input.write_all(&[0x13]).unwrap();
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(25), || {
            let jobs = send_json_request(
                &api_socket,
                &serde_json::json!({"id":"wizard-list","method":"cron.list","params":{}})
                    .to_string(),
            );
            jobs["result"]["jobs"]
                .as_array()
                .is_some_and(|jobs| jobs.iter().any(|job| job["name"] == "fixture-job"))
        }),
        "wizard job was not saved; screen: {}",
        screen()
    );
    assert!(
        screen().contains("fixture-job"),
        "job absent from UI: {}",
        screen()
    );
    drop(input);
    drop(client);
    cleanup_spawned_mercury(server, base);
}

#[test]
fn learn_tab_inspects_and_undoes_real_ledger_item_over_client_pty() {
    let _lock = test_lock();
    let logical_base = unique_test_dir();
    let base = logical_base
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap()
        .join(logical_base.file_name().unwrap());
    fs::create_dir_all(&base).unwrap();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");
    let target = base.join("AGENTS.md");
    let before = b"private-before\n";
    fs::write(&target, before).unwrap();
    let policy = Policy::new(vec![TargetPolicy {
        id: "instructions".into(),
        path: target.clone(),
        kind: TargetKind::Instructions,
        max_bytes: 1024,
    }])
    .unwrap();
    let storage = Storage::open(&config_home.join(app_dir_name()).join("learn")).unwrap();
    let record = Changes::new(storage)
        .apply(
            &policy,
            true,
            Proposal {
                target_id: "instructions".into(),
                content: "private-after\n".into(),
                reason: "private fixture reason".into(),
                source: "test/session".into(),
            },
            &hash(before),
        )
        .unwrap();

    let server = spawn_server(&config_home, &runtime_dir, &api_socket, &client_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));
    let client = spawn_client_shell_process(&config_home, &runtime_dir, &api_socket);
    let master = client._master.as_ref().unwrap();
    let output = spawn_pty_drain(master.try_clone_reader().unwrap());
    let screen = || {
        let bytes = output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .bytes
            .clone();
        terminal_screen::text(&bytes, 80, 24)
    };
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(25), || {
            let text = screen();
            text.contains("history") && text.contains("learn")
        }),
        "client initial frame: {}",
        screen()
    );
    let mut input = master.take_writer().unwrap();
    input.write_all(b"\x02U").unwrap();
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(25), || {
            let text = screen();
            text.contains("Learn") && text.contains("instructions")
        }),
        "Learn list: {}",
        screen()
    );
    input.write_all(b"\r").unwrap();
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(25), || {
            let text = screen();
            text.contains("Backup privado: verificado") && text.contains("Diff: -1 +1")
        }),
        "Learn detail: {}",
        screen()
    );
    assert!(!screen().contains("private-before"));
    assert!(!screen().contains("private-after"));
    assert!(!screen().contains("private fixture reason"));
    assert!(
        !screen().contains("MLearn"),
        "stale pane text: {}",
        screen()
    );
    assert!(screen().contains("Conteúdo do diff: [redigido]"));
    assert!(screen().contains("aplicadas 1  conflitos 0  falhas 0  undo 0"));
    let detail_frame = screen();
    let hash_line = detail_frame
        .lines()
        .find(|line| line.contains("Hash anterior:"))
        .unwrap();
    assert!(hash_line.trim_end().ends_with('…'), "{hash_line}");
    if std::env::var_os("MERCURY_LEARN_TUI_DUMP").is_some() {
        eprintln!("Learn detail frame:\n{}", screen());
    }
    input.write_all(b"u").unwrap();
    assert!(
        wait_until(Duration::from_secs(5), Duration::from_millis(25), || {
            screen().contains("Confirmar undo")
        }),
        "Learn undo confirmation: {}",
        screen()
    );
    assert_eq!(fs::read(&target).unwrap(), b"private-after\n");
    if std::env::var_os("MERCURY_LEARN_TUI_DUMP").is_some() {
        eprintln!("Learn undo confirmation frame:\n{}", screen());
    }
    input.write_all(b"u").unwrap();
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(25), || {
            fs::read(&target).ok().as_deref() == Some(&before[..])
        }),
        "Learn undo did not restore fixture; screen: {}",
        screen()
    );
    let status = send_json_request(
        &api_socket,
        &serde_json::json!({"id":"learn-tui-status","method":"learn.show","params":{"change_id":record.id}}).to_string(),
    );
    assert_eq!(status["result"]["detail"]["change"]["status"], "undone");
    drop(input);
    drop(client);
    cleanup_spawned_mercury(server, base);
}

#[test]
fn headless_cron_usage_reaches_learn_metrics_tab_over_client_pty() {
    let _lock = test_lock();
    let logical_base = unique_test_dir();
    let base = logical_base
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap()
        .join(logical_base.file_name().unwrap());
    fs::create_dir_all(&base).unwrap();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");
    let target = base.join("AGENTS.md");
    fs::write(&target, b"unchanged fixture\n").unwrap();
    let fake_bin = base.join("bin");
    fs::create_dir(&fake_bin).unwrap();
    let fake_claude = fake_bin.join("claude");
    fs::write(
        &fake_claude,
        "#!/bin/sh\nprintf '%s\\n' '{\"result\":\"fixture-done\",\"usage\":{\"input_tokens\":120,\"output_tokens\":30},\"total_cost_usd\":0.000125}'\n",
    )
    .unwrap();
    fs::set_permissions(&fake_claude, fs::Permissions::from_mode(0o700)).unwrap();
    let server_path = format!("{}:/usr/bin:/bin", fake_bin.display());
    let isolated_data = base.join("data");
    let isolated_data = isolated_data.to_str().unwrap();
    let config = format!(
        "onboarding = false\n[learn]\nenabled = true\nweekly = false\n[[learn.target]]\nid = 'instructions'\npath = '{}'\nkind = 'instructions'\nmax_bytes = 1024\n",
        target.display()
    );
    let server = spawn_server_with_config_and_env(
        &config_home,
        &runtime_dir,
        &api_socket,
        &client_socket,
        &config,
        &[("PATH", &server_path), ("XDG_DATA_HOME", isolated_data)],
    );
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));

    let add = send_json_request(
        &api_socket,
        &serde_json::json!({
            "id": "add-observed",
            "method": "cron.add",
            "params": {"definition": {
                "name": "observed-fixture",
                "schedule": "every 2h",
                "utc_offset": null,
                "target": {"kind": "headless", "provider": "claude", "prompt": "fixture prompt",
                    "prompt_files": [], "model": null, "effort": null, "skills": []},
                "workdir": base,
                "timeout_seconds": 10,
                "enabled": true,
                "catch_up": "skip",
                "repeat_times": null,
                "deliver": []
            }}
        })
        .to_string(),
    );
    let job_id = add["result"]["job"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("cron.add failed: {add}"))
        .to_owned();
    let run = send_json_request(
        &api_socket,
        &serde_json::json!({"id":"run-observed","method":"cron.run",
            "params":{"job_id":job_id}})
        .to_string(),
    );
    assert!(
        run["result"]["run_id"].is_string(),
        "cron.run failed: {run}"
    );
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(25), || {
            let response = send_json_request(
                &api_socket,
                &serde_json::json!({"id":"metrics","method":"learn.status","params":{}})
                    .to_string(),
            );
            response["result"]["status"]["recent_metrics"][0]["input_tokens"] == 120
                && response["result"]["status"]["recent_metrics"][0]["output_tokens"] == 30
        }),
        "headless usage did not reach Learn"
    );
    let runs = send_json_request(
        &api_socket,
        &serde_json::json!({"id":"observed-runs","method":"cron.runs",
            "params":{"job_id":job_id,"limit":1}})
        .to_string(),
    );
    let output_path = runs["result"]["runs"][0]["output_path"]
        .as_str()
        .unwrap_or_else(|| panic!("headless run has no output path: {runs}"));
    assert!(PathBuf::from(output_path).starts_with(base.join("data")));
    assert_eq!(fs::read(&target).unwrap(), b"unchanged fixture\n");

    let client = spawn_client_shell_process(&config_home, &runtime_dir, &api_socket);
    let master = client._master.as_ref().unwrap();
    let output = spawn_pty_drain(master.try_clone_reader().unwrap());
    let screen = || {
        let bytes = output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .bytes
            .clone();
        terminal_screen::text(&bytes, 80, 24)
    };
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(25), || {
            screen().contains("learn")
        }),
        "client initial frame: {}",
        screen()
    );
    let mut input = master.take_writer().unwrap();
    input.write_all(b"\x02U").unwrap();
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(25), || {
            screen().contains("Learn ·")
        }),
        "Learn view absent: {}",
        screen()
    );
    input.write_all(b"m").unwrap();
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(25), || {
            let frame = screen();
            frame.contains("dados insuficientes")
                && frame.contains("120→30 tokens")
                && frame.contains("$0.000125")
                && frame.contains("ok")
        }),
        "observed metric absent from Learn tab: {}",
        screen()
    );
    assert!(!screen().contains("fixture prompt"));
    drop(input);
    drop(client);
    cleanup_spawned_mercury(server, base);
}

#[test]
fn pane_cron_prompts_existing_fixture_agent_and_records_reply() {
    let _lock = test_lock();
    let logical_base = unique_test_dir();
    let base = logical_base
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap()
        .join(logical_base.file_name().unwrap());
    fs::create_dir_all(&base).unwrap();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");
    let fake_bin = base.join("bin");
    fs::create_dir(&fake_bin).unwrap();
    let receipt = base.join("received-prompt");
    let fake_codex = fake_bin.join("codex");
    fs::write(
        &fake_codex,
        format!(
            "#!/bin/sh\nif [ \"$1\" = app-server ]; then exit 1; fi\n\"{}\" pane report-agent \"$MERCURY_PANE_ID\" --source fake-cron --agent codex --state idle >/dev/null 2>&1\nIFS= read -r task || exit 0\nprintf '%s\\n' \"$task\" >> \"{}\"\n\"{}\" pane report-agent \"$MERCURY_PANE_ID\" --source fake-cron --agent codex --state working >/dev/null 2>&1\n/bin/sleep 1\nprintf 'CRON_PANE_OK\\n'\n\"{}\" pane report-agent \"$MERCURY_PANE_ID\" --source fake-cron --agent codex --state idle >/dev/null 2>&1\nread pause\n",
            env!("CARGO_BIN_EXE_mercury"),
            receipt.display(),
            env!("CARGO_BIN_EXE_mercury"),
            env!("CARGO_BIN_EXE_mercury")
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_codex, fs::Permissions::from_mode(0o700)).unwrap();
    let server_path = format!("{}:/usr/bin:/bin", fake_bin.display());
    let isolated_data = base.join("data");
    let isolated_data = isolated_data.to_str().unwrap();
    let server = spawn_server_with_config_and_env(
        &config_home,
        &runtime_dir,
        &api_socket,
        &client_socket,
        "onboarding = false\n[terminal]\ndefault_shell = '/bin/sh'\nshell_mode = 'non_login'\n",
        &[("PATH", &server_path), ("XDG_DATA_HOME", isolated_data)],
    );
    wait_for_socket(&api_socket, Duration::from_secs(10));
    let workspace = send_json_request(
        &api_socket,
        &serde_json::json!({"id":"workspace","method":"workspace.create",
            "params":{"cwd":base,"focus":true}})
        .to_string(),
    );
    let pane_id = workspace["result"]["root_pane"]["pane_id"]
        .as_str()
        .unwrap_or_else(|| panic!("workspace.create failed: {workspace}"))
        .to_owned();
    let mut last_start = Value::Null;
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(50), || {
            last_start = send_json_request(
                &api_socket,
                &serde_json::json!({"id":"start-agent","method":"agent.start",
                    "params":{"name":"cron-fixture","kind":"codex","pane_id":pane_id,
                        "timeout_ms":8000}})
                .to_string(),
            );
            if last_start["result"]["type"] == "agent_started" {
                return true;
            }
            assert_eq!(
                last_start["error"]["code"], "agent_pane_busy",
                "{last_start}"
            );
            false
        }),
        "fixture agent could not start: {last_start}"
    );
    let mut last_agent = Value::Null;
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(50), || {
            last_agent = send_json_request(
                &api_socket,
                &serde_json::json!({"id":"agent","method":"agent.get",
                    "params":{"target":pane_id}})
                .to_string(),
            );
            let agent = &last_agent["result"]["agent"];
            agent["name"] == "cron-fixture"
                && agent["agent"] == "codex"
                && agent["interactive_ready"] == true
                && agent["launch_pending"] != true
        }),
        "fixture agent did not become interactive: {last_agent}"
    );

    let prompt = "cron-fixture-private-task";
    let add = send_json_request(
        &api_socket,
        &serde_json::json!({"id":"add-pane-cron","method":"cron.add",
        "params":{"definition":{
            "name":"pane-fixture","schedule":"every 2h","utc_offset":null,
            "target":{"kind":"pane","pane_id":pane_id,"prompt":prompt},
            "workdir":null,"timeout_seconds":10,"enabled":true,
            "catch_up":"skip","repeat_times":null,"deliver":[]
        }}})
        .to_string(),
    );
    let job_id = add["result"]["job"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("cron.add failed: {add}"))
        .to_owned();
    let run = send_json_request(
        &api_socket,
        &serde_json::json!({"id":"run-pane-cron","method":"cron.run",
            "params":{"job_id":job_id}})
        .to_string(),
    );
    let run_id = run["result"]["run_id"]
        .as_str()
        .unwrap_or_else(|| panic!("cron.run failed: {run}"))
        .to_owned();
    if !wait_until(Duration::from_secs(10), Duration::from_millis(50), || {
        fs::read_to_string(&receipt)
            .ok()
            .is_some_and(|text| text.lines().any(|line| line == prompt))
    }) {
        let runs = send_json_request(
            &api_socket,
            &serde_json::json!({"id":"failed-pane-runs","method":"cron.runs",
                "params":{"job_id":job_id,"limit":1}})
            .to_string(),
        );
        let agent = send_json_request(
            &api_socket,
            &serde_json::json!({"id":"failed-pane-agent","method":"agent.get",
                "params":{"target":pane_id}})
            .to_string(),
        );
        let pane = send_json_request(
            &api_socket,
            &serde_json::json!({"id":"failed-pane-read","method":"pane.read",
                "params":{"pane_id":pane_id,"source":"recent","lines":20}})
            .to_string(),
        );
        panic!("fixture agent did not receive pane Cron prompt: runs={runs}, agent={agent}, pane={pane}");
    }
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(50), || {
            let runs = send_json_request(
                &api_socket,
                &serde_json::json!({"id":"pane-runs","method":"cron.runs",
                    "params":{"job_id":job_id,"limit":1}})
                .to_string(),
            );
            runs["result"]["runs"][0]["id"] == run_id
                && runs["result"]["runs"][0]["status"] == "success"
        }),
        "pane Cron did not finish successfully"
    );
    let runs = send_json_request(
        &api_socket,
        &serde_json::json!({"id":"finished-pane-runs","method":"cron.runs",
            "params":{"job_id":job_id,"limit":1}})
        .to_string(),
    );
    let output_path = runs["result"]["runs"][0]["output_path"]
        .as_str()
        .unwrap_or_else(|| panic!("pane run has no output path: {runs}"));
    assert!(PathBuf::from(output_path).starts_with(base.join("data")));
    let log = send_json_request(
        &api_socket,
        &serde_json::json!({"id":"pane-log","method":"cron.log",
            "params":{"run_id":run_id}})
        .to_string(),
    );
    assert!(log.to_string().contains("CRON_PANE_OK"), "cron.log: {log}");
    assert_eq!(fs::read_to_string(&receipt).unwrap(), format!("{prompt}\n"));
    cleanup_spawned_mercury(server, base);
}

#[test]
fn claude_statusline_updates_pane_context_and_usage_warning_over_client_pty() {
    let _lock = test_lock();
    let base = unique_test_dir();
    fs::create_dir_all(&base).unwrap();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");
    let claude_home = base.join("claude");
    let session_id = "11111111-2222-3333-4444-555555555599";
    let transcript = claude_home
        .join("projects")
        .join(mercury_usage::locate::claude_project_slug(&base))
        .join(format!("{session_id}.jsonl"));
    fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    let timestamp = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    fs::write(
        &transcript,
        format!(
            "{}\n",
            serde_json::json!({"type":"assistant","sessionId":session_id,
                "requestId":"req-usage","timestamp":timestamp,
                "message":{"id":"msg-usage","model":"claude-sonnet-5",
                    "usage":{"input_tokens":10,"cache_creation_input_tokens":0,
                        "cache_read_input_tokens":0,"output_tokens":500}}})
        ),
    )
    .unwrap();
    let fake_bin = base.join("bin");
    fs::create_dir(&fake_bin).unwrap();
    let fake_claude = fake_bin.join("claude");
    fs::write(
        &fake_claude,
        format!(
            "#!/bin/sh\n\"{}\" pane report-agent \"$MERCURY_PANE_ID\" --source mercury:claude --agent claude --state idle >/dev/null 2>&1\n\"{}\" pane report-agent-session \"$MERCURY_PANE_ID\" --source mercury:claude --agent claude --agent-session-id {} --agent-session-path \"{}\" >/dev/null 2>&1\nread pause\n",
            env!("CARGO_BIN_EXE_mercury"),
            env!("CARGO_BIN_EXE_mercury"),
            session_id,
            transcript.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_claude, fs::Permissions::from_mode(0o700)).unwrap();
    let server_path = format!("{}:/usr/bin:/bin", fake_bin.display());
    let claude_home_str = claude_home.to_str().unwrap();
    let server = spawn_server_with_config_and_env(
        &config_home,
        &runtime_dir,
        &api_socket,
        &client_socket,
        "onboarding = false\n[terminal]\ndefault_shell = '/bin/sh'\nshell_mode = 'non_login'\n[usage]\ncompact_at = 80\n[[usage.budget]]\nname = 'claude'\nscope = { provider = 'claude' }\nwindow = { kind = 'rolling', hours = 1 }\nlimit = { tokens = 1000 }\nwarn_at = [50]\n",
        &[("PATH", &server_path), ("CLAUDE_CONFIG_DIR", claude_home_str)],
    );
    wait_for_socket(&api_socket, Duration::from_secs(10));
    let workspace = send_json_request(
        &api_socket,
        &serde_json::json!({"id":"workspace","method":"workspace.create",
            "params":{"cwd":base,"focus":true}})
        .to_string(),
    );
    let pane_id = workspace["result"]["root_pane"]["pane_id"]
        .as_str()
        .unwrap_or_else(|| panic!("workspace.create failed: {workspace}"))
        .to_owned();
    let mut last_start = Value::Null;
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(50), || {
            last_start = send_json_request(
                &api_socket,
                &serde_json::json!({"id":"start-agent","method":"agent.start",
                    "params":{"name":"usage-fixture","kind":"claude","pane_id":pane_id,
                        "timeout_ms":8000}})
                .to_string(),
            );
            if last_start["result"]["type"] == "agent_started" {
                return true;
            }
            assert_eq!(
                last_start["error"]["code"], "agent_pane_busy",
                "{last_start}"
            );
            false
        }),
        "fixture Claude could not start: {last_start}"
    );
    let mut last_agent = Value::Null;
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(50), || {
            last_agent = send_json_request(
                &api_socket,
                &serde_json::json!({"id":"agent","method":"agent.get",
                    "params":{"target":pane_id}})
                .to_string(),
            );
            last_agent["result"]["agent"]["interactive_ready"] == true
                && last_agent["result"]["agent"]["launch_pending"] != true
        }),
        "fixture Claude did not become interactive: {last_agent}"
    );
    let mut last_panes = Value::Null;
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(50), || {
            last_panes = send_json_request(
                &api_socket,
                &serde_json::json!({"id":"panes","method":"pane.list","params":{}}).to_string(),
            );
            last_panes["result"]["panes"]
                .as_array()
                .is_some_and(|panes| {
                    panes.iter().any(|pane| {
                        pane["pane_id"] == pane_id && pane["agent_session"]["value"] == session_id
                    })
                })
        }),
        "fixture Claude session was not attached: {last_panes}"
    );

    let mut statusline = std::process::Command::new(env!("CARGO_BIN_EXE_mercury"))
        .arg("statusline")
        .env("MERCURY_PANE_ID", &pane_id)
        .env("MERCURY_SOCKET_PATH", &api_socket)
        .env("XDG_DATA_HOME", base.join("data"))
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let payload = serde_json::json!({
        "session_id":session_id,"cwd":base,
        "model":{"id":"claude-sonnet-5","display_name":"Sonnet 5"},
        "cost":{"total_cost_usd":0.25},
        "context_window":{"context_window_size":100000,"used_percentage":85.0},
        "rate_limits":{"five_hour":{"used_percentage":61.0}}
    });
    statusline
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    assert!(statusline.wait().unwrap().success());

    wait_for_socket(&client_socket, Duration::from_secs(10));
    let client = spawn_client_shell_process(&config_home, &runtime_dir, &api_socket);
    let master = client._master.as_ref().unwrap();
    let output = spawn_pty_drain(master.try_clone_reader().unwrap());
    let screen = || {
        let bytes = output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .bytes
            .clone();
        terminal_screen::text(&bytes, 80, 24)
    };
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(25), || {
            screen().contains("usage")
        }),
        "client initial frame: {}",
        screen()
    );
    let mut input = master.take_writer().unwrap();
    input.write_all(b"\x02u").unwrap();
    assert!(
        wait_until(Duration::from_secs(12), Duration::from_millis(50), || {
            let frame = screen();
            frame
                .lines()
                .any(|line| line.contains("five_hour") && line.contains("61%"))
        }),
        "account source absent from Usage frame: {}",
        screen()
    );
    input.write_all(b"\x1b[6~").unwrap();
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(50), || {
            let frame = screen();
            frame.lines().any(|line| {
                line.contains("usage-fixture") && line.contains("ctx") && line.contains("85%")
            }) && frame
                .lines()
                .any(|line| line.contains("510 / 1k") && line.contains('⚠'))
        }),
        "scrollable session/budget absent: {}",
        screen()
    );
    input.write_all(b"\x1b[H").unwrap();
    let mut no_limits_statusline = std::process::Command::new(env!("CARGO_BIN_EXE_mercury"))
        .arg("statusline")
        .env("MERCURY_PANE_ID", &pane_id)
        .env("MERCURY_SOCKET_PATH", &api_socket)
        .env("XDG_DATA_HOME", base.join("data"))
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    no_limits_statusline
        .stdin
        .take()
        .unwrap()
        .write_all(
            serde_json::json!({"session_id":session_id})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    assert!(no_limits_statusline.wait().unwrap().success());
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(50), || {
            let frame = screen();
            frame.contains("Aguardando limites da conta") && !frame.contains("five_hour")
        }),
        "missing limits did not replace the session percentage: {}",
        screen()
    );
    drop(input);
    drop(client);
    cleanup_spawned_mercury(server, base);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn client_connects_and_receives_pane_surface() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");

    let spawned = spawn_server(&config_home, &runtime_dir, &api_socket, &client_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));

    let mut stream = UnixStream::connect(&client_socket).expect("should connect to client socket");
    let (version, error) = client_shell_handshake(&mut stream, CURRENT_PROTOCOL, 54, 23)
        .expect("handshake should succeed");
    assert_eq!(version, CURRENT_PROTOCOL);
    assert!(error.is_none(), "{error:?}");
    wait_for_client_shell_bootstrap(&mut stream, Duration::from_secs(10))
        .expect("should receive the shell snapshot and pane surface");

    cleanup_spawned_mercury(spawned, base);
}

#[test]
fn direct_attach_initial_mouse_capture_follows_config() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");
    let config_path = config_home.join(app_dir_name()).join("config.toml");

    let spawned_server = spawn_server_with_config(
        &config_home,
        &runtime_dir,
        &api_socket,
        &client_socket,
        "onboarding = false\n[ui]\nmouse_capture = false\n",
    );
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));
    let created = send_json_request(
        &api_socket,
        &serde_json::json!({
            "id": "create-workspace-for-direct-attach",
            "method": "workspace.create",
            "params": {"cwd": base},
        })
        .to_string(),
    );
    let terminal_id = created["result"]["root_pane"]["terminal_id"]
        .as_str()
        .expect("created terminal id")
        .to_string();

    let mut attach = spawn_client_process_with_args(
        &config_home,
        &runtime_dir,
        &api_socket,
        &["terminal", "attach", &terminal_id],
    );
    let output = spawn_pty_drain(
        attach
            ._master
            .as_ref()
            .expect("direct attach master")
            .try_clone_reader()
            .expect("clone direct attach PTY reader"),
    );
    assert!(
        wait_until(Duration::from_secs(5), Duration::from_millis(20), || {
            read_output(&output).contains("\x1b[?7l")
        }),
        "direct attach terminal setup should complete; output: {:?}",
        read_output(&output)
    );
    assert!(
        !read_output(&output).contains("\x1b[?1000h"),
        "mouse capture disabled must not enable host mouse reporting; output: {:?}",
        read_output(&output)
    );
    assert!(
        read_output(&output).contains("\x1b[?2004h"),
        "direct attach must enable host bracketed paste; output: {:?}",
        read_output(&output)
    );
    assert!(
        !read_output(&output).contains("\x1b[?u"),
        "direct attach must not query rendered-client keyboard state; output: {:?}",
        read_output(&output)
    );

    let restore_watermark = output_len(&output);
    attach
        ._master
        .as_ref()
        .expect("direct attach master")
        .take_writer()
        .expect("direct attach PTY writer")
        .write_all(b"\x02q")
        .expect("detach direct attach client");
    let restore_output = drain_until_client_exits(&mut attach, &output, restore_watermark);
    assert!(
        restore_output.contains("\x1b[?2004l"),
        "direct attach must disable host bracketed paste on restore; output: {restore_output:?}"
    );
    drop(attach);

    fs::write(
        &config_path,
        "onboarding = false\n[ui]\nmouse_capture = true\n",
    )
    .unwrap();
    let attach = spawn_client_process_with_args(
        &config_home,
        &runtime_dir,
        &api_socket,
        &["terminal", "attach", &terminal_id],
    );
    let output = spawn_pty_drain(
        attach
            ._master
            .as_ref()
            .expect("direct attach master")
            .try_clone_reader()
            .expect("clone direct attach PTY reader"),
    );
    assert!(
        wait_until(Duration::from_secs(5), Duration::from_millis(20), || {
            read_output(&output).contains("\x1b[?7l")
        }),
        "direct attach terminal setup should complete; output: {:?}",
        read_output(&output)
    );
    assert!(
        read_output(&output).contains("\x1b[?1000h"),
        "mouse capture enabled must retain host mouse reporting; output: {:?}",
        read_output(&output)
    );

    drop(spawned_server);
    cleanup_spawned_mercury(attach, base);
}

#[test]
fn client_sees_headless_startup_config_diagnostic() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");

    let app_dir = if cfg!(debug_assertions) {
        "mercury-dev"
    } else {
        "mercury"
    };
    fs::create_dir_all(config_home.join(app_dir)).unwrap();
    fs::write(
        config_home.join(app_dir).join("config.toml"),
        "[keys\nprefix = \"ctrl+a\"\n",
    )
    .unwrap();
    fs::create_dir_all(&runtime_dir).unwrap();
    register_runtime_dir(&runtime_dir);

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();

    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_mercury"));
    cmd.arg("server");
    cmd.env("XDG_CONFIG_HOME", &config_home);
    cmd.env("XDG_RUNTIME_DIR", &runtime_dir);
    cmd.env("XDG_DATA_HOME", runtime_dir.join("data"));
    cmd.env("MERCURY_SOCKET_PATH", &api_socket);
    cmd.env_remove("MERCURY_CLIENT_SOCKET_PATH");
    cmd.env("SHELL", "/bin/sh");
    cmd.env_remove("MERCURY_ENV");

    let child = pair.slave.spawn_command(cmd).unwrap();
    register_spawned_mercury_pid(child.process_id());
    drop(pair.slave);

    let spawned = SpawnedMercury {
        _master: Some(pair.master),
        child,
    };
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));

    let client = spawn_client_shell_process(&config_home, &runtime_dir, &api_socket);
    let output = spawn_pty_drain(
        client
            ._master
            .as_ref()
            .expect("client shell master")
            .try_clone_reader()
            .expect("clone client shell reader"),
    );
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(20), || {
            let output = read_output(&output);
            output.contains("config.toml") && output.contains("mercury config check")
        }),
        "client shell should render startup config diagnostic; output: {:?}",
        read_output(&output)
    );

    drop(spawned);
    cleanup_spawned_mercury(client, base);
}

#[test]
fn server_unreachable_shows_clear_error() {
    // when server is unreachable, the client exits quickly
    // with an actionable connection-failed message.
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");

    fs::create_dir_all(config_home.join("mercury")).unwrap();
    fs::create_dir_all(&runtime_dir).unwrap();
    register_runtime_dir(&runtime_dir);
    fs::write(
        config_home.join("mercury/config.toml"),
        "onboarding = false\n",
    )
    .unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mercury"))
        .arg("client")
        .env("MERCURY_DISABLE_SOUND", "1")
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_RUNTIME_DIR", &runtime_dir)
        .env("XDG_STATE_HOME", runtime_dir.join("state"))
        .env("MERCURY_SOCKET_PATH", &api_socket)
        .env_remove("MERCURY_CLIENT_SOCKET_PATH")
        .env_remove("MERCURY_ENV")
        .output()
        .expect("client command should run");

    assert!(
        !output.status.success(),
        "client should fail when no server is running"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to connect to server"),
        "stderr should mention connection failure: {stderr}"
    );
    assert!(
        stderr.contains("Is mercury server running?"),
        "stderr should include actionable guidance: {stderr}"
    );
    assert!(
        stderr.contains("Socket path:"),
        "stderr should include attempted socket path: {stderr}"
    );

    cleanup_test_base(&base);
}

#[test]
fn server_crash_after_attach_causes_lost_connection_error() {
    // attach a real thin client connection, kill server unexpectedly,
    // assert clean non-zero client exit plus lost-connection signal.
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");

    let mut spawned = spawn_server(&config_home, &runtime_dir, &api_socket, &client_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));

    // Attach a real thin client (client subcommand) through PTY so handshake and
    // terminal setup paths are exercised.
    let mut thin_client = spawn_client_process(&config_home, &runtime_dir, &api_socket);

    // Prove attached before kill by waiting for recognizable rendered app content.
    let mut thin_reader = thin_client
        ._master
        .as_ref()
        .expect("thin client master")
        .try_clone_reader()
        .expect("clone client PTY reader");
    let (attached_before_kill, attach_output) = {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut buf = [0u8; 4096];
        let mut seen = false;
        let mut output = String::new();
        while Instant::now() < deadline {
            match thin_reader.read(&mut buf) {
                Ok(n) if n > 0 => {
                    let out = String::from_utf8_lossy(&buf[..n]);
                    output.push_str(&out);
                    if out.contains("\u{2500}")
                        || out.contains("workspace")
                        || out.contains("pane")
                        || out.contains("terminal")
                    {
                        seen = true;
                        break;
                    }
                    if output.to_lowercase().contains("mercury:") {
                        break;
                    }
                }
                Ok(_) => thread::sleep(Duration::from_millis(30)),
                Err(_) => thread::sleep(Duration::from_millis(30)),
            }
        }
        (seen, output)
    };
    assert!(
        attached_before_kill,
        "thin client must complete attach and receive frame before server crash; output: {attach_output:?}"
    );

    // Kill server unexpectedly.
    if let Some(pid) = spawned.child.process_id() {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
    }
    spawned.close_master();

    // Client should exit non-zero after connection loss.
    let mut crash_output = String::new();
    let exited = {
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut exited = false;
        while Instant::now() < deadline {
            if thin_client.child.try_wait().ok().flatten().is_some() {
                exited = true;
                break;
            }
            // Keep draining client output so the process can progress to exit.
            let mut buf = [0u8; 1024];
            if let Ok(n) = thin_reader.read(&mut buf) {
                if n > 0 {
                    crash_output.push_str(&String::from_utf8_lossy(&buf[..n]));
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        exited
    };
    assert!(exited, "thin client should exit after server SIGKILL");

    let status = thin_client.child.wait().expect("wait thin client status");
    assert!(
        !status.success(),
        "thin client should exit non-zero after lost server connection"
    );

    // Drain trailing output and require the explicit user-visible lost-connection message.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut buf = [0u8; 2048];
    while Instant::now() < deadline {
        match thin_reader.read(&mut buf) {
            Ok(n) if n > 0 => crash_output.push_str(&String::from_utf8_lossy(&buf[..n])),
            Ok(_) => break,
            Err(_) => break,
        }
        thread::sleep(Duration::from_millis(30));
    }

    let crash_output_lc = crash_output.to_lowercase();
    assert!(
        crash_output_lc.contains("lost connection to server"),
        "thin client must emit explicit lost-connection message after server crash; output: {crash_output:?}"
    );

    // Ensure server is gone.
    let _ = spawned.child.wait();

    cleanup_test_base(&base);
}

/// Any of the mouse-disable modes emitted by `clear_host_mouse_reporting` on
/// terminal restore. Their presence in the client's PTY output proves the
/// restore path (`TerminalGuard::Drop` → `restore_terminal_state`) ran.
const MOUSE_TEARDOWN_MARKERS: [&str; 2] = ["\u{1b}[?1003l", "\u{1b}[?1000l"];

fn output_has_mouse_teardown(output: &str) -> bool {
    MOUSE_TEARDOWN_MARKERS
        .iter()
        .all(|marker| output.contains(marker))
}

/// Shared buffer fed by a background PTY reader thread. Reading on a thread
/// keeps the blocking `Box<dyn Read>` (which has no timeout) off the test's
/// main thread, so a client that never exits fails the deadline instead of
/// hanging the whole test forever.
#[derive(Default)]
struct PtyOutput {
    bytes: Vec<u8>,
    // Keep legacy raw-string watermarks stable even across split UTF-8 reads.
    text: String,
}

type SharedOutput = std::sync::Arc<Mutex<PtyOutput>>;

fn spawn_pty_drain(mut reader: Box<dyn Read + Send>) -> SharedOutput {
    let output: SharedOutput = std::sync::Arc::new(Mutex::new(PtyOutput::default()));
    let thread_output = output.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let mut captured = thread_output.lock().unwrap_or_else(|p| p.into_inner());
                    captured.bytes.extend_from_slice(&buf[..n]);
                    captured.text.push_str(&String::from_utf8_lossy(&buf[..n]));
                }
                Err(_) => break,
            }
        }
    });
    output
}

#[test]
fn screen_capture_preserves_split_utf8_bytes() {
    let reader = std::io::Cursor::new(b"caf\xc3").chain(std::io::Cursor::new(b"\xa9"));
    let output = spawn_pty_drain(Box::new(reader));
    assert!(wait_until(
        Duration::from_secs(2),
        Duration::from_millis(10),
        || {
            let bytes = output.lock().unwrap().bytes.clone();
            bytes == "café".as_bytes() && terminal_screen::text(&bytes, 80, 24).contains("café")
        }
    ));
}

fn read_output(output: &SharedOutput) -> String {
    output
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .text
        .clone()
}

#[test]
fn sigwinch_refreshes_host_palette_without_resizing() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");
    let server = spawn_server_with_config(
        &config_home,
        &runtime_dir,
        &api_socket,
        &client_socket,
        "onboarding = false\n[theme]\nname = \"terminal\"\n",
    );
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));
    let client = spawn_client_shell_process(&config_home, &runtime_dir, &api_socket);
    let master = client._master.as_ref().expect("client PTY");
    let output = spawn_pty_drain(master.try_clone_reader().unwrap());
    let mut writer = master.take_writer().unwrap();
    let queries = "\x1b]10;?\x1b\\\x1b]11;?\x1b\\";
    assert!(
        wait_until(Duration::from_secs(5), Duration::from_millis(25), || {
            read_output(&output).contains(queries)
        }),
        "client should query the initial palette: {:?}",
        read_output(&output)
    );

    // Complete the startup query with a light palette. No appearance notification
    // is sent: this models a terminal whose colors are changed directly by OSC.
    let mut light =
        String::from("\x1b]10;rgb:0000/0000/0000\x1b\\\x1b]11;rgb:ffff/ffff/ffff\x1b\\");
    for index in 0..=u8::MAX {
        light.push_str(&format!("\x1b]4;{index};rgb:ffff/ffff/ffff\x1b\\"));
    }
    writer.write_all(light.as_bytes()).unwrap();
    writer.flush().unwrap();
    // Let startup settle and the resize watcher install its signal handler.
    thread::sleep(Duration::from_millis(250));

    for _ in 0..2 {
        let watermark = output_len(&output);
        assert_eq!(
            unsafe { libc::kill(client.child.process_id().unwrap() as i32, libc::SIGWINCH) },
            0
        );
        assert!(
            wait_until(Duration::from_secs(5), Duration::from_millis(25), || {
                let captured = read_output(&output);
                let refreshed = &captured[watermark..];
                refreshed.contains(queries) && refreshed.contains("\x1b]4;15;?\x1b\\")
            }),
            "SIGWINCH should query default colors and the ANSI palette without changing PTY size"
        );
        writer
            .write_all(b"\x1b]10;rgb:eeee/eeee/eeee\x1b\\\x1b]11;rgb:1111/2222/3333\x1b\\")
            .unwrap();
        writer.flush().unwrap();
    }

    drop(writer);
    drop(client);
    cleanup_spawned_mercury(server, base);
}

/// Current captured byte length, used as a watermark so a test can search only
/// the output emitted *after* a trigger. The teardown markers also appear in
/// normal attach-phase output, so matching the whole buffer is meaningless.
fn output_len(output: &SharedOutput) -> usize {
    output.lock().unwrap_or_else(|p| p.into_inner()).text.len()
}

fn sidebar_row_click(screen: &str, label: &str) -> Vec<u8> {
    let sidebar_width = screen
        .lines()
        .find_map(|line| {
            line.chars()
                .position(|character| character == '│')
                .filter(|column| *column > 0)
        })
        .expect("visible sidebar boundary");
    let row = screen
        .lines()
        .position(|line| {
            line.chars()
                .take(sidebar_width)
                .collect::<String>()
                .contains(label)
        })
        .unwrap_or_else(|| panic!("sidebar row {label:?} is not visible: {screen}"))
        + 1;
    format!("\x1b[<0;7;{row}M\x1b[<0;7;{row}m").into_bytes()
}

#[test]
fn sidebar_row_click_ignores_notice_borders() {
    let screen = "┌─────────────────────────┐\n│● Endpoint unavailable   │\n└─────────────────────────┘\n   · local-returned      │\n";
    assert_eq!(
        sidebar_row_click(screen, "local-returned"),
        b"\x1b[<0;7;4M\x1b[<0;7;4m"
    );
}

#[test]
fn sidebar_row_click_tracks_restored_workspace_count() {
    for restored in [false, true] {
        let screen = format!(
            " machines                │\n                         │\n ▾ Local                 │local-returned in pane output\n{}   · local-returned      └─────────────────\n",
            if restored { "   · restored            │\n" } else { "" }
        );
        let row = if restored { 5 } else { 4 };
        assert_eq!(
            sidebar_row_click(&screen, "local-returned"),
            format!("\x1b[<0;7;{row}M\x1b[<0;7;{row}m").into_bytes()
        );
    }
}

/// Spawns a server + real thin client under a PTY and waits until the client
/// has attached and rendered a frame. Returns the pieces plus a shared buffer
/// that keeps accumulating PTY output (including teardown) on a background
/// thread.
fn attach_thin_client(
    config_home: &PathBuf,
    runtime_dir: &PathBuf,
    api_socket: &PathBuf,
    client_socket: &PathBuf,
) -> (SpawnedMercury, SpawnedMercury, SharedOutput) {
    attach_thin_client_with_config(
        config_home,
        runtime_dir,
        api_socket,
        client_socket,
        "onboarding = false\n",
    )
}

fn attach_thin_client_with_config(
    config_home: &PathBuf,
    runtime_dir: &PathBuf,
    api_socket: &PathBuf,
    client_socket: &PathBuf,
    config: &str,
) -> (SpawnedMercury, SpawnedMercury, SharedOutput) {
    let spawned_server =
        spawn_server_with_config(config_home, runtime_dir, api_socket, client_socket, config);
    wait_for_socket(api_socket, Duration::from_secs(10));
    wait_for_socket(client_socket, Duration::from_secs(10));

    let thin_client = spawn_client_process(config_home, runtime_dir, api_socket);
    let reader = thin_client
        ._master
        .as_ref()
        .expect("thin client master")
        .try_clone_reader()
        .expect("clone client PTY reader");
    let output = spawn_pty_drain(reader);

    let deadline = Instant::now() + Duration::from_secs(8);
    let mut attached = false;
    while Instant::now() < deadline {
        let out = read_output(&output);
        if out.contains('\u{2500}')
            || out.contains("workspace")
            || out.contains("pane")
            || out.contains("terminal")
        {
            attached = true;
            break;
        }
        if out.to_lowercase().contains("mercury:") {
            break;
        }
        thread::sleep(Duration::from_millis(30));
    }
    assert!(
        attached,
        "thin client must attach and render a frame; output: {:?}",
        read_output(&output)
    );

    (spawned_server, thin_client, output)
}

#[test]
fn federated_launch_opens_local_directly_while_saved_ssh_is_unavailable() {
    use std::os::unix::fs::PermissionsExt;

    let _lock = test_lock();
    for select_remote in [false, true] {
        let base = unique_test_dir();
        let config_home = base.join("config");
        let runtime_dir = base.join("runtime");
        let api_socket = runtime_dir.join("mercury.sock");
        fs::create_dir_all(config_home.join(app_dir_name())).unwrap();
        fs::write(
            config_home.join(app_dir_name()).join("config.toml"),
            "onboarding = false\n",
        )
        .unwrap();
        let catalog_dir = runtime_dir
            .join("state")
            .join(app_dir_name())
            .join("client");
        fs::create_dir_all(&catalog_dir).unwrap();
        let profile = "0123456789abcdef0123456789abcdef";
        fs::write(catalog_dir.join("endpoints.json"), serde_json::json!({
            "version": 1, "selected_profile": select_remote.then_some(profile),
            "ssh": [{"id": profile, "label": "Unavailable remote", "target": "test-only", "session": "default", "enabled": true}],
        }).to_string()).unwrap();
        let bin = base.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("ssh"), "#!/bin/sh\nexit 255\n").unwrap();
        fs::set_permissions(bin.join("ssh"), fs::Permissions::from_mode(0o700)).unwrap();
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );

        // Exercise both auto-start and a subsequent attach to the healthy Local server.
        for args in [&[][..], &["client"][..]] {
            let client = spawn_client_process_with_args_and_env(
                &config_home,
                &runtime_dir,
                &api_socket,
                args,
                &[("PATH", &path)],
            );
            let output =
                spawn_pty_drain(client._master.as_ref().unwrap().try_clone_reader().unwrap());
            wait_for_socket(&api_socket, Duration::from_secs(10));
            assert!(wait_until(
                Duration::from_secs(10),
                Duration::from_millis(20),
                || { read_output(&output).contains("Local") }
            ));
            let mut input = client._master.as_ref().unwrap().take_writer().unwrap();
            // Input is gated until Local's active surface is ready, and that readiness can lag
            // the first rendered frame (the unavailable remote must not extend the wait). Retry
            // the write instead of assuming a single write lands, matching the recovered-Local
            // path below.
            assert!(wait_until(Duration::from_secs(10), Duration::from_millis(20), || {
                if read_output(&output).contains("LOCAL_DIRECT_READY") {
                    return true;
                }
                input
                    .write_all(b"printf 'LOCAL_%s\\n' DIRECT_READY\r")
                    .unwrap();
                false
            }), "Local must accept input without waiting for SSH (remote selected: {select_remote}): {}", read_output(&output));
            let text = read_output(&output);
            assert!(!text.contains("Local: connecting"), "{text}");
            assert!(!text.contains("Local: reconnecting"), "{text}");
            drop(input);
            drop(client);
        }
        let _ = send_json_request(
            &api_socket,
            r#"{"id":"stop","method":"server.stop","params":{}}"#,
        );
        cleanup_test_base(&base);
    }
}

#[test]
fn federated_client_starts_without_local_and_survives_its_restart() {
    use std::os::unix::fs::PermissionsExt;

    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");
    let remote_config = base.join("remote-config");
    let remote_runtime = base.join("remote-runtime");
    let remote_api = remote_runtime.join("mercury.sock");
    let remote_client = remote_runtime.join("mercury-client.sock");
    let mut remote_server =
        spawn_server(&remote_config, &remote_runtime, &remote_api, &remote_client);
    wait_for_socket(&remote_api, Duration::from_secs(10));
    wait_for_socket(&remote_client, Duration::from_secs(10));
    let created = send_json_request(
        &remote_api,
        &serde_json::json!({
            "id": "remote-workspace", "method": "workspace.create",
            "params": {"cwd": base, "focus": true, "label": "remote-ready"},
        })
        .to_string(),
    );
    let remote_pane = created["result"]["root_pane"]["pane_id"].as_str().unwrap();
    send_pane_shell_command(&remote_api, remote_pane, "printf 'REMOTE_INITIAL_FRAME\\n'");

    fs::create_dir_all(config_home.join(app_dir_name())).unwrap();
    fs::write(
        config_home.join(app_dir_name()).join("config.toml"),
        "onboarding = false\n",
    )
    .unwrap();
    let catalog_dir = runtime_dir
        .join("state")
        .join(app_dir_name())
        .join("client");
    fs::create_dir_all(&catalog_dir).unwrap();
    let profile = "0123456789abcdef0123456789abcdef";
    fs::write(catalog_dir.join("endpoints.json"), serde_json::json!({
        "version": 1, "selected_profile": profile,
        "ssh": [{"id": profile, "label": "Test remote", "target": "test-only", "session": "default", "enabled": true}],
    }).to_string()).unwrap();

    // The SSH executable is private to this client. Discovery and the stdio bridge run the real
    // binary against a second disposable local server, never the developer's saved hosts.
    let bin = base.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(base.join("home")).unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_mercury"), bin.join("mercury")).unwrap();
    let quote =
        |path: &std::path::Path| format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
    let ssh_commands = base.join("ssh-commands");
    let bridge_pid = base.join("bridge-pid");
    fs::write(bin.join("ssh"), format!(
        "#!/bin/sh\nexport HOME={} XDG_CONFIG_HOME={} XDG_RUNTIME_DIR={} MERCURY_SOCKET_PATH={}\nunset MERCURY_CLIENT_SOCKET_PATH MERCURY_SESSION\nfor arg do last=\"$arg\"; done\nprintf '%s\\n' \"$last\" >> {}\ncase \"$last\" in *remote-client-bridge*) printf '%s\\n' \"$$\" > {};; esac\nexec /bin/sh -c \"$last\"\n",
        quote(&base.join("home")), quote(&remote_config), quote(&remote_runtime), quote(&remote_api), quote(&ssh_commands), quote(&bridge_pid),
    )).unwrap();
    fs::set_permissions(bin.join("ssh"), fs::Permissions::from_mode(0o700)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut client = spawn_client_process_with_args_and_env(
        &config_home,
        &runtime_dir,
        &api_socket,
        &["client"],
        &[("PATH", &path)],
    );
    let output = spawn_pty_drain(client._master.as_ref().unwrap().try_clone_reader().unwrap());
    let screen_text = || {
        let bytes = output
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .bytes
            .clone();
        terminal_screen::text(&bytes, 80, 24)
    };
    assert!(
        wait_until(Duration::from_secs(12), Duration::from_millis(20), || {
            screen_text().contains("REMOTE_INITIAL_FRAME")
        }),
        "remote must be usable before Local exists: {}",
        read_output(&output)
    );
    assert!(
        fs::read_to_string(&ssh_commands)
            .unwrap()
            .contains("remote-client-bridge --idle-timeout-v1"),
        "saved machine discovery must opt into the advertised bridge idle timeout"
    );

    let mut input = client._master.as_ref().unwrap().take_writer().unwrap();
    send_pane_shell_command(&remote_api, remote_pane, "reconnect_survivor=ALIVE");
    for cycle in 1..=3 {
        let pid: libc::pid_t = fs::read_to_string(&bridge_pid)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
        let marker = format!("REMOTE_RECONNECTED_{cycle}");
        send_pane_shell_command(&remote_api, remote_pane, &format!("printf '{marker}\\n'"));
        assert!(
            wait_until(Duration::from_secs(15), Duration::from_millis(20), || {
                screen_text().contains(&marker)
            }),
            "remote reconnect {cycle} must restore the visible screen without switching machines"
        );
        assert!(
            wait_until(Duration::from_secs(8), Duration::from_millis(100), || {
                if screen_text().contains(&format!("REMOTE_ALIVE_INPUT_{cycle}")) {
                    return true;
                }
                write!(
                    input,
                    "printf 'REMOTE_%s_INPUT_{cycle}\\n' \"$reconnect_survivor\"\r"
                )
                .unwrap();
                false
            }),
            "remote reconnect {cycle} must restore visible input and preserve the shell"
        );
    }

    let mut local = spawn_server(&config_home, &runtime_dir, &api_socket, &client_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));
    let created = send_json_request(
        &api_socket,
        &serde_json::json!({
            "id": "local-workspace", "method": "workspace.create",
            "params": {"cwd": base, "focus": true, "label": "local-online"},
        })
        .to_string(),
    );
    assert_eq!(created["result"]["type"], "workspace_created");
    assert!(wait_until(
        Duration::from_secs(10),
        Duration::from_millis(20),
        || screen_text().contains("local-online")
    ));

    local.child.kill().unwrap();
    local.close_master();
    drop(local);
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(20), || {
            if screen_text().contains("REMOTE_SURVIVED") {
                return true;
            }
            input
                .write_all(b"printf 'REMOTE_%s\\n' SURVIVED\r")
                .unwrap();
            false
        }),
        "Local loss must not interrupt remote input or output: {}",
        screen_text()
    );
    assert!(client.child.try_wait().unwrap().is_none());

    let restarted = spawn_server(&config_home, &runtime_dir, &api_socket, &client_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));
    let created = send_json_request(
        &api_socket,
        &serde_json::json!({
            "id": "local-returned", "method": "workspace.create",
            "params": {"cwd": base, "focus": true, "label": "local-returned"},
        })
        .to_string(),
    );
    assert_eq!(created["result"]["type"], "workspace_created");
    assert!(
        wait_until(Duration::from_secs(12), Duration::from_millis(20), || {
            screen_text().contains("local-returned")
        }),
        "Local must reconnect with fresh metadata"
    );
    input
        .write_all(b"printf 'REMOTE_%s\\n' STILL_SELECTED\r")
        .unwrap();
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(20), || {
            screen_text().contains("REMOTE_STILL_SELECTED")
        }),
        "Local recovery must not steal selection: {}",
        screen_text()
    );
    let local_pane = created["result"]["root_pane"]["pane_id"].as_str().unwrap();
    send_pane_shell_command(
        &api_socket,
        local_pane,
        "printf 'LOCAL_WHILE_REMOTE_STALLED\\n'",
    );
    {
        struct ResumeBridge(libc::pid_t);
        impl Drop for ResumeBridge {
            fn drop(&mut self) {
                unsafe { libc::kill(self.0, libc::SIGCONT) };
            }
        }
        let bridge: libc::pid_t = fs::read_to_string(&bridge_pid)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(unsafe { libc::kill(bridge, libc::SIGSTOP) }, 0);
        let _resume_bridge = ResumeBridge(bridge);
        input
            .write_all(&sidebar_row_click(&screen_text(), "local-returned"))
            .unwrap();
        assert!(
            wait_until(Duration::from_secs(3), Duration::from_millis(20), || {
                screen_text().contains("LOCAL_WHILE_REMOTE_STALLED")
            }),
            "one Local selection must not wait for the remote bridge: {}",
            screen_text()
        );
        assert!(
            wait_until(Duration::from_secs(3), Duration::from_millis(100), || {
                if screen_text().contains("LOCAL_INPUT_WHILE_REMOTE_STALLED") {
                    return true;
                }
                input
                    .write_all(b"printf 'LOCAL_%s\\n' INPUT_WHILE_REMOTE_STALLED\r")
                    .unwrap();
                false
            }),
            "Local input must become usable while the remote bridge remains stopped"
        );
    }
    input
        .write_all(&sidebar_row_click(&screen_text(), "remote-ready"))
        .unwrap();
    assert!(wait_until(
        Duration::from_secs(10),
        Duration::from_millis(20),
        || screen_text().contains("REMOTE_STILL_SELECTED")
    ));

    let watermark = output_len(&output);
    remote_server.child.kill().unwrap();
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(20), || {
            read_output(&output)[watermark..].contains("reconnecting")
        }),
        "the selected remote must be marked disconnected"
    );
    let text = read_output(&output);
    assert!(
        text.rfind("\x1b[?1000h") > text.rfind("\x1b[?1000l"),
        "losing the selected remote must keep host mouse reporting enabled"
    );

    send_pane_shell_command(
        &api_socket,
        local_pane,
        "printf 'LOCAL_RECOVERED_SURFACE\\n'",
    );
    // Select the fresh workspace below Local's restored workspace.
    // A fast shutdown may leave no saved workspace, so locate the actual row.
    input
        .write_all(&sidebar_row_click(&screen_text(), "local-returned"))
        .unwrap();
    assert!(
        wait_until(Duration::from_secs(10), Duration::from_millis(20), || {
            screen_text().contains("LOCAL_RECOVERED_SURFACE")
        }),
        "recovered Local must be selectable: {}",
        screen_text()
    );
    // A coherent frame precedes the final host-effects fence; input stays gated until then.
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(100), || {
            if screen_text().contains("LOCAL_INPUT_RECOVERED") {
                return true;
            }
            input
                .write_all(b"printf 'LOCAL_%s\\n' INPUT_RECOVERED\r")
                .unwrap();
            false
        }),
        "recovered Local must accept input: {}",
        screen_text()
    );
    drop(input);
    drop(client);
    drop(restarted);
    drop(remote_server);
    cleanup_test_base(&base);
}

#[test]
fn client_shell_detaches_restores_and_freshly_reattaches_to_current_state() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");

    let mut server = spawn_server(&config_home, &runtime_dir, &api_socket, &client_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));

    let created = send_json_request(
        &api_socket,
        &serde_json::json!({
            "id": "client-shell-lifecycle-workspace",
            "method": "workspace.create",
            "params": {"cwd": base, "focus": true, "label": "shell-lifecycle"},
        })
        .to_string(),
    );
    assert_eq!(created["result"]["type"], "workspace_created", "{created}");
    let pane_id = created["result"]["root_pane"]["pane_id"]
        .as_str()
        .expect("root pane id")
        .to_string();
    send_pane_shell_command(&api_socket, &pane_id, "printf 'SHELL_LIFECYCLE_INITIAL\\n'");

    let mut client_a = spawn_client_shell_process(&config_home, &runtime_dir, &api_socket);
    let output_a = spawn_pty_drain(
        client_a
            ._master
            .as_ref()
            .expect("first client shell PTY")
            .try_clone_reader()
            .expect("clone first client shell reader"),
    );
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(20), || {
            let output = read_output(&output_a);
            output.contains("shell-lifecycle") && output.contains("SHELL_LIFECYCLE_INITIAL")
        }),
        "client shell should compose one coherent snapshot and pane surface; output: {:?}",
        read_output(&output_a)
    );

    let detach_watermark = output_len(&output_a);
    client_a
        ._master
        .as_ref()
        .expect("first client shell PTY")
        .take_writer()
        .expect("first client shell writer")
        .write_all(b"\x02q")
        .expect("detach first client shell");
    let detach_output = drain_until_client_exits(&mut client_a, &output_a, detach_watermark);
    assert!(
        output_has_mouse_teardown(&detach_output),
        "client shell should restore the host terminal after detach; output: {detach_output:?}"
    );
    assert!(
        ping_socket(&api_socket).contains("pong"),
        "server should remain alive after client shell detach"
    );
    drop(client_a);

    send_pane_shell_command(
        &api_socket,
        &pane_id,
        "printf 'SHELL_LIFECYCLE_DETACHED\\n'",
    );
    let mut client_b = spawn_client_shell_process(&config_home, &runtime_dir, &api_socket);
    let output_b = spawn_pty_drain(
        client_b
            ._master
            .as_ref()
            .expect("reattached client shell PTY")
            .try_clone_reader()
            .expect("clone reattached client shell reader"),
    );
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(20), || {
            let output = read_output(&output_b);
            output.contains("shell-lifecycle") && output.contains("SHELL_LIFECYCLE_DETACHED")
        }),
        "fresh client shell should receive current state and detached-period output; output: {:?}",
        read_output(&output_b)
    );

    let disconnect_watermark = output_len(&output_b);
    if let Some(pid) = server.child.process_id() {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
    }
    server.close_master();
    let disconnect_output =
        drain_until_client_exits(&mut client_b, &output_b, disconnect_watermark);
    assert!(
        output_has_mouse_teardown(&disconnect_output),
        "client shell should restore the host terminal after endpoint loss; output: {disconnect_output:?}"
    );
    assert!(
        disconnect_output
            .to_lowercase()
            .contains("lost connection to server"),
        "client shell should explain endpoint loss; output: {disconnect_output:?}"
    );

    drop(server);
    cleanup_spawned_mercury(client_b, base);
}

fn captured_window_titles(output: &SharedOutput) -> Vec<String> {
    read_output(output)
        .split("\x1b]0;")
        .skip(1)
        .filter_map(|suffix| {
            suffix
                .split_once('\x07')
                .map(|(title, _)| title.to_string())
        })
        .collect()
}

fn wait_for_window_title(output: &SharedOutput, expected_suffix: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(title) = captured_window_titles(output)
            .into_iter()
            .find(|title| title.ends_with(expected_suffix))
        {
            return title;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "outer window title ending in {expected_suffix:?} was not emitted; titles: {:?}; output: {:?}",
        captured_window_titles(output),
        read_output(output)
    );
}

fn wait_for_pane_terminal_title(socket_path: &PathBuf, pane_id: &str, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let request = serde_json::json!({
            "id": "window-title-pane-get",
            "method": "pane.get",
            "params": {"pane_id": pane_id},
        });
        let response = send_json_request(socket_path, &request.to_string());
        if response["result"]["pane"]["terminal_title"].as_str() == Some(expected) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("pane {pane_id} did not report terminal title {expected:?}");
}

fn send_pane_shell_command(socket_path: &PathBuf, pane_id: &str, command: &str) {
    let request = serde_json::json!({
        "id": "window-title-command",
        "method": "pane.send_input",
        "params": {
            "pane_id": pane_id,
            "text": command,
            "keys": ["Enter"],
        }
    });
    let response = send_json_request(socket_path, &request.to_string());
    assert_eq!(response["result"]["type"], "ok", "{response}");
}

#[test]
fn configured_window_title_tracks_all_tokens_and_focused_osc_only() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");
    let (server, client, output) = attach_thin_client_with_config(
        &config_home,
        &runtime_dir,
        &api_socket,
        &client_socket,
        "onboarding = false\n[ui]\nwindow_title = \"H={hostname}|W={workspace}|T={tab}|P={pane}|O={terminal_title}\"\n",
    );

    let created = send_json_request(
        &api_socket,
        &serde_json::json!({
            "id": "create-workspace",
            "method": "workspace.create",
            "params": {"cwd": base, "focus": true},
        })
        .to_string(),
    );
    assert_eq!(created["result"]["type"], "workspace_created", "{created}");
    let workspace_id = created["result"]["workspace"]["workspace_id"]
        .as_str()
        .expect("workspace id")
        .to_string();
    let pane_id = created["result"]["root_pane"]["pane_id"]
        .as_str()
        .expect("pane id")
        .to_string();
    let tab_id = created["result"]["tab"]["tab_id"]
        .as_str()
        .expect("tab id")
        .to_string();

    for request in [
        serde_json::json!({
            "id": "rename-workspace",
            "method": "workspace.rename",
            "params": {"workspace_id": workspace_id, "label": "space-a"},
        }),
        serde_json::json!({
            "id": "rename-tab",
            "method": "tab.rename",
            "params": {"tab_id": tab_id, "label": "tab-a"},
        }),
        serde_json::json!({
            "id": "rename-pane",
            "method": "pane.rename",
            "params": {"pane_id": pane_id, "label": "pane-a"},
        }),
    ] {
        let response = send_json_request(&api_socket, &request.to_string());
        assert!(response.get("result").is_some(), "{response}");
    }

    let renamed = wait_for_window_title(&output, "|W=space-a|T=tab-a|P=pane-a|O=");
    assert!(renamed.starts_with("H="));
    assert!(
        !renamed.starts_with("H=|"),
        "hostname token was empty: {renamed}"
    );

    send_pane_shell_command(&api_socket, &pane_id, r"printf '\033]0;building\007'");
    wait_for_window_title(&output, "|W=space-a|T=tab-a|P=pane-a|O=building");

    let second_tab = send_json_request(
        &api_socket,
        &serde_json::json!({
            "id": "second-tab",
            "method": "tab.create",
            "params": {"workspace_id": workspace_id, "focus": true},
        })
        .to_string(),
    );
    assert_eq!(second_tab["result"]["type"], "tab_created", "{second_tab}");
    let second_pane_id = second_tab["result"]["root_pane"]["pane_id"]
        .as_str()
        .expect("second pane id")
        .to_string();
    wait_for_window_title(&output, "|W=space-a|T=2|P=|O=");
    let titles_before_hidden_update = captured_window_titles(&output).len();
    send_pane_shell_command(&api_socket, &pane_id, r"printf '\033]0;hidden update\007'");
    // Intentionally consume the AppState title through a read-only request
    // before the queued source is handled.
    wait_for_pane_terminal_title(&api_socket, &pane_id, "hidden update");
    send_pane_shell_command(
        &api_socket,
        &second_pane_id,
        r"printf '\033]0;foreground marker\007'",
    );
    wait_for_window_title(&output, "|W=space-a|T=2|P=|O=foreground marker");
    assert!(
        captured_window_titles(&output)[titles_before_hidden_update..]
            .iter()
            .all(|title| !title.ends_with("|O=hidden update")),
        "a hidden pane title reached the outer terminal"
    );

    let focused = send_json_request(
        &api_socket,
        &serde_json::json!({
            "id": "focus-first-tab",
            "method": "tab.focus",
            "params": {"tab_id": tab_id},
        })
        .to_string(),
    );
    assert_eq!(focused["result"]["tab"]["focused"], true, "{focused}");
    wait_for_window_title(&output, "|W=space-a|T=tab-a|P=pane-a|O=hidden update");

    drop(server);
    cleanup_spawned_mercury(client, base);
}

/// Polls until the client exits, then returns only the output captured after
/// the `since` byte watermark. Panics if the client does not exit within the
/// deadline.
fn drain_until_client_exits(
    thin_client: &mut SpawnedMercury,
    output: &SharedOutput,
    since: usize,
) -> String {
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut exited = false;
    while Instant::now() < deadline {
        if thin_client.child.try_wait().ok().flatten().is_some() {
            exited = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    // Give the reader thread a beat to flush trailing teardown bytes.
    thread::sleep(Duration::from_millis(100));
    let full = read_output(output);
    assert!(exited, "thin client should exit; output: {full:?}");
    full.get(since..).unwrap_or_default().to_string()
}

/// Attaches a thin client, runs `trigger` to force an exit, and asserts the
/// client emits the mouse teardown after that point. The teardown markers also
/// appear in normal attach output, so only bytes emitted after the trigger
/// (past the watermark) count.
fn assert_client_restores_terminal(trigger: impl FnOnce(&mut SpawnedMercury, &mut SpawnedMercury)) {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");

    let (mut spawned_server, mut thin_client, pty_output) =
        attach_thin_client(&config_home, &runtime_dir, &api_socket, &client_socket);

    let since = output_len(&pty_output);
    trigger(&mut spawned_server, &mut thin_client);

    let output = drain_until_client_exits(&mut thin_client, &pty_output, since);
    assert!(
        output_has_mouse_teardown(&output),
        "client must emit mouse teardown after trigger; output after trigger: {output:?}"
    );

    // SpawnedMercury::Drop kills and reaps both processes with a bounded wait.
    drop(spawned_server);
    cleanup_spawned_mercury(thin_client, base);
}

/// The `--remote` ssh-death path: killing the bridge closes the socket, the
/// client sees EOF and unwinds normally, so the terminal is restored. This is
/// the path that does NOT deliver a signal to the client. Guards against a
/// regression that would leave mouse reporting on after an ssh disconnect.
#[test]
fn client_restores_terminal_on_server_eof() {
    assert_client_restores_terminal(|server, _client| {
        // Kill the server unexpectedly; the client socket closes and the
        // client reader hits EOF, mirroring the ssh bridge dying under
        // `mercury --remote`.
        if let Some(pid) = server.child.process_id() {
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGKILL);
            }
        }
        server.close_master();
    });
}

/// A direct SIGHUP/SIGTERM with a writable terminal follows the graceful quit
/// path and emits the terminal teardown. Actual terminal-window closure also
/// makes the PTY unwritable and is covered separately below.
#[test]
fn client_restores_terminal_on_sighup() {
    assert_client_restores_terminal(|_server, client| {
        let pid = client.child.process_id().expect("thin client pid") as libc::pid_t;
        unsafe {
            libc::kill(pid, libc::SIGHUP);
        }
    });
}

fn read_until_client_attaches(client: &SpawnedMercury) -> String {
    let master = client._master.as_ref().expect("thin client master");
    let fd = master.as_raw_fd().expect("thin client PTY file descriptor");
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert_ne!(flags, -1, "read thin client PTY flags");
    assert_ne!(
        unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) },
        -1,
        "make thin client PTY nonblocking"
    );

    let mut reader = master.try_clone_reader().expect("clone client PTY reader");
    let mut output = String::new();
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        let mut buf = [0u8; 4096];
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => output.push_str(&String::from_utf8_lossy(&buf[..n])),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(err) => panic!("read thin client PTY: {err}"),
        }
        if output.contains('\u{2500}')
            || output.contains("workspace")
            || output.contains("pane")
            || output.contains("terminal")
        {
            return output;
        }
    }
    panic!("thin client must attach and render a frame; output: {output:?}");
}

#[test]
fn client_exits_cleanly_when_terminal_and_transport_hang_up() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");

    let mut spawned_server = spawn_server(&config_home, &runtime_dir, &api_socket, &client_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));

    let mut thin_client = spawn_client_process(&config_home, &runtime_dir, &api_socket);
    read_until_client_attaches(&thin_client);

    // Freeze the client so the dead terminal and transport EOF are both
    // observable when it resumes, making the `--remote` shutdown race deterministic.
    let client_pid = thin_client.child.process_id().expect("thin client pid") as libc::pid_t;
    assert_eq!(
        unsafe { libc::kill(client_pid, libc::SIGSTOP) },
        0,
        "stop thin client"
    );
    let server_pid = spawned_server.child.process_id().expect("server pid") as libc::pid_t;
    assert_eq!(
        unsafe { libc::kill(server_pid, libc::SIGKILL) },
        0,
        "kill server transport"
    );
    spawned_server.close_master();
    thin_client.close_master();
    assert_eq!(
        unsafe { libc::kill(client_pid, libc::SIGCONT) },
        0,
        "resume thin client"
    );

    let deadline = Instant::now() + Duration::from_secs(12);
    let status = loop {
        if let Some(status) = thin_client.child.try_wait().expect("poll thin client") {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        thread::sleep(Duration::from_millis(20));
    };

    drop(spawned_server);
    cleanup_spawned_mercury(thin_client, base);

    let status = status.expect("thin client should exit after terminal and transport hang up");
    assert!(
        status.success(),
        "thin client should exit cleanly after terminal and transport hang up, got {status}"
    );
}

#[test]
fn client_exits_cleanly_when_terminal_hangs_up() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");

    let spawned_server = spawn_server(&config_home, &runtime_dir, &api_socket, &client_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));

    let mut thin_client = spawn_client_process(&config_home, &runtime_dir, &api_socket);
    let attached_output = read_until_client_attaches(&thin_client);

    // Closing the final PTY master models the outer terminal disappearing: the
    // foreground client receives SIGHUP and writes to stdout/stderr fail.
    thin_client.close_master();
    let deadline = Instant::now() + Duration::from_secs(12);
    let status = loop {
        if let Some(status) = thin_client.child.try_wait().expect("poll thin client") {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        thread::sleep(Duration::from_millis(20));
    };
    let server_response = ping_socket(&api_socket);

    drop(spawned_server);
    cleanup_spawned_mercury(thin_client, base);

    let status = status.unwrap_or_else(|| {
        panic!("thin client did not exit after PTY hangup; attach output: {attached_output:?}")
    });
    assert!(
        status.success(),
        "thin client should exit cleanly after PTY hangup, got {status}; attach output: {attached_output:?}"
    );
    assert!(
        server_response.contains("pong"),
        "server should survive client PTY hangup: {server_response}"
    );
}

#[test]
fn client_receives_pane_surface_after_pane_output() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");

    let spawned = spawn_server(&config_home, &runtime_dir, &api_socket, &client_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));

    let mut stream = UnixStream::connect(&client_socket).expect("should connect to client socket");
    let (version, error) = client_shell_handshake(&mut stream, CURRENT_PROTOCOL, 54, 23)
        .expect("handshake should succeed");
    assert_eq!(version, CURRENT_PROTOCOL);
    assert!(error.is_none(), "{error:?}");
    wait_for_client_shell_bootstrap(&mut stream, Duration::from_secs(10))
        .expect("initial client shell bootstrap");

    let created = send_json_request(
        &api_socket,
        &serde_json::json!({
            "id": "create-output-workspace",
            "method": "workspace.create",
            "params": {"label": "output", "focus": true}
        })
        .to_string(),
    );
    let pane_id = created["result"]["root_pane"]["pane_id"]
        .as_str()
        .expect("root pane id");
    assert!(wait_for_message_variant(
        &mut stream,
        Duration::from_secs(5),
        SERVER_MESSAGE_PANE_SURFACE,
    )
    .expect("wait for created workspace surface"));

    let sent = send_json_request(
        &api_socket,
        &serde_json::json!({
            "id": "send-output",
            "method": "pane.send_text",
            "params": {"pane_id": pane_id, "text": "printf 'test-output\\n'\\n"}
        })
        .to_string(),
    );
    assert!(sent.get("error").is_none(), "{sent}");
    assert!(
        wait_for_message_variants(
            &mut stream,
            Duration::from_secs(5),
            &[
                SERVER_MESSAGE_PANE_SURFACE,
                SERVER_MESSAGE_PANE_SURFACE_PATCH,
            ],
        )
        .expect("wait for post-output pane surface"),
        "should receive a pane surface update after pane output"
    );

    cleanup_spawned_mercury(spawned, base);
}

#[test]
fn unavailable_restored_pane_keeps_saved_cwd_in_server() {
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");
    let data_dir = config_home.join(app_dir_name());
    let missing_cwd = base.join("missing-cwd-for-test");
    let missing_cwd = missing_cwd.to_str().expect("test cwd should be UTF-8");
    fs::create_dir_all(&data_dir).unwrap();
    let session = serde_json::json!({
        "version": 2,
        "workspaces": [{
            "custom_name": "missing-cwd",
            "layout": { "Pane": 0 },
            "panes": { "0": { "cwd": missing_cwd } },
            "zoomed": false,
            "focused": 0,
            "root_pane": 0
        }],
        "active": 0,
        "selected": 0
    });
    fs::write(
        data_dir.join("session.json"),
        serde_json::to_vec_pretty(&session).unwrap(),
    )
    .unwrap();

    let spawned = spawn_server(&config_home, &runtime_dir, &api_socket, &client_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));

    let workspaces = send_json_request(
        &api_socket,
        r#"{"id":"workspace_list","method":"workspace.list","params":{}}"#,
    );
    let restored_workspace = workspaces["result"]["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|workspace| workspace["label"] == "missing-cwd")
        .expect("server should restore workspace with missing pane cwd");
    let workspace_id = restored_workspace["workspace_id"]
        .as_str()
        .expect("restored workspace should have public id");
    let pane_id = first_pane_id_in_workspace(&api_socket, workspace_id);
    let pane = send_json_request(
        &api_socket,
        &format!(r#"{{"id":"pane_get","method":"pane.get","params":{{"pane_id":"{pane_id}"}}}}"#),
    );
    assert_eq!(pane["result"]["pane"]["workspace_id"], workspace_id);
    let cwd = pane["result"]["pane"]["cwd"]
        .as_str()
        .expect("restored pane should retain saved cwd");
    assert_eq!(cwd, missing_cwd);
    assert!(pane["result"]["pane"]["restore_error"]
        .as_str()
        .is_some_and(|error| error.contains("directory")));

    let client_shell = spawn_client_shell_process(&config_home, &runtime_dir, &api_socket);
    let output = spawn_pty_drain(
        client_shell
            ._master
            .as_ref()
            .expect("restored client shell PTY")
            .try_clone_reader()
            .expect("clone restored client shell reader"),
    );
    assert!(
        wait_until(Duration::from_secs(8), Duration::from_millis(20), || {
            let screen = read_output(&output);
            screen.contains("missing-cwd") && screen.contains("unavailable")
        }),
        "client shell should render the unavailable pane; output: {:?}",
        read_output(&output)
    );
    drop(client_shell);
    let stopped = send_json_request(
        &api_socket,
        r#"{"id":"stop","method":"server.stop","params":{}}"#,
    );
    assert!(stopped.get("error").is_none(), "{stopped}");
    let mut spawned = spawned;
    assert!(wait_until(
        Duration::from_secs(10),
        Duration::from_millis(20),
        || { spawned.child.try_wait().unwrap().is_some() }
    ));
    drop(spawned);

    fs::create_dir(missing_cwd).unwrap();
    let restarted = spawn_server(&config_home, &runtime_dir, &api_socket, &client_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));
    let recovered = send_json_request(
        &api_socket,
        &format!(r#"{{"id":"recovered","method":"pane.get","params":{{"pane_id":"{pane_id}"}}}}"#),
    );
    assert_eq!(
        std::fs::canonicalize(recovered["result"]["pane"]["cwd"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(missing_cwd).unwrap()
    );
    assert!(recovered["result"]["pane"]["restore_error"].is_null());
    let sent = send_json_request(
        &api_socket,
        &serde_json::json!({"id": "type", "method": "pane.send_text", "params": {
            "pane_id": pane_id, "text": "printf 'RESTORE_RETRY_OK\\n'\n"
        }})
        .to_string(),
    );
    assert!(sent.get("error").is_none(), "{sent}");
    cleanup_spawned_mercury(restarted, base);
}

#[test]
fn graceful_shutdown_sends_server_shutdown_to_client() {
    // Issue 2 fix: SIGINT triggers initiate_shutdown → ServerShutdown
    // broadcast to all clients before the server exits.
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");

    let mut spawned = spawn_server(&config_home, &runtime_dir, &api_socket, &client_socket);
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));

    let mut stream = UnixStream::connect(&client_socket).expect("should connect to client socket");
    let (version, error) = client_shell_handshake(&mut stream, CURRENT_PROTOCOL, 54, 23)
        .expect("handshake should succeed");
    assert_eq!(version, CURRENT_PROTOCOL);
    assert!(error.is_none(), "{error:?}");
    wait_for_client_shell_bootstrap(&mut stream, Duration::from_secs(5))
        .expect("client shell bootstrap");

    // Send SIGINT to the server process to trigger graceful shutdown.
    if let Some(pid) = spawned.child.process_id() {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGINT);
        }
    }

    // The client should receive a ServerShutdown message
    // before the connection is closed, not just an abrupt EOF.
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let result = read_server_message(&mut stream);
    match result {
        Ok((variant, _payload)) => {
            assert_eq!(
                variant, SERVER_MESSAGE_SERVER_SHUTDOWN,
                "expected ServerShutdown, got variant {variant}"
            );
        }
        Err(e) => {
            panic!("expected ServerShutdown message before connection close, got error: {e}");
        }
    }

    // Wait for the server to exit.
    spawned.close_master();
    let _ = spawned.child.wait();

    drop(spawned);
    cleanup_test_base(&base);
}

#[test]
fn client_receives_notify_on_agent_state_change() {
    // Notification events (sound/toast) are forwarded as
    // ServerMessage::Notify to connected clients when an agent state change
    // is triggered via the API (pane.report_agent).
    let _lock = test_lock();
    let base = unique_test_dir();
    let config_home = base.join("config");
    let runtime_dir = base.join("runtime");
    let api_socket = runtime_dir.join("mercury.sock");
    let client_socket = runtime_dir.join("mercury-client.sock");

    // Enable toast and sound in config so the server produces notifications.
    fs::create_dir_all(config_home.join(app_dir_name())).unwrap();
    fs::write(
        config_home.join(app_dir_name()).join("config.toml"),
        "onboarding = false\n[ui.toast]\nenabled = true\n[ui.sound]\nenabled = true\n",
    )
    .unwrap();
    fs::create_dir_all(&runtime_dir).unwrap();
    register_runtime_dir(&runtime_dir);

    // Spawn the server directly (not using spawn_server helper because it
    // overwrites the config file with a minimal one).
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();

    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_mercury"));
    cmd.arg("server");
    cmd.env("XDG_CONFIG_HOME", &config_home);
    cmd.env("XDG_RUNTIME_DIR", &runtime_dir);
    cmd.env("XDG_DATA_HOME", runtime_dir.join("data"));
    cmd.env("MERCURY_SOCKET_PATH", &api_socket);
    cmd.env_remove("MERCURY_CLIENT_SOCKET_PATH");
    cmd.env("SHELL", "/bin/sh");
    cmd.env_remove("MERCURY_ENV");

    let child = pair.slave.spawn_command(cmd).unwrap();
    register_spawned_mercury_pid(child.process_id());
    drop(pair.slave);

    let spawned = SpawnedMercury {
        _master: Some(pair.master),
        child,
    };
    wait_for_socket(&api_socket, Duration::from_secs(10));
    wait_for_socket(&client_socket, Duration::from_secs(10));

    let mut stream = UnixStream::connect(&client_socket).expect("should connect");
    let (version, error) = client_shell_handshake(&mut stream, CURRENT_PROTOCOL, 54, 23)
        .expect("handshake should succeed");
    assert_eq!(version, CURRENT_PROTOCOL);
    assert!(error.is_none(), "{error:?}");
    wait_for_client_shell_bootstrap(&mut stream, Duration::from_secs(5))
        .expect("client shell bootstrap");

    // Create a workspace via the API.
    let mut ws_stream = UnixStream::connect(&api_socket).expect("connect to API");
    let request = r#"{"id":"1","method":"workspace.create","params":{}}"#;
    writeln!(ws_stream, "{}", request).unwrap();
    let mut reader = BufReader::new(ws_stream);
    let mut ws_response = String::new();
    reader.read_line(&mut ws_response).unwrap();

    // Extract the workspace ID and pane ID from the response.
    let ws_id = ws_response
        .split('"')
        .find(|s| s.starts_with("w_"))
        .unwrap_or("w_1")
        .to_string();

    // Get pane list to find a pane ID.
    let mut pane_stream = UnixStream::connect(&api_socket).expect("connect to API");
    let pane_request =
        format!(r#"{{"id":"2","method":"pane.list","params":{{"workspace_id":"{ws_id}"}}}}"#);
    writeln!(pane_stream, "{}", pane_request).unwrap();
    let mut pane_reader = BufReader::new(pane_stream);
    let mut pane_response = String::new();
    pane_reader.read_line(&mut pane_response).unwrap();

    // Extract first pane ID (format: p_<ws>_<pane>).
    let pane_id = pane_response
        .split('"')
        .find(|s| s.starts_with("p_"))
        .unwrap_or("p_1_1")
        .to_string();

    // Report agent as Blocked via the API — this should trigger a
    // ServerMessage::Notify with kind=Sound (Request sound).
    let mut report_stream = UnixStream::connect(&api_socket).expect("connect to API");
    let report_request = format!(
        r#"{{"id":"3","method":"pane.report_agent","params":{{"pane_id":"{pane_id}","agent":"pi","state":"blocked","source":"test"}}}}"#
    );
    writeln!(report_stream, "{}", report_request).unwrap();
    let mut report_reader = BufReader::new(report_stream);
    let mut report_response = String::new();
    report_reader.read_line(&mut report_response).unwrap();

    // Read messages from the client stream and look for the semantic notification.
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut found_notify = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match read_server_message(&mut stream) {
            Ok((variant, _payload)) => {
                if variant == SERVER_MESSAGE_SEMANTIC_NOTIFICATION {
                    found_notify = true;
                    break;
                }
                // Snapshot and pane-surface messages may arrive first.
            }
            Err(_) => {
                break;
            }
        }
    }

    assert!(
        found_notify,
        "client should receive a semantic notification after pane.report_agent"
    );

    // Now report Idle from Working — this should trigger a Done sound
    // if the pane is in a background workspace.
    // First, create a second workspace to make the first one "background".
    let mut ws2_stream = UnixStream::connect(&api_socket).expect("connect to API");
    let ws2_request = r#"{"id":"4","method":"workspace.create","params":{}}"#;
    writeln!(ws2_stream, "{}", ws2_request).unwrap();
    let mut ws2_reader = BufReader::new(ws2_stream);
    let mut ws2_response = String::new();
    ws2_reader.read_line(&mut ws2_response).unwrap();

    // Focus the new workspace (making the first one background).
    let ws2_id = ws2_response
        .split('"')
        .find(|s| s.starts_with("w_"))
        .unwrap_or("w_2")
        .to_string();
    let mut focus_stream = UnixStream::connect(&api_socket).expect("connect to API");
    let focus_request = format!(
        r#"{{"id":"5","method":"workspace.focus","params":{{"workspace_id":"{ws2_id}"}}}}"#
    );
    writeln!(focus_stream, "{}", focus_request).unwrap();
    let mut focus_reader = BufReader::new(focus_stream);
    let mut focus_response = String::new();
    focus_reader.read_line(&mut focus_response).unwrap();

    assert!(
        wait_until(Duration::from_secs(2), Duration::from_millis(25), || {
            ping_socket(&api_socket).contains("pong")
        }),
        "server should stay responsive after workspace focus"
    );

    // Report agent as Working first, then Idle — this transition in a
    // background workspace should trigger a Done sound notification.
    let mut work_stream = UnixStream::connect(&api_socket).expect("connect to API");
    let work_request = format!(
        r#"{{"id":"6","method":"pane.report_agent","params":{{"pane_id":"{pane_id}","agent":"pi","state":"working","source":"test"}}}}"#
    );
    writeln!(work_stream, "{}", work_request).unwrap();
    let mut work_reader = BufReader::new(work_stream);
    let mut work_response = String::new();
    work_reader.read_line(&mut work_response).unwrap();

    assert!(
        wait_until(Duration::from_secs(2), Duration::from_millis(25), || {
            ping_socket(&api_socket).contains("pong")
        }),
        "server should stay responsive after working state report"
    );

    let mut idle_stream = UnixStream::connect(&api_socket).expect("connect to API");
    let idle_request = format!(
        r#"{{"id":"7","method":"pane.report_agent","params":{{"pane_id":"{pane_id}","agent":"pi","state":"idle","source":"test"}}}}"#
    );
    writeln!(idle_stream, "{}", idle_request).unwrap();
    let mut idle_reader = BufReader::new(idle_stream);
    let mut idle_response = String::new();
    idle_reader.read_line(&mut idle_response).unwrap();

    // Read messages and look for the done semantic notification.
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut found_done_notify = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match read_server_message(&mut stream) {
            Ok((variant, _payload)) => {
                if variant == SERVER_MESSAGE_SEMANTIC_NOTIFICATION {
                    found_done_notify = true;
                    break;
                }
                // Snapshot and pane-surface messages may arrive first.
            }
            Err(e) => {
                eprintln!("read error while looking for done notification: {e}");
                break;
            }
        }
    }

    assert!(
        found_done_notify,
        "client should receive a semantic notification when a background pane transitions Working→Idle"
    );

    cleanup_spawned_mercury(spawned, base);
}
