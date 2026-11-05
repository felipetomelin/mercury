use std::collections::{HashMap, HashSet};

use crate::api::schema::AgentStatus;
use crate::config::UsageConfig;
use crate::protocol::{
    ClientShellAgent, ClientShellBudgetLevel, ClientShellSnapshot, PaneSurfaceFrame,
};

#[derive(Clone, Debug, Default)]
pub(super) struct EndpointAgentPresentation {
    boot_id: Option<String>,
    acknowledged: HashMap<String, u64>,
    completed: HashMap<String, u64>,
    working: HashSet<String>,
    /// Panes that already fired the "contexto alto" toast since the last
    /// time their `context_permille` dropped back below the threshold (or
    /// the pane disappeared). Keeps the alert a one-shot per crossing
    /// instead of repeating on every snapshot while context stays high.
    context_alerted: HashSet<String>,
    /// Budgets that already fired a `warn_at` toast, keyed by budget name,
    /// storing the highest `warn_threshold` already alerted. Mirrors
    /// `context_alerted`'s one-shot-per-crossing dedupe: cleared when the
    /// budget drops back to `Ok`/`Unavailable` (or disappears), so crossing
    /// a *higher* threshold (e.g. 70 -> 90) still re-fires.
    budget_alerted: HashMap<String, u8>,
    pending_completions: Option<(
        Option<u64>,
        crate::protocol::endpoint::EndpointAgentCompletions,
    )>,
}

impl EndpointAgentPresentation {
    pub(super) fn receive_completions(
        &mut self,
        generation: Option<u64>,
        completions: crate::protocol::endpoint::EndpointAgentCompletions,
    ) {
        if self
            .pending_completions
            .as_ref()
            .is_some_and(|(current_generation, current)| {
                *current_generation == generation
                    && current.boot_id == completions.boot_id
                    && current.revision >= completions.revision
            })
        {
            return;
        }
        self.pending_completions = Some((generation, completions));
    }

    #[cfg(test)]
    pub(super) fn project_snapshot(&mut self, snapshot: &mut ClientShellSnapshot) {
        self.project_snapshot_for_generation(snapshot, None);
    }

    pub(super) fn project_snapshot_for_generation(
        &mut self,
        snapshot: &mut ClientShellSnapshot,
        generation: Option<u64>,
    ) {
        if self.boot_id.as_deref() != Some(snapshot.boot_id.as_str()) {
            self.boot_id = Some(snapshot.boot_id.clone());
            self.acknowledged.clear();
            self.completed.clear();
            self.working.clear();
            self.context_alerted.clear();
            self.budget_alerted.clear();
            self.acknowledged.extend(
                snapshot
                    .agents
                    .iter()
                    .map(|agent| (agent.pane_id.clone(), agent.state_change_seq)),
            );
        }
        let pane_ids: HashSet<&str> = snapshot
            .agents
            .iter()
            .map(|agent| agent.pane_id.as_str())
            .collect();
        self.acknowledged
            .retain(|pane_id, _| pane_ids.contains(pane_id.as_str()));
        self.completed
            .retain(|pane_id, _| pane_ids.contains(pane_id.as_str()));
        self.working
            .retain(|pane_id| pane_ids.contains(pane_id.as_str()));
        self.context_alerted
            .retain(|pane_id| pane_ids.contains(pane_id.as_str()));
        let completions = self
            .pending_completions
            .take()
            .filter(|(received_generation, projection)| {
                *received_generation == generation
                    && projection.boot_id == snapshot.boot_id
                    && projection.revision == snapshot.revision
            })
            .map(|(_, projection)| projection.completions);
        for agent in &mut snapshot.agents {
            match agent.agent_status {
                AgentStatus::Working => {
                    self.working.insert(agent.pane_id.clone());
                    self.completed.remove(&agent.pane_id);
                }
                AgentStatus::Blocked => {
                    self.completed.remove(&agent.pane_id);
                }
                AgentStatus::Idle | AgentStatus::Done => {
                    let observed_work = self.working.remove(&agent.pane_id);
                    let completed = completions.as_ref().map_or_else(
                        || {
                            observed_work
                                || self.completed.get(&agent.pane_id)
                                    == Some(&agent.state_change_seq)
                        },
                        |completions| {
                            completions.get(&agent.pane_id) == Some(&agent.state_change_seq)
                        },
                    );
                    if completed {
                        self.completed
                            .insert(agent.pane_id.clone(), agent.state_change_seq);
                    } else {
                        self.completed.remove(&agent.pane_id);
                    }
                }
                _ => {
                    self.working.remove(&agent.pane_id);
                    self.completed.remove(&agent.pane_id);
                }
            }
            agent.agent_status = self.projected_status(agent);
        }
        project_aggregate_status(snapshot);
    }

    /// Returns one `SemanticNotification` per pane whose `context_permille`
    /// just crossed `usage_config`'s `compact_at` threshold (global, or the
    /// pane's agent-specific override — `UsageConfig::compact_at_permille_for`)
    /// since the last snapshot, so callers can route it through the regular
    /// notification pipeline (`ClientShellState::receive_notification`). A
    /// pane fires at most once per crossing: it must drop back below the
    /// threshold (or lose its usage data) before it can fire again.
    pub(super) fn context_alerts(
        &mut self,
        snapshot: &ClientShellSnapshot,
        usage_config: &UsageConfig,
    ) -> Vec<crate::protocol::SemanticNotification> {
        let mut alerts = Vec::new();
        for agent in &snapshot.agents {
            let threshold = usage_config.compact_at_permille_for(agent.agent.as_deref());
            let permille = agent
                .usage
                .as_ref()
                .and_then(|usage| usage.context_permille);
            let Some(permille) = permille.filter(|permille| *permille >= threshold) else {
                self.context_alerted.remove(&agent.pane_id);
                continue;
            };
            if !self.context_alerted.insert(agent.pane_id.clone()) {
                continue;
            }
            let exact = agent
                .usage
                .as_ref()
                .is_some_and(|usage| usage.context_exact);
            let estimate_marker = if exact { "" } else { "~" };
            let percent = permille / 10;
            alerts.push(crate::protocol::SemanticNotification {
                kind: crate::protocol::SemanticNotificationKind::Custom,
                title: format!("contexto {estimate_marker}{percent}% — considere /compact"),
                body: None,
                sound: None,
                agent: agent.agent.clone(),
                workspace_id: Some(agent.workspace_id.clone()),
                tab_id: Some(agent.tab_id.clone()),
                pane_id: Some(agent.pane_id.clone()),
                position: None,
            });
        }
        alerts
    }

    /// Returns one `SemanticNotification` per budget whose `warn_at` level
    /// just crossed a *new* threshold since the last snapshot (dedup per
    /// budget name + threshold, mirroring `context_alerts`). `on_exceed`
    /// (`notify`/`route`/`pause`) doesn't gate this: `route`/`pause` are
    /// no-ops until the Fase 5 router exists (see `mercury_usage::OnExceed`
    /// and `docs/dev/usage-ui-design.md`), so every budget still at least
    /// notifies.
    pub(super) fn budget_alerts(
        &mut self,
        snapshot: &ClientShellSnapshot,
    ) -> Vec<crate::protocol::SemanticNotification> {
        let mut alerts = Vec::new();
        let budgets = snapshot
            .usage
            .as_ref()
            .map(|usage| usage.budgets.as_slice())
            .unwrap_or_default();
        let active_names: HashSet<&str> =
            budgets.iter().map(|budget| budget.name.as_str()).collect();
        self.budget_alerted
            .retain(|name, _| active_names.contains(name.as_str()));
        for budget in budgets {
            let Some(threshold) = budget.warn_threshold.filter(|_| {
                matches!(
                    budget.level,
                    ClientShellBudgetLevel::Warn | ClientShellBudgetLevel::Exceeded
                )
            }) else {
                self.budget_alerted.remove(&budget.name);
                continue;
            };
            if self.budget_alerted.get(&budget.name) == Some(&threshold) {
                continue;
            }
            self.budget_alerted.insert(budget.name.clone(), threshold);
            let exceeded = budget.level == ClientShellBudgetLevel::Exceeded;
            let verb = if exceeded { "estourou" } else { "atingiu" };
            alerts.push(crate::protocol::SemanticNotification {
                kind: crate::protocol::SemanticNotificationKind::Custom,
                title: format!("orçamento {}: {verb} {threshold}%", budget.name),
                body: None,
                sound: None,
                agent: None,
                workspace_id: None,
                tab_id: None,
                pane_id: None,
                position: None,
            });
        }
        alerts
    }

    pub(super) fn acknowledge_surface(
        &mut self,
        snapshot: &mut ClientShellSnapshot,
        surface: &PaneSurfaceFrame,
        outer_focused: Option<bool>,
    ) -> bool {
        if outer_focused == Some(false)
            || self.boot_id.as_deref() != Some(surface.boot_id.as_str())
            || snapshot.boot_id != surface.boot_id
            || snapshot.revision != surface.projection_revision
        {
            return false;
        }

        let mut changed = false;
        for pane in &surface.panes {
            let Some(agent) = snapshot
                .agents
                .iter()
                .find(|agent| agent.pane_id == pane.pane_id)
            else {
                continue;
            };
            let acknowledged = self.acknowledged.entry(agent.pane_id.clone()).or_default();
            if *acknowledged < agent.state_change_seq {
                *acknowledged = agent.state_change_seq;
                changed = true;
            }
        }
        if changed {
            for agent in &mut snapshot.agents {
                agent.agent_status = self.projected_status(agent);
            }
            project_aggregate_status(snapshot);
        }
        changed
    }

    pub(super) fn seen(&self, agent: &ClientShellAgent) -> bool {
        self.completed.get(&agent.pane_id).is_none_or(|completion| {
            self.acknowledged
                .get(&agent.pane_id)
                .is_some_and(|sequence| sequence >= completion)
        })
    }

    fn projected_status(&self, agent: &ClientShellAgent) -> AgentStatus {
        match agent.agent_status {
            AgentStatus::Idle | AgentStatus::Done => {
                if self.seen(agent) {
                    AgentStatus::Idle
                } else {
                    AgentStatus::Done
                }
            }
            status => status,
        }
    }
}

fn project_aggregate_status(snapshot: &mut ClientShellSnapshot) {
    for tab in &mut snapshot.tabs {
        if let Some(status) = snapshot
            .agents
            .iter()
            .filter(|agent| agent.tab_id == tab.tab_id)
            .map(|agent| agent.agent_status)
            .max_by_key(|status| super::status_priority(*status))
        {
            tab.agent_status = status;
        }
    }
    for workspace in &mut snapshot.workspaces {
        if let Some(status) = snapshot
            .agents
            .iter()
            .filter(|agent| agent.workspace_id == workspace.workspace_id)
            .map(|agent| agent.agent_status)
            .max_by_key(|status| super::status_priority(*status))
        {
            workspace.agent_status = status;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        endpoint::EndpointAgentCompletions, FrameData, PaneSurfacePane, SurfaceRect,
    };

    fn agent(status: AgentStatus, sequence: u64) -> ClientShellAgent {
        ClientShellAgent {
            pane_id: "agent-pane".into(),
            workspace_id: "workspace".into(),
            tab_id: "tab".into(),
            name: None,
            display_agent: None,
            agent: None,
            title: None,
            terminal_title: None,
            terminal_title_stripped: None,
            agent_status: status,
            state_change_seq: sequence,
            state_labels: Vec::new(),
            tokens: Vec::new(),
            focused: true,
            usage: None,
        }
    }

    fn snapshot(status: AgentStatus, sequence: u64, revision: u64) -> ClientShellSnapshot {
        let mut snapshot = crate::client::shell::tests::snapshot();
        snapshot.boot_id = "endpoint-boot".into();
        snapshot.revision = revision;
        snapshot.agents = vec![agent(status, sequence)];
        snapshot
    }

    fn surface(revision: u64) -> PaneSurfaceFrame {
        let rect = SurfaceRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        PaneSurfaceFrame {
            boot_id: "endpoint-boot".into(),
            projection_revision: revision,
            surface_revision: 1,
            frame: FrameData {
                cells: Vec::new(),
                width: 0,
                height: 0,
                cursor: None,
                hyperlinks: Vec::new(),
                graphics: Vec::new(),
            },
            panes: vec![PaneSurfacePane {
                pane_id: "agent-pane".into(),
                content_revision: 1,
                rect,
                inner_rect: rect,
                scrollbar_rect: None,
                scroll: None,
                focused: true,
                mouse_reporting: false,
                sgr_pixel_mouse: false,
                alternate_screen_active: false,
                pixel_width: 0,
                pixel_height: 0,
            }],
            splits: Vec::new(),
            popup: None,
            graphics: Default::default(),
        }
    }

    #[test]
    fn first_snapshot_establishes_an_idle_baseline_without_server_seen_authority() {
        let mut presentation = EndpointAgentPresentation::default();
        let mut snapshot = snapshot(AgentStatus::Done, 4, 1);

        presentation.project_snapshot(&mut snapshot);

        assert_eq!(snapshot.agents[0].agent_status, AgentStatus::Idle);
    }

    fn assert_idle_sequence(states: &[(AgentStatus, u64)]) {
        let mut presentation = EndpointAgentPresentation::default();
        let mut projected = AgentStatus::Unknown;
        for (revision, &(status, seq)) in states.iter().enumerate() {
            let mut snapshot = snapshot(status, seq, revision as u64 + 1);
            presentation.project_snapshot(&mut snapshot);
            projected = snapshot.agents[0].agent_status;
        }
        assert_eq!(projected, AgentStatus::Idle, "{states:?}");
    }

    fn completions(boot: &str, revision: u64, seq: Option<u64>) -> EndpointAgentCompletions {
        EndpointAgentCompletions {
            boot_id: boot.into(),
            revision,
            completions: seq
                .map(|seq| ("agent-pane".into(), seq))
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn completion_guard_initial_unknown_to_idle_does_not_project_done() {
        assert_idle_sequence(&[(AgentStatus::Unknown, 0), (AgentStatus::Idle, 1)]);
    }

    #[test]
    fn completion_guard_new_idle_agent_is_not_completed_work() {
        let mut presentation = EndpointAgentPresentation::default();
        let mut baseline = snapshot(AgentStatus::Idle, 0, 1);
        baseline.agents.clear();
        presentation.project_snapshot(&mut baseline);
        let mut ready = snapshot(AgentStatus::Idle, 1, 2);

        presentation.project_snapshot(&mut ready);

        assert_eq!(ready.agents[0].agent_status, AgentStatus::Idle);
    }

    #[test]
    fn completion_guard_session_rebind_does_not_project_done() {
        assert_idle_sequence(&[
            (AgentStatus::Idle, 4),
            (AgentStatus::Unknown, 5),
            (AgentStatus::Idle, 6),
        ]);
    }

    #[test]
    fn completion_guard_startup_blocker_does_not_project_done() {
        assert_idle_sequence(&[
            (AgentStatus::Unknown, 0),
            (AgentStatus::Blocked, 1),
            (AgentStatus::Idle, 2),
        ]);
    }

    #[test]
    fn completion_guard_distinguishes_coalesced_work_from_rebind() {
        for completion in [None, Some(6)] {
            let mut presentation = EndpointAgentPresentation::default();
            let mut baseline = snapshot(AgentStatus::Idle, 4, 1);
            presentation.project_snapshot(&mut baseline);
            presentation.receive_completions(None, completions(&baseline.boot_id, 2, completion));
            let mut settled = snapshot(AgentStatus::Idle, 6, 2);

            presentation.project_snapshot(&mut settled);

            let expected = if completion.is_some() {
                AgentStatus::Done
            } else {
                AgentStatus::Idle
            };
            assert_eq!(settled.agents[0].agent_status, expected);
            presentation.project_snapshot(&mut settled);
            assert_eq!(settled.agents[0].agent_status, expected);
        }
    }

    #[test]
    fn completion_guard_server_suppression_overrides_client_observed_work() {
        let mut presentation = EndpointAgentPresentation::default();
        let mut baseline = snapshot(AgentStatus::Working, 1, 1);
        presentation.project_snapshot(&mut baseline);
        presentation.receive_completions(None, completions(&baseline.boot_id, 2, None));
        let mut settled = snapshot(AgentStatus::Idle, 2, 2);

        presentation.project_snapshot(&mut settled);

        assert_eq!(settled.agents[0].agent_status, AgentStatus::Idle);
    }

    #[test]
    fn completion_guard_companion_is_scoped_to_boot_revision_and_connection() {
        for (boot, revision, generation, seq) in [
            ("old-boot", 2, Some(1), 6),
            ("endpoint-boot", 1, Some(1), 6),
            ("endpoint-boot", 2, Some(0), 6),
            ("endpoint-boot", 2, Some(1), 5),
        ] {
            let mut presentation = EndpointAgentPresentation::default();
            let mut baseline = snapshot(AgentStatus::Idle, 4, 1);
            presentation.project_snapshot_for_generation(&mut baseline, Some(1));
            presentation.receive_completions(generation, completions(boot, revision, Some(seq)));
            let mut settled = snapshot(AgentStatus::Idle, 6, 2);

            presentation.project_snapshot_for_generation(&mut settled, Some(1));

            assert_eq!(settled.agents[0].agent_status, AgentStatus::Idle);
        }
    }

    #[test]
    fn completion_guard_new_boot_baselines_existing_completions() {
        let mut presentation = EndpointAgentPresentation::default();
        let mut old = snapshot(AgentStatus::Working, 1, 1);
        old.boot_id = "old-boot".into();
        presentation.project_snapshot(&mut old);
        presentation.receive_completions(None, completions("endpoint-boot", 1, Some(6)));
        let mut restored = snapshot(AgentStatus::Idle, 6, 1);

        presentation.project_snapshot(&mut restored);

        assert_eq!(restored.agents[0].agent_status, AgentStatus::Idle);
    }

    #[test]
    fn unpresented_working_completion_projects_done() {
        let mut presentation = EndpointAgentPresentation::default();
        let mut initial = snapshot(AgentStatus::Working, 4, 1);
        presentation.project_snapshot(&mut initial);
        let mut completed = snapshot(AgentStatus::Idle, 5, 2);

        presentation.project_snapshot(&mut completed);

        assert_eq!(completed.agents[0].agent_status, AgentStatus::Done);
    }

    #[test]
    fn coherent_presented_surface_acknowledges_completion() {
        let mut presentation = EndpointAgentPresentation::default();
        let mut initial = snapshot(AgentStatus::Working, 4, 1);
        presentation.project_snapshot(&mut initial);
        let mut completed = snapshot(AgentStatus::Idle, 5, 2);
        presentation.project_snapshot(&mut completed);

        assert!(presentation.acknowledge_surface(&mut completed, &surface(2), Some(true)));
        assert_eq!(completed.agents[0].agent_status, AgentStatus::Idle);
    }

    #[test]
    fn clients_acknowledge_the_same_endpoint_completion_independently() {
        let mut viewing_client = EndpointAgentPresentation::default();
        let mut background_client = EndpointAgentPresentation::default();
        let mut initial_for_viewer = snapshot(AgentStatus::Working, 4, 1);
        let mut initial_for_background = initial_for_viewer.clone();
        viewing_client.project_snapshot(&mut initial_for_viewer);
        background_client.project_snapshot(&mut initial_for_background);
        let mut completed_for_viewer = snapshot(AgentStatus::Idle, 5, 2);
        let mut completed_for_background = completed_for_viewer.clone();
        viewing_client.project_snapshot(&mut completed_for_viewer);
        background_client.project_snapshot(&mut completed_for_background);

        assert!(viewing_client.acknowledge_surface(
            &mut completed_for_viewer,
            &surface(2),
            Some(true)
        ));

        assert_eq!(
            completed_for_viewer.agents[0].agent_status,
            AgentStatus::Idle
        );
        assert_eq!(
            completed_for_background.agents[0].agent_status,
            AgentStatus::Done
        );
    }

    #[test]
    fn stale_or_unfocused_surface_does_not_acknowledge_completion() {
        let mut presentation = EndpointAgentPresentation::default();
        let mut initial = snapshot(AgentStatus::Working, 4, 1);
        presentation.project_snapshot(&mut initial);
        let mut completed = snapshot(AgentStatus::Idle, 5, 2);
        presentation.project_snapshot(&mut completed);

        assert!(!presentation.acknowledge_surface(&mut completed, &surface(1), Some(true)));
        assert!(!presentation.acknowledge_surface(&mut completed, &surface(2), Some(false)));
        assert_eq!(completed.agents[0].agent_status, AgentStatus::Done);
    }

    fn agent_with_context(
        pane_id: &str,
        permille: u16,
        agent_name: Option<&str>,
    ) -> ClientShellAgent {
        let mut agent = agent(AgentStatus::Working, 1);
        agent.pane_id = pane_id.into();
        agent.agent = agent_name.map(str::to_string);
        agent.usage = Some(crate::protocol::ClientShellAgentUsage {
            model: None,
            context_permille: Some(permille),
            context_exact: true,
            context_window: Some(200_000),
            tokens_in: 0,
            tokens_out: 0,
            tokens_cache_read: 0,
            tokens_cache_write: 0,
            cost_micros_usd: None,
            updated_at_unix_ms: None,
        });
        agent
    }

    #[test]
    fn context_alert_uses_per_agent_compact_at_override() {
        #[derive(serde::Deserialize)]
        struct Wrapper {
            usage: UsageConfig,
        }
        let config = toml::from_str::<Wrapper>(
            r#"
            [usage]
            compact_at = 80
            [usage.agents.claude]
            compact_at = 50
            "#,
        )
        .unwrap()
        .usage;

        let mut with_override = EndpointAgentPresentation::default();
        let mut snap = snapshot(AgentStatus::Working, 1, 1);
        snap.agents = vec![agent_with_context("pane-1", 600, Some("claude"))];
        assert_eq!(
            with_override.context_alerts(&snap, &config).len(),
            1,
            "60% crosses claude's 50% override even though the global compact_at is 80%"
        );

        let mut without_override = EndpointAgentPresentation::default();
        let mut other_agent_snap = snapshot(AgentStatus::Working, 1, 1);
        other_agent_snap.agents = vec![agent_with_context("pane-2", 600, Some("codex"))];
        assert!(
            without_override
                .context_alerts(&other_agent_snap, &config)
                .is_empty(),
            "60% stays under the global 80% compact_at for an agent without an override"
        );
    }

    fn budget(
        name: &str,
        level: crate::protocol::ClientShellBudgetLevel,
        warn_threshold: Option<u8>,
    ) -> crate::protocol::ClientShellUsageBudget {
        crate::protocol::ClientShellUsageBudget {
            name: name.into(),
            scope: "global".into(),
            level,
            used_permille: Some(750),
            used_scaled: 750,
            limit_scaled: 1000,
            unit: crate::protocol::ClientShellBudgetUnit::Tokens,
            warn_threshold,
            resets_at_unix_ms: None,
            forecast_exhausted_at_unix_ms: None,
            partial: false,
        }
    }

    fn snapshot_with_budgets(
        budgets: Vec<crate::protocol::ClientShellUsageBudget>,
    ) -> ClientShellSnapshot {
        let mut snap = crate::client::shell::tests::snapshot();
        snap.boot_id = "endpoint-boot".into();
        snap.usage = Some(crate::protocol::ClientShellUsageSummary {
            activity: None,
            providers: Vec::new(),
            budgets,
        });
        snap
    }

    #[test]
    fn budget_warn_alert_fires_once_per_threshold_crossing() {
        use crate::protocol::ClientShellBudgetLevel::{Exceeded, Ok, Warn};

        let mut presentation = EndpointAgentPresentation::default();
        let warn_70 = snapshot_with_budgets(vec![budget("claude-mensal", Warn, Some(70))]);
        let alerts = presentation.budget_alerts(&warn_70);
        assert_eq!(alerts.len(), 1);

        // Same threshold again: no repeat.
        assert!(presentation.budget_alerts(&warn_70).is_empty());

        // Crossing a higher threshold re-fires.
        let warn_90 = snapshot_with_budgets(vec![budget("claude-mensal", Exceeded, Some(90))]);
        assert_eq!(presentation.budget_alerts(&warn_90).len(), 1);
        assert!(presentation.budget_alerts(&warn_90).is_empty());

        // Dropping back to Ok clears the dedupe state, so crossing 70 again re-fires.
        let back_to_ok = snapshot_with_budgets(vec![budget("claude-mensal", Ok, None)]);
        assert!(presentation.budget_alerts(&back_to_ok).is_empty());
        assert_eq!(presentation.budget_alerts(&warn_70).len(), 1);
    }

    #[test]
    fn budget_alert_dedupe_is_scoped_per_budget_name() {
        use crate::protocol::ClientShellBudgetLevel::Warn;

        let mut presentation = EndpointAgentPresentation::default();
        let both = snapshot_with_budgets(vec![
            budget("claude-mensal", Warn, Some(70)),
            budget("reviewer-3h", Warn, Some(70)),
        ]);
        assert_eq!(presentation.budget_alerts(&both).len(), 2);
        assert!(presentation.budget_alerts(&both).is_empty());
    }
}
