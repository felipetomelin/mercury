use super::*;
use crossterm::event::KeyCode;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Clear, Paragraph, Widget, Wrap},
};
#[derive(Default)]
pub(super) struct KnowledgeViewState {
    pub query: String,
    pub editing: bool,
    pub loading: bool,
    pub data: serde_json::Value,
    pub selected: usize,
    pub detail: bool,
    pub error: Option<String>,
}
pub(super) fn render(
    buffer: &mut Buffer,
    area: Rect,
    config: &ClientShellConfig,
    state: &KnowledgeViewState,
) {
    Clear.render(area, buffer);
    if area.is_empty() {
        return;
    }
    let palette = &config.palette;
    let mut lines = vec![
        "Knowledge · memória local".to_owned(),
        "/ buscar  Enter fonte  ↑↓ selecionar  r status  q sair".into(),
        format!(
            "Busca: {}{}",
            state.query,
            if state.loading {
                " (carregando…)"
            } else {
                ""
            }
        ),
    ];
    if let Some(e) = &state.error {
        lines.push(e.clone())
    }
    if let Some(hits) = state.data["hits"].as_array() {
        if state.detail {
            if let Some(hit) = hits.get(state.selected) {
                lines.push(format!(
                    "Fonte: {} · {} · revisão {}",
                    hit["path"].as_str().unwrap_or("?"),
                    hit["status"].as_str().unwrap_or("?"),
                    state.data["revision"].as_str().unwrap_or("?")
                ));
                lines.push(hit["text"].as_str().unwrap_or("").into());
            }
        } else {
            for (i, h) in hits.iter().enumerate() {
                lines.push(format!(
                    "{} {} · {}",
                    if i == state.selected { ">" } else { " " },
                    h["path"].as_str().unwrap_or("?"),
                    h["heading"].as_str().unwrap_or("")
                ));
            }
        }
        if hits.is_empty() {
            lines.push(
                "Nenhuma fonte encontrada · adicione notas ao vault e indexe novamente".into(),
            );
        }
    } else if state.data["enabled"] == false {
        lines.push("Memória desativada · configure [knowledge] para começar".into());
    } else if let Some(index) = state.data["index"].as_object() {
        lines.push("Memória local ativa".into());
        if let Some(automation) = state.data["automation"].as_object() {
            let state_label = if automation.get("enabled").and_then(|v| v.as_bool()) != Some(true) {
                "pausada"
            } else if automation.get("external_allowed").and_then(|v| v.as_bool()) != Some(true) {
                "indexação local · modelos aguardam configuração"
            } else {
                match state.data["automation"]["state"]["runtime"]["state"].as_str() {
                    Some("error") => "atenção: consulte o diagnóstico abaixo",
                    Some("watching") => "ativa · monitorando notas e sessões",
                    _ => "iniciando ou processando",
                }
            };
            lines.push(format!("Automação: {state_label}"));
            let captures = &state.data["automation"]["state"]["captures"];
            lines.push(format!(
                "Capturas: {} concluídas · {} pendentes · {} falhas",
                captures["done"].as_u64().unwrap_or(0),
                captures["queued"].as_u64().unwrap_or(0)
                    + captures["published"].as_u64().unwrap_or(0),
                captures["failed"].as_u64().unwrap_or(0)
            ));
            if let Some(error) = state.data["automation"]["state"]["runtime"]["error"].as_str() {
                lines.push(error.into());
            }
            if let Some(error) = state.data["automation"]["state"]["graph"]["error"].as_str() {
                lines.push(format!("GraphRAG aguardando recuperação: {error}"));
            }
        }
        for (key, label) in [
            ("notes", "Notas"),
            ("chunks", "Trechos"),
            ("relations", "Relações explícitas"),
            ("entities", "Entidades"),
            ("inferred_relations", "Relações inferidas"),
            ("summaries", "Resumos"),
        ] {
            lines.push(format!(
                "{label}: {}",
                index.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
            ));
        }
        if index.get("notes").and_then(|v| v.as_u64()) == Some(0) {
            lines.push(
                "Vault vazio · adicione notas ou conclua uma sessão para capturar aprendizados"
                    .into(),
            );
        }
    } else if state.loading {
        lines.push("Consultando memória…".into());
    } else {
        lines.push("r consulta status · / busca nas fontes locais".into());
    }
    let styled: Vec<ratatui::text::Line> = lines
        .iter()
        .enumerate()
        .flat_map(|(i, line)| {
            let style = if i == 0 {
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(ratatui::style::Modifier::BOLD)
            } else if state.error.as_ref() == Some(line) {
                Style::default().fg(palette.red)
            } else if line.starts_with('>') {
                Style::default()
                    .fg(palette.accent)
                    .bg(palette.active_row_bg)
            } else if i == 1 {
                Style::default().fg(palette.subtext0)
            } else {
                Style::default().fg(palette.text)
            };
            line.split('\n').map(move |s| {
                ratatui::text::Line::styled(
                    s.chars().filter(|c| !c.is_control()).collect::<String>(),
                    style,
                )
            })
        })
        .collect();
    Paragraph::new(styled)
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(palette.text).bg(palette.panel_bg))
        .render(area, buffer);
}
impl ClientShellState {
    pub(super) fn toggle_knowledge_view(&mut self, outcome: &mut ClientShellInput) {
        if self.view == ClientShellView::Knowledge {
            self.view = ClientShellView::Terminal
        } else {
            self.view = ClientShellView::Knowledge;
            self.knowledge_view = KnowledgeViewState::default();
            self.request_knowledge(false, outcome);
        }
        outcome.repaint = true;
    }
    fn request_knowledge(&mut self, search: bool, outcome: &mut ClientShellInput) {
        if self.knowledge_view.loading {
            return;
        }
        let mut params = crate::api::schema::KnowledgeParams::new(if search {
            crate::api::schema::KnowledgeAction::Search
        } else {
            crate::api::schema::KnowledgeAction::Status
        });
        params.query = self.knowledge_view.query.clone();
        self.knowledge_view.loading = self.push_endpoint_method_with_kind(
            crate::api::schema::Method::Knowledge(params),
            PendingEndpointKind::Knowledge,
            outcome,
        );
        self.knowledge_view.error = None;
        outcome.repaint = true;
    }
    pub(super) fn handle_knowledge_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        if self.knowledge_view.editing {
            match key.code {
                KeyCode::Esc => self.knowledge_view.editing = false,
                KeyCode::Enter => {
                    self.knowledge_view.editing = false;
                    self.knowledge_view.selected = 0;
                    self.knowledge_view.detail = false;
                    self.request_knowledge(true, outcome);
                }
                KeyCode::Backspace => {
                    self.knowledge_view.query.pop();
                }
                KeyCode::Char(c) if !c.is_control() && self.knowledge_view.query.len() < 500 => {
                    self.knowledge_view.query.push(c)
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    if self.knowledge_view.detail {
                        self.knowledge_view.detail = false
                    } else {
                        self.view = ClientShellView::Terminal
                    }
                }
                KeyCode::Char('/') => self.knowledge_view.editing = true,
                KeyCode::Char('r') => self.request_knowledge(false, outcome),
                KeyCode::Down => {
                    let count = self.knowledge_view.data["hits"]
                        .as_array()
                        .map_or(0, Vec::len);
                    self.knowledge_view.selected =
                        (self.knowledge_view.selected + 1).min(count.saturating_sub(1));
                }
                KeyCode::Up => {
                    self.knowledge_view.selected = self.knowledge_view.selected.saturating_sub(1)
                }
                KeyCode::Enter => self.knowledge_view.detail = true,
                _ => {}
            }
        }
        outcome.repaint = true;
    }
}
