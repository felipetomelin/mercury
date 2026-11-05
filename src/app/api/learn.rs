use std::path::Path;

use mercury_learn::transcript::Provider;

use crate::api::schema::{LearnClaudeSessionEndParams, ResponseResult};
use crate::app::learn::{ReviewTrigger, SubmitResult};
use crate::app::App;

use super::responses::{encode_error, encode_success};

impl App {
    pub(super) fn handle_learn_claude_session_end(
        &mut self,
        id: String,
        params: LearnClaudeSessionEndParams,
    ) -> String {
        self.handle_learn_claude_signal(id, params, false)
    }

    pub(super) fn handle_learn_claude_turn_end(
        &mut self,
        id: String,
        params: LearnClaudeSessionEndParams,
    ) -> String {
        self.handle_learn_claude_signal(id, params, true)
    }

    fn handle_learn_claude_signal(
        &mut self,
        id: String,
        params: LearnClaudeSessionEndParams,
        turn: bool,
    ) -> String {
        if !self.learn_config.enabled && self.knowledge_automation.is_none() {
            return encode_error(id, "learn_disabled", "Learn is disabled");
        }
        let Some((ws_idx, pane_id)) = self.parse_pane_id(&params.pane_id) else {
            return encode_error(id, "pane_not_found", "pane not found");
        };
        let Some(session_ref) = crate::agent_resume::AgentSessionRef::id(&params.session_id) else {
            return encode_error(id, "invalid_session", "invalid Claude session ID");
        };
        let trigger = if turn {
            ReviewTrigger::turn_probe(
                Provider::Claude,
                &params.session_id,
                Path::new(&params.transcript_path),
                self.learn_config.review_every_turns,
            )
        } else {
            ReviewTrigger::session_end(
                Provider::Claude,
                &params.session_id,
                Path::new(&params.transcript_path),
            )
        };
        let Some(trigger) = trigger else {
            return encode_error(id, "invalid_session", "invalid Claude session signal");
        };
        let current = self.pane_terminal(ws_idx, pane_id).is_some_and(|terminal| {
            terminal.session_ref_is_current_for("mercury:claude", "claude", &session_ref)
        });
        let known_path = self
            .learn_transcripts
            .get(&(pane_id, Provider::Claude))
            .is_some_and(|observed| {
                observed.session_id == params.session_id
                    && observed.path == Path::new(&params.transcript_path)
            });
        if !current || !known_path {
            return encode_error(
                id,
                "stale_session",
                "Claude session is not current in this pane",
            );
        }
        if self
            .learn_excluded_sessions
            .get(&(pane_id, Provider::Claude))
            == Some(&params.session_id)
        {
            return encode_error(
                id,
                "learn_excluded_origin",
                "session originated from automation",
            );
        }
        let captured = !turn && self.capture_completed_session(&trigger);
        let Some(worker) = self.learn.as_ref().filter(|_| self.learn_config.enabled) else {
            return if captured {
                encode_success(id, ResponseResult::Ok {})
            } else {
                encode_error(
                    id,
                    "learn_unavailable",
                    "Learn worker is unavailable; check external processing configuration",
                )
            };
        };
        match worker.submit(trigger) {
            SubmitResult::Queued => encode_success(id, ResponseResult::Ok {}),
            SubmitResult::Disabled => encode_error(id, "learn_disabled", "Learn is disabled"),
            SubmitResult::QueueFull => encode_error(id, "learn_busy", "Learn review queue is full"),
            SubmitResult::WorkerClosed => {
                encode_error(id, "learn_unavailable", "Learn worker is unavailable")
            }
            SubmitResult::ExcludedOrigin | SubmitResult::InvalidKey => {
                encode_error(id, "invalid_session", "invalid Claude session signal")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::PaneReportAgentSessionParams;
    use crate::config::Config;
    use crate::detect::{Agent, AgentState};
    use crate::workspace::Workspace;

    fn app_with_claude_session() -> (App, String) {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            rx,
            crate::api::EventHub::default(),
        );
        app.learn_config.enabled = false;
        app.state.workspaces = vec![Workspace::test_new("learn")];
        app.state.ensure_test_terminals();
        let pane = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].tabs[0].panes[&pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .set_detected_state(Some(Agent::Claude), AgentState::Working);
        let public_pane = app.public_pane_id(0, pane).unwrap();
        app.handle_pane_report_agent_session(
            "start".into(),
            PaneReportAgentSessionParams {
                pane_id: public_pane.clone(),
                source: "mercury:claude".into(),
                agent: "claude".into(),
                seq: Some(1),
                agent_session_id: Some("session-1".into()),
                agent_session_path: Some("/tmp/session-1.jsonl".into()),
                session_start_source: Some("startup".into()),
                resume_argv: None,
            },
        );
        (app, public_pane)
    }

    #[test]
    fn knowledge_captures_only_current_completed_session_independently_of_learn() {
        let (mut app, pane_id) = app_with_claude_session();
        let (worker, receiver) =
            crate::server::headless::knowledge::AutomationWorker::test_channel();
        app.knowledge_automation = Some(worker);
        let params = LearnClaudeSessionEndParams {
            pane_id,
            session_id: "session-1".into(),
            transcript_path: "/tmp/session-1.jsonl".into(),
        };
        assert!(!app.learn_config.enabled);
        app.handle_learn_claude_turn_end("turn".into(), params.clone());
        assert!(receiver.try_recv().is_err());
        let result = app.handle_learn_claude_session_end("end".into(), params.clone());
        assert!(!result.contains("error"), "{result}");
        assert_eq!(receiver.try_recv().unwrap().session_id, "session-1");
        let mut forged = params;
        forged.transcript_path = "/tmp/forged.jsonl".into();
        assert!(app
            .handle_learn_claude_session_end("forged".into(), forged)
            .contains("stale_session"));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn automated_sessions_never_feed_knowledge_capture() {
        let (mut app, pane_id) = app_with_claude_session();
        let pane = app.state.workspaces[0].tabs[0].root_pane;
        let (worker, receiver) =
            crate::server::headless::knowledge::AutomationWorker::test_channel();
        app.knowledge_automation = Some(worker);
        app.learn_excluded_sessions
            .insert((pane, Provider::Claude), "session-1".into());
        let result = app.handle_learn_claude_session_end(
            "end".into(),
            LearnClaudeSessionEndParams {
                pane_id,
                session_id: "session-1".into(),
                transcript_path: "/tmp/session-1.jsonl".into(),
            },
        );
        assert!(result.contains("learn_excluded_origin"));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn claude_end_requires_enabled_current_session_and_start_path() {
        let (mut app, pane_id) = app_with_claude_session();
        let params = LearnClaudeSessionEndParams {
            pane_id,
            session_id: "session-1".into(),
            transcript_path: "/tmp/session-1.jsonl".into(),
        };
        assert!(app
            .handle_learn_claude_session_end("disabled".into(), params.clone())
            .contains("learn_disabled"));
        app.learn_config.enabled = true;
        assert!(app
            .handle_learn_claude_session_end("valid".into(), params.clone())
            .contains("learn_unavailable"));
        let mut forged_path = params.clone();
        forged_path.transcript_path = "/tmp/other.jsonl".into();
        assert!(app
            .handle_learn_claude_session_end("path".into(), forged_path)
            .contains("stale_session"));
        let mut stale_session = params;
        stale_session.session_id = "session-2".into();
        assert!(app
            .handle_learn_claude_session_end("session".into(), stale_session)
            .contains("stale_session"));
    }

    #[test]
    fn claude_turn_signal_requires_current_session_and_never_marks_end() {
        let (mut app, pane_id) = app_with_claude_session();
        let params = LearnClaudeSessionEndParams {
            pane_id,
            session_id: "session-1".into(),
            transcript_path: "/tmp/session-1.jsonl".into(),
        };
        assert!(app
            .handle_learn_claude_turn_end("disabled".into(), params.clone())
            .contains("learn_disabled"));
        app.learn_config.enabled = true;
        assert!(app
            .handle_learn_claude_turn_end("current".into(), params.clone())
            .contains("learn_unavailable"));
        let mut forged = params.clone();
        forged.transcript_path = "/tmp/forged.jsonl".into();
        assert!(app
            .handle_learn_claude_turn_end("forged".into(), forged)
            .contains("stale_session"));
        let pane = app.state.workspaces[0].tabs[0].root_pane;
        assert!(app
            .learn_transcripts
            .contains_key(&(pane, Provider::Claude)));
        app.learn_excluded_sessions
            .insert((pane, Provider::Claude), "session-1".into());
        assert!(app
            .handle_learn_claude_turn_end("cron".into(), params.clone())
            .contains("learn_excluded_origin"));
        assert!(app
            .handle_learn_claude_session_end("cron-end".into(), params)
            .contains("learn_excluded_origin"));
    }

    #[test]
    fn codex_start_keeps_only_accepted_transcript_path() {
        let (mut app, pane_id) = app_with_claude_session();
        let pane = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].tabs[0].panes[&pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .set_detected_state(Some(Agent::Codex), AgentState::Working);
        let report = |seq, session_id: &str, path: &str| PaneReportAgentSessionParams {
            pane_id: pane_id.clone(),
            source: "mercury:codex".into(),
            agent: "codex".into(),
            seq: Some(seq),
            agent_session_id: Some(session_id.into()),
            agent_session_path: Some(path.into()),
            session_start_source: Some("startup".into()),
            resume_argv: None,
        };
        app.handle_pane_report_agent_session(
            "first".into(),
            report(10, "codex-session", "/tmp/codex-session.jsonl"),
        );
        let accepted = app.learn_transcripts.get(&(pane, Provider::Codex)).unwrap();
        assert_eq!(accepted.session_id, "codex-session");
        assert_eq!(accepted.path, Path::new("/tmp/codex-session.jsonl"));
        app.learn_excluded_sessions
            .insert((pane, Provider::Codex), "codex-session".into());
        app.handle_pane_report_agent_session(
            "stale".into(),
            report(9, "stale-session", "/tmp/stale.jsonl"),
        );
        let accepted = app.learn_transcripts.get(&(pane, Provider::Codex)).unwrap();
        assert_eq!(accepted.session_id, "codex-session");
        assert_eq!(accepted.path, Path::new("/tmp/codex-session.jsonl"));
        assert_eq!(
            app.learn_excluded_sessions.get(&(pane, Provider::Codex)),
            Some(&"codex-session".to_string())
        );
        app.handle_pane_report_agent_session(
            "next".into(),
            report(11, "next-session", "/tmp/next-session.jsonl"),
        );
        assert!(!app
            .learn_excluded_sessions
            .contains_key(&(pane, Provider::Codex)));
        app.learn_excluded_sessions
            .insert((pane, Provider::Codex), "*".into());
        app.handle_pane_report_agent_session(
            "unknown-cron".into(),
            report(12, "cron-session", "/tmp/cron-session.jsonl"),
        );
        assert_eq!(
            app.learn_excluded_sessions.get(&(pane, Provider::Codex)),
            Some(&"cron-session".to_string())
        );
        app.handle_pane_report_agent_session(
            "after-cron".into(),
            report(13, "user-session", "/tmp/user-session.jsonl"),
        );
        assert!(!app
            .learn_excluded_sessions
            .contains_key(&(pane, Provider::Codex)));
    }

    #[test]
    fn codex_review_requires_observed_process_exit_not_idle() {
        let (mut app, pane_id) = app_with_claude_session();
        let pane = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].tabs[0].panes[&pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .set_detected_state(Some(Agent::Codex), AgentState::Working);
        app.handle_pane_report_agent_session(
            "codex".into(),
            PaneReportAgentSessionParams {
                pane_id,
                source: "mercury:codex".into(),
                agent: "codex".into(),
                seq: Some(10),
                agent_session_id: Some("codex-session".into()),
                agent_session_path: Some("/tmp/codex-session.jsonl".into()),
                session_start_source: Some("startup".into()),
                resume_argv: None,
            },
        );
        struct TestRoot(std::path::PathBuf);
        impl Drop for TestRoot {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }
        let root = TestRoot(std::env::temp_dir().canonicalize().unwrap().join(format!(
            "mercury-codex-exit-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        )));
        std::fs::create_dir(&root.0).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root.0, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        app.learn = Some(
            crate::app::learn::LearnWorker::start(
                &root.0.join("learn"),
                true,
                move |trigger, _| {
                    sender.send(trigger.key().to_owned()).unwrap();
                    true
                },
            )
            .unwrap(),
        );
        app.learn_config.enabled = true;
        let idle = crate::events::AppEvent::StateChanged {
            pane_id: pane,
            agent: Some(Agent::Codex),
            state: AgentState::Idle,
            visible_blocker: false,
            visible_working: false,
            process_exited: false,
            observed_at: std::time::Instant::now(),
        };
        app.handle_internal_event(idle);
        assert!(app.learn_transcripts.contains_key(&(pane, Provider::Codex)));
        assert!(receiver.try_recv().is_err());
        app.handle_internal_event(crate::events::AppEvent::StateChanged {
            pane_id: pane,
            agent: Some(Agent::Codex),
            state: AgentState::Idle,
            visible_blocker: false,
            visible_working: false,
            process_exited: true,
            observed_at: std::time::Instant::now(),
        });
        assert_eq!(
            receiver
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            "codex:codex-session:end"
        );
        assert!(!app.learn_transcripts.contains_key(&(pane, Provider::Codex)));
        app.learn.take().unwrap().shutdown();
    }

    #[test]
    fn codex_idle_probes_exact_turn_and_cron_exclusion_blocks_session_end() {
        let (mut app, pane_id) = app_with_claude_session();
        let pane = app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.state.workspaces[0].tabs[0].panes[&pane]
            .attached_terminal_id
            .clone();
        app.state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .set_detected_state(Some(Agent::Codex), AgentState::Working);
        struct TestRoot(std::path::PathBuf);
        impl Drop for TestRoot {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }
        let root = TestRoot(std::env::temp_dir().canonicalize().unwrap().join(format!(
            "mercury-codex-turn-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        )));
        std::fs::create_dir(&root.0).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root.0, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let transcript = root.0.join("rollout.jsonl");
        std::fs::write(
            &transcript,
            "{\"type\":\"session_meta\",\"payload\":{\"session_id\":\"session-1\"}}\n\
             {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"u1\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hello\"}]}}\n\
             {\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"a1\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"done\"}]}}\n",
        )
        .unwrap();
        app.handle_pane_report_agent_session(
            "codex".into(),
            PaneReportAgentSessionParams {
                pane_id,
                source: "mercury:codex".into(),
                agent: "codex".into(),
                seq: Some(10),
                agent_session_id: Some("session-1".into()),
                agent_session_path: Some(transcript.display().to_string()),
                session_start_source: Some("startup".into()),
                resume_argv: None,
            },
        );
        let (sender, receiver) = std::sync::mpsc::channel();
        app.learn = Some(
            crate::app::learn::LearnWorker::start(
                &root.0.join("learn"),
                true,
                move |trigger, _| {
                    sender.send(trigger.key().to_owned()).unwrap();
                    true
                },
            )
            .unwrap(),
        );
        app.learn_config.enabled = true;
        app.learn_config.review_every_turns = 1;
        app.handle_internal_event(crate::events::AppEvent::StateChanged {
            pane_id: pane,
            agent: Some(Agent::Codex),
            state: AgentState::Idle,
            visible_blocker: false,
            visible_working: false,
            process_exited: false,
            observed_at: std::time::Instant::now(),
        });
        assert_eq!(
            receiver
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            "codex:session-1:turn:1"
        );
        app.learn_excluded_sessions
            .insert((pane, Provider::Codex), "session-1".into());
        app.handle_internal_event(crate::events::AppEvent::StateChanged {
            pane_id: pane,
            agent: Some(Agent::Codex),
            state: AgentState::Idle,
            visible_blocker: false,
            visible_working: false,
            process_exited: true,
            observed_at: std::time::Instant::now(),
        });
        app.learn.take().unwrap().shutdown();
        assert!(receiver.try_recv().is_err());
        assert!(!app.learn_transcripts.contains_key(&(pane, Provider::Codex)));
    }
}
