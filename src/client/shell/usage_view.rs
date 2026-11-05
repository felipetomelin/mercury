//! The Usage view (`ClientShellView::Usage`): a persistent navigation mode
//! that replaces the pane surface with provider limits, evaluated budgets
//! (`[[usage.budget]]`, Fase 2 passo 5) and per-agent usage detail. See D6/D7
//! in `docs/dev/usage-ui-design.md` for the full layout and rationale.
//!
//! Follows the same "overlay drawn over `layout.pane_surface`, hits for
//! panes/splits/popup cleared" pattern as `render_mobile_switcher`
//! (`src/client/shell/mobile.rs`), but only covers the pane surface — the
//! sidebar and tab bar keep rendering around it.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Clear, Widget},
};

use super::render::{display_width, put_text};
use super::*;

/// One rendered row of the (scrollable) Usage view body. Section headers and
/// empty-state rows carry no `pane_id`; agent rows do, so Enter/click can
/// focus the pane and leave the view (D6).
struct UsageLine {
    text: String,
    style: Style,
    pane_id: Option<String>,
}

impl UsageLine {
    fn plain(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
            pane_id: None,
        }
    }
}

fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// `há 12s` for a past timestamp, `em 3h` for a future one (used for provider
/// reset times); never fabricates a number, only humanizes real timestamps
/// the server already reported.
fn format_relative_ms(now_ms: i64, at_ms: i64) -> String {
    let diff = now_ms - at_ms;
    let (prefix, magnitude) = if diff >= 0 {
        ("há ", diff)
    } else {
        ("em ", -diff)
    };
    let secs = magnitude / 1000;
    let text = if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    };
    format!("{prefix}{text}")
}

fn header_age_text(snapshot: &ClientShellSnapshot) -> String {
    let latest = snapshot
        .agents
        .iter()
        .filter_map(|agent| {
            agent
                .usage
                .as_ref()
                .and_then(|usage| usage.updated_at_unix_ms)
        })
        .chain(
            snapshot
                .usage
                .as_ref()
                .into_iter()
                .flat_map(|u| u.providers.iter().filter_map(|p| p.observed_at_unix_ms)),
        )
        .max();
    match latest {
        Some(at) => format!("dados de {}", format_relative_ms(now_unix_ms(), at)),
        None => "sem dados ainda".to_string(),
    }
}

fn format_window_minutes(minutes: u64) -> String {
    if minutes == 0 {
        "n/a".to_string()
    } else if minutes.is_multiple_of(1440) {
        format!("{}d", minutes / 1440)
    } else if minutes.is_multiple_of(60) {
        format!("{}h", minutes / 60)
    } else {
        format!("{minutes}min")
    }
}

fn section_header(label: &str, width: usize) -> String {
    let mut text = format!("─ {label} ");
    while display_width(&text) < width as u16 {
        text.push('─');
    }
    text
}

fn provider_lines(
    providers: &[crate::protocol::ClientShellProviderUsage],
    palette: &Palette,
) -> Vec<UsageLine> {
    let mut lines = Vec::new();
    for p in providers {
        let stale = p.status == "desatualizado";
        let color = if stale {
            palette.yellow
        } else if p.status == "indisponível" {
            palette.red
        } else {
            palette.accent
        };
        let plan = p.plan_type.as_deref().unwrap_or("plano não informado");
        lines.push(UsageLine::plain(
            format!("  {} · perfil atual · plano {}", p.provider, plan),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
        let age = p
            .observed_at_unix_ms
            .map(|at| format_relative_ms(now_unix_ms(), at))
            .unwrap_or_else(|| "sem amostra".into());
        lines.push(UsageLine::plain(
            format!("  {} · {} · {}", p.status, p.detail, age),
            Style::default().fg(if stale {
                palette.yellow
            } else {
                palette.subtext0
            }),
        ));
        if let Some(credits) = &p.credits {
            lines.push(UsageLine::plain(
                format!(
                    "  Créditos: {}",
                    if credits.unlimited {
                        "ilimitados"
                    } else if credits.has_credits {
                        "disponíveis"
                    } else {
                        "esgotados"
                    }
                ),
                Style::default().fg(palette.subtext0),
            ));
        }
        if p.limits.is_empty() {
            lines.push(UsageLine::plain(
                if p.plan_type.is_some() {
                    "  Plano não informou janelas de limite nesta consulta"
                } else {
                    "  Aguardando limites da conta · r atualizar"
                },
                Style::default().fg(palette.subtext0),
            ));
        }
        for limit in &p.limits {
            let used = (u32::from(limit.used_permille) + 5) / 10;
            let label = if limit.key == "individual" {
                format!(
                    "{}% restante · {}% utilizado",
                    100u32.saturating_sub(used),
                    used
                )
            } else {
                format!("{used}% utilizado")
            };
            let fill = usize::from(limit.used_permille.min(1000)) / 50;
            let bar = format!("{}{}", "━".repeat(fill), "─".repeat(20 - fill));
            let reset = limit
                .resets_at_unix_ms
                .map(|at| format_relative_ms(now_unix_ms(), at))
                .unwrap_or_else(|| "não informado".into());
            let window = limit
                .window_minutes
                .map(format_window_minutes)
                .unwrap_or_else(|| "cota da conta".into());
            let color = if limit.used_permille >= 900 {
                palette.gauge_crit
            } else if limit.used_permille >= 700 {
                palette.gauge_warn
            } else {
                palette.green
            };
            lines.push(UsageLine::plain(
                format!("  {} · {label} · {window}", limit.key),
                Style::default().fg(color),
            ));
            lines.push(UsageLine::plain(
                format!("  {bar} · renova {reset}"),
                Style::default().fg(palette.subtext0),
            ));
        }
        lines.push(UsageLine::plain("", Style::default()));
    }
    if lines.is_empty() {
        lines.push(UsageLine::plain(
            "  Consultando contas conectadas…",
            Style::default().fg(palette.subtext0),
        ));
    }
    lines
}

/// Number of filled/empty cells in a budget gauge bar (`▰▰▰▱▱▱▱▱▱▱`); wider
/// than the per-agent context gauge (D6 mockup) since this row has more
/// horizontal room to spare (no per-agent token/cost columns beside it).
const BUDGET_GAUGE_CELLS: u16 = 10;

fn budget_gauge_bar(used_permille: Option<u16>) -> String {
    let filled = used_permille.map_or(0, |permille| {
        ((u32::from(permille) * u32::from(BUDGET_GAUGE_CELLS) + 500) / 1000)
            .min(u32::from(BUDGET_GAUGE_CELLS)) as u16
    });
    format!(
        "{}{}",
        "▰".repeat(filled as usize),
        "▱".repeat((BUDGET_GAUGE_CELLS - filled) as usize)
    )
}

fn format_budget_amount(scaled: u64, unit: crate::protocol::ClientShellBudgetUnit) -> String {
    match unit {
        crate::protocol::ClientShellBudgetUnit::Usd => {
            format!("${:.2}", scaled as f64 / 1_000_000.0)
        }
        crate::protocol::ClientShellBudgetUnit::Tokens => crate::ui::format_count(scaled),
        crate::protocol::ClientShellBudgetUnit::Percent => {
            format!("{}%", (scaled + 5) / 10)
        }
        crate::protocol::ClientShellBudgetUnit::Unknown => scaled.to_string(),
    }
}

fn budget_level_color(
    level: crate::protocol::ClientShellBudgetLevel,
    palette: &Palette,
) -> ratatui::style::Color {
    match level {
        crate::protocol::ClientShellBudgetLevel::Exceeded => palette.gauge_crit,
        crate::protocol::ClientShellBudgetLevel::Warn => palette.gauge_warn,
        crate::protocol::ClientShellBudgetLevel::Ok => palette.text,
        crate::protocol::ClientShellBudgetLevel::Unavailable
        | crate::protocol::ClientShellBudgetLevel::Unknown => palette.overlay0,
    }
}

fn budget_lines(
    budgets: &[crate::protocol::ClientShellUsageBudget],
    palette: &Palette,
) -> Vec<UsageLine> {
    if budgets.is_empty() {
        return vec![UsageLine::plain(
            "  nenhum orçamento configurado ainda",
            Style::default().fg(palette.overlay0),
        )];
    }
    let now_ms = now_unix_ms();
    budgets
        .iter()
        .map(|budget| {
            let mut text = format!(
                "  {:<16} {}",
                budget.name,
                budget_gauge_bar(budget.used_permille)
            );
            if budget.level == crate::protocol::ClientShellBudgetLevel::Unavailable {
                text.push_str("  n/a (janela não reportada pela fonte)");
            } else {
                text.push_str(&format!(
                    "  {} / {}",
                    format_budget_amount(budget.used_scaled, budget.unit),
                    format_budget_amount(budget.limit_scaled, budget.unit)
                ));
                if let Some(permille) = budget.used_permille {
                    text.push_str(&format!("   {}%", (u32::from(permille) + 5) / 10));
                }
                if budget.level == crate::protocol::ClientShellBudgetLevel::Warn {
                    text.push_str(" ⚠");
                } else if budget.level == crate::protocol::ClientShellBudgetLevel::Exceeded {
                    text.push_str(" ✗");
                }
                if let Some(resets_at) = budget.resets_at_unix_ms {
                    text.push_str(&format!(
                        "  reset {}",
                        format_relative_ms(now_ms, resets_at)
                    ));
                }
                if let Some(forecast) = budget.forecast_exhausted_at_unix_ms {
                    text.push_str(&format!(
                        "  estoura {}",
                        format_relative_ms(now_ms, forecast)
                    ));
                }
                if budget.partial {
                    text.push_str(" · parcial");
                }
            }
            UsageLine::plain(
                text,
                Style::default().fg(budget_level_color(budget.level, palette)),
            )
        })
        .collect()
}

fn agent_lines(
    agents: &[crate::protocol::ClientShellAgent],
    config: &ClientShellConfig,
) -> Vec<UsageLine> {
    let relevant = agents
        .iter()
        .filter(|agent| agent.usage.is_some())
        .collect::<Vec<_>>();
    if relevant.is_empty() {
        return vec![UsageLine::plain(
            "  nenhum agente com usage reportado",
            Style::default().fg(config.palette.overlay0),
        )];
    }
    relevant
        .into_iter()
        .map(|agent| {
            let usage = agent.usage.as_ref().expect("filtered by usage.is_some()");
            let label = agent
                .display_agent
                .as_deref()
                .or(agent.name.as_deref())
                .or(agent.agent.as_deref())
                .or(agent.title.as_deref())
                .unwrap_or("agent");
            let model = usage
                .model
                .as_deref()
                .map(crate::ui::format_model_token)
                .unwrap_or_else(|| "n/a".to_string());
            let context = usage
                .context_permille
                .map(|permille| {
                    format!(
                        "ctx {}",
                        crate::ui::context_gauge_text(permille, usage.context_exact)
                    )
                })
                .unwrap_or_else(|| "ctx n/a".to_string());
            let tokens = crate::ui::format_tokens_token(usage);
            let cost = usage
                .cost_micros_usd
                .map(crate::ui::format_cost_token)
                .unwrap_or_else(|| "n/a".to_string());
            let dot = status_dot(agent.agent_status);
            let color = status_color(agent.agent_status, &config.palette);
            UsageLine {
                text: format!("{dot} {label} · {model}    {context}   {tokens}   {cost}"),
                style: Style::default().fg(color),
                pane_id: Some(agent.pane_id.clone()),
            }
        })
        .collect()
}

fn usage_lines(
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    width: usize,
) -> Vec<UsageLine> {
    let palette = &config.palette;
    let header_style = Style::default()
        .fg(palette.overlay1)
        .add_modifier(Modifier::BOLD);
    let mut lines = vec![UsageLine::plain(
        section_header("Provedor", width),
        header_style,
    )];
    let (providers, budgets) = snapshot
        .usage
        .as_ref()
        .map(|summary| (summary.providers.as_slice(), summary.budgets.as_slice()))
        .unwrap_or((&[], &[]));
    lines.extend(provider_lines(providers, palette));
    if let Some(activity) = snapshot.usage.as_ref().and_then(|u| u.activity.as_ref()) {
        lines.push(UsageLine::plain(
            "─ Histórico local · Hoje (UTC)",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ));
        let cache = if activity.input_today == 0 {
            0
        } else {
            u128::from(activity.cache_read_today) * 100 / u128::from(activity.input_today)
        };
        lines.push(UsageLine::plain(
            format!(
                "  {} tokens · {}% da entrada em cache",
                crate::ui::format_count(activity.tokens_today),
                cache
            ),
            Style::default().fg(palette.text),
        ));
        let cost = activity
            .cost_microusd_today
            .map(|v| {
                format!(
                    "≥ ${:.2} informado · cobertura parcial",
                    v as f64 / 1_000_000.0
                )
            })
            .unwrap_or_else(|| "Custo não informado pelas fontes".into());
        lines.push(UsageLine::plain(
            format!("  {cost}"),
            Style::default().fg(palette.subtext0),
        ));
        lines.push(UsageLine::plain(
            "  Logs locais podem incluir logins anteriores; não são a cota da conta.",
            Style::default().fg(palette.subtext0),
        ));
        for (provider, at) in &activity.last_activity {
            lines.push(UsageLine::plain(
                format!(
                    "  {provider}: última atividade {}",
                    format_relative_ms(now_unix_ms(), *at)
                ),
                Style::default().fg(palette.text),
            ));
        }
        let max = activity
            .daily_tokens
            .iter()
            .copied()
            .max()
            .unwrap_or(1)
            .max(1);
        let levels = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        let trend: String = activity
            .daily_tokens
            .iter()
            .map(|n| levels[(u128::from(*n) * 7 / u128::from(max)) as usize])
            .collect();
        lines.push(UsageLine::plain(
            format!("  Tendência 7 dias UTC: {trend} · último dia = hoje"),
            Style::default().fg(palette.accent),
        ));
    }
    lines.push(UsageLine::plain(
        section_header("Orçamentos", width),
        header_style,
    ));
    lines.extend(budget_lines(budgets, palette));
    lines.push(UsageLine::plain(
        section_header("Agentes", width),
        header_style,
    ));
    lines.extend(agent_lines(&snapshot.agents, config));
    lines
}

/// Indices into `lines` that carry a `pane_id` (i.e. agent rows), in order —
/// used to map the keyboard cursor (agent-only) onto the full scrollable
/// line list.
fn agent_line_indices(lines: &[UsageLine]) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| line.pane_id.is_some().then_some(index))
        .collect()
}

pub(super) fn render_usage_view(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    scroll: &mut usize,
    cursor: &mut Option<usize>,
    reveal_cursor: &mut bool,
    hits: &mut ShellHitMap,
) {
    hits.usage_rows.clear();
    hits.usage_body = Rect::default();
    hits.usage_max_scroll = 0;
    hits.usage_cursor_pane_id = None;
    if area.is_empty() {
        *scroll = 0;
        return;
    }
    let palette = &config.palette;
    Clear.render(area, buffer);
    buffer.set_style(area, Style::default().bg(palette.panel_bg));
    let age_text = header_age_text(snapshot);
    let age_width = display_width(&age_text).min(area.width.saturating_sub(1));
    put_text(
        buffer,
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(age_width + 3),
        "Usage · contas conectadas",
        Style::default()
            .fg(palette.text)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );

    put_text(
        buffer,
        area.right().saturating_sub(age_width).saturating_sub(1),
        area.y,
        age_width,
        &age_text,
        Style::default().fg(palette.overlay0).bg(palette.panel_bg),
    );
    if area.height < 2 {
        *scroll = 0;
        return;
    }
    put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        "r atualizar · ↑↓ agentes · Enter abrir · PgUp/PgDn rolar · q sair",
        Style::default().fg(palette.subtext0).bg(palette.panel_bg),
    );
    let viewport = Rect::new(
        area.x,
        area.y + 2,
        area.width,
        area.height.saturating_sub(2),
    );
    if viewport.is_empty() {
        *scroll = 0;
        return;
    }
    let content_width = viewport.width.saturating_sub(1) as usize;
    let lines = usage_lines(snapshot, config, content_width.max(1));
    let agent_indices = agent_line_indices(&lines);
    *cursor = if agent_indices.is_empty() {
        None
    } else {
        Some(cursor.unwrap_or(0).min(agent_indices.len() - 1))
    };
    let max_scroll = lines.len().saturating_sub(usize::from(viewport.height));
    if *reveal_cursor {
        if let Some(line_index) = cursor.and_then(|index| agent_indices.get(index).copied()) {
            if line_index < *scroll {
                *scroll = line_index;
            } else if line_index >= scroll.saturating_add(usize::from(viewport.height)) {
                *scroll = line_index + 1 - usize::from(viewport.height);
            }
        }
        *reveal_cursor = false;
    }
    *scroll = (*scroll).min(max_scroll);
    hits.usage_max_scroll = max_scroll;
    hits.usage_body = viewport;
    let cursor_line = cursor.and_then(|index| agent_indices.get(index).copied());
    hits.usage_cursor_pane_id =
        cursor_line.and_then(|line_index| lines[line_index].pane_id.clone());
    for (offset, line) in lines
        .iter()
        .enumerate()
        .skip(*scroll)
        .take(viewport.height as usize)
    {
        let y = viewport.y + (offset - *scroll) as u16;
        let row_rect = Rect::new(viewport.x, y, viewport.width, 1);
        let highlighted = cursor_line == Some(offset);
        if highlighted {
            buffer.set_style(row_rect, Style::default().bg(palette.active_row_bg));
        }
        put_text(
            buffer,
            viewport.x,
            y,
            viewport.width,
            &line.text,
            line.style,
        );
        if let Some(pane_id) = &line.pane_id {
            hits.usage_rows.push((row_rect, pane_id.clone()));
        }
    }
}

impl ClientShellState {
    /// Toggles between `Terminal` and `Usage` (`prefix+u` / the tab bar chip
    /// / `Esc`/`q` while open). See D6.
    pub(super) fn toggle_usage_view(&mut self, outcome: &mut ClientShellInput) {
        self.view = match self.view {
            ClientShellView::Terminal => ClientShellView::Usage,
            ClientShellView::Usage => ClientShellView::Terminal,
            ClientShellView::Crons => ClientShellView::Usage,
            ClientShellView::History => ClientShellView::Usage,
            ClientShellView::Learn | ClientShellView::Knowledge => ClientShellView::Usage,
        };
        if self.view == ClientShellView::Usage {
            self.usage_view_scroll = 0;
            self.usage_view_cursor = Some(0);
        }
        self.usage_view_reveal_cursor = false;
        outcome.repaint = true;
    }

    pub(crate) fn tick_account_usage(&mut self, now: std::time::Instant) -> ClientShellInput {
        let mut outcome = ClientShellInput::default();
        if self.view == ClientShellView::Usage
            && self.usage_refresh_due.is_none_or(|due| now >= due)
        {
            self.usage_refresh_due = Some(now + std::time::Duration::from_secs(60));
            self.push_endpoint_method(crate::api::schema::Method::UsageRefresh(crate::api::schema::EmptyParams::default()),&mut outcome);
        }
        outcome
    }

    pub(super) fn close_usage_view(&mut self, outcome: &mut ClientShellInput) {
        if self.view == ClientShellView::Usage {
            self.view = ClientShellView::Terminal;
            outcome.repaint = true;
        }
    }

    pub(super) fn focus_usage_row_pane(&mut self, pane_id: String, outcome: &mut ClientShellInput) {
        self.push_endpoint_method(
            crate::api::schema::Method::PaneFocus(crate::api::schema::PaneTarget { pane_id }),
            outcome,
        );
        self.close_usage_view(outcome);
    }

    /// Esc/q close the view; j/k/arrows move the agent-row cursor (the list
    /// auto-scrolls to keep it visible); Enter focuses the selected agent's
    /// pane and returns to Terminal (D6). Always consumes the key — nothing
    /// should leak to a hidden pane while this view is open.
    pub(super) fn handle_usage_view_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.close_usage_view(outcome),
            KeyCode::Char('r') => {
                self.push_endpoint_method(
                    crate::api::schema::Method::UsageRefresh(
                        crate::api::schema::EmptyParams::default(),
                    ),
                    outcome,
                );
                outcome.repaint = true;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.usage_view_cursor =
                    Some(self.usage_view_cursor.unwrap_or(0).saturating_sub(1));
                self.usage_view_reveal_cursor = true;
                outcome.repaint = true;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.usage_view_cursor =
                    Some(self.usage_view_cursor.unwrap_or(0).saturating_add(1));
                self.usage_view_reveal_cursor = true;
                outcome.repaint = true;
            }
            KeyCode::PageDown | KeyCode::PageUp | KeyCode::Home | KeyCode::End => {
                let page = usize::from(self.hits.usage_body.height).max(1);
                self.usage_view_scroll = match key.code {
                    KeyCode::Home => 0,
                    KeyCode::End => self.hits.usage_max_scroll,
                    KeyCode::PageUp => self.usage_view_scroll.saturating_sub(page),
                    _ => self
                        .usage_view_scroll
                        .saturating_add(page)
                        .min(self.hits.usage_max_scroll),
                };
                self.usage_view_reveal_cursor = false;
                outcome.repaint = true;
            }
            KeyCode::Enter => {
                if let Some(pane_id) = self.hits.usage_cursor_pane_id.clone() {
                    self.focus_usage_row_pane(pane_id, outcome);
                }
            }
            _ => {}
        }
    }
}
