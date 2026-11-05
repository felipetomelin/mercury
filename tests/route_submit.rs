//! Socket-level route submission using a fake interactive Codex executable.

#![cfg(unix)]

pub mod support;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
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

impl Fixture {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = loop {
            let sequence = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = PathBuf::from(format!(
                "/tmp/mr-submit-{}-{nanos:x}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create route fixture: {error}"),
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

    fn prompt_file(&self) -> PathBuf {
        self.root.join("received-prompt")
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mercury"));
        command
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_RUNTIME_DIR", self.root.join("runtime"))
            .env("MERCURY_SOCKET_PATH", self.socket())
            .env("MERCURY_CONFIG_PATH", self.root.join("config.toml"));
        command
    }

    fn start(&mut self) {
        self.start_with_agent_failure(false);
    }

    fn start_with_agent_failure(&mut self, fail_before_prompt: bool) {
        fs::create_dir_all(self.root.join("runtime")).unwrap();
        fs::create_dir_all(self.root.join("config")).unwrap();
        support::register_runtime_dir(&self.root.join("runtime"));
        fs::write(
            self.root.join("config.toml"),
            format!(
                "onboarding = false\n[terminal]\ndefault_shell = \"/bin/sh\"\nshell_mode = \"non_login\"\n[route]\nenabled = true\ndefault_provider = \"codex\"\n[worktrees]\ndirectory = \"{}\"\n",
                self.root.join("worktrees").display()
            ),
        ).unwrap();
        let project = self.root.join("project");
        fs::create_dir(&project).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&project)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git fixture command failed: {args:?}"
            );
        };
        git(&["init", "--quiet"]);
        fs::write(project.join("README.md"), "fixture repository\n").unwrap();
        git(&["add", "README.md"]);
        git(&[
            "-c",
            "user.name=Mercury Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--quiet",
            "-m",
            "initial",
        ]);
        let bin = self.root.join("bin");
        fs::create_dir(&bin).unwrap();
        let fake = bin.join("codex");
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = app-server ]; then\n  read init\n  printf '%s\\n' '{{\"id\":1,\"result\":{{}}}}'\n  read initialized\n  read limits\n  printf '%s\\n' '{{\"id\":2,\"result\":{{\"rateLimits\":{{\"primary\":null,\"secondary\":null}}}}}}'\n  exit 0\nfi\nif [ \"$MERCURY_ROUTE_FIXTURE_FAIL_START\" = 1 ]; then\n  \"{}\" pane report-agent \"$MERCURY_PANE_ID\" --source fake-route --agent codex --state blocked >/dev/null 2>&1\n  read pause\n  exit 1\nfi\n\"{}\" pane report-agent \"$MERCURY_PANE_ID\" --source fake-route --agent codex --state idle >/dev/null 2>&1\nIFS= read -r task || exit 0\nprintf '%s\\n' \"$task\" >> \"{}\"\n\"{}\" pane report-agent \"$MERCURY_PANE_ID\" --source fake-route --agent codex --state working >/dev/null 2>&1\n/bin/sleep 1\nprintf 'mock-route-complete\\n'\n\"{}\" pane report-agent \"$MERCURY_PANE_ID\" --source fake-route --agent codex --state idle >/dev/null 2>&1\nread pause\n",
            env!("CARGO_BIN_EXE_mercury"),
            env!("CARGO_BIN_EXE_mercury"),
            self.prompt_file().display(),
            env!("CARGO_BIN_EXE_mercury"),
            env!("CARGO_BIN_EXE_mercury")
        );
        fs::write(&fake, script).unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();

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
        command.env("PATH", format!("{}:/usr/bin:/bin", bin.display()));
        command.env("SHELL", "/bin/sh");
        command.env(
            "MERCURY_ROUTE_FIXTURE_FAIL_START",
            if fail_before_prompt { "1" } else { "0" },
        );
        command.env_remove("MERCURY_ENV");
        command.env_remove("MERCURY_CLIENT_SOCKET_PATH");
        let child = pair.slave.spawn_command(command).unwrap();
        support::register_spawned_mercury_pid(child.process_id());
        self.server = Some(child);
        self._pty = Some(pair.master);
        support::wait_for_socket(&self.socket(), Duration::from_secs(10));
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

#[test]
fn route_submits_prompt_once_to_selected_fake_agent() {
    let mut fixture = Fixture::new();
    fixture.start();
    let workspace = request(
        &fixture.socket(),
        json!({"id":"workspace","method":"workspace.create",
        "params":{"cwd":fixture.root.join("project"),"focus":true}}),
    );
    assert_eq!(workspace["result"]["type"], "workspace_created");

    let mut child = fixture
        .command()
        .args(["route", "submit", "--stdin", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"fixture-route-prompt")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let response: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "route response was not JSON: {error}; status={:?}; stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert!(output.status.success(), "route status: {response}");
    assert_eq!(response["result"]["status"], "submitted");
    assert_eq!(response["result"]["provider"], "codex");

    let deadline = Instant::now() + Duration::from_secs(5);
    while !fixture.prompt_file().exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    let received = fs::read_to_string(fixture.prompt_file()).expect("fake agent received prompt");
    assert_eq!(received.trim(), "fixture-route-prompt");
}

#[test]
fn route_wait_returns_completion_without_echoing_prompt() {
    let mut fixture = Fixture::new();
    fixture.start();
    let workspace = request(
        &fixture.socket(),
        json!({"id":"workspace","method":"workspace.create",
            "params":{"cwd":fixture.root.join("project"),"focus":true}}),
    );
    assert_eq!(workspace["result"]["type"], "workspace_created");

    let mut child = fixture
        .command()
        .args([
            "route",
            "submit",
            "--stdin",
            "--wait",
            "--timeout",
            "15000",
            "--json",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"fixture-route-wait")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(output.status.success(), "route status: {response}");
    assert_eq!(response["result"]["status"], "completed");
    let summary = response["result"]["summary"].as_str().unwrap();
    assert!(summary.contains("mock-route-complete"));
    assert!(!summary.contains("fixture-route-wait"));
    let received = fs::read_to_string(fixture.prompt_file()).expect("fake agent received prompt");
    assert_eq!(received.trim(), "fixture-route-wait");
}

#[test]
fn route_worktree_submits_in_isolated_checkout() {
    let mut fixture = Fixture::new();
    fixture.start();
    let workspace = request(
        &fixture.socket(),
        json!({"id":"workspace","method":"workspace.create",
            "params":{"cwd":fixture.root.join("project"),"focus":true}}),
    );
    assert_eq!(workspace["result"]["type"], "workspace_created");
    let source_workspace = workspace["result"]["workspace"]["workspace_id"]
        .as_str()
        .unwrap();

    let mut child = fixture
        .command()
        .args(["route", "submit", "--stdin", "--worktree", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"fixture-route-worktree")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(output.status.success(), "route status: {response}");
    assert_eq!(response["result"]["status"], "submitted");
    assert_eq!(response["result"]["provider"], "codex");
    let destination = response["result"]["workspace_id"].as_str().unwrap();
    assert_ne!(destination, source_workspace);
    assert!(response["result"]["pane_id"].as_str().is_some());
    let checkout = PathBuf::from(response["result"]["worktree_path"].as_str().unwrap());
    assert!(checkout.starts_with(fixture.root.join("worktrees")));
    assert!(checkout.join("README.md").is_file());

    let deadline = Instant::now() + Duration::from_secs(5);
    while !fixture.prompt_file().exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    let received = fs::read_to_string(fixture.prompt_file()).expect("fake agent received prompt");
    assert_eq!(received.trim(), "fixture-route-worktree");
}

#[test]
fn route_worktree_start_failure_removes_checkout_before_fallback() {
    let mut fixture = Fixture::new();
    fixture.start_with_agent_failure(true);
    let workspace = request(
        &fixture.socket(),
        json!({"id":"workspace","method":"workspace.create",
            "params":{"cwd":fixture.root.join("project"),"focus":true}}),
    );
    assert_eq!(workspace["result"]["type"], "workspace_created");

    let mut child = fixture
        .command()
        .args(["route", "submit", "--stdin", "--worktree", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"fixture-route-must-not-send")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let response: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(
        !output.status.success(),
        "route unexpectedly succeeded: {response}"
    );
    assert_eq!(response["error"]["code"], "route_start_failed");
    assert!(!fixture.prompt_file().exists());
    assert_eq!(
        response["residual_branches"].as_array().unwrap().len(),
        1,
        "removed checkout may leave one branch for explicit cleanup"
    );

    let worktrees = Command::new("git")
        .arg("-C")
        .arg(fixture.root.join("project"))
        .args(["worktree", "list", "--porcelain"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(worktrees.status.success());
    let listed = String::from_utf8(worktrees.stdout).unwrap();
    assert_eq!(
        listed
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        1
    );
    let worktree_root = fixture.root.join("worktrees");
    if worktree_root.exists() {
        for repository_dir in fs::read_dir(worktree_root).unwrap() {
            let repository_dir = repository_dir.unwrap();
            assert!(repository_dir.file_type().unwrap().is_dir());
            assert_eq!(fs::read_dir(repository_dir.path()).unwrap().count(), 0);
        }
    }
}
