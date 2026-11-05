use crate::config::{
    AgentSidebarToken, AgentsSidebarConfig, SidebarTokenStyle, SpaceSidebarToken,
    SpacesSidebarConfig,
};
use crate::protocol::ClientShellAgentUsage;

/// Context fill (permille, 0..=1000+) at or above which the context gauge
/// renders red. Mirrors `compact_at` (Fase 2 passo 5 config); hardcoded here
/// until `[usage]` lands.
const CONTEXT_RED_AT_PERMILLE: u16 = 800;
/// Below this fill the gauge renders green; between it and
/// `CONTEXT_RED_AT_PERMILLE` it renders yellow.
const CONTEXT_YELLOW_AT_PERMILLE: u16 = 600;
/// Number of filled/empty cells in the `context` gauge bar (`▰▰▰▱▱`).
const CONTEXT_GAUGE_CELLS: u16 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedToken {
    pub kind: ResolvedTokenKind,
    pub style: SidebarTokenStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedTokenKind {
    StateIcon,
    StateText(String),
    Machine(String),
    Workspace(String),
    Tab(String),
    Pane(String),
    Agent(String),
    TerminalTitle(String),
    Branch(String),
    GitStatus {
        ahead: usize,
        behind: usize,
    },
    Custom(String),
    /// `context` token: `▰▰▰▱▱ 43%`. `permille` drives both the filled-cell
    /// count and the color threshold at render time; `exact` controls the
    /// `~` prefix for transcript-derived estimates (see D5/D8 in
    /// `docs/dev/usage-ui-design.md`).
    ContextGauge {
        permille: u16,
        exact: bool,
    },
    /// Plain-text usage tokens (`context_pct`, `tokens`, `cost`, `model`):
    /// already fully formatted, rendered like `Custom`.
    Text(String),
}

impl ResolvedTokenKind {
    fn text_value(&self) -> Option<&str> {
        match self {
            Self::StateText(value)
            | Self::Machine(value)
            | Self::Workspace(value)
            | Self::Tab(value)
            | Self::Pane(value)
            | Self::Agent(value)
            | Self::TerminalTitle(value)
            | Self::Branch(value)
            | Self::Custom(value)
            | Self::Text(value) => Some(value),
            Self::StateIcon | Self::GitStatus { .. } | Self::ContextGauge { .. } => None,
        }
    }
}

/// `tokens_in/tokens_cache_read -> cache hit rate`. Cache reads are tokens
/// that didn't need to be re-sent as fresh input, so the hit rate is the
/// share of "would-be input" that was served from cache:
/// `cache_read / (cache_read + tokens_in)`. `None` (segment omitted) when
/// both are zero, since there's nothing to take a ratio of yet.
fn cache_hit_rate_pct(tokens_in: u64, cache_read: u64) -> Option<u64> {
    let denominator = tokens_in.checked_add(cache_read)?;
    (denominator > 0).then(|| (cache_read.saturating_mul(100) + denominator / 2) / denominator)
}

/// Shared with the Usage view's Orçamentos section (D6/D7: token-unit budget
/// amounts use the same humanized-count formatting as the sidebar `tokens` token).
pub(crate) fn format_count(value: u64) -> String {
    fn scaled(value: f64, suffix: &str) -> String {
        let rounded = (value * 10.0).round() / 10.0;
        if (rounded.fract()).abs() < f64::EPSILON {
            format!("{rounded:.0}{suffix}")
        } else {
            format!("{rounded:.1}{suffix}")
        }
    }
    if value >= 1_000_000 {
        scaled(value as f64 / 1_000_000.0, "M")
    } else if value >= 1_000 {
        scaled(value as f64 / 1_000.0, "k")
    } else {
        value.to_string()
    }
}

/// `↑1.2k ↓22k ⟳97%` (in, out, cache hit); shared by the sidebar `tokens`
/// token and the Usage view's Agentes section (D6) so neither duplicates the
/// count-humanizing/cache-hit-rate logic.
pub(crate) fn format_tokens_token(usage: &ClientShellAgentUsage) -> String {
    let mut text = format!(
        "↑{} ↓{}",
        format_count(usage.tokens_in),
        format_count(usage.tokens_out)
    );
    if let Some(hit_rate) = cache_hit_rate_pct(usage.tokens_in, usage.tokens_cache_read) {
        text.push_str(&format!(" ⟳{hit_rate}%"));
    }
    text
}

/// `$3.41`; shared with the Usage view's Agentes section (D6).
pub(crate) fn format_cost_token(cost_micros_usd: u64) -> String {
    format!("${:.2}", cost_micros_usd as f64 / 1_000_000.0)
}

/// Strips the `claude-` prefix (`opus-5.5`); shared with the Usage view's
/// Agentes section (D6).
pub(crate) fn format_model_token(model: &str) -> String {
    model.strip_prefix("claude-").unwrap_or(model).to_string()
}

pub(crate) fn format_context_pct(permille: u16, exact: bool) -> String {
    let marker = if exact { "" } else { "~" };
    let pct = (u32::from(permille) + 5) / 10;
    format!("{marker}{pct}%")
}

pub(crate) fn context_gauge_color(
    permille: u16,
    palette: &crate::app::state::Palette,
) -> ratatui::style::Color {
    if permille >= CONTEXT_RED_AT_PERMILLE {
        palette.gauge_crit
    } else if permille >= CONTEXT_YELLOW_AT_PERMILLE {
        palette.gauge_warn
    } else {
        palette.gauge_ok
    }
}

pub(crate) fn context_gauge_filled_cells(permille: u16) -> u16 {
    let filled = (u32::from(permille) * u32::from(CONTEXT_GAUGE_CELLS) + 500) / 1000;
    u16::try_from(filled)
        .unwrap_or(CONTEXT_GAUGE_CELLS)
        .min(CONTEXT_GAUGE_CELLS)
}

pub(crate) fn context_gauge_text(permille: u16, exact: bool) -> String {
    let filled = context_gauge_filled_cells(permille);
    let empty = CONTEXT_GAUGE_CELLS - filled;
    format!(
        "{}{} {}",
        "▰".repeat(filled as usize),
        "▱".repeat(empty as usize),
        format_context_pct(permille, exact)
    )
}

impl ResolvedToken {
    fn new(kind: ResolvedTokenKind, style: SidebarTokenStyle) -> Self {
        Self { kind, style }
    }

    #[cfg(test)]
    pub(super) fn unstyled(kind: ResolvedTokenKind) -> Self {
        Self::new(kind, SidebarTokenStyle::default())
    }
}

pub(crate) struct AgentTokenContext<'a> {
    pub(crate) machine: Option<&'a str>,
    pub(crate) workspace: &'a str,
    pub(crate) tab: Option<&'a str>,
    pub(crate) pane: Option<&'a str>,
    pub(crate) agent_label: Option<&'a str>,
    pub(crate) terminal_title: Option<&'a str>,
    pub(crate) terminal_title_stripped: Option<&'a str>,
    pub(crate) canonical_agent: Option<crate::detect::Agent>,
    pub(crate) tokens: &'a std::collections::HashMap<String, String>,
    /// `None` when no hook-reported session is tracked for this pane ("n/a",
    /// never a fabricated number) — the `context`/`context_pct`/`tokens`/
    /// `cost`/`model` tokens all resolve to `None` in that case, so rows made
    /// only of those tokens disappear rather than showing stale/empty cells.
    pub(crate) usage: Option<&'a ClientShellAgentUsage>,
}

pub(crate) fn agent_rows(
    config: &AgentsSidebarConfig,
    context: AgentTokenContext<'_>,
    state_text: &str,
) -> Vec<Vec<ResolvedToken>> {
    config
        .rows_for_agent(context.canonical_agent)
        .iter()
        .filter_map(|row| {
            let resolved = row
                .iter()
                .filter_map(|configured| {
                    let (token, style) = configured.parts();
                    let kind = match token {
                        AgentSidebarToken::StateIcon => Some(ResolvedTokenKind::StateIcon),
                        AgentSidebarToken::StateText => {
                            Some(ResolvedTokenKind::StateText(state_text.to_string()))
                        }
                        AgentSidebarToken::Machine => context
                            .machine
                            .map(|value| ResolvedTokenKind::Machine(value.to_string())),
                        AgentSidebarToken::Workspace => {
                            Some(ResolvedTokenKind::Workspace(context.workspace.to_string()))
                        }
                        AgentSidebarToken::Tab => context
                            .tab
                            .map(|value| ResolvedTokenKind::Tab(value.to_string())),
                        AgentSidebarToken::Pane => context
                            .pane
                            .map(|value| ResolvedTokenKind::Pane(value.to_string())),
                        AgentSidebarToken::Agent => context
                            .agent_label
                            .map(|value| ResolvedTokenKind::Agent(value.to_string())),
                        AgentSidebarToken::TerminalTitle => context
                            .terminal_title
                            .map(|value| ResolvedTokenKind::TerminalTitle(value.to_string())),
                        AgentSidebarToken::TerminalTitleStripped => context
                            .terminal_title_stripped
                            .map(|value| ResolvedTokenKind::TerminalTitle(value.to_string())),
                        AgentSidebarToken::Context => context.usage.and_then(|usage| {
                            usage
                                .context_permille
                                .map(|permille| ResolvedTokenKind::ContextGauge {
                                    permille,
                                    exact: usage.context_exact,
                                })
                        }),
                        AgentSidebarToken::ContextPct => context.usage.and_then(|usage| {
                            usage.context_permille.map(|permille| {
                                ResolvedTokenKind::Text(format_context_pct(
                                    permille,
                                    usage.context_exact,
                                ))
                            })
                        }),
                        AgentSidebarToken::Tokens => context
                            .usage
                            .map(|usage| ResolvedTokenKind::Text(format_tokens_token(usage))),
                        AgentSidebarToken::Cost => context.usage.and_then(|usage| {
                            usage
                                .cost_micros_usd
                                .map(|cost| ResolvedTokenKind::Text(format_cost_token(cost)))
                        }),
                        AgentSidebarToken::Model => context.usage.and_then(|usage| {
                            usage
                                .model
                                .as_deref()
                                .map(|model| ResolvedTokenKind::Text(format_model_token(model)))
                        }),
                        AgentSidebarToken::Custom(name) => context
                            .tokens
                            .get(name)
                            .cloned()
                            .map(ResolvedTokenKind::Custom),
                        AgentSidebarToken::Styled { .. } => None,
                    }?;
                    let style = kind
                        .text_value()
                        .map_or(Some(style), |value| configured.style_for_value(value))?;
                    Some(ResolvedToken::new(kind, style))
                })
                .collect::<Vec<_>>();
            (!resolved.is_empty()).then_some(resolved)
        })
        .collect()
}

pub(crate) struct SpaceTokenContext<'a> {
    pub(crate) workspace: &'a str,
    pub(crate) branch: Option<&'a str>,
    pub(crate) state_text: &'a str,
    pub(crate) ahead_behind: Option<(usize, usize)>,
    pub(crate) tokens: &'a std::collections::HashMap<String, String>,
    pub(crate) suppress_git_details: bool,
}

pub(crate) fn space_rows(
    config: &SpacesSidebarConfig,
    context: SpaceTokenContext<'_>,
) -> Vec<Vec<ResolvedToken>> {
    config
        .rows
        .iter()
        .filter_map(|row| {
            let resolved = row
                .iter()
                .filter_map(|configured| {
                    let (token, style) = configured.parts();
                    let kind = match token {
                        SpaceSidebarToken::StateIcon => Some(ResolvedTokenKind::StateIcon),
                        SpaceSidebarToken::StateText => {
                            Some(ResolvedTokenKind::StateText(context.state_text.to_string()))
                        }
                        SpaceSidebarToken::Workspace => {
                            Some(ResolvedTokenKind::Workspace(context.workspace.to_string()))
                        }
                        SpaceSidebarToken::Branch if !context.suppress_git_details => context
                            .branch
                            .map(|branch| ResolvedTokenKind::Branch(branch.to_string())),
                        SpaceSidebarToken::Branch => None,
                        SpaceSidebarToken::GitStatus if !context.suppress_git_details => context
                            .ahead_behind
                            .filter(|(ahead, behind)| *ahead > 0 || *behind > 0)
                            .map(|(ahead, behind)| ResolvedTokenKind::GitStatus { ahead, behind }),
                        SpaceSidebarToken::GitStatus => None,
                        SpaceSidebarToken::Custom(name) => context
                            .tokens
                            .get(name)
                            .cloned()
                            .map(ResolvedTokenKind::Custom),
                        SpaceSidebarToken::Styled { .. } => None,
                    }?;
                    let style = kind
                        .text_value()
                        .map_or(Some(style), |value| configured.style_for_value(value))?;
                    Some(ResolvedToken::new(kind, style))
                })
                .collect::<Vec<_>>();
            (!resolved.is_empty()).then_some(resolved)
        })
        .collect()
}

pub(crate) fn separator(previous: &ResolvedToken, current: &ResolvedToken) -> &'static str {
    if matches!(previous.kind, ResolvedTokenKind::StateIcon)
        || matches!(current.kind, ResolvedTokenKind::GitStatus { .. })
    {
        " "
    } else {
        " · "
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentSidebarToken, SpaceSidebarToken};

    struct Entry {
        workspace: String,
        tab: Option<String>,
        pane: Option<String>,
        agent_label: Option<String>,
        terminal_title: Option<String>,
        terminal_title_stripped: Option<String>,
        canonical_agent: Option<crate::detect::Agent>,
        tokens: std::collections::HashMap<String, String>,
        usage: Option<ClientShellAgentUsage>,
    }

    fn entry() -> Entry {
        Entry {
            workspace: "repo".into(),
            tab: None,
            pane: None,
            agent_label: Some("pi".into()),
            terminal_title: None,
            terminal_title_stripped: None,
            canonical_agent: Some(crate::detect::Agent::Pi),
            tokens: std::collections::HashMap::new(),
            usage: None,
        }
    }

    fn usage_fixture() -> ClientShellAgentUsage {
        ClientShellAgentUsage {
            model: Some("claude-opus-5.5".into()),
            context_permille: Some(431),
            context_exact: true,
            context_window: Some(200_000),
            tokens_in: 1200,
            tokens_out: 22_000,
            tokens_cache_read: 38_800,
            tokens_cache_write: 0,
            cost_micros_usd: Some(3_410_000),
            updated_at_unix_ms: Some(0),
        }
    }

    fn context(entry: &Entry) -> AgentTokenContext<'_> {
        AgentTokenContext {
            machine: None,
            workspace: &entry.workspace,
            tab: entry.tab.as_deref(),
            pane: entry.pane.as_deref(),
            agent_label: entry.agent_label.as_deref(),
            terminal_title: entry.terminal_title.as_deref(),
            terminal_title_stripped: entry.terminal_title_stripped.as_deref(),
            canonical_agent: entry.canonical_agent,
            tokens: &entry.tokens,
            usage: entry.usage.as_ref(),
        }
    }

    #[test]
    fn conditional_styles_merge_first_match_and_keep_missing_values_absent() {
        let config: AgentsSidebarConfig = toml::from_str(r##"
rows = [["state_icon", { token = "machine", fg = "#fff", bold = true, dim = true, rules = [{ equals = "Local", fg = "#f00", bold = false }, { contains = "Loc", fg = "#0f0", dim = false }] }], [{ token = "$missing", rules = [{ equals = "", bold = true }] }]]
"##).unwrap();
        let entry = entry();
        for (machine, color, bold, dim) in [
            ("Local", (255, 0, 0), false, true),
            ("Localhost", (0, 255, 0), true, false),
            ("local", (255, 255, 255), true, true),
        ] {
            let mut context = context(&entry);
            context.machine = Some(machine);
            let rows = agent_rows(&config, context, "working");
            assert_eq!(rows.len(), 1);
            assert_eq!(
                rows[0][0],
                ResolvedToken::unstyled(ResolvedTokenKind::StateIcon)
            );
            let token = &rows[0][1];
            assert_eq!(token.kind, ResolvedTokenKind::Machine(machine.into()));
            assert_eq!(
                token.style.fg.unwrap().ratatui(),
                ratatui::style::Color::Rgb(color.0, color.1, color.2)
            );
            assert_eq!(token.style.bold, Some(bold));
            assert_eq!(token.style.dim, Some(dim));
        }
        assert_eq!(agent_rows(&config, context(&entry), "working")[0].len(), 1);
    }

    #[test]
    fn conditional_style_survives_truncation_and_removes_theme_modifiers() {
        use ratatui::style::{Color, Modifier, Style};
        let config: AgentsSidebarConfig = toml::from_str(r##"
rows = [[{ token = "workspace", rules = [{ equals = "long-workspace-name", fg = "#f00", bold = false, dim = false }] }]]
"##).unwrap();
        let mut entry = entry();
        entry.workspace = "long-workspace-name".into();
        let rows = agent_rows(&config, context(&entry), "working");
        let theme = Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD | Modifier::DIM);
        for width in [4, 40] {
            let spans = super::super::resolved_token_spans(
                &rows[0],
                ("*", theme),
                theme,
                theme,
                theme,
                theme,
                &super::super::Palette::catppuccin(),
                width,
            );
            assert_eq!(spans.len(), 1);
            assert!(super::super::display_width(&spans[0].content) <= width);
            assert_eq!(spans[0].style.fg, Some(Color::Rgb(255, 0, 0)));
            assert!(!spans[0]
                .style
                .add_modifier
                .intersects(Modifier::BOLD | Modifier::DIM));
            assert!(spans[0]
                .style
                .sub_modifier
                .contains(Modifier::BOLD | Modifier::DIM));
        }
    }

    #[test]
    fn custom_numeric_rules_resolve_in_agent_overrides_and_space_rows() {
        let config: crate::config::SidebarConfig = toml::from_str(
            r#"
[agents]
rows = [["workspace"]]
[agents.rows_by_agent]
pi = [[{ token = "$load", rules = [{ gt = 80, bold = true }, { gt = 50, dim = true }] }]]
[spaces]
rows = [[{ token = "$load", rules = [{ lt = 50, dim = true }] }]]
"#,
        )
        .unwrap();
        let mut entry = entry();
        for (value, bold, dim) in [
            ("90", Some(true), None),
            ("60", None, Some(true)),
            ("20", None, None),
            ("90%", None, None),
        ] {
            entry.tokens.insert("load".into(), value.into());
            let rows = agent_rows(&config.agents, context(&entry), "working");
            assert_eq!(rows[0][0].kind, ResolvedTokenKind::Custom(value.into()));
            assert_eq!(rows[0][0].style.bold, bold);
            assert_eq!(rows[0][0].style.dim, dim);
            let spaces = space_rows(
                &config.spaces,
                SpaceTokenContext {
                    workspace: "repo",
                    branch: None,
                    state_text: "working",
                    ahead_behind: None,
                    suppress_git_details: false,
                    tokens: &entry.tokens,
                },
            );
            assert_eq!(spaces[0][0].style.dim, (value == "20").then_some(true));
        }
    }

    #[test]
    fn conditional_hide_removes_tokens_and_empty_rows() {
        let config: crate::config::SidebarConfig = toml::from_str(
            r##"
[agents]
rows = [[{ token = "machine", fg = "#61afef", rules = [{ equals = "Local", hide = true }] }, "agent"]]
[agents.rows_by_agent]
pi = [[{ token = "$load", rules = [{ lt = 50, hide = true }] }], ["agent"]]
[spaces]
rows = [[{ token = "$load", rules = [{ lt = 50, hide = true }] }], ["workspace"]]
"##,
        ).unwrap();
        let encoded = toml::to_string(&config).unwrap();
        assert!(encoded.contains("hide = true"));
        let config: crate::config::SidebarConfig = toml::from_str(&encoded).unwrap();
        let mut entry = entry();
        entry.canonical_agent = None;
        for (machine, count) in [("Local", 1), ("Remote", 2)] {
            let mut ctx = context(&entry);
            ctx.machine = Some(machine);
            let rows = agent_rows(&config.agents, ctx, "working");
            assert_eq!(rows[0].len(), count);
            assert_eq!(
                rows[0].last().unwrap().kind,
                ResolvedTokenKind::Agent("pi".into())
            );
        }
        entry.canonical_agent = Some(crate::detect::Agent::Pi);
        for (value, count) in [("20", 1), ("90", 2)] {
            entry.tokens.insert("load".into(), value.into());
            assert_eq!(
                agent_rows(&config.agents, context(&entry), "working").len(),
                count
            );
            let rows = space_rows(
                &config.spaces,
                SpaceTokenContext {
                    workspace: "repo",
                    branch: None,
                    state_text: "working",
                    ahead_behind: None,
                    suppress_git_details: false,
                    tokens: &entry.tokens,
                },
            );
            assert_eq!(rows.len(), count);
        }
    }

    #[test]
    fn conditional_hide_preserves_first_match_wins() {
        for first in ["hide = false", "bold = true"] {
            let config: AgentsSidebarConfig = toml::from_str(&format!(
                "rows = [[{{ token = 'agent', rules = [{{ equals = 'pi', {first} }}, {{ contains = '', hide = true }}] }}]]"
            )).unwrap();
            let entry = entry();
            let rows = agent_rows(&config, context(&entry), "working");
            assert_eq!(rows[0][0].kind, ResolvedTokenKind::Agent("pi".into()));
        }
    }

    #[test]
    fn missing_custom_tokens_elide_rows_and_separators() {
        let entry = entry();
        let config = AgentsSidebarConfig {
            rows: vec![
                vec![
                    AgentSidebarToken::StateIcon,
                    AgentSidebarToken::Custom("missing".into()),
                ],
                vec![AgentSidebarToken::Custom("missing".into())],
                vec![AgentSidebarToken::Agent],
            ],
            ..Default::default()
        };

        let rows = agent_rows(&config, context(&entry), "working");

        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            vec![ResolvedToken::unstyled(ResolvedTokenKind::StateIcon)]
        );
        assert_eq!(
            rows[1],
            vec![ResolvedToken::unstyled(ResolvedTokenKind::Agent(
                "pi".into()
            ))]
        );
    }

    #[test]
    fn machine_token_only_resolves_for_multi_machine_rows() {
        let entry = entry();
        let config = AgentsSidebarConfig {
            rows: vec![vec![
                AgentSidebarToken::Machine,
                AgentSidebarToken::Workspace,
            ]],
            ..Default::default()
        };

        assert_eq!(
            agent_rows(&config, context(&entry), "working"),
            vec![vec![ResolvedToken::unstyled(ResolvedTokenKind::Workspace(
                "repo".into()
            ))]]
        );

        let mut remote_context = context(&entry);
        remote_context.machine = Some("Build");
        assert_eq!(
            agent_rows(&config, remote_context, "working"),
            vec![vec![
                ResolvedToken::unstyled(ResolvedTokenKind::Machine("Build".into())),
                ResolvedToken::unstyled(ResolvedTokenKind::Workspace("repo".into())),
            ]]
        );
    }

    #[test]
    fn state_text_and_arbitrary_values_are_independent_tokens() {
        let mut entry = entry();
        entry
            .tokens
            .insert("summary".into(), "reviewing auth".into());
        let config = AgentsSidebarConfig {
            rows: vec![vec![
                AgentSidebarToken::StateText,
                AgentSidebarToken::Custom("summary".into()),
            ]],
            ..Default::default()
        };

        assert_eq!(
            agent_rows(&config, context(&entry), "deep in the mines"),
            vec![vec![
                ResolvedToken::unstyled(ResolvedTokenKind::StateText("deep in the mines".into())),
                ResolvedToken::unstyled(ResolvedTokenKind::Custom("reviewing auth".into())),
            ]]
        );
    }

    #[test]
    fn terminal_title_builtins_are_distinct_from_custom_tokens() {
        let mut entry = entry();
        entry.terminal_title = Some("⠋ raw title".into());
        entry.terminal_title_stripped = Some("raw title".into());
        entry
            .tokens
            .insert("terminal_title".into(), "custom title".into());
        let config = AgentsSidebarConfig {
            rows: vec![vec![
                AgentSidebarToken::TerminalTitle,
                AgentSidebarToken::TerminalTitleStripped,
                AgentSidebarToken::Custom("terminal_title".into()),
            ]],
            ..Default::default()
        };

        assert_eq!(
            agent_rows(&config, context(&entry), "working"),
            vec![vec![
                ResolvedToken::unstyled(ResolvedTokenKind::TerminalTitle("⠋ raw title".into())),
                ResolvedToken::unstyled(ResolvedTokenKind::TerminalTitle("raw title".into())),
                ResolvedToken::unstyled(ResolvedTokenKind::Custom("custom title".into())),
            ]]
        );
    }

    #[test]
    fn known_agent_override_replaces_default_rows() {
        let mut config = AgentsSidebarConfig {
            rows: vec![vec![AgentSidebarToken::Workspace]],
            ..Default::default()
        };
        config
            .rows_by_agent
            .insert("pi".into(), vec![vec![AgentSidebarToken::Agent]]);
        let mut pi = entry();
        pi.agent_label = Some("renamed pi".into());

        assert_eq!(
            agent_rows(&config, context(&pi), "working"),
            vec![vec![ResolvedToken::unstyled(ResolvedTokenKind::Agent(
                "renamed pi".into()
            ))]]
        );

        pi.canonical_agent = None;
        assert_eq!(
            agent_rows(&config, context(&pi), "working"),
            vec![vec![ResolvedToken::unstyled(ResolvedTokenKind::Workspace(
                "repo".into()
            ))]]
        );
    }

    #[test]
    fn grouped_children_suppress_all_builtin_git_details() {
        let config = SpacesSidebarConfig::default();

        assert_eq!(
            space_rows(
                &config,
                SpaceTokenContext {
                    workspace: "feature",
                    branch: Some("worktree/feature"),
                    state_text: "idle",
                    ahead_behind: Some((2, 1)),
                    tokens: &std::collections::HashMap::new(),
                    suppress_git_details: true,
                },
            ),
            vec![vec![
                ResolvedToken::unstyled(ResolvedTokenKind::StateIcon),
                ResolvedToken::unstyled(ResolvedTokenKind::Workspace("feature".into())),
            ]]
        );
    }

    #[test]
    fn workspace_custom_token_can_replace_git_specific_details() {
        let tokens = std::collections::HashMap::from([("jj_status".into(), "2 changes".into())]);
        let config = SpacesSidebarConfig {
            rows: vec![vec![SpaceSidebarToken::Custom("jj_status".into())]],
            ..Default::default()
        };

        assert_eq!(
            space_rows(
                &config,
                SpaceTokenContext {
                    workspace: "repo",
                    branch: None,
                    state_text: "idle",
                    ahead_behind: None,
                    tokens: &tokens,
                    suppress_git_details: false,
                },
            ),
            vec![vec![ResolvedToken::unstyled(ResolvedTokenKind::Custom(
                "2 changes".into()
            ))]]
        );
    }

    #[test]
    fn usage_tokens_resolve_from_agent_usage() {
        let mut entry = entry();
        entry.usage = Some(usage_fixture());
        let config = AgentsSidebarConfig {
            rows: vec![vec![
                AgentSidebarToken::Model,
                AgentSidebarToken::Context,
                AgentSidebarToken::ContextPct,
                AgentSidebarToken::Tokens,
                AgentSidebarToken::Cost,
            ]],
            ..Default::default()
        };

        let rows = agent_rows(&config, context(&entry), "working");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0].kind, ResolvedTokenKind::Text("opus-5.5".into()));
        assert_eq!(
            rows[0][1].kind,
            ResolvedTokenKind::ContextGauge {
                permille: 431,
                exact: true,
            }
        );
        assert_eq!(rows[0][2].kind, ResolvedTokenKind::Text("43%".into()));
        assert_eq!(
            rows[0][3].kind,
            ResolvedTokenKind::Text("↑1.2k ↓22k ⟳97%".into())
        );
        assert_eq!(rows[0][4].kind, ResolvedTokenKind::Text("$3.41".into()));
    }

    #[test]
    fn usage_tokens_absent_without_usage_elide_the_row() {
        let entry = entry();
        assert_eq!(entry.usage, None);
        let config = AgentsSidebarConfig {
            rows: vec![
                vec![AgentSidebarToken::Context],
                vec![AgentSidebarToken::ContextPct],
                vec![AgentSidebarToken::Tokens],
                vec![AgentSidebarToken::Cost],
                vec![AgentSidebarToken::Model],
                vec![AgentSidebarToken::Agent],
            ],
            ..Default::default()
        };

        let rows = agent_rows(&config, context(&entry), "working");
        assert_eq!(
            rows,
            vec![vec![ResolvedToken::unstyled(ResolvedTokenKind::Agent(
                "pi".into()
            ))]]
        );
    }

    #[test]
    fn usage_tokens_absent_per_field_elide_only_that_token() {
        let mut entry = entry();
        entry.usage = Some(ClientShellAgentUsage {
            model: None,
            context_permille: None,
            context_exact: false,
            context_window: None,
            tokens_in: 0,
            tokens_out: 0,
            tokens_cache_read: 0,
            tokens_cache_write: 0,
            cost_micros_usd: None,
            updated_at_unix_ms: None,
        });
        let config = AgentsSidebarConfig {
            rows: vec![vec![
                AgentSidebarToken::Model,
                AgentSidebarToken::Context,
                AgentSidebarToken::ContextPct,
                AgentSidebarToken::Cost,
                AgentSidebarToken::Tokens,
            ]],
            ..Default::default()
        };

        let rows = agent_rows(&config, context(&entry), "working");
        // model/context/context_pct/cost all resolve to None (their field is
        // None); `tokens` is still present because it has no optional field
        // to be missing (all counters default to 0, which is a real value).
        assert_eq!(
            rows,
            vec![vec![ResolvedToken::unstyled(ResolvedTokenKind::Text(
                "↑0 ↓0".into()
            ))]]
        );
    }

    #[test]
    fn context_gauge_renders_bar_and_estimate_marker() {
        assert_eq!(context_gauge_text(431, true), "▰▰▱▱▱ 43%");
        assert_eq!(context_gauge_text(431, false), "▰▰▱▱▱ ~43%");
        assert_eq!(context_gauge_text(0, true), "▱▱▱▱▱ 0%");
        assert_eq!(context_gauge_text(1000, true), "▰▰▰▰▰ 100%");
        // Rounds to nearest cell/percent rather than always flooring.
        assert_eq!(context_gauge_text(199, true), "▰▱▱▱▱ 20%");
        assert_eq!(context_gauge_filled_cells(199), 1);
    }

    #[test]
    fn context_gauge_color_thresholds_match_green_yellow_red() {
        let palette = crate::app::state::Palette::catppuccin();
        assert_eq!(context_gauge_color(0, &palette), palette.gauge_ok);
        assert_eq!(context_gauge_color(599, &palette), palette.gauge_ok);
        assert_eq!(context_gauge_color(600, &palette), palette.gauge_warn);
        assert_eq!(context_gauge_color(799, &palette), palette.gauge_warn);
        assert_eq!(context_gauge_color(800, &palette), palette.gauge_crit);
        assert_eq!(context_gauge_color(1000, &palette), palette.gauge_crit);
    }

    #[test]
    fn format_count_scales_and_trims_trailing_zero() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_200), "1.2k");
        assert_eq!(format_count(22_000), "22k");
        assert_eq!(format_count(1_300_000), "1.3M");
        assert_eq!(format_count(137_000), "137k");
    }

    #[test]
    fn cache_hit_rate_uses_cache_read_over_cache_read_plus_tokens_in() {
        assert_eq!(cache_hit_rate_pct(0, 0), None);
        assert_eq!(cache_hit_rate_pct(100, 0), Some(0));
        assert_eq!(cache_hit_rate_pct(0, 100), Some(100));
        assert_eq!(cache_hit_rate_pct(3, 97), Some(97));
    }

    #[test]
    fn model_token_strips_claude_prefix_only_when_present() {
        let mut entry = entry();
        let config = AgentsSidebarConfig {
            rows: vec![vec![AgentSidebarToken::Model]],
            ..Default::default()
        };
        for (model, expected) in [
            ("claude-opus-5.5", "opus-5.5"),
            ("gpt-5.6", "gpt-5.6"),
            ("claude", "claude"),
        ] {
            entry.usage = Some(ClientShellAgentUsage {
                model: Some(model.into()),
                ..usage_fixture()
            });
            let rows = agent_rows(&config, context(&entry), "working");
            assert_eq!(rows[0][0].kind, ResolvedTokenKind::Text(expected.into()));
        }
    }
}
