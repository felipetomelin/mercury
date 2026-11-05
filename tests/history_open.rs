//! End-to-end History resume against an isolated server and a disposable CLI.

#![cfg(unix)]

pub mod support;
#[path = "support/terminal_screen.rs"]
mod terminal_screen;

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde_json::{json, Value};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    server: Option<Box<dyn Child + Send + Sync>>,
    _pty: Option<Box<dyn MasterPty + Send>>,
}

struct ClientPty {
    child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    output: Arc<Mutex<Vec<u8>>>,
}

impl ClientPty {
    fn screen(&self) -> String {
        let output = self.output.lock().unwrap();
        terminal_screen::text(&output, 80, 24)
    }
}

impl Drop for ClientPty {
    fn drop(&mut self) {
        let pid = self.child.process_id();
        let _ = self.child.kill();
        support::unregister_spawned_mercury_pid(pid);
    }
}

impl Fixture {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = loop {
            let sequence = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = PathBuf::from(format!(
                "/tmp/mh-open-{}-{nanos:x}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create history fixture: {error}"),
            }
        };
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            root,
            server: None,
            _pty: None,
        }
    }

    fn socket(&self) -> PathBuf {
        self.root.join("runtime/mercury.sock")
    }

    fn env_command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mercury"));
        command
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_RUNTIME_DIR", self.root.join("runtime"))
            .env("MERCURY_SOCKET_PATH", self.socket())
            .env("MERCURY_CONFIG_PATH", self.root.join("config.toml"))
            .env("CODEX_HOME", self.root.join("codex"))
            .env("CLAUDE_CONFIG_DIR", self.root.join("claude"));
        command
    }

    fn start(&mut self) {
        fs::create_dir_all(self.root.join("config")).unwrap();
        fs::write(
            self.root.join("config.toml"),
            "onboarding = false\n[terminal]\ndefault_shell = \"/bin/sh\"\nshell_mode = \"non_login\"\n",
        )
        .unwrap();
        fs::create_dir_all(self.root.join("runtime")).unwrap();
        support::register_runtime_dir(&self.root.join("runtime"));
        let project = self.root.join("project");
        fs::create_dir(&project).unwrap();
        let transcript = self
            .root
            .join("codex/sessions/2024/01/02/rollout-fixture-codex.jsonl");
        fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        let line = json!({"timestamp":"2024-01-02T12:00:00Z","type":"session_meta",
            "payload":{"session_id":"fixture-codex","cwd":project}});
        let message = json!({"timestamp":"2024-01-02T12:00:01Z","type":"response_item",
            "payload":{"type":"message","role":"user",
                "content":[{"type":"input_text","text":"fixture-needle"}]}});
        fs::write(transcript, format!("{line}\n{message}\n")).unwrap();

        let mock_bin = self.root.join("bin");
        fs::create_dir(&mock_bin).unwrap();
        let mock_codex = mock_bin.join("codex");
        fs::write(
            &mock_codex,
            "#!/bin/sh\nprintf 'mock-codex-resume:%s\\n' \"$*\"\nread pause\n",
        )
        .unwrap();
        fs::set_permissions(&mock_codex, fs::Permissions::from_mode(0o700)).unwrap();

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_mercury"));
        command.arg("server");
        command.env("XDG_CONFIG_HOME", self.root.join("config"));
        command.env("XDG_RUNTIME_DIR", self.root.join("runtime"));
        command.env("XDG_DATA_HOME", self.root.join("data"));
        command.env("MERCURY_SOCKET_PATH", self.socket());
        command.env("MERCURY_CONFIG_PATH", self.root.join("config.toml"));
        command.env("CODEX_HOME", self.root.join("codex"));
        command.env("CLAUDE_CONFIG_DIR", self.root.join("claude"));
        command.env("PATH", format!("{}:/usr/bin:/bin", mock_bin.display()));
        command.env("SHELL", "/bin/sh");
        command.env_remove("MERCURY_ENV");
        command.env_remove("MERCURY_CLIENT_SOCKET_PATH");
        let child = pair.slave.spawn_command(command).unwrap();
        support::register_spawned_mercury_pid(child.process_id());
        self.server = Some(child);
        self._pty = Some(pair.master);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.socket().exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        if !self.socket().exists() {
            panic!("isolated server did not create socket");
        }
    }

    fn client(&self) -> ClientPty {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_mercury"));
        command.arg("client");
        command.env("XDG_CONFIG_HOME", self.root.join("config"));
        command.env("XDG_RUNTIME_DIR", self.root.join("runtime"));
        command.env("MERCURY_SOCKET_PATH", self.socket());
        command.env("MERCURY_CONFIG_PATH", self.root.join("config.toml"));
        command.env("CODEX_HOME", self.root.join("codex"));
        command.env("CLAUDE_CONFIG_DIR", self.root.join("claude"));
        command.env("SHELL", "/bin/sh");
        command.env("MERCURY_DISABLE_SOUND", "1");
        command.env_remove("MERCURY_ENV");
        command.env_remove("MERCURY_CLIENT_SOCKET_PATH");
        let child = pair.slave.spawn_command(command).unwrap();
        support::register_spawned_mercury_pid(child.process_id());
        drop(pair.slave);
        let output = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&output);
        let mut reader = pair.master.try_clone_reader().unwrap();
        thread::spawn(move || {
            let mut buffer = [0; 8192];
            while let Ok(size) = reader.read(&mut buffer) {
                if size == 0 {
                    break;
                }
                captured.lock().unwrap().extend_from_slice(&buffer[..size]);
            }
        });
        ClientPty {
            child,
            master: pair.master,
            output,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut server) = self.server.take() {
            let pid = server.process_id();
            let _ = server.kill();
            support::unregister_spawned_mercury_pid(pid);
        }
        self._pty.take();
        support::cleanup_test_base(&self.root);
    }
}

fn request(socket: &Path, value: Value) -> Value {
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    writeln!(stream, "{value}").unwrap();
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response).unwrap();
    serde_json::from_str(&response).unwrap()
}

fn wait_for_mock_resume(fixture: &Fixture, pane_id: &str, client: Option<&ClientPty>) {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let read = request(
            &fixture.socket(),
            json!({"id":"read","method":"pane.read",
                "params":{"pane_id":pane_id,"source":"recent","lines":20}}),
        );
        if read["result"]["read"]["text"].as_str().is_some_and(|text| {
            text.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .contains("mock-codex-resume:resume fixture-codex")
        }) {
            return;
        }
        if Instant::now() >= deadline {
            let agent = request(
                &fixture.socket(),
                json!({"id":"agent","method":"agent.get","params":{"target":pane_id}}),
            );
            panic!(
                "mock resume never appeared: status={:?}, launch_pending={:?}, screen={:?}",
                agent["result"]["agent"]["agent_status"].as_str(),
                agent["result"]["agent"]["launch_pending"].as_bool(),
                client.map(ClientPty::screen)
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn history_tab_reopens_fixture_session_in_new_pane_without_real_provider() {
    let mut fixture = Fixture::new();
    fixture.start();
    let workspace = request(
        &fixture.socket(),
        json!({"id":"workspace","method":"workspace.create",
            "params":{"cwd":fixture.root.join("project"),"focus":true}}),
    );
    assert_eq!(workspace["result"]["type"], "workspace_created");

    let client = fixture.client();
    let deadline = Instant::now() + Duration::from_secs(8);
    let chip_column = loop {
        let screen = client.screen();
        if let Some(column) = screen.lines().next().and_then(|line| line.find("history")) {
            break column + 2;
        }
        assert!(Instant::now() < deadline, "History chip absent: {screen}");
        thread::sleep(Duration::from_millis(25));
    };
    let mut input = client.master.take_writer().unwrap();
    input
        .write_all(format!("\x1b[<0;{chip_column};1M\x1b[<0;{chip_column};1m").as_bytes())
        .unwrap();
    input.write_all(b"/fixture-needle\r").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let screen = client.screen();
        if screen.contains("History") && screen.contains("fixture-needle") {
            break;
        }
        assert!(Instant::now() < deadline, "History result absent: {screen}");
        thread::sleep(Duration::from_millis(25));
    }
    input.write_all(b"\r").unwrap();
    wait_for_mock_resume(&fixture, "w1:p2", Some(&client));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let screen = client.screen();
        if !screen.contains("History ·") && screen.contains("mock-codex-resume") {
            break;
        }
        assert!(Instant::now() < deadline, "History did not close: {screen}");
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn search_and_open_resume_in_new_pane_without_real_provider() {
    let mut fixture = Fixture::new();
    fixture.start();

    let search = fixture
        .env_command()
        .args(["history", "search", "--query", "fixture-needle", "--json"])
        .output()
        .unwrap();
    assert!(
        search.status.success(),
        "{}",
        String::from_utf8_lossy(&search.stderr)
    );
    let found: Value = serde_json::from_slice(&search.stdout).unwrap();
    assert_eq!(found["result"]["hits"][0]["session_id"], "fixture-codex");
    let created = request(
        &fixture.socket(),
        json!({"id":"workspace","method":"workspace.create",
            "params":{"cwd":fixture.root.join("project"),"focus":true}}),
    );
    assert_eq!(created["result"]["type"], "workspace_created");

    let open = fixture
        .env_command()
        .args(["history", "open", "codex", "fixture-codex", "--json"])
        .output()
        .unwrap();
    assert!(
        open.status.success(),
        "{}",
        String::from_utf8_lossy(&open.stderr)
    );
    let opened: Value = serde_json::from_slice(&open.stdout).unwrap();
    assert_eq!(opened["result"]["status"], "started");
    let pane_id = opened["result"]["pane_id"].as_str().unwrap();
    assert_eq!(opened["result"]["workspace_id"], "w1");
    assert_eq!(pane_id, "w1:p2");
    wait_for_mock_resume(&fixture, pane_id, None);
}

#[test]
fn open_creates_workspace_for_headless_server_without_real_provider() {
    let mut fixture = Fixture::new();
    fixture.start();

    let open = fixture
        .env_command()
        .args(["history", "open", "codex", "fixture-codex", "--json"])
        .output()
        .unwrap();
    assert!(
        open.status.success(),
        "{}",
        String::from_utf8_lossy(&open.stderr)
    );
    let opened: Value = serde_json::from_slice(&open.stdout).unwrap();
    assert_eq!(opened["result"]["status"], "started");
    assert_eq!(opened["result"]["workspace_id"], "w1");
    assert_eq!(opened["result"]["pane_id"], "w1:p1");
    wait_for_mock_resume(&fixture, "w1:p1", None);
}

#[test]
fn knowledge_cli_graph_jobs_context_and_tui_use_only_isolated_fixture_data() {
    let mut fixture = Fixture::new();
    fixture.root = fixture.root.canonicalize().unwrap();
    fixture.start();
    let vault = fixture.root.join("vault");
    fs::create_dir(&vault).unwrap();
    fs::write(vault.join("Alpha.md"),"---\nid: alpha\nproject: PV\nstatus: validated\ndepends_on: '[[Beta]]'\n---\nAlpha depends on Beta. Alpha deploys with verified rollback.\n").unwrap();
    fs::write(
        vault.join("Beta.md"),
        "---\nid: beta\nproject: PV\n---\nBeta provides the queue for Alpha.\n",
    )
    .unwrap();
    let script = r#"#!/usr/bin/env python3
import json,sys
if '--version' in sys.argv:
 print('codex knowledge-fixture-v1');sys.exit(0)
v=json.load(sys.stdin);task=v['task'];data=v['input']
if task.startswith('extract'):
 cid=data['chunk_id'];text=data['text']
 entities=[{'name':name,'kind':'service','evidence':[cid]} for name in ['Alpha','Beta'] if name in text]
 relations=[{'source':'Alpha','target':'Beta','kind':'depends_on','evidence':[cid],'quote':'Alpha depends on Beta'}] if 'Alpha depends on Beta' in text else []
 answer={'keywords':['Alpha','Beta'],'entities':entities,'relations':relations}
elif task.startswith('expand'):
 answer={'keywords':['Alpha','Beta']}
elif 'propose reusable' in task:
 answer={'markdown':'Alpha depends on [[Beta]]; verify rollback before deployment.'}
else:
 cid=(data.get('chunks') or data.get('sources') or [{}])[0].get('id') or (data.get('sources') or [{}])[0].get('chunk_id') or (data.get('allowed_evidence') or ['missing'])[0]
 answer={'insufficient':False,'claims':[{'text':'Alpha depends on Beta.','evidence':[cid]}]}
print(json.dumps({'type':'item.completed','item':{'type':'agent_message','text':json.dumps(answer)}}))
print(json.dumps({'type':'turn.completed','usage':{'input_tokens':12,'output_tokens':7}}))
"#;
    let binary = fixture.root.join("bin/codex");
    fs::write(&binary, script).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    let config=format!("onboarding=false\n[terminal]\ndefault_shell='/bin/sh'\nshell_mode='non_login'\n[knowledge]\nenabled=true\nvault={}\nallow_external=true\nallowed_providers=['codex']\n[knowledge.executables]\ncodex={}\n",json!(vault.to_string_lossy()),json!(binary.to_string_lossy()));
    fs::write(fixture.root.join("config.toml"), config).unwrap();
    let reload = request(
        &fixture.socket(),
        json!({"id":"reload","method":"server.reload_config","params":{}}),
    );
    assert!(reload.get("error").is_none(), "{reload}");
    let output = fixture
        .env_command()
        .args(["knowledge", "index", "--graph", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    let job = result["result"]["data"]["job_id"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let r = request(
            &fixture.socket(),
            json!({"id":"job","method":"knowledge.request","params":{"action":"job_show","id":job}}),
        );
        let state = r["result"]["data"]["state"].as_str();
        if state == Some("completed") {
            break;
        }
        assert!(state != Some("failed"), "{r}");
        assert!(Instant::now() < deadline, "{r}");
        thread::sleep(Duration::from_millis(50));
    }
    let output = fixture
        .env_command()
        .args([
            "knowledge",
            "ask",
            "--query",
            "What does Alpha depend on?",
            "--project",
            "PV",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["result"]["data"]["insufficient"], false);
    assert!(response["result"]["data"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .all(|h| h["project"] == "PV"));
    let output = fixture
        .env_command()
        .args([
            "context",
            "build",
            "--query",
            "Alpha",
            "--project",
            "PV",
            "--json",
        ])
        .output()
        .unwrap();
    let bundle: Value = serde_json::from_slice(&output.stdout).unwrap();
    let bundle_id = bundle["result"]["data"]["id"].as_str().unwrap();
    let rejected = request(
        &fixture.socket(),
        json!({"id":"egress","method":"knowledge.request","params":{"action":"route_context","id":bundle_id,"project":"PV","provider":"codex"}}),
    );
    assert!(rejected.get("error").is_some());
    let client = fixture.client();
    let deadline = Instant::now() + Duration::from_secs(10);
    let (x, y) = loop {
        let screen = client.screen();
        if let Some((y, x)) = screen.lines().enumerate().find_map(|(y, line)| {
            line.find("knowledge")
                .or_else(|| line.find(" kg"))
                .map(|x| (y, x))
        }) {
            break (x + 2, y + 1);
        }
        assert!(
            Instant::now() < deadline,
            "missing knowledge chip: {screen}"
        );
        thread::sleep(Duration::from_millis(50));
    };
    let mut writer = client.master.take_writer().unwrap();
    write!(writer, "\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m").unwrap();
    writer.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !client.screen().contains("memória local") {
        assert!(Instant::now() < deadline, "{}", client.screen());
        thread::sleep(Duration::from_millis(50));
    }
    writer.write_all(b"/Alpha\r").unwrap();
    writer.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !client.screen().contains("Alpha.md") {
        assert!(Instant::now() < deadline, "{}", client.screen());
        thread::sleep(Duration::from_millis(50));
    }
    let captured = fixture
        .env_command()
        .args([
            "knowledge",
            "capture",
            "--provider",
            "codex",
            "--id",
            "fixture-codex",
            "--completed",
            "--title",
            "Learning",
            "--project",
            "PV",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        captured.status.success(),
        "{}",
        format!(
            "stdout={} stderr={} exit={:?}",
            String::from_utf8_lossy(&captured.stdout),
            String::from_utf8_lossy(&captured.stderr),
            captured.status.code()
        )
    );
    let captured: Value = serde_json::from_slice(&captured.stdout).unwrap();
    let proposal = captured["result"]["data"]["id"].as_str().unwrap();
    assert_eq!(fs::read_dir(&vault).unwrap().count(), 2);
    for action in ["accept", "undo"] {
        let result = fixture
            .env_command()
            .args([
                "knowledge",
                "review",
                "--id",
                proposal,
                "--mode",
                action,
                "--json",
            ])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stdout)
        );
        if action == "accept" {
            let response: Value = serde_json::from_slice(&result.stdout).unwrap();
            let markdown = response["result"]["data"]["markdown"].as_str().unwrap();
            assert!(markdown.contains("[[Alpha]]"));
            assert!(markdown.contains("Relações explícitas relacionadas"));
        }
    }
    assert_eq!(fs::read_dir(vault.join("Mercury")).unwrap().count(), 0);
    assert_eq!(
        fs::read_dir(&vault)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
            .count(),
        2
    );
}
