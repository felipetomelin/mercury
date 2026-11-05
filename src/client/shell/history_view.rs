//! Server-backed local conversation search on the persistent pane surface.

use crossterm::event::KeyCode;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Clear, Widget},
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::render::put_text;
use super::*;

static NEXT_HISTORY_AGENT: AtomicU32 = AtomicU32::new(0);

pub(super) fn history_agent_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let nonce = NEXT_HISTORY_AGENT.fetch_add(1, Ordering::Relaxed);
    format!("history-{nanos:016x}-{:04x}", nonce & 0xffff)
}

fn from_date(mode: u8) -> Option<String> {
    let days = match mode {
        1 => 30,
        2 => 7,
        _ => return None,
    };
    let date = (time::OffsetDateTime::now_utc() - time::Duration::days(days)).date();
    date.format(&time::format_description::parse("[year]-[month]-[day]").ok()?)
        .ok()
}

pub(super) fn render_history_view(
    buffer: &mut Buffer,
    area: Rect,
    config: &ClientShellConfig,
    state: &mut HistoryViewState,
    hits: &mut ShellHitMap,
) {
    hits.history_rows.clear();
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
        "History · conversas Claude + Codex",
        Style::default()
            .fg(palette.accent)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    if area.height < 2 {
        return;
    }
    let provider = state.provider.as_deref().unwrap_or("todos");
    let project = state.cwd.as_deref().unwrap_or("todos");
    let date = match state.date_mode {
        1 => "30d",
        2 => "7d",
        _ => "todas",
    };
    let hint = format!("/ buscar  Enter reabrir  p agente:{provider}  d data:{date}  w projeto:{project}  r atualizar  q sair");
    put_text(
        buffer,
        area.x.saturating_add(1),
        area.y + 1,
        area.width.saturating_sub(1),
        &hint,
        Style::default().fg(palette.overlay0).bg(palette.panel_bg),
    );
    if area.height < 3 {
        return;
    }
    let query = format!(
        "{}Busca: {}{}",
        if state.editing { "> " } else { "  " },
        state.query,
        if state.loading {
            "  (carregando…)"
        } else {
            ""
        }
    );
    put_text(
        buffer,
        area.x.saturating_add(1),
        area.y + 2,
        area.width.saturating_sub(1),
        &query,
        Style::default()
            .fg(if state.editing {
                palette.accent
            } else if state.loading {
                palette.yellow
            } else {
                palette.text
            })
            .bg(palette.panel_bg),
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
    if state.hits.is_empty() {
        let empty = if state.loading {
            "Buscando conversas…"
        } else {
            "Nenhuma conversa encontrada"
        };
        put_text(
            buffer,
            body.x.saturating_add(1),
            body.y,
            body.width.saturating_sub(1),
            empty,
            Style::default().fg(palette.overlay0).bg(palette.panel_bg),
        );
        return;
    }
    state.selected = state.selected.min(state.hits.len() - 1);
    if state.selected < state.scroll {
        state.scroll = state.selected;
    }
    if state.selected >= state.scroll + body.height as usize {
        state.scroll = state.selected + 1 - body.height as usize;
    }
    for (index, hit) in state
        .hits
        .iter()
        .enumerate()
        .skip(state.scroll)
        .take(body.height as usize)
    {
        let y = body.y + (index - state.scroll) as u16;
        let row = Rect::new(body.x, y, body.width, 1);
        let style = if index == state.selected {
            Style::default().fg(palette.text).bg(palette.active_row_bg)
        } else {
            Style::default().fg(palette.text).bg(palette.panel_bg)
        };
        buffer.set_style(row, style);
        let project = hit.cwd.as_deref().unwrap_or("?");
        let date = hit.timestamp.get(..10).unwrap_or(&hit.timestamp);
        let line = format!(
            "{}  {}  {}  {}  {}",
            hit.provider, date, hit.snippet, project, hit.session_id
        );
        put_text(
            buffer,
            row.x.saturating_add(1),
            y,
            row.width.saturating_sub(1),
            &line,
            style,
        );
        hits.history_rows.push((row, index));
    }
}

impl ClientShellState {
    pub(crate) fn tick_history_resume(&mut self, now: std::time::Instant) -> ClientShellInput {
        let mut outcome = ClientShellInput::default();
        if self
            .history_resume_retry
            .as_ref()
            .is_none_or(|retry| now < retry.at)
        {
            return outcome;
        }
        let Some(retry) = self.history_resume_retry.take() else {
            return outcome;
        };
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::AgentStart(retry.params.clone()),
            PendingEndpointKind::HistoryAgentStart {
                params: retry.params,
                attempts: retry.attempts,
            },
            &mut outcome,
        );
        outcome
    }

    pub(super) fn toggle_history_view(&mut self, outcome: &mut ClientShellInput) {
        if self.view == ClientShellView::History {
            self.view = ClientShellView::Terminal;
        } else {
            self.view = ClientShellView::History;
            self.history_view = HistoryViewState::default();
            self.refresh_history(outcome);
        }
        outcome.repaint = true;
    }

    pub(super) fn refresh_history(&mut self, outcome: &mut ClientShellInput) {
        if self.history_view.loading {
            self.history_view.refresh_pending = true;
            return;
        }
        let params = crate::api::schema::HistorySearchParams {
            query: self.history_view.query.clone(),
            provider: self.history_view.provider.clone(),
            cwd: self.history_view.cwd.clone(),
            from: from_date(self.history_view.date_mode),
            until: None,
            limit: 100,
        };
        self.history_view.loading = self.push_endpoint_method_with_kind(
            crate::api::schema::Method::HistorySearch(params),
            PendingEndpointKind::HistorySearch,
            outcome,
        );
        outcome.repaint = true;
    }

    pub(super) fn open_selected_history(&mut self, outcome: &mut ClientShellInput) {
        let Some(hit) = self.history_view.hits.get(self.history_view.selected) else {
            return;
        };
        let (provider, session_id) = (hit.provider.clone(), hit.session_id.clone());
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::HistoryResolve(crate::api::schema::HistoryResolveParams {
                provider: provider.clone(),
                session_id: session_id.clone(),
            }),
            PendingEndpointKind::HistoryResolve {
                provider,
                session_id,
            },
            outcome,
        );
        outcome.repaint = true;
    }

    pub(super) fn handle_history_view_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        if self.history_view.editing {
            match key.code {
                KeyCode::Enter => {
                    self.history_view.editing = false;
                    self.history_view.editing_original_query = None;
                    self.refresh_history(outcome);
                }
                KeyCode::Esc => {
                    self.history_view.editing = false;
                    if let Some(original) = self.history_view.editing_original_query.take() {
                        self.history_view.query = original;
                    }
                }
                KeyCode::Backspace => {
                    self.history_view.query.pop();
                }
                KeyCode::Char(ch)
                    if !ch.is_control() && self.history_view.query.len() + ch.len_utf8() <= 512 =>
                {
                    self.history_view.query.push(ch);
                }
                _ => {}
            }
            outcome.repaint = true;
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.toggle_history_view(outcome),
            KeyCode::Char('/') => {
                self.history_view.editing_original_query = Some(self.history_view.query.clone());
                self.history_view.editing = true;
            }
            KeyCode::Char('r') => self.refresh_history(outcome),
            KeyCode::Char('p') => {
                self.history_view.provider = match self.history_view.provider.as_deref() {
                    None => Some("claude".into()),
                    Some("claude") => Some("codex".into()),
                    _ => None,
                };
                self.refresh_history(outcome);
            }
            KeyCode::Char('d') => {
                self.history_view.date_mode = (self.history_view.date_mode + 1) % 3;
                self.refresh_history(outcome);
            }
            KeyCode::Char('w') => {
                self.history_view.cwd = if self.history_view.cwd.is_some() {
                    None
                } else {
                    self.history_view
                        .hits
                        .get(self.history_view.selected)
                        .and_then(|hit| hit.cwd.clone())
                };
                self.refresh_history(outcome);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.history_view.selected = self.history_view.selected.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.history_view.selected = (self.history_view.selected + 1)
                    .min(self.history_view.hits.len().saturating_sub(1))
            }
            KeyCode::Enter => self.open_selected_history(outcome),
            _ => {}
        }
        outcome.repaint = true;
    }
}
