//! Redacted, server-backed Learn audit surface. Never stores file bytes.

use crossterm::event::KeyCode;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Clear, Widget},
};
use unicode_width::UnicodeWidthStr;

use super::render::put_text as render_text;
use super::*;

fn put_text(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    let visible = crate::ui::truncate_end(text, width as usize);
    render_text(buffer, x, y, width, &visible, style);
}

pub(super) fn render_learn_view(
    buffer: &mut Buffer,
    area: Rect,
    config: &ClientShellConfig,
    state: &mut LearnViewState,
    hits: &mut ShellHitMap,
) {
    hits.learn_rows.clear();
    if area.is_empty() {
        return;
    }
    let palette = &config.palette;
    Clear.render(area, buffer);
    buffer.set_style(area, Style::default().fg(palette.text).bg(palette.panel_bg));
    put_text(
        buffer,
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(1),
        "Learn · alterações automáticas",
        Style::default()
            .fg(palette.accent)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    if area.height < 2 {
        return;
    }
    let status = match &state.status {
        _ if state.load_error => "Learn indisponível · verifique/reinicie servidor".to_owned(),
        Some(status) if status.ledger_present => {
            let full = format!(
                "aplicadas {}  conflitos {}  falhas {}  revertidas {}  gatilhos {}",
                status.changes.applied,
                status.changes.conflict,
                status.changes.failed,
                status.changes.undone,
                status.triggers.succeeded,
            );
            if UnicodeWidthStr::width(full.as_str()) <= area.width.saturating_sub(1) as usize {
                full
            } else {
                format!(
                    "aplicadas {}  conflitos {}  falhas {}  undo {}",
                    status.changes.applied,
                    status.changes.conflict,
                    status.changes.failed,
                    status.changes.undone,
                )
            }
        }
        Some(_) => "ledger vazio".to_owned(),
        None => "status carregando…".to_owned(),
    };
    put_text(
        buffer,
        area.x.saturating_add(1),
        area.y + 1,
        area.width.saturating_sub(1),
        &status,
        Style::default()
            .fg(if state.load_error {
                palette.red
            } else {
                palette.subtext0
            })
            .bg(palette.panel_bg),
    );
    if area.height < 3 {
        return;
    }
    let hint = match state.page {
        LearnPage::List => {
            "↑↓ selecionar  Enter detalhe  t gatilhos  m métricas  r atualizar  q sair"
        }
        LearnPage::Detail => "u duas vezes para reverter  r atualizar  Esc voltar  q sair",
        LearnPage::Triggers => "↑↓ gatilhos  t/Esc alterações  r atualizar  q sair",
        LearnPage::Metrics => "↑↓ amostras  m/Esc alterações  r atualizar  q sair",
    };
    let compact_hint = match state.page {
        LearnPage::List => "Enter detalhe  t gatilhos  m métricas  q sair",
        LearnPage::Detail => "u duas vezes para reverter  Esc voltar",
        LearnPage::Triggers => "↑↓ gatilhos  Esc alterações  q sair",
        LearnPage::Metrics => "↑↓ amostras  Esc alterações  q sair",
    };
    put_text(
        buffer,
        area.x.saturating_add(1),
        area.y + 2,
        area.width.saturating_sub(1),
        if UnicodeWidthStr::width(hint) <= area.width.saturating_sub(1) as usize {
            hint
        } else {
            compact_hint
        },
        Style::default().fg(palette.overlay0).bg(palette.panel_bg),
    );
    let body = Rect::new(
        area.x,
        area.y + 3,
        area.width,
        area.height.saturating_sub(3),
    );
    if body.is_empty() {
        return;
    }
    match state.page {
        LearnPage::List => render_list(buffer, body, config, state, hits),
        LearnPage::Detail => render_detail(buffer, body, config, state),
        LearnPage::Triggers => render_triggers(buffer, body, config, state),
        LearnPage::Metrics => render_metrics(buffer, body, config, state),
    }
}

fn render_metrics(
    buffer: &mut Buffer,
    body: Rect,
    config: &ClientShellConfig,
    state: &mut LearnViewState,
) {
    let palette = &config.palette;
    let comparison_heading = state
        .status
        .as_ref()
        .and_then(|status| status.recent_metrics.get(state.selected_metric))
        .and_then(|metric| metric.comparison.as_ref())
        .map(|comparison| {
            format!(
                "Antes/depois descritivo · {}→{} runs · tokens médios {}→{}",
                comparison.baseline_runs,
                comparison.after_runs,
                comparison.baseline_average_tokens,
                comparison.after_average_tokens
            )
        });
    put_text(
        buffer,
        body.x.saturating_add(1),
        body.y,
        body.width.saturating_sub(1),
        comparison_heading.as_deref().unwrap_or(if body.width < 70 {
            "Métricas · dados insuficientes para comparar"
        } else {
            "Amostras observadas · dados insuficientes para comparar antes/depois"
        }),
        Style::default().fg(palette.overlay0).bg(palette.panel_bg),
    );
    let Some(status) = &state.status else { return };
    let metrics = &status.recent_metrics;
    if metrics.is_empty() {
        if body.height > 1 {
            put_text(
                buffer,
                body.x.saturating_add(1),
                body.y + 1,
                body.width.saturating_sub(1),
                "Nenhuma amostra observada",
                Style::default().fg(palette.overlay0).bg(palette.panel_bg),
            );
        }
        return;
    }
    let rows = body.height.saturating_sub(1) as usize / 2;
    if rows == 0 {
        if body.height > 1 {
            put_text(
                buffer,
                body.x.saturating_add(1),
                body.y + 1,
                body.width.saturating_sub(1),
                "Aumente a altura para ver as amostras",
                Style::default().fg(palette.overlay0).bg(palette.panel_bg),
            );
        }
        return;
    }
    state.selected_metric = state.selected_metric.min(metrics.len() - 1);
    if state.selected_metric < state.metric_scroll {
        state.metric_scroll = state.selected_metric;
    }
    if state.selected_metric >= state.metric_scroll + rows {
        state.metric_scroll = state.selected_metric + 1 - rows;
    }
    for (index, metric) in metrics
        .iter()
        .enumerate()
        .skip(state.metric_scroll)
        .take(rows)
    {
        let y = body.y + 1 + 2 * (index - state.metric_scroll) as u16;
        let row = Rect::new(body.x, y, body.width, 2);
        let style = if index == state.selected_metric {
            Style::default().fg(palette.text).bg(palette.active_row_bg)
        } else {
            Style::default().fg(palette.text).bg(palette.panel_bg)
        };
        buffer.set_style(row, style);
        let observed_at = time::OffsetDateTime::from_unix_timestamp(metric.observed_at_ms / 1000)
            .map(|instant| {
                format!(
                    "{} {:02}:{:02}",
                    instant.date(),
                    instant.hour(),
                    instant.minute()
                )
            })
            .unwrap_or_else(|_| "data inválida".into());
        let task = metric
            .task_key
            .strip_prefix("cron:")
            .unwrap_or(&metric.task_key);
        let short_task = task.chars().take(12).collect::<String>();
        let cost = metric.cost_microusd.map_or_else(
            || "?".into(),
            |value| format!("${}.{:06}", value / 1_000_000, value % 1_000_000),
        );
        let outcome = match metric.outcome.as_deref() {
            Some("succeeded") => "ok",
            Some("failed") => "falha",
            _ => "desconhecido",
        };
        let identity = format!("{observed_at}  {}  {short_task}", metric.provider);
        put_text(
            buffer,
            body.x.saturating_add(1),
            y,
            body.width.saturating_sub(1),
            &identity,
            style,
        );
        let numbers = format!(
            "{}→{} tokens  {cost}  {outcome}",
            metric.input_tokens, metric.output_tokens
        );
        put_text(
            buffer,
            body.x.saturating_add(1),
            y + 1,
            body.width.saturating_sub(1),
            &numbers,
            style,
        );
    }
}

fn render_list(
    buffer: &mut Buffer,
    body: Rect,
    config: &ClientShellConfig,
    state: &mut LearnViewState,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    if state.changes.is_empty() {
        put_text(
            buffer,
            body.x.saturating_add(1),
            body.y,
            body.width.saturating_sub(1),
            if state.load_error {
                "Learn indisponível · verifique/reinicie servidor"
            } else if state.loading {
                "Carregando alterações…"
            } else {
                "Nenhuma alteração Learn encontrada"
            },
            Style::default().fg(palette.overlay0).bg(palette.panel_bg),
        );
        return;
    }
    state.selected = state.selected.min(state.changes.len() - 1);
    if state.selected < state.scroll {
        state.scroll = state.selected;
    }
    if state.selected >= state.scroll + body.height as usize {
        state.scroll = state.selected + 1 - body.height as usize;
    }
    for (index, change) in state
        .changes
        .iter()
        .enumerate()
        .skip(state.scroll)
        .take(body.height as usize)
    {
        let y = body.y + (index - state.scroll) as u16;
        let row = Rect::new(body.x, y, body.width, 1);
        let style = if index == state.selected {
            Style::default()
                .fg(super::render::semantic_status_color(
                    palette,
                    &change.status,
                ))
                .bg(palette.active_row_bg)
        } else {
            Style::default()
                .fg(super::render::semantic_status_color(
                    palette,
                    &change.status,
                ))
                .bg(palette.panel_bg)
        };
        buffer.set_style(row, style);
        let line = format!(
            "{}  {}  {}  {}",
            change.status, change.target_id, change.id, change.created_at_ms
        );
        put_text(
            buffer,
            row.x.saturating_add(1),
            y,
            row.width.saturating_sub(1),
            &line,
            style,
        );
        hits.learn_rows.push((row, index));
    }
}

fn render_detail(
    buffer: &mut Buffer,
    body: Rect,
    config: &ClientShellConfig,
    state: &LearnViewState,
) {
    let palette = &config.palette;
    let Some(change) = state.changes.get(state.selected) else {
        return;
    };
    let mut lines = vec![
        format!("ID: {}", change.id),
        format!("Alvo: {}", change.target_id),
        format!("Estado: {}", change.status),
        format!("Criada em (ms UTC): {}", change.created_at_ms),
    ];
    if state.confirm_undo_id.as_deref() == Some(change.id.as_str()) {
        lines.push("Confirmar undo: pressione u novamente; Esc cancela".to_owned());
    }
    if let Some(detail) = state
        .detail
        .as_ref()
        .filter(|detail| detail.change.id == change.id)
    {
        lines.push(format!("Hash anterior: {}", detail.before_hash));
        lines.push(format!("Hash posterior: {}", detail.after_hash));
        lines.push("Motivo: [redigido]".to_owned());
        lines.push(format!(
            "Backup privado: {}",
            match detail.backup_available {
                Some(true) => "verificado",
                Some(false) => "indisponível",
                None => "não informado",
            }
        ));
    }
    match state
        .diff
        .as_ref()
        .filter(|diff| diff.change.id == change.id)
    {
        Some(diff) => match &diff.summary {
            Some(summary) => {
                lines.push(format!(
                    "Diff: -{} +{} linhas; prefixo {} / sufixo {} iguais",
                    summary.removed_lines,
                    summary.added_lines,
                    summary.unchanged_prefix_lines,
                    summary.unchanged_suffix_lines,
                ));
                lines.push("Conteúdo do diff: [redigido]".to_owned());
            }
            None => lines.push("Diff indisponível: snapshot posterior ausente".to_owned()),
        },
        None => lines.push("Diff: carregando…".to_owned()),
    }
    for (offset, line) in lines.iter().take(body.height as usize).enumerate() {
        put_text(
            buffer,
            body.x.saturating_add(1),
            body.y + offset as u16,
            body.width.saturating_sub(1),
            line,
            Style::default().fg(palette.text).bg(palette.panel_bg),
        );
    }
}

fn render_triggers(
    buffer: &mut Buffer,
    body: Rect,
    config: &ClientShellConfig,
    state: &mut LearnViewState,
) {
    let palette = &config.palette;
    let Some(status) = &state.status else {
        put_text(
            buffer,
            body.x.saturating_add(1),
            body.y,
            body.width.saturating_sub(1),
            "Carregando gatilhos…",
            Style::default().fg(palette.overlay0).bg(palette.panel_bg),
        );
        return;
    };
    let triggers = &status.recent_triggers;
    if triggers.is_empty() {
        put_text(
            buffer,
            body.x.saturating_add(1),
            body.y,
            body.width.saturating_sub(1),
            "Nenhum gatilho Learn recente",
            Style::default().fg(palette.overlay0).bg(palette.panel_bg),
        );
        return;
    }
    state.selected_trigger = state.selected_trigger.min(triggers.len() - 1);
    if state.selected_trigger < state.trigger_scroll {
        state.trigger_scroll = state.selected_trigger;
    }
    if state.selected_trigger >= state.trigger_scroll + body.height as usize {
        state.trigger_scroll = state.selected_trigger + 1 - body.height as usize;
    }
    for (index, trigger) in triggers
        .iter()
        .enumerate()
        .skip(state.trigger_scroll)
        .take(body.height as usize)
    {
        let y = body.y + (index - state.trigger_scroll) as u16;
        let row = Rect::new(body.x, y, body.width, 1);
        let style = if index == state.selected_trigger {
            Style::default()
                .fg(super::render::semantic_status_color(
                    palette,
                    &trigger.status,
                ))
                .bg(palette.active_row_bg)
        } else {
            Style::default()
                .fg(super::render::semantic_status_color(
                    palette,
                    &trigger.status,
                ))
                .bg(palette.panel_bg)
        };
        buffer.set_style(row, style);
        let line = format!(
            "{}  {}  {}  {}  {} → {}",
            trigger.provider,
            trigger.kind,
            trigger.fingerprint,
            trigger.status,
            trigger.created_at_ms,
            trigger
                .finished_at_ms
                .map_or_else(|| "pendente".to_owned(), |value| value.to_string()),
        );
        put_text(
            buffer,
            row.x.saturating_add(1),
            y,
            row.width.saturating_sub(1),
            &line,
            style,
        );
    }
}

impl ClientShellState {
    pub(super) fn toggle_learn_view(&mut self, outcome: &mut ClientShellInput) {
        if self.view == ClientShellView::Learn {
            self.view = ClientShellView::Terminal;
        } else {
            self.view = ClientShellView::Learn;
            self.learn_view = LearnViewState::default();
            self.refresh_learn(outcome);
        }
        outcome.repaint = true;
    }

    pub(super) fn refresh_learn(&mut self, outcome: &mut ClientShellInput) {
        if self.learn_view.loading {
            self.learn_view.refresh_pending = true;
            return;
        }
        self.learn_view.load_error = false;
        let list_started = self.push_endpoint_method_with_kind(
            crate::api::schema::Method::LearnList(crate::api::schema::LearnListParams {
                limit: Some(100),
            }),
            PendingEndpointKind::LearnList,
            outcome,
        );
        let status_started = self.push_endpoint_method_with_kind(
            crate::api::schema::Method::LearnStatus(crate::api::schema::EmptyParams::default()),
            PendingEndpointKind::LearnStatus,
            outcome,
        );
        self.learn_view.loading = list_started;
        self.learn_view.load_error = !list_started || !status_started;
        outcome.repaint = true;
    }

    pub(super) fn open_selected_learn(&mut self, outcome: &mut ClientShellInput) {
        let Some(change_id) = self
            .learn_view
            .changes
            .get(self.learn_view.selected)
            .map(|change| change.id.clone())
        else {
            return;
        };
        self.learn_view.page = LearnPage::Detail;
        self.learn_view.detail = None;
        self.learn_view.diff = None;
        self.learn_view.confirm_undo_id = None;
        self.request_learn_detail(change_id, outcome);
        outcome.repaint = true;
    }

    pub(super) fn request_learn_detail(
        &mut self,
        change_id: String,
        outcome: &mut ClientShellInput,
    ) {
        let target = crate::api::schema::LearnChangeTarget {
            change_id: change_id.clone(),
        };
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::LearnShow(target.clone()),
            PendingEndpointKind::LearnShow {
                change_id: change_id.clone(),
            },
            outcome,
        );
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::LearnDiff(target),
            PendingEndpointKind::LearnDiff { change_id },
            outcome,
        );
    }

    pub(super) fn handle_learn_view_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        match key.code {
            KeyCode::Char('q') => self.toggle_learn_view(outcome),
            KeyCode::Esc if self.learn_view.confirm_undo_id.take().is_some() => {}
            KeyCode::Esc if self.learn_view.page == LearnPage::Detail => {
                self.learn_view.page = LearnPage::List;
            }
            KeyCode::Esc if self.learn_view.page == LearnPage::Triggers => {
                self.learn_view.page = LearnPage::List;
            }
            KeyCode::Esc if self.learn_view.page == LearnPage::Metrics => {
                self.learn_view.page = LearnPage::List;
            }
            KeyCode::Esc => self.toggle_learn_view(outcome),
            KeyCode::Char('t') if self.learn_view.page == LearnPage::List => {
                self.learn_view.page = LearnPage::Triggers;
            }
            KeyCode::Char('t') if self.learn_view.page == LearnPage::Triggers => {
                self.learn_view.page = LearnPage::List;
            }
            KeyCode::Char('m') if self.learn_view.page == LearnPage::List => {
                self.learn_view.page = LearnPage::Metrics;
            }
            KeyCode::Char('m') if self.learn_view.page == LearnPage::Metrics => {
                self.learn_view.page = LearnPage::List;
            }
            KeyCode::Char('r') => {
                self.learn_view.confirm_undo_id = None;
                self.refresh_learn(outcome);
                if self.learn_view.page == LearnPage::Detail {
                    if let Some(change) = self.learn_view.changes.get(self.learn_view.selected) {
                        self.request_learn_detail(change.id.clone(), outcome);
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') if self.learn_view.page == LearnPage::List => {
                self.learn_view.selected = self.learn_view.selected.saturating_sub(1);
                self.learn_view.confirm_undo_id = None;
            }
            KeyCode::Down | KeyCode::Char('j') if self.learn_view.page == LearnPage::List => {
                self.learn_view.selected = (self.learn_view.selected + 1)
                    .min(self.learn_view.changes.len().saturating_sub(1));
                self.learn_view.confirm_undo_id = None;
            }
            KeyCode::Up | KeyCode::Char('k') if self.learn_view.page == LearnPage::Triggers => {
                self.learn_view.selected_trigger =
                    self.learn_view.selected_trigger.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') if self.learn_view.page == LearnPage::Triggers => {
                let len = self
                    .learn_view
                    .status
                    .as_ref()
                    .map_or(0, |status| status.recent_triggers.len());
                self.learn_view.selected_trigger =
                    (self.learn_view.selected_trigger + 1).min(len.saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') if self.learn_view.page == LearnPage::Metrics => {
                self.learn_view.selected_metric = self.learn_view.selected_metric.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') if self.learn_view.page == LearnPage::Metrics => {
                let len = self
                    .learn_view
                    .status
                    .as_ref()
                    .map_or(0, |status| status.recent_metrics.len());
                self.learn_view.selected_metric =
                    (self.learn_view.selected_metric + 1).min(len.saturating_sub(1));
            }
            KeyCode::Enter if self.learn_view.page == LearnPage::List => {
                self.open_selected_learn(outcome);
            }
            KeyCode::Char('u') if self.learn_view.page == LearnPage::Detail => {
                let Some(change) = self.learn_view.changes.get(self.learn_view.selected) else {
                    return;
                };
                if !matches!(change.status.as_str(), "applied" | "conflict") {
                    self.set_endpoint_error(
                        "Learn · alteração não pode ser revertida neste estado",
                    );
                } else if self.learn_view.confirm_undo_id.as_deref() == Some(change.id.as_str()) {
                    let change_id = change.id.clone();
                    self.learn_view.confirm_undo_id = None;
                    self.push_endpoint_method_with_kind(
                        crate::api::schema::Method::LearnUndo(
                            crate::api::schema::LearnChangeTarget {
                                change_id: change_id.clone(),
                            },
                        ),
                        PendingEndpointKind::LearnUndo { change_id },
                        outcome,
                    );
                } else {
                    self.learn_view.confirm_undo_id = Some(change.id.clone());
                }
            }
            _ => {
                self.learn_view.confirm_undo_id = None;
            }
        }
        outcome.repaint = true;
    }
}
