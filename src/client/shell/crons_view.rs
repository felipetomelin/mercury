//! Crons panel. Its list, run history and log are fetched through the active
//! endpoint API and kept in transient client state, never in shell snapshots.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Clear, Widget},
};
use std::borrow::Cow;

use super::render::put_text;
use super::*;

fn format_time(at: Option<i64>) -> String {
    at.and_then(|at| time::OffsetDateTime::from_unix_timestamp(at).ok())
        .and_then(|at| {
            at.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_else(|| "—".into())
}

pub(super) fn render_crons_view(
    buffer: &mut Buffer,
    area: Rect,
    config: &ClientShellConfig,
    state: &mut CronsViewState,
    hits: &mut ShellHitMap,
) {
    hits.crons_rows.clear();
    hits.cron_controls.clear();
    if let Some(form) = state.form.as_mut() {
        super::cron_form::render(buffer, area, config, form, hits);
        return;
    }
    if let Some((id, name)) = &state.deleting {
        Clear.render(area, buffer);
        let p = &config.palette;
        buffer.set_style(area, Style::default().fg(p.text).bg(p.panel_bg));
        for (i, line) in [
            format!("Excluir agendamento: {name}"),
            format!("ID: {id}"),
            "Remove execuções futuras e preserva histórico/logs.".into(),
            "Execução já iniciada continua.".into(),
            "y confirmar exclusão · Esc cancelar".into(),
            state.error.clone().unwrap_or_default(),
        ]
        .iter()
        .enumerate()
        .take(area.height as usize)
        {
            put_text(
                buffer,
                area.x + 1,
                area.y + i as u16,
                area.width.saturating_sub(2),
                line,
                Style::default().fg(if i == 0 || i == 5 { p.red } else { p.text }),
            );
        }
        if area.height > 4 {
            hits.cron_controls.push((
                Rect::new(
                    area.x + 1,
                    area.y + 4,
                    22.min(area.width.saturating_sub(2)),
                    1,
                ),
                'y',
            ));
            if area.width > 26 {
                hits.cron_controls.push((
                    Rect::new(area.x + 25, area.y + 4, area.width.saturating_sub(26), 1),
                    'x',
                ));
            }
        }
        return;
    }

    if area.is_empty() {
        return;
    }
    let palette = &config.palette;
    Clear.render(area, buffer);
    buffer.set_style(area, Style::default().fg(palette.text).bg(palette.panel_bg));
    let (title, hint) = match state.page {
        CronsPage::Jobs => (
            "Crons · jobs",
            "a criar  Enter detalhe  n executar  p pausar  e editar  d excluir  r atualizar",
        ),
        CronsPage::Runs => (
            "Crons · detalhe e histórico",
            "Enter log  e editar  d excluir  p pausar  r atualizar  Esc voltar",
        ),
        CronsPage::Log => ("Crons · log", "↑↓ rolar  Esc detalhe  q sair"),
    };
    put_text(
        buffer,
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(1),
        title,
        Style::default()
            .fg(palette.accent)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    if area.height < 2 {
        return;
    }
    let header_rows = if state.page == CronsPage::Log { 2 } else { 3 };
    if state.page == CronsPage::Log {
        put_text(
            buffer,
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            hint,
            Style::default().fg(palette.subtext0),
        );
    } else {
        let pause = if state
            .jobs
            .get(state.selected_job)
            .is_some_and(|j| !j.enabled)
        {
            "[p Retomar]"
        } else {
            "[p Pausar]"
        };
        for (row, controls) in [
            vec![
                ("[a Criar]", 'a'),
                ("[e Editar]", 'e'),
                ("[d Excluir]", 'd'),
            ],
            vec![(pause, 'p'), ("[n Executar]", 'n'), ("[r Atualizar]", 'r')],
        ]
        .into_iter()
        .enumerate()
        {
            let y = area.y + 1 + row as u16;
            if y >= area.bottom() {
                break;
            }
            let mut x = area.x + 1;
            for (label, key) in controls {
                let width = label.chars().count() as u16;
                if x + width > area.right() {
                    break;
                }
                let style = Style::default()
                    .fg(if key == 'd' {
                        palette.red
                    } else {
                        palette.accent
                    })
                    .bg(palette.panel_bg);
                put_text(buffer, x, y, width, label, style);
                hits.cron_controls.push((Rect::new(x, y, width, 1), key));
                x += width + 1;
            }
        }
    }
    let message_rows = u16::from(state.error.is_some() || state.notice.is_some());
    if let Some(error) = &state.error {
        if area.height > header_rows {
            put_text(
                buffer,
                area.x.saturating_add(1),
                area.y + header_rows,
                area.width.saturating_sub(1),
                error,
                Style::default().fg(palette.gauge_crit).bg(palette.panel_bg),
            );
        }
    }
    if state.error.is_none() {
        if let Some(notice) = &state.notice {
            if area.height > header_rows {
                put_text(
                    buffer,
                    area.x.saturating_add(1),
                    area.y + header_rows,
                    area.width.saturating_sub(1),
                    notice,
                    Style::default().fg(palette.text).bg(palette.panel_bg),
                );
            }
        }
    }
    let body = Rect::new(
        area.x,
        area.y + header_rows + message_rows,
        area.width,
        area.height.saturating_sub(header_rows + message_rows),
    );
    if body.is_empty() {
        return;
    }
    let mut lines: Vec<Cow<'_, str>> = match state.page {
        CronsPage::Jobs => state
            .jobs
            .iter()
            .map(|job| {
                Cow::Owned(format!(
                    "{}  {}  [{}]  próximo {}  último {}",
                    if job.enabled { "●" } else { "○" },
                    job.name,
                    job.target_kind,
                    format_time(job.next_run_at),
                    job.last_status.as_deref().unwrap_or("—"),
                ))
            })
            .collect(),
        CronsPage::Runs => {
            let mut detail = Vec::new();
            if let Some(job) = state.jobs.get(state.selected_job) {
                detail.push(Cow::Owned(format!("{} · {}", job.name, job.id)));
                detail.push(Cow::Owned(format!(
                    "Agenda: {}  Próximo: {}",
                    job.schedule,
                    format_time(job.next_run_at)
                )));
                detail.push(Cow::Owned(format!(
                    "Estado: {}  Último: {}",
                    if job.enabled { "ativo" } else { "pausado" },
                    job.last_status.as_deref().unwrap_or("—")
                )));
            }
            if let Some(definition) = &state.definition {
                detail.push(Cow::Owned(format!(
                    "Alvo: {}",
                    format_target(&definition.target)
                )));
                detail.push(Cow::Owned(format!(
                    "Workdir: {}",
                    definition
                        .workdir
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "—".into())
                )));
                detail.push(Cow::Owned(format!(
                    "Timeout: {}  Fuso: {}",
                    definition
                        .timeout_seconds
                        .map(|seconds| format!("{seconds}s"))
                        .unwrap_or_else(|| "padrão".into()),
                    definition.utc_offset.as_deref().unwrap_or("—")
                )));
                if let Some(prompt) = target_prompt(&definition.target) {
                    detail.push(Cow::Owned(format!(
                        "Prompt: {}",
                        prompt.replace(['\n', '\r'], " ")
                    )));
                }
            } else {
                detail.push(Cow::Borrowed("Carregando definição…"));
            }
            detail.push(Cow::Borrowed("Histórico (até 50 execuções):"));
            detail.extend(state.runs.iter().map(|run| {
                Cow::Owned(format!(
                    "{}  {}  {}  início {}  fim {}  erro {}  {}",
                    run.status,
                    run.trigger,
                    format_time(Some(run.occurrence_at)),
                    format_time(run.started_at),
                    format_time(run.finished_at),
                    run.error_class.as_deref().unwrap_or("—"),
                    run.id,
                ))
            }));
            if state.runs.is_empty() {
                detail.push(Cow::Borrowed(if state.loading {
                    "Carregando histórico…"
                } else {
                    "Nenhuma execução"
                }));
            }
            detail
        }
        CronsPage::Log => state
            .log
            .as_deref()
            .unwrap_or("")
            .lines()
            .map(Cow::Borrowed)
            .collect(),
    };
    if lines.is_empty() {
        lines.push(Cow::Borrowed(if state.loading {
            "Carregando…"
        } else {
            match state.page {
                CronsPage::Jobs => "Nenhum job agendado",
                CronsPage::Runs => "Nenhuma execução",
                CronsPage::Log => "Log vazio",
            }
        }));
    }
    let selected = match state.page {
        CronsPage::Jobs => Some(state.selected_job),
        CronsPage::Runs => Some(detail_header_rows(state) + state.selected_run),
        CronsPage::Log => None,
    };
    let max_scroll = lines.len().saturating_sub(body.height as usize);
    if let Some(selected) = selected {
        if selected < state.scroll {
            state.scroll = selected;
        }
        if selected >= state.scroll.saturating_add(body.height as usize) {
            state.scroll = selected + 1 - body.height as usize;
        }
    }
    state.scroll = state.scroll.min(max_scroll);
    for (index, line) in lines
        .iter()
        .enumerate()
        .skip(state.scroll)
        .take(body.height as usize)
    {
        let y = body.y + (index - state.scroll) as u16;
        let rect = Rect::new(body.x, y, body.width, 1);
        let style = if selected == Some(index) {
            buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
            Style::default().fg(palette.text).bg(palette.active_row_bg)
        } else {
            Style::default().fg(palette.text).bg(palette.panel_bg)
        };
        let color = match state.page {
            CronsPage::Jobs => state.jobs.get(index).map(|job| {
                if job.enabled {
                    super::render::semantic_status_color(
                        palette,
                        job.last_status.as_deref().unwrap_or("pending"),
                    )
                } else {
                    palette.subtext0
                }
            }),
            CronsPage::Runs => index
                .checked_sub(detail_header_rows(state))
                .and_then(|n| state.runs.get(n))
                .map(|r| super::render::semantic_status_color(palette, &r.status)),
            _ => None,
        };
        let style = style.fg(color.unwrap_or(palette.text));
        put_text(
            buffer,
            body.x.saturating_add(1),
            y,
            body.width.saturating_sub(1),
            line,
            style,
        );
        match state.page {
            CronsPage::Jobs => hits.crons_rows.push((rect, index)),
            CronsPage::Runs
                if index >= detail_header_rows(state)
                    && index - detail_header_rows(state) < state.runs.len() =>
            {
                hits.crons_rows
                    .push((rect, index - detail_header_rows(state)));
            }
            _ => {}
        }
    }
}

fn detail_header_rows(state: &CronsViewState) -> usize {
    usize::from(state.jobs.get(state.selected_job).is_some()) * 3
        + if let Some(definition) = &state.definition {
            3 + usize::from(target_prompt(&definition.target).is_some())
        } else {
            1
        }
        + 1
}

fn format_target(target: &mercury_cron::JobTarget) -> String {
    match target {
        mercury_cron::JobTarget::Headless {
            provider,
            model,
            effort,
            ..
        } => format!(
            "{provider} headless  modelo {}  effort {}",
            model.as_deref().unwrap_or("padrão"),
            effort.as_deref().unwrap_or("padrão")
        ),
        mercury_cron::JobTarget::Pane { pane_id, .. } => format!("pane {pane_id}"),
        mercury_cron::JobTarget::Script { program, args } => {
            format!("script {program} {}", args.join(" "))
        }
    }
}

fn target_prompt(target: &mercury_cron::JobTarget) -> Option<&str> {
    match target {
        mercury_cron::JobTarget::Headless { prompt, .. }
        | mercury_cron::JobTarget::Pane { prompt, .. } => Some(prompt),
        mercury_cron::JobTarget::Script { .. } => None,
    }
}

impl ClientShellState {
    pub(super) fn toggle_crons_view(&mut self, outcome: &mut ClientShellInput) {
        if self.view == ClientShellView::Crons {
            self.view = ClientShellView::Terminal;
        } else {
            self.view = ClientShellView::Crons;
            self.crons_view = CronsViewState::default();
            self.refresh_crons(outcome);
        }
        outcome.repaint = true;
    }

    pub(super) fn refresh_crons(&mut self, outcome: &mut ClientShellInput) {
        self.crons_view.loading = self.push_endpoint_method_with_kind(
            crate::api::schema::Method::CronList(crate::api::schema::EmptyParams::default()),
            PendingEndpointKind::CronList,
            outcome,
        );
        self.crons_view.latest_list_request_id = if self.crons_view.loading {
            outcome.actions.last().and_then(|action| match action {
                ClientShellAction::Endpoint { request, .. } => Some(request.id.clone()),
                _ => None,
            })
        } else {
            None
        };
        if !self.crons_view.loading {
            self.crons_view.notice = None;
            self.crons_view.error =
                Some("Crons · não foi possível consultar a listagem no endpoint ativo".into());
        }
        outcome.repaint = true;
    }

    fn selected_cron(&self) -> Option<&crate::api::schema::CronJobInfo> {
        self.crons_view.jobs.get(self.crons_view.selected_job)
    }

    pub(super) fn open_selected_runs(&mut self, outcome: &mut ClientShellInput) {
        let Some(job_id) = self.selected_cron().map(|job| job.id.clone()) else {
            return;
        };
        self.crons_view.page = CronsPage::Runs;
        self.crons_view.definition = None;
        self.crons_view.runs.clear();
        self.crons_view.selected_run = 0;
        self.crons_view.scroll = 0;
        self.crons_view.error = None;
        let definition_requested = self.push_endpoint_method_with_kind(
            crate::api::schema::Method::CronGet(crate::api::schema::CronJobTarget {
                job_id: job_id.clone(),
            }),
            PendingEndpointKind::CronDetail {
                job_id: job_id.clone(),
            },
            outcome,
        );
        let runs_requested = self.push_endpoint_method_with_kind(
            crate::api::schema::Method::CronRuns(crate::api::schema::CronRunsParams {
                job_id: job_id.clone(),
                limit: Some(50),
            }),
            PendingEndpointKind::CronRuns { job_id },
            outcome,
        );
        self.crons_view.loading = definition_requested || runs_requested;
        if !definition_requested || !runs_requested {
            self.crons_view.error =
                Some("Crons · não foi possível carregar o detalhe completo".into());
        }
        outcome.repaint = true;
    }

    pub(super) fn open_selected_log(&mut self, outcome: &mut ClientShellInput) {
        let Some(run_id) = self
            .crons_view
            .runs
            .get(self.crons_view.selected_run)
            .map(|run| run.id.clone())
        else {
            return;
        };
        self.crons_view.loading = self.push_endpoint_method_with_kind(
            crate::api::schema::Method::CronLog(crate::api::schema::CronLogParams {
                run_id: run_id.clone(),
            }),
            PendingEndpointKind::CronLog { run_id },
            outcome,
        );
        outcome.repaint = true;
    }

    fn run_selected_cron(&mut self, outcome: &mut ClientShellInput) {
        let Some(job_id) = self.selected_cron().map(|job| job.id.clone()) else {
            return;
        };
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::CronRun(crate::api::schema::CronJobTarget { job_id }),
            PendingEndpointKind::CronMutation,
            outcome,
        );
    }

    fn pause_selected_cron(&mut self, outcome: &mut ClientShellInput) {
        let Some(job) = self.selected_cron() else {
            return;
        };
        let (job_id, paused) = (job.id.clone(), job.enabled);
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::CronPause(crate::api::schema::CronPauseParams {
                job_id,
                paused,
            }),
            PendingEndpointKind::CronMutation,
            outcome,
        );
    }

    fn edit_selected_cron(&mut self, outcome: &mut ClientShellInput) {
        let Some(job_id) = self.selected_cron().map(|job| job.id.clone()) else {
            return;
        };
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::CronGet(crate::api::schema::CronJobTarget {
                job_id: job_id.clone(),
            }),
            PendingEndpointKind::CronGet { job_id },
            outcome,
        );
    }

    pub(super) fn handle_crons_view_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        if self.crons_view.form.is_some() {
            self.handle_cron_form_key(key, outcome);
            return;
        }
        if self.crons_view.deleting.is_some() {
            self.handle_cron_delete_key(key, outcome);
            return;
        }
        match key.code {
            KeyCode::Char('a') if self.crons_view.page != CronsPage::Log => {
                self.open_cron_form(None, outcome)
            }
            KeyCode::Char('q') => self.toggle_crons_view(outcome),
            KeyCode::Esc => match self.crons_view.page {
                CronsPage::Jobs => self.toggle_crons_view(outcome),
                CronsPage::Runs => {
                    self.crons_view.page = CronsPage::Jobs;
                    self.crons_view.scroll = 0;
                    outcome.repaint = true;
                }
                CronsPage::Log => {
                    self.crons_view.page = CronsPage::Runs;
                    self.crons_view.scroll = 0;
                    outcome.repaint = true;
                }
            },
            KeyCode::Up | KeyCode::Char('k') => {
                match self.crons_view.page {
                    CronsPage::Jobs => {
                        self.crons_view.selected_job =
                            self.crons_view.selected_job.saturating_sub(1)
                    }
                    CronsPage::Runs => {
                        self.crons_view.selected_run =
                            self.crons_view.selected_run.saturating_sub(1)
                    }
                    CronsPage::Log => {
                        self.crons_view.scroll = self.crons_view.scroll.saturating_sub(1)
                    }
                }
                outcome.repaint = true;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                match self.crons_view.page {
                    CronsPage::Jobs => {
                        self.crons_view.selected_job = self
                            .crons_view
                            .selected_job
                            .saturating_add(1)
                            .min(self.crons_view.jobs.len().saturating_sub(1))
                    }
                    CronsPage::Runs => {
                        self.crons_view.selected_run = self
                            .crons_view
                            .selected_run
                            .saturating_add(1)
                            .min(self.crons_view.runs.len().saturating_sub(1))
                    }
                    CronsPage::Log => {
                        self.crons_view.scroll = self.crons_view.scroll.saturating_add(1)
                    }
                }
                outcome.repaint = true;
            }
            KeyCode::Enter | KeyCode::Char('l') => match self.crons_view.page {
                CronsPage::Jobs => self.open_selected_runs(outcome),
                CronsPage::Runs => self.open_selected_log(outcome),
                CronsPage::Log => {}
            },
            KeyCode::Char('r') => match self.crons_view.page {
                CronsPage::Jobs => self.refresh_crons(outcome),
                CronsPage::Runs => self.open_selected_runs(outcome),
                CronsPage::Log => {}
            },
            KeyCode::Char('n') if self.crons_view.page != CronsPage::Log => {
                self.run_selected_cron(outcome)
            }
            KeyCode::Char('p') if self.crons_view.page != CronsPage::Log => {
                self.pause_selected_cron(outcome)
            }
            KeyCode::Char('e') if self.crons_view.page != CronsPage::Log => {
                self.edit_selected_cron(outcome)
            }
            KeyCode::Char('d') | KeyCode::Delete if self.crons_view.page != CronsPage::Log => {
                if let Some(job) = self.selected_cron() {
                    self.crons_view.deleting = Some((job.id.clone(), job.name.clone()));
                    self.crons_view.error = None;
                    outcome.repaint = true;
                }
            }
            _ => {}
        }
    }

    pub(super) fn handle_cron_delete_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        if self.crons_view.deleting_pending {
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('x') | KeyCode::Char('n') => {
                self.crons_view.deleting = None;
                self.crons_view.error = None;
            }
            KeyCode::Char('y') => {
                if let Some((id, _)) = &self.crons_view.deleting {
                    let id = id.clone();
                    self.crons_view.deleting_pending = self.push_endpoint_method_with_kind(
                        crate::api::schema::Method::CronRemove(crate::api::schema::CronJobTarget {
                            job_id: id.clone(),
                        }),
                        PendingEndpointKind::CronFormRemove { job_id: id },
                        outcome,
                    );
                }
            }
            _ => {}
        }
        outcome.repaint = true;
    }
}
