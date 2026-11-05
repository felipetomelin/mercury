use super::*;
use crate::api::schema::{
    LearnChangeCounts, LearnChangeDetail, LearnChangeInfo, LearnComparisonInfo, LearnDiffInfo,
    LearnDiffSummary, LearnObservedMetricInfo, LearnStatusInfo, LearnTriggerCounts,
    LearnTriggerInfo, Method, ResponseResult,
};

fn prepared() -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_endpoint_methods(Some(
        crate::server::client_commands::supported_client_shell_method_names()
            .iter()
            .map(|method| (*method).to_owned())
            .collect(),
    ));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(110, 30).unwrap();
    state
}

fn requests(outcome: &ClientShellInput) -> Vec<&crate::api::schema::Request> {
    outcome
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request.as_ref()),
            _ => None,
        })
        .collect()
}

fn change(status: &str) -> LearnChangeInfo {
    LearnChangeInfo {
        id: "change-1".into(),
        target_id: "instructions".into(),
        status: status.into(),
        created_at_ms: 1,
    }
}

fn opened() -> ClientShellState {
    let mut state = prepared();
    state.handle_input_bytes(&[0x02]);
    let outcome = state.handle_input_bytes(b"U");
    let requests = requests(&outcome);
    assert_eq!(requests.len(), 2);
    assert!(matches!(&requests[0].method, Method::LearnList(_)));
    assert!(matches!(&requests[1].method, Method::LearnStatus(_)));
    state.handle_endpoint_result(
        "boot-1",
        &requests[0].id,
        Ok(ResponseResult::LearnList {
            changes: vec![change("applied")],
        }),
    );
    state.handle_endpoint_result(
        "boot-1",
        &requests[1].id,
        Ok(ResponseResult::LearnStatus {
            status: LearnStatusInfo {
                runtime: None,
                ledger_present: true,
                changes: LearnChangeCounts {
                    prepared: 0,
                    applied: 1,
                    undo_prepared: 0,
                    conflict: 0,
                    failed: 0,
                    undone: 0,
                },
                triggers: LearnTriggerCounts {
                    claimed: 0,
                    succeeded: 1,
                    failed: 0,
                },
                recent_triggers: Vec::new(),
                recent_metrics: Vec::new(),
            },
        }),
    );
    state
}

#[test]
fn learn_tab_reports_unavailable_instead_of_loading_forever_on_old_server() {
    let mut state = prepared();
    state.handle_input_bytes(&[0x02]);
    let outcome = state.handle_input_bytes(b"U");
    let request_ids: Vec<_> = requests(&outcome)
        .iter()
        .map(|request| request.id.clone())
        .collect();
    assert_eq!(request_ids.len(), 2);
    for id in request_ids {
        state.handle_endpoint_result(
            "boot-1",
            &id,
            Err(ClientShellEndpointError {
                code: Some("invalid_request".into()),
                message: "unknown method on old server".into(),
            }),
        );
    }

    assert!(!state.learn_view.loading);
    let frame = state.compose(110, 30).unwrap();
    let text = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("Learn indisponível"), "{text}");
    assert!(!text.contains("carregando"), "{text}");
}

#[test]
fn learn_tab_reports_unavailable_when_old_server_does_not_advertise_methods() {
    let mut state = prepared();
    state.set_endpoint_methods(Some(Vec::new()));
    state.handle_input_bytes(&[0x02]);
    let outcome = state.handle_input_bytes(b"U");

    assert!(requests(&outcome).is_empty());
    assert!(!state.learn_view.loading);
    assert!(state.learn_view.load_error);
    let frame = state.compose(110, 30).unwrap();
    let text = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("Learn indisponível"), "{text}");
    assert!(!text.contains("carregando"), "{text}");
}

#[test]
fn learn_tab_lists_redacted_metadata_and_hides_terminal_surface() {
    let mut state = opened();
    assert_eq!(state.view, ClientShellView::Learn);
    let frame = state.compose(110, 30).unwrap();
    let text = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("Learn"));
    assert!(text.contains("instructions"));
    assert!(text.contains("aplicadas 1"));
    assert!(!text.contains("LIVE"));
    assert!(state.hits.panes.is_empty());
    assert!(!state.hits.learn_chip.is_empty());
    assert_eq!(state.hits.learn_rows.len(), 1);
}

#[test]
fn detail_renders_structural_diff_and_undo_requires_second_u() {
    let mut state = opened();
    let open = state.handle_input_bytes(b"\r");
    let open_requests = requests(&open);
    assert_eq!(open_requests.len(), 2);
    assert!(matches!(&open_requests[0].method, Method::LearnShow(_)));
    assert!(matches!(&open_requests[1].method, Method::LearnDiff(_)));
    state.handle_endpoint_result(
        "boot-1",
        &open_requests[0].id,
        Ok(ResponseResult::LearnShow {
            detail: LearnChangeDetail {
                change: change("applied"),
                before_hash: "before-hash".into(),
                after_hash: "after-hash".into(),
                reason_redacted: true,
                backup_available: Some(true),
            },
        }),
    );
    state.handle_endpoint_result(
        "boot-1",
        &open_requests[1].id,
        Ok(ResponseResult::LearnDiff {
            diff: LearnDiffInfo {
                change: change("applied"),
                summary: Some(LearnDiffSummary {
                    before_lines: 1,
                    after_lines: 1,
                    unchanged_prefix_lines: 0,
                    unchanged_suffix_lines: 0,
                    removed_lines: 1,
                    added_lines: 1,
                }),
                content_redacted: true,
            },
        }),
    );
    let frame = state.compose(110, 30).unwrap();
    let text = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("Hash anterior"));
    assert!(text.contains("Backup privado: verificado"));
    assert!(text.contains("Diff: -1 +1"));
    assert!(!text.contains("private reason"));
    state.learn_view.detail.as_mut().unwrap().backup_available = None;
    let older_text = state
        .compose(110, 30)
        .unwrap()
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(older_text.contains("Backup privado: não informado"));

    let first = state.handle_input_bytes(b"u");
    assert!(requests(&first).is_empty());
    assert_eq!(
        state.learn_view.confirm_undo_id.as_deref(),
        Some("change-1")
    );
    let second = state.handle_input_bytes(b"u");
    let sent = requests(&second);
    assert_eq!(sent.len(), 1);
    assert!(matches!(&sent[0].method, Method::LearnUndo(_)));
    assert!(state.learn_view.confirm_undo_id.is_none());
    let (_, next) = state.handle_endpoint_result(
        "boot-1",
        &sent[0].id,
        Ok(ResponseResult::LearnUndo {
            change: change("undone"),
        }),
    );
    assert_eq!(state.learn_view.changes[0].status, "undone");
    assert!(next.iter().any(|action| matches!(action,
        ClientShellAction::Endpoint { request, .. } if matches!(&request.method, Method::LearnList(_))
    )));
    let repeat = state.handle_input_bytes(b"u");
    assert!(requests(&repeat).is_empty());
}

#[test]
fn trigger_audit_shows_fingerprints_and_returns_to_changes() {
    let mut state = opened();
    state.learn_view.status.as_mut().unwrap().recent_triggers = vec![
        LearnTriggerInfo {
            fingerprint: "0123456789abcdef".into(),
            provider: "claude".into(),
            kind: "session_end".into(),
            status: "succeeded".into(),
            created_at_ms: 10,
            finished_at_ms: Some(11),
        },
        LearnTriggerInfo {
            fingerprint: "fedcba9876543210".into(),
            provider: "codex".into(),
            kind: "turn_milestone".into(),
            status: "failed".into(),
            created_at_ms: 12,
            finished_at_ms: Some(13),
        },
    ];

    state.handle_input_bytes(b"t");
    assert_eq!(state.learn_view.page, LearnPage::Triggers);
    let frame = state.compose(110, 30).unwrap();
    let text = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("session_end"));
    assert!(text.contains("0123456789abcdef"));
    assert!(!text.contains("session-id"));
    state.handle_input_bytes(b"j");
    assert_eq!(state.learn_view.selected_trigger, 1);
    state.handle_input_bytes(b"\x1b");
    assert_eq!(state.learn_view.page, LearnPage::List);
}

#[test]
fn metrics_page_shows_observed_tokens_without_claiming_improvement() {
    let mut state = opened();
    state.learn_view.status.as_mut().unwrap().recent_metrics = vec![LearnObservedMetricInfo {
        task_key: "cron:12345678-aaaa".into(),
        provider: "claude".into(),
        input_tokens: 120,
        output_tokens: 30,
        cost_microusd: Some(125),
        outcome: Some("succeeded".into()),
        observed_at_ms: 1_760_000_000_000,
        comparison: None,
    }];
    state.handle_input_bytes(b"m");
    assert_eq!(state.learn_view.page, LearnPage::Metrics);
    let text = state
        .compose(110, 30)
        .unwrap()
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("120→30 tokens"));
    assert!(text.contains("$0.000125"));
    assert!(text.contains("dados insuficientes"));
    assert!(!text.contains("private prompt"));
    let narrow_text = state
        .compose(80, 24)
        .unwrap()
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(narrow_text.contains("120→30 tokens"));
    assert!(narrow_text.contains("$0.000125"));
    assert!(narrow_text.contains("ok"));
    state.learn_view.status.as_mut().unwrap().recent_metrics[0].comparison =
        Some(LearnComparisonInfo {
            baseline_runs: 2,
            after_runs: 2,
            baseline_average_tokens: 150,
            after_average_tokens: 80,
            baseline_succeeded: 2,
            after_succeeded: 2,
        });
    let compared = state
        .compose(110, 30)
        .unwrap()
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(compared.contains("Antes/depois descritivo"));
    assert!(compared.contains("150→80"));
    state.handle_input_bytes(b"m");
    assert_eq!(state.learn_view.page, LearnPage::List);
}

#[test]
fn cancelling_confirmation_never_sends_undo() {
    let mut state = opened();
    state.handle_input_bytes(b"\r");
    state.handle_input_bytes(b"u");
    let cancel = state.handle_input_bytes(b"\x1b");
    assert!(requests(&cancel).is_empty());
    assert!(state.learn_view.confirm_undo_id.is_none());
    assert_eq!(state.learn_view.page, LearnPage::Detail);
}

#[test]
fn undo_confirmation_stays_visible_on_short_terminal() {
    let mut state = opened();
    state.handle_input_bytes(b"\r");
    state.handle_input_bytes(b"u");
    let text = state
        .compose(80, 12)
        .unwrap()
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("Confirmar undo"), "{text}");
}

#[test]
fn refresh_returns_to_list_when_open_item_disappears() {
    let mut state = opened();
    state.handle_input_bytes(b"\r");
    let refresh = state.handle_input_bytes(b"r");
    let list = requests(&refresh)
        .into_iter()
        .find(|request| matches!(&request.method, Method::LearnList(_)))
        .expect("Learn list refresh");
    state.handle_endpoint_result(
        "boot-1",
        &list.id,
        Ok(ResponseResult::LearnList {
            changes: Vec::new(),
        }),
    );
    assert_eq!(state.learn_view.page, LearnPage::List);
    assert!(state.learn_view.detail.is_none());
    assert!(state.learn_view.diff.is_none());
}
