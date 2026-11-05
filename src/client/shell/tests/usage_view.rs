use super::*;
use crate::protocol::{
    ClientShellAgentUsage, ClientShellBudgetLevel, ClientShellBudgetUnit,
    ClientShellProviderCredits, ClientShellProviderLimit, ClientShellProviderUsage,
    ClientShellUsageBudget, ClientShellUsageSummary,
};

fn usage_agent(
    pane_id: &str,
    model: Option<&str>,
    cost_micros_usd: Option<u64>,
) -> ClientShellAgent {
    ClientShellAgent {
        pane_id: pane_id.into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        name: Some("mercury".into()),
        display_agent: Some("mercury".into()),
        agent: Some("claude".into()),
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: crate::api::schema::AgentStatus::Working,
        state_change_seq: 1,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused: true,
        usage: Some(ClientShellAgentUsage {
            model: model.map(|model| model.to_string()),
            context_permille: Some(431),
            context_exact: true,
            context_window: Some(200_000),
            tokens_in: 1_200,
            tokens_out: 22_000,
            tokens_cache_read: 38_800,
            tokens_cache_write: 0,
            cost_micros_usd,
            updated_at_unix_ms: Some(0),
        }),
    }
}

fn frame_text(frame: &FrameData) -> String {
    frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>()
}

fn prepared_state(
    agents: Vec<ClientShellAgent>,
    usage: Option<ClientShellUsageSummary>,
) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut projected = snapshot();
    projected.agents = agents;
    projected.usage = usage;
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state
}

#[test]
fn prefix_u_opens_usage_view_over_pane_surface_and_clears_pane_hits() {
    let mut state = prepared_state(
        vec![usage_agent(
            "pane_1",
            Some("claude-opus-5.5"),
            Some(3_410_000),
        )],
        None,
    );

    let terminal_frame = state.compose(106, 30).expect("terminal frame");
    assert!(frame_text(&terminal_frame).contains("LIVE"));
    assert!(
        !state.hits.panes.is_empty(),
        "pane hit should exist in Terminal view"
    );

    let toggled = state.handle_input_bytes(&[0x02]);
    assert!(toggled.actions.is_empty());
    let opened = state.handle_input_bytes(b"u");
    assert!(opened.repaint);
    assert_eq!(state.view, ClientShellView::Usage);

    let usage_frame = state.compose(106, 30).expect("usage frame");
    let text = frame_text(&usage_frame);
    assert!(text.contains("Usage"), "missing header: {text}");
    assert!(text.contains("Provedor"));
    assert!(text.contains("Orçamentos"));
    assert!(text.contains("Agentes"));
    assert!(
        text.contains("client-shell"),
        "sidebar must still render: {text}"
    );
    assert!(
        !text.contains("LIVE"),
        "the pane surface must be replaced by the Usage panel"
    );
    assert!(
        state.hits.panes.is_empty(),
        "pane hits must be cleared while the Usage view is open"
    );
    assert!(state.hits.pane_splits.is_empty());
    assert!(state.hits.popup.is_none());

    // prefix+u again toggles back to Terminal and restores pane hits.
    state.handle_input_bytes(&[0x02]);
    let closed = state.handle_input_bytes(b"u");
    assert!(closed.repaint);
    assert_eq!(state.view, ClientShellView::Terminal);
    let restored = state.compose(106, 30).expect("restored terminal frame");
    assert!(frame_text(&restored).contains("LIVE"));
    assert!(!state.hits.panes.is_empty());
}

#[test]
fn esc_and_q_close_the_usage_view() {
    for close_bytes in [&b"\x1b"[..], &b"q"[..]] {
        let mut state = prepared_state(Vec::new(), None);
        state.compose(106, 30).expect("terminal frame");
        state.handle_input_bytes(&[0x02]);
        state.handle_input_bytes(b"u");
        assert_eq!(state.view, ClientShellView::Usage);

        let closed = state.handle_input_bytes(close_bytes);
        assert!(closed.repaint);
        assert_eq!(state.view, ClientShellView::Terminal);
    }
}

#[test]
fn clicking_the_tab_bar_chip_toggles_the_usage_view() {
    let mut state = prepared_state(Vec::new(), None);
    state.compose(106, 30).expect("terminal frame");
    let chip = state.hits.usage_chip;
    assert!(!chip.is_empty(), "usage chip should be hit-testable");

    let click = || {
        crate::raw_input::RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: chip.x,
            row: chip.y,
            modifiers: crossterm::event::KeyModifiers::empty(),
        })
    };
    let opened = state.handle_raw_events(vec![click()]);
    assert!(opened.repaint);
    assert_eq!(state.view, ClientShellView::Usage);

    state.compose(106, 30).expect("usage frame");
    let closed = state.handle_raw_events(vec![click()]);
    assert!(closed.repaint);
    assert_eq!(state.view, ClientShellView::Terminal);
}

#[test]
fn provider_without_reported_limits_renders_na_never_a_fabricated_number() {
    let usage = ClientShellUsageSummary {
        activity: None,
        providers: vec![ClientShellProviderUsage {
            status: "atualizado".into(),
            detail: String::new(),
            observed_at_unix_ms: None,
            provider: "claude".into(),
            limits: Vec::new(),
            credits: None,
            plan_type: None,
        }],
        budgets: Vec::new(),
    };
    let mut state = prepared_state(Vec::new(), Some(usage));
    state.compose(106, 30).expect("terminal frame");
    state.handle_input_bytes(&[0x02]);
    state.handle_input_bytes(b"u");

    let frame = state.compose(106, 30).expect("usage frame");
    let text = frame_text(&frame);
    assert!(text.contains("claude"), "missing provider row: {text}");
    assert!(
        text.contains("Aguardando limites da conta"),
        "provider without limits must explain its state: {text}"
    );
    assert!(
        text.contains("nenhum orçamento configurado ainda"),
        "budgets section must show its empty state: {text}"
    );
}

#[test]
fn provider_with_reported_limit_renders_real_percentage_and_credits() {
    let usage = ClientShellUsageSummary {
        activity: None,
        providers: vec![ClientShellProviderUsage {
            status: "atualizado".into(),
            detail: String::new(),
            observed_at_unix_ms: None,
            provider: "codex".into(),
            limits: vec![ClientShellProviderLimit {
                key: "primary".into(),
                used_permille: 420,
                window_minutes: Some(300),
                resets_at_unix_ms: None,
            }],
            credits: Some(ClientShellProviderCredits {
                has_credits: true,
                unlimited: false,
                balance: None,
            }),
            plan_type: Some("business".into()),
        }],
        budgets: Vec::new(),
    };
    let mut state = prepared_state(Vec::new(), Some(usage));
    state.compose(106, 30).expect("terminal frame");
    state.handle_input_bytes(&[0x02]);
    state.handle_input_bytes(b"u");

    let frame = state.compose(106, 30).expect("usage frame");
    let text = frame_text(&frame);
    assert!(text.contains("codex"));
    assert!(text.contains("primary"));
    assert!(
        text.contains("42%"),
        "used_permille=420 must render as 42%: {text}"
    );
    assert!(
        text.contains("5h"),
        "window_minutes=300 must render as 5h: {text}"
    );
    assert!(text.contains("plano business"));
}

#[test]
fn business_individual_quota_shows_remaining_and_used_without_any_agent_pane() {
    let usage = ClientShellUsageSummary {
        activity: None,
        budgets: vec![],
        providers: vec![ClientShellProviderUsage {
            provider: "codex".into(),
            status: "atualizado".into(),
            detail: "Conta conectada".into(),
            observed_at_unix_ms: None,
            credits: None,
            plan_type: Some("business".into()),
            limits: vec![ClientShellProviderLimit {
                key: "individual".into(),
                used_permille: 770,
                window_minutes: None,
                resets_at_unix_ms: None,
            }],
        }],
    };
    let mut state = prepared_state(Vec::new(), Some(usage));
    state.compose(106, 30).unwrap();
    state.handle_input_bytes(&[0x02]);
    state.handle_input_bytes(b"u");
    let text = frame_text(&state.compose(106, 30).unwrap());
    assert!(text.contains("23% restante"));
    assert!(text.contains("77% utilizado"));
    assert_eq!(state.usage_view_scroll, 0);
}

#[test]
fn evaluated_budgets_render_gauge_amounts_and_partial_badge() {
    let usage = ClientShellUsageSummary {
        activity: None,
        providers: Vec::new(),
        budgets: vec![
            ClientShellUsageBudget {
                name: "claude-mensal".into(),
                scope: "provider: claude".into(),
                level: ClientShellBudgetLevel::Warn,
                used_permille: Some(750),
                used_scaled: 250_000_000,
                limit_scaled: 1_000_000_000,
                unit: ClientShellBudgetUnit::Usd,
                warn_threshold: Some(70),
                resets_at_unix_ms: Some(0),
                forecast_exhausted_at_unix_ms: Some(0),
                partial: true,
            },
            ClientShellUsageBudget {
                name: "reviewer-3h".into(),
                scope: "agent: reviewer".into(),
                level: ClientShellBudgetLevel::Exceeded,
                used_permille: Some(1000),
                used_scaled: 2_000_000,
                limit_scaled: 2_000_000,
                unit: ClientShellBudgetUnit::Tokens,
                warn_threshold: Some(90),
                resets_at_unix_ms: None,
                forecast_exhausted_at_unix_ms: None,
                partial: false,
            },
        ],
    };
    let mut state = prepared_state(Vec::new(), Some(usage));
    state.compose(160, 30).expect("terminal frame");
    state.handle_input_bytes(&[0x02]);
    state.handle_input_bytes(b"u");

    let frame = state.compose(160, 30).expect("usage frame");
    let text = frame_text(&frame);
    assert!(text.contains("claude-mensal"), "{text}");
    assert!(text.contains("$250.00"), "{text}");
    assert!(text.contains("$1000.00"), "{text}");
    assert!(text.contains("75%"), "{text}");
    assert!(text.contains("parcial"), "missing partial badge: {text}");
    assert!(text.contains("reviewer-3h"), "{text}");
    assert!(
        text.contains("2M"),
        "token amount must be humanized: {text}"
    );
    assert!(
        !text.contains("nenhum orçamento configurado ainda"),
        "budgets must replace the empty state once evaluated: {text}"
    );
}

#[test]
fn agent_row_shows_detail_and_enter_focuses_the_pane() {
    let mut state = prepared_state(
        vec![usage_agent(
            "pane_1",
            Some("claude-opus-5.5"),
            Some(3_410_000),
        )],
        None,
    );
    state.compose(106, 30).expect("terminal frame");
    state.handle_input_bytes(&[0x02]);
    state.handle_input_bytes(b"u");
    let frame = state.compose(106, 30).expect("usage frame");
    let text = frame_text(&frame);
    assert!(text.contains("mercury"), "missing agent label: {text}");
    assert!(
        text.contains("opus-5.5"),
        "missing model (claude- stripped): {text}"
    );
    assert!(text.contains("43%"), "missing context gauge pct: {text}");
    assert!(text.contains("$3.41"), "missing cost: {text}");
    assert_eq!(
        state.hits.usage_cursor_pane_id.as_deref(),
        Some("pane_1"),
        "the sole agent row should be the cursor by default"
    );

    let enter = state.handle_input_bytes(b"\r");
    assert!(enter.actions.iter().any(|action| matches!(
        action,
        ClientShellAction::Endpoint { request, .. }
            if matches!(
                &request.method,
                crate::api::schema::Method::PaneFocus(params)
                    if params.pane_id == "pane_1"
            )
    )));
    assert_eq!(
        state.view,
        ClientShellView::Terminal,
        "Enter must focus the pane and leave the Usage view"
    );
}

#[test]
fn agent_without_usage_is_omitted_and_empty_state_shown_when_none_reported() {
    let mut state = prepared_state(vec![usage_agent("pane_1", None, None)], None);
    // Strip usage entirely for this pane to exercise the "no agent reported usage" state.
    state.snapshot.as_mut().unwrap().agents[0].usage = None;
    state.compose(106, 30).expect("terminal frame");
    state.handle_input_bytes(&[0x02]);
    state.handle_input_bytes(b"u");

    let frame = state.compose(106, 30).expect("usage frame");
    let text = frame_text(&frame);
    assert!(text.contains("nenhum agente com usage reportado"), "{text}");
    assert!(state.hits.usage_cursor_pane_id.is_none());
}
