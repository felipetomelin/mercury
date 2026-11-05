use super::*;
use crate::api::schema::{CronJobDefinition, CronJobInfo, CronRunInfo, Method, ResponseResult};

fn prepared() -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state
}

fn endpoint_request(outcome: &ClientShellInput) -> &crate::api::schema::Request {
    let [ClientShellAction::Endpoint { request, .. }] = outcome.actions.as_slice() else {
        panic!("expected one endpoint request, got {:?}", outcome.actions);
    };
    request
}

fn job() -> CronJobInfo {
    CronJobInfo {
        id: "job-1".into(),
        name: "Daily check".into(),
        schedule: "every 1h".into(),
        next_run_at: Some(1_791_273_600),
        enabled: true,
        target_kind: "script".into(),
        repeat_completed: None,
        repeat_times: None,
        last_status: Some("success".into()),
    }
}

fn definition() -> CronJobDefinition {
    CronJobDefinition {
        name: "Daily check".into(),
        schedule: "every 1h".into(),
        utc_offset: Some("+00:00".into()),
        target: mercury_cron::JobTarget::Script {
            program: "/bin/echo".into(),
            args: vec!["ok".into()],
        },
        workdir: None,
        timeout_seconds: Some(60),
        enabled: Some(true),
        catch_up: mercury_cron::CatchUp::Skip,
        repeat_times: None,
        deliver: Vec::new(),
    }
}

fn load_job(state: &mut ClientShellState) {
    state.compose(110, 30).expect("terminal frame");
    state.handle_input_bytes(&[0x02]);
    let opened = state.handle_input_bytes(b"C");
    let request = endpoint_request(&opened);
    assert!(matches!(request.method, Method::CronList(_)));
    let id = request.id.clone();
    let (repaint, actions) = state.handle_endpoint_result(
        "boot-1",
        &id,
        Ok(ResponseResult::CronList { jobs: vec![job()] }),
    );
    assert!(repaint);
    assert!(actions.is_empty());
}

#[test]
fn crons_tab_uses_endpoint_api_and_hides_pane_surface() {
    let mut state = prepared();
    load_job(&mut state);
    assert_eq!(state.view, ClientShellView::Crons);
    let frame = state.compose(110, 30).expect("crons frame");
    let text = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("Crons"));
    assert!(text.contains("Daily check"));
    assert!(text.contains("success"));
    assert!(!text.contains("LIVE"));
    assert!(state.hits.panes.is_empty());
    assert!(!state.hits.crons_chip.is_empty());
    let runs = state.handle_input_bytes(b"\r");
    assert_eq!(state.crons_view.page, CronsPage::Runs);
    let requests: Vec<_> = runs
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request),
            _ => None,
        })
        .collect();
    assert_eq!(requests.len(), 2);
    let detail_id = requests.iter().find(|request| matches!(&request.method, Method::CronGet(params) if params.job_id == "job-1")).unwrap().id.clone();
    let id = requests.iter().find(|request| matches!(&request.method, Method::CronRuns(params) if params.job_id == "job-1")).unwrap().id.clone();
    state.handle_endpoint_result(
        "boot-1",
        &detail_id,
        Ok(ResponseResult::CronDefinition {
            job_id: "job-1".into(),
            definition: definition(),
        }),
    );
    state.handle_endpoint_result(
        "boot-1",
        &id,
        Ok(ResponseResult::CronRuns {
            runs: vec![CronRunInfo {
                id: "run-1".into(),
                job_id: "job-1".into(),
                trigger: "manual".into(),
                occurrence_at: 1_791_273_600,
                status: "success".into(),
                started_at: None,
                finished_at: None,
                error_class: None,
                output_path: None,
            }],
        }),
    );
    assert_eq!(state.crons_view.page, CronsPage::Runs);
    let frame = state.compose(110, 30).unwrap();
    let text = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("Agenda:"));
    assert!(text.contains("/bin/echo"));
    assert!(text.contains("Histórico"));
    assert!(text.contains("run-1"));
    let log = state.handle_input_bytes(b"\r");
    let request = endpoint_request(&log);
    assert!(matches!(&request.method, Method::CronLog(params) if params.run_id == "run-1"));
    let id = request.id.clone();
    state.handle_endpoint_result(
        "boot-1",
        &id,
        Ok(ResponseResult::CronLog {
            run_id: "run-1".into(),
            text: "cron output".into(),
        }),
    );
    assert_eq!(state.crons_view.page, CronsPage::Log);
    let frame = state.compose(110, 30).unwrap();
    let text = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("cron output"));
}

#[test]
fn clicking_cron_and_run_opens_detail_then_log() {
    let mut state = prepared();
    load_job(&mut state);
    state.compose(110, 30).unwrap();
    let (job_row, _) = state.hits.crons_rows[0];
    let detail =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: job_row.x,
            row: job_row.y,
            modifiers: crossterm::event::KeyModifiers::empty(),
        })]);
    assert_eq!(state.crons_view.page, CronsPage::Runs);
    assert_eq!(detail.actions.len(), 2);
    let runs_id = detail
        .actions
        .iter()
        .find_map(|action| match action {
            ClientShellAction::Endpoint { request, .. }
                if matches!(&request.method, Method::CronRuns(_)) =>
            {
                Some(request.id.clone())
            }
            _ => None,
        })
        .unwrap();
    state.handle_endpoint_result(
        "boot-1",
        &runs_id,
        Ok(ResponseResult::CronRuns {
            runs: vec![CronRunInfo {
                id: "run-1".into(),
                job_id: "job-1".into(),
                trigger: "manual".into(),
                occurrence_at: 1_791_273_600,
                status: "success".into(),
                started_at: None,
                finished_at: None,
                error_class: None,
                output_path: None,
            }],
        }),
    );
    state.compose(110, 30).unwrap();
    let (run_row, run_index) = state.hits.crons_rows[0];
    assert_eq!(run_index, 0);
    let log = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: run_row.x,
        row: run_row.y,
        modifiers: crossterm::event::KeyModifiers::empty(),
    })]);
    assert!(
        matches!(&endpoint_request(&log).method, Method::CronLog(params) if params.run_id == "run-1")
    );
}

#[test]
fn crons_actions_run_pause_and_edit_schedule_preserving_other_fields() {
    use super::super::cron_form::Field;
    let mut state = prepared();
    load_job(&mut state);
    assert!(matches!(
        &endpoint_request(&state.handle_input_bytes(b"n")).method,
        Method::CronRun(_)
    ));
    assert!(matches!(
        &endpoint_request(&state.handle_input_bytes(b"p")).method,
        Method::CronPause(_)
    ));
    let edit = state.handle_input_bytes(b"e");
    let id = endpoint_request(&edit).id.clone();
    let (_, actions) = state.handle_endpoint_result(
        "boot-1",
        &id,
        Ok(ResponseResult::CronDefinition {
            job_id: "job-1".into(),
            definition: definition(),
        }),
    );
    answer_preview(
        &mut state,
        &ClientShellInput {
            actions,
            ..Default::default()
        },
    );
    state
        .crons_view
        .form
        .as_mut()
        .unwrap()
        .set(Field::Raw, "every 2h");
    let review = state.handle_input_bytes(&[0x13]);
    answer_preview(&mut state, &review);
    let save = state.handle_input_bytes(&[0x13]);
    assert!(
        matches!(&endpoint_request(&save).method,Method::CronEdit(p) if p.job_id=="job-1"&&p.definition.schedule=="every 2h"&&matches!(&p.definition.target,mercury_cron::JobTarget::Script{args,..} if args==&vec!["ok".to_string()]))
    );
}

#[test]
fn crons_state_is_cleared_when_endpoint_becomes_unavailable() {
    let mut state = prepared();
    load_job(&mut state);
    assert_eq!(state.crons_view.jobs.len(), 1);
    state.select_unavailable_local();
    assert_eq!(state.view, ClientShellView::Terminal);
    assert!(state.crons_view.jobs.is_empty());
    assert!(state.crons_view.log.is_none());
}

fn fill_form(state: &mut ClientShellState, kind: usize) {
    use super::super::cron_form::Field;
    let f = state.crons_view.form.as_mut().unwrap();
    f.set(Field::Name, "daily");
    f.preset = 7;
    f.set(Field::Raw, "every 2h");
    f.set(Field::Offset, "-03:00");
    f.kind = kind;
    f.set(Field::Prompt, "summarize\nwith sources");
    f.set(
        Field::Address,
        if kind == 2 { "pane_1" } else { "/bin/echo" },
    );
    f.preview = vec![1_800_000_000];
    f.preview_key = f.key().ok();
}
fn answer_preview(state: &mut ClientShellState, outcome: &ClientShellInput) {
    for action in &outcome.actions {
        if let ClientShellAction::Endpoint { request, .. } = action {
            if matches!(request.method, Method::CronPreview(_)) {
                state.handle_endpoint_result(
                    "boot-1",
                    &request.id,
                    Ok(ResponseResult::CronPreview {
                        utc_offset: "-03:00".into(),
                        occurrences: vec![1_800_000_000],
                    }),
                );
            }
        }
    }
}

#[test]
fn create_wizard_sends_each_target_through_active_endpoint_and_refreshes() {
    for kind in 0..4 {
        let mut state = prepared();
        load_job(&mut state);
        state.handle_input_bytes(b"a");
        fill_form(&mut state, kind);
        let review = state.handle_input_bytes(&[0x13]);
        answer_preview(&mut state, &review);
        assert!(state.crons_view.form.as_ref().unwrap().reviewing);
        let save = state.handle_input_bytes(&[0x13]);
        let request = endpoint_request(&save);
        let Method::CronAdd(params) = &request.method else {
            panic!("expected add")
        };
        assert_eq!(params.definition.name, "daily");
        assert_eq!(params.definition.utc_offset.as_deref(), Some("-03:00"));
        match (&params.definition.target, kind) {
            (
                mercury_cron::JobTarget::Headless {
                    provider, prompt, ..
                },
                0 | 1,
            ) => {
                assert_eq!(provider, if kind == 0 { "claude" } else { "codex" });
                assert!(prompt.contains('\n'));
            }
            (mercury_cron::JobTarget::Pane { pane_id, .. }, 2) => assert_eq!(pane_id, "pane_1"),
            (mercury_cron::JobTarget::Script { program, .. }, 3) => {
                assert_eq!(program, "/bin/echo")
            }
            _ => panic!("wrong target"),
        }
        let id = request.id.clone();
        let (_, actions) =
            state.handle_endpoint_result("boot-1", &id, Ok(ResponseResult::CronJob { job: job() }));
        assert!(state.crons_view.form.is_none());
        assert!(matches!(
            endpoint_request(&ClientShellInput {
                actions,
                ..Default::default()
            })
            .method,
            Method::CronList(_)
        ));
    }
}

#[test]
fn rejected_cron_mutation_remains_visible_in_jobs_panel() {
    let mut state = prepared();
    load_job(&mut state);
    let request = state.handle_input_bytes(b"p");
    let id = endpoint_request(&request).id.clone();
    state.handle_endpoint_result(
        "boot-1",
        &id,
        Err(ClientShellEndpointError {
            code: Some("cron_add_failed".into()),
            message: "cannot persist job".into(),
        }),
    );
    assert_eq!(
        state.crons_view.error.as_deref(),
        Some("Crons · não salvo: cannot persist job")
    );
    let frame = state.compose(110, 30).unwrap();
    let text = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("cannot persist job"));
}

#[test]
fn confirmed_cron_missing_from_refresh_reports_divergence() {
    let mut state = prepared();
    load_job(&mut state);
    state.handle_input_bytes(b"a");
    fill_form(&mut state, 3);
    let review = state.handle_input_bytes(&[0x13]);
    answer_preview(&mut state, &review);
    let request = state.handle_input_bytes(&[0x13]);
    let mut created = job();
    created.id = "job-created".into();
    let (_, actions) = state.handle_endpoint_result(
        "boot-1",
        &endpoint_request(&request).id,
        Ok(ResponseResult::CronJob { job: created }),
    );
    assert_eq!(
        state.crons_view.confirmed_job_id.as_deref(),
        Some("job-created")
    );
    let refresh = ClientShellInput {
        actions,
        ..ClientShellInput::default()
    };
    state.handle_endpoint_result(
        "boot-1",
        &endpoint_request(&refresh).id,
        Ok(ResponseResult::CronList { jobs: Vec::new() }),
    );
    assert!(state
        .crons_view
        .error
        .as_deref()
        .unwrap()
        .contains("listagem não o retornou"));
    assert_eq!(
        state.crons_view.confirmed_job_id.as_deref(),
        Some("job-created")
    );
    let frame = state.compose(110, 30).unwrap();
    let text = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("servidor confirmou"));
}

#[test]
fn stale_cron_list_cannot_erase_a_newly_confirmed_job() {
    let mut state = prepared();
    load_job(&mut state);
    let stale = state.handle_input_bytes(b"r");
    let stale_id = endpoint_request(&stale).id.clone();

    state.handle_input_bytes(b"a");
    fill_form(&mut state, 3);
    let review = state.handle_input_bytes(&[0x13]);
    answer_preview(&mut state, &review);
    let submitted = state.handle_input_bytes(&[0x13]);
    let mut created = job();
    created.id = "job-2".into();
    created.name = "second".into();
    let (_, actions) = state.handle_endpoint_result(
        "boot-1",
        &endpoint_request(&submitted).id,
        Ok(ResponseResult::CronJob {
            job: created.clone(),
        }),
    );
    let latest = ClientShellInput {
        actions,
        ..ClientShellInput::default()
    };
    state.handle_endpoint_result(
        "boot-1",
        &endpoint_request(&latest).id,
        Ok(ResponseResult::CronList {
            jobs: vec![job(), created],
        }),
    );
    let (repaint, actions) = state.handle_endpoint_result(
        "boot-1",
        &stale_id,
        Ok(ResponseResult::CronList { jobs: vec![job()] }),
    );
    assert!(!repaint);
    assert!(actions.is_empty());
    assert!(state
        .crons_view
        .jobs
        .iter()
        .any(|entry| entry.id == "job-2"));
    assert!(state.crons_view.error.is_none());
}

#[test]
fn cron_list_from_previous_tab_opening_is_ignored() {
    let mut state = prepared();
    load_job(&mut state);
    let old = state.handle_input_bytes(b"r");
    let old_id = endpoint_request(&old).id.clone();
    state.handle_input_bytes(b"q");
    state.handle_input_bytes(&[0x02]);
    let reopened = state.handle_input_bytes(b"C");
    let current_id = endpoint_request(&reopened).id.clone();
    assert_ne!(old_id, current_id);
    let (repaint, _) = state.handle_endpoint_result(
        "boot-1",
        &old_id,
        Err(ClientShellEndpointError {
            code: Some("endpoint_timeout".into()),
            message: "old request timed out".into(),
        }),
    );
    assert!(!repaint);
    assert!(state.crons_view.error.is_none());
    assert_eq!(
        state.crons_view.latest_list_request_id.as_deref(),
        Some(current_id.as_str())
    );
}

#[test]
fn stale_cron_detail_response_cannot_replace_new_selection() {
    let mut state = prepared();
    load_job(&mut state);
    state.crons_view.jobs.push(CronJobInfo {
        id: "job-2".into(),
        name: "Second".into(),
        ..job()
    });
    let first = state.handle_input_bytes(b"\r");
    let first_detail_id = first
        .actions
        .iter()
        .find_map(|action| match action {
            ClientShellAction::Endpoint { request, .. }
                if matches!(&request.method, Method::CronGet(_)) =>
            {
                Some(request.id.clone())
            }
            _ => None,
        })
        .unwrap();
    state.handle_input_bytes(b"\x1b");
    state.handle_input_bytes(b"j");
    state.handle_input_bytes(b"\r");
    state.handle_endpoint_result(
        "boot-1",
        &first_detail_id,
        Ok(ResponseResult::CronDefinition {
            job_id: "job-1".into(),
            definition: definition(),
        }),
    );
    assert!(state.crons_view.definition.is_none());
}

#[test]
fn create_wizard_rejects_invalid_fields_and_escape_does_not_create() {
    let mut state = prepared();
    load_job(&mut state);
    state.handle_input_bytes(b"a");
    let result = state.handle_input_bytes(&[0x13]);
    assert!(result.actions.is_empty());
    assert!(state
        .crons_view
        .form
        .as_ref()
        .unwrap()
        .error
        .as_deref()
        .unwrap()
        .contains("Nome"));
    let cancel = state.handle_input_bytes(b"\x1b");
    assert!(cancel.actions.is_empty());
    assert!(state.crons_view.form.is_none());
}

#[test]
fn create_wizard_rejects_unknown_target_and_relative_workdir() {
    use super::super::cron_form::Field;
    let mut state = prepared();
    load_job(&mut state);
    state.handle_input_bytes(b"a");
    fill_form(&mut state, 3);
    state
        .crons_view
        .form
        .as_mut()
        .unwrap()
        .set(Field::Address, "relative/program");
    assert!(state.handle_input_bytes(&[0x13]).actions.is_empty());
    assert!(state
        .crons_view
        .form
        .as_ref()
        .unwrap()
        .error
        .as_deref()
        .unwrap()
        .contains("absoluto"));
}

#[test]
fn create_wizard_keeps_draft_when_endpoint_cannot_accept_add() {
    let mut state = prepared();
    load_job(&mut state);
    state.handle_input_bytes(b"a");
    fill_form(&mut state, 3);
    let review = state.handle_input_bytes(&[0x13]);
    answer_preview(&mut state, &review);
    state.set_endpoint_methods(Some(vec!["cron.list".into()]));
    assert!(state.handle_input_bytes(&[0x13]).actions.is_empty());
    let f = state.crons_view.form.as_ref().unwrap();
    assert!(!f.submitting);
    assert!(f.error.is_some());
}

#[test]
fn create_wizard_is_discarded_when_endpoint_becomes_unavailable() {
    let mut state = prepared();
    load_job(&mut state);
    state.handle_input_bytes(b"a");
    assert!(state.crons_view.form.is_some());
    state.select_unavailable_local();
    assert!(state.crons_view.form.is_none());
}

#[test]
fn delete_requires_confirmation_and_refreshes_only_after_server_acknowledges() {
    let mut state = prepared();
    load_job(&mut state);
    assert!(state.handle_input_bytes(b"d").actions.is_empty());
    assert!(state.crons_view.deleting.is_some());
    state.handle_input_bytes(b"\x1b");
    assert_eq!(state.crons_view.jobs.len(), 1);
    state.handle_input_bytes(b"d");
    let remove = state.handle_input_bytes(b"y");
    let request = endpoint_request(&remove);
    assert!(matches!(&request.method,Method::CronRemove(p)if p.job_id=="job-1"));
    assert!(
        state.handle_input_bytes(b"y").actions.is_empty(),
        "cannot send duplicate delete while pending"
    );
    let (_, actions) =
        state.handle_endpoint_result("boot-1", &request.id, Ok(ResponseResult::Ok {}));
    assert!(state.crons_view.jobs.is_empty());
    assert!(state.crons_view.deleting.is_none());
    assert!(matches!(
        endpoint_request(&ClientShellInput {
            actions,
            ..Default::default()
        })
        .method,
        Method::CronList(_)
    ));
}

#[test]
fn delete_action_remains_visible_at_eighty_columns() {
    let mut state = prepared();
    load_job(&mut state);
    let frame = state.compose(80, 24).unwrap();
    let text = frame
        .cells
        .iter()
        .map(|c| c.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("[d Excluir]"));
    assert!(state.hits.cron_controls.iter().any(|(_, key)| *key == 'd'));
}

#[test]
fn stale_preview_after_cancel_cannot_change_a_new_form() {
    use super::super::cron_form::Field;
    let mut state = prepared();
    load_job(&mut state);
    let old = state.handle_input_bytes(b"a");
    let id = endpoint_request(&old).id.clone();
    state.handle_input_bytes(b"\x1b");
    state.handle_input_bytes(b"a");
    state.handle_endpoint_result(
        "boot-1",
        &id,
        Ok(ResponseResult::CronPreview {
            utc_offset: "+12:00".into(),
            occurrences: vec![42],
        }),
    );
    let f = state.crons_view.form.as_ref().unwrap();
    assert!(f.preview.is_empty());
    assert!(f.value(Field::Offset).is_empty());
}

#[test]
fn form_paste_keeps_multiline_instruction_and_never_leaks_into_hidden_pane() {
    use super::super::cron_form::Field;
    let mut state = prepared();
    load_job(&mut state);
    state.handle_input_bytes(b"a");
    let f = state.crons_view.form.as_mut().unwrap();
    f.focus = f.fields().iter().position(|f| *f == Field::Prompt).unwrap();
    let result = state.handle_raw_events(vec![RawInputEvent::Paste("revisar\ncom fontes".into())]);
    assert!(result.requests.is_empty());
    assert_eq!(
        state.crons_view.form.as_ref().unwrap().value(Field::Prompt),
        "revisar\ncom fontes"
    );
    state.handle_input_bytes(b"qdpn");
    assert!(state.crons_view.form.is_some());
    assert_eq!(state.mode, ClientShellMode::Terminal);
}
