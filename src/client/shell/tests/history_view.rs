use super::*;
use crate::api::schema::{HistoryHitInfo, HistoryResumeInfo, Method, ResponseResult};

fn prepared() -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state
}

fn request(actions: &[ClientShellAction]) -> &crate::api::schema::Request {
    let [ClientShellAction::Endpoint { request, .. }] = actions else {
        panic!("expected one endpoint request: {actions:?}");
    };
    request
}

fn hit() -> HistoryHitInfo {
    HistoryHitInfo {
        provider: "codex".into(),
        session_id: "session-1".into(),
        cwd: Some("/project".into()),
        timestamp: "2020-01-01T00:00:00Z".into(),
        snippet: "old conversation".into(),
    }
}

fn opened() -> ClientShellState {
    let mut state = prepared();
    state.compose(110, 30).unwrap();
    let mut outcome = ClientShellInput::default();
    state.toggle_history_view(&mut outcome);
    assert!(
        matches!(&request(&outcome.actions).method, Method::HistorySearch(params) if params.query.is_empty())
    );
    let id = request(&outcome.actions).id.clone();
    state.handle_endpoint_result(
        "boot-1",
        &id,
        Ok(ResponseResult::HistorySearch { hits: vec![hit()] }),
    );
    state
}

#[test]
fn history_tab_search_filters_and_rendering() {
    let mut state = opened();
    assert_eq!(state.view, ClientShellView::History);
    let frame = state.compose(110, 30).unwrap();
    let text = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("History"));
    assert!(text.contains("old conversation"));
    assert!(!text.contains("LIVE"));
    assert!(state.hits.panes.is_empty());
    assert!(!state.hits.history_chip.is_empty());
    let filter = state.handle_input_bytes(b"p");
    assert!(
        matches!(&request(&filter.actions).method, Method::HistorySearch(params) if params.provider.as_deref() == Some("claude"))
    );
    let id = request(&filter.actions).id.clone();
    state.handle_endpoint_result(
        "boot-1",
        &id,
        Ok(ResponseResult::HistorySearch { hits: vec![hit()] }),
    );
    let project = state.handle_input_bytes(b"w");
    assert!(
        matches!(&request(&project.actions).method, Method::HistorySearch(params) if params.cwd.as_deref() == Some("/project"))
    );
}

#[test]
fn history_row_keeps_search_snippet_visible_with_long_project_path() {
    let mut state = opened();
    state.history_view.hits[0].cwd =
        Some("/private/tmp/a-very-long-fixture-project-directory/with/deep/nested/path".into());
    state.history_view.hits[0].snippet = "needle-visible-in-row".into();

    let frame = state.compose(80, 24).unwrap();
    let text = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("needle-visible-in-row"));
}

#[test]
fn reopen_resolves_starts_native_command_and_returns_to_terminal_on_success() {
    let mut state = opened();
    let open = state.handle_input_bytes(b"\r");
    assert!(
        matches!(&request(&open.actions).method, Method::HistoryResolve(params) if params.provider == "codex" && params.session_id == "session-1")
    );
    let id = request(&open.actions).id.clone();
    let (_, actions) = state.handle_endpoint_result(
        "boot-1",
        &id,
        Ok(ResponseResult::HistoryResolve {
            target: Some(HistoryResumeInfo {
                provider: "codex".into(),
                session_id: "session-1".into(),
                cwd: "/project".into(),
            }),
        }),
    );
    assert!(
        matches!(&request(&actions).method, Method::PaneSplit(params) if params.cwd.as_deref() == Some("/project"))
    );
    let id = request(&actions).id.clone();
    let pane = crate::api::schema::PaneInfo {
        pane_id: "new-pane".into(),
        terminal_id: "new-terminal".into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        focused: true,
        cwd: Some("/project".into()),
        foreground_cwd: None,
        restore_error: None,
        label: None,
        agent: None,
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        display_agent: None,
        agent_status: crate::api::schema::AgentStatus::Idle,
        state_labels: HashMap::new(),
        tokens: HashMap::new(),
        agent_session: None,
        scroll: None,
        revision: 1,
    };
    let (_, actions) =
        state.handle_endpoint_result("boot-1", &id, Ok(ResponseResult::PaneInfo { pane }));
    assert!(
        matches!(&request(&actions).method, Method::AgentStart(params) if params.pane_id == "new-pane" && params.kind == "codex" && params.args == ["resume", "session-1"])
    );
    let id = request(&actions).id.clone();
    let agent = serde_json::from_value(serde_json::json!({
        "terminal_id": "new-terminal",
        "agent_status": "idle",
        "workspace_id": "ws_1",
        "tab_id": "tab_1",
        "pane_id": "new-pane",
        "focused": true,
        "revision": 1
    }))
    .unwrap();
    let (_, actions) = state.handle_endpoint_result(
        "boot-1",
        &id,
        Ok(ResponseResult::AgentStarted {
            agent,
            argv: vec!["codex".into(), "resume".into(), "session-1".into()],
        }),
    );
    assert!(actions.is_empty());
    assert_eq!(state.view, ClientShellView::Terminal);
}

#[test]
fn absent_session_does_not_create_pane() {
    let mut state = opened();
    let open = state.handle_input_bytes(b"\r");
    let id = request(&open.actions).id.clone();
    let (_, actions) = state.handle_endpoint_result(
        "boot-1",
        &id,
        Ok(ResponseResult::HistoryResolve { target: None }),
    );
    assert!(actions.is_empty());
    assert!(state
        .endpoint_error
        .as_deref()
        .is_some_and(|error| error.contains("indisponível")));
}

#[test]
fn query_escape_restores_previous_search_without_request() {
    let mut state = opened();
    let start = state.handle_input_bytes(b"/");
    assert!(start.actions.is_empty());
    state.handle_input_bytes(b"draft");
    assert_eq!(state.history_view.query, "draft");
    let cancel = state.handle_input_bytes(b"\x1b");
    assert!(cancel.actions.is_empty());
    assert!(state.history_view.query.is_empty());
}

#[test]
fn row_click_selects_without_starting_agent() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
    let mut state = opened();
    state.history_view.hits.push(HistoryHitInfo {
        provider: "claude".into(),
        session_id: "session-2".into(),
        cwd: Some("/other".into()),
        timestamp: "2020-01-02T00:00:00Z".into(),
        snippet: "another conversation".into(),
    });
    state.compose(110, 30).unwrap();
    let row = state.hits.history_rows[1].0;
    let click = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: row.x + 1,
        row: row.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(click.actions.is_empty());
    assert_eq!(state.history_view.selected, 1);
    let open = state.handle_input_bytes(b"\r");
    assert!(
        matches!(&request(&open.actions).method, Method::HistoryResolve(params) if params.provider == "claude" && params.session_id == "session-2")
    );
}

#[test]
fn busy_new_pane_retries_resume_on_tick_without_blocking_input() {
    let mut state = opened();
    let open = state.handle_input_bytes(b"\r");
    let id = request(&open.actions).id.clone();
    let (_, actions) = state.handle_endpoint_result(
        "boot-1",
        &id,
        Ok(ResponseResult::HistoryResolve {
            target: Some(HistoryResumeInfo {
                provider: "codex".into(),
                session_id: "session-1".into(),
                cwd: "/project".into(),
            }),
        }),
    );
    let id = request(&actions).id.clone();
    let pane = crate::api::schema::PaneInfo {
        pane_id: "new-pane".into(),
        terminal_id: "new-terminal".into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        focused: true,
        cwd: Some("/project".into()),
        foreground_cwd: None,
        restore_error: None,
        label: None,
        agent: None,
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        display_agent: None,
        agent_status: crate::api::schema::AgentStatus::Idle,
        state_labels: HashMap::new(),
        tokens: HashMap::new(),
        agent_session: None,
        scroll: None,
        revision: 1,
    };
    let (_, actions) =
        state.handle_endpoint_result("boot-1", &id, Ok(ResponseResult::PaneInfo { pane }));
    let id = request(&actions).id.clone();
    let (_, actions) = state.handle_endpoint_result(
        "boot-1",
        &id,
        Err(ClientShellEndpointError {
            code: Some("agent_pane_busy".into()),
            message: "not ready".into(),
        }),
    );
    assert!(actions.is_empty());
    assert!(state.visible_endpoint_notice.is_none());
    let deadline = state.history_resume_retry.as_ref().unwrap().at;
    assert!(state
        .tick_history_resume(deadline - std::time::Duration::from_millis(1))
        .actions
        .is_empty());
    let retry = state.tick_history_resume(deadline);
    assert!(
        matches!(&request(&retry.actions).method, Method::AgentStart(params) if params.pane_id == "new-pane" && params.args == ["resume", "session-1"])
    );
}
