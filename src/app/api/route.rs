//! Read-only route preview. Submission is a separate operation so a preview
//! can never create a pane or send the prompt accidentally.

use mercury_router::{Headroom, RouteRequest};
use mercury_usage::{
    Budget, BudgetLevel, OnExceed, Provider, ProviderLimits, Scope, UsageRecord, UsageWindow,
};
use std::time::{Duration, Instant};
use time::OffsetDateTime;

use crate::api::schema::{Method, Request, ResponseResult, RoutePreviewInfo, RoutePreviewParams};
use crate::app::App;
use crate::events::AppEvent;

use super::responses::{encode_error, encode_success};

impl App {
    pub(crate) fn handle_deferred_route_api_request(
        &mut self,
        request: Request,
        respond_to: std::sync::mpsc::Sender<String>,
    ) {
        let Method::RoutePreview(params) = &request.method else {
            return;
        };
        let codex_relevant = self.route.default_provider == Provider::Codex
            || self.route.rules.iter().any(|rule| {
                rule.provider == Provider::Codex || rule.fallback.contains(&Provider::Codex)
            });
        let valid_prompt = !params.prompt.trim().is_empty() && params.prompt.len() <= 128 * 1024;
        if !self.route.enabled
            || !codex_relevant
            || !valid_prompt
            || !route_metadata_valid(params)
            || !route_exclusions_valid(params)
            || !self
                .usage
                .route_codex_refresh_needed(self.route.limit_ttl_seconds, OffsetDateTime::now_utc())
        {
            let response = self.handle_api_request_after_internal_events_drained(request);
            let _ = respond_to.send(response);
            return;
        }
        if self
            .route_refresh_retry_at
            .is_some_and(|retry_at| Instant::now() < retry_at)
        {
            let Method::RoutePreview(params) = request.method else {
                return;
            };
            let response = self.handle_route_preview(request.id, params, Some("cooldown"));
            let _ = respond_to.send(response);
            return;
        }
        if self.pending_route_previews.len() >= 32 {
            let _ = respond_to.send(encode_error(
                request.id,
                "route_refresh_busy",
                "too many route previews waiting for limit refresh",
            ));
            return;
        }
        self.pending_route_previews.push((request, respond_to));
        if self.route_refresh_in_flight {
            return;
        }
        self.route_refresh_in_flight = true;
        let binary = self.usage.route_codex_binary();
        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let limits = super::super::usage_codex_app_server::read_route_limits(&binary)
                .ok()
                .flatten();
            let _ = event_tx.blocking_send(AppEvent::RouteLimitsRefreshed { limits });
        });
    }

    pub(crate) fn handle_route_limits_refreshed(&mut self, limits: Option<ProviderLimits>) -> bool {
        self.route_refresh_in_flight = false;
        let refresh_status = if limits.is_some() {
            "refreshed"
        } else {
            "unavailable"
        };
        self.route_refresh_retry_at = Some(
            Instant::now()
                + if limits.is_some() {
                    Duration::from_secs(5)
                } else {
                    Duration::from_secs(30)
                },
        );
        let changed = limits.is_some_and(|limits| self.usage.apply_route_codex_limits(limits));
        self.refresh_automation_limits();
        for (request, respond_to) in std::mem::take(&mut self.pending_route_previews) {
            let Method::RoutePreview(params) = request.method else {
                continue;
            };
            let response = self.handle_route_preview(request.id, params, Some(refresh_status));
            let _ = respond_to.send(response);
        }
        changed
    }

    pub(super) fn handle_route_preview(
        &mut self,
        id: String,
        params: RoutePreviewParams,
        codex_limit_refresh: Option<&str>,
    ) -> String {
        if params.prompt.trim().is_empty() || params.prompt.len() > 128 * 1024 {
            return encode_error(id, "invalid_route_request", "prompt must be 1–131072 bytes");
        }
        if !route_metadata_valid(&params) {
            return encode_error(id, "invalid_route_request", "invalid route metadata");
        }
        let workspace_index = if let Some(workspace_id) = params.workspace_id.as_deref() {
            match self.parse_workspace_id(workspace_id) {
                Some(index) => Some(index),
                None => return encode_error(id, "invalid_route_request", "workspace not found"),
            }
        } else if let Some(project) = params.project.as_deref() {
            let matching: Vec<_> = (0..self.state.workspaces.len())
                .filter(|&index| self.workspace_info(index).label == project)
                .collect();
            match matching.as_slice() {
                [index] => Some(*index),
                _ => None,
            }
        } else {
            self.state.active
        };
        let workspace_label = workspace_index.map(|index| self.workspace_info(index).label);
        if params.workspace_id.is_some()
            && params
                .project
                .as_deref()
                .is_some_and(|project| workspace_label.as_deref() != Some(project))
        {
            return encode_error(id, "invalid_route_request", "workspace label mismatch");
        }
        let route_project = params.project.as_deref().or(workspace_label.as_deref());
        let budget_project = workspace_index
            .and_then(|index| self.state.workspaces.get(index))
            .map(|workspace| workspace.display_name_from_terminals(&self.state.terminals));
        let tags: Vec<&str> = params.tags.iter().map(String::as_str).collect();
        let mut excluded = Vec::new();
        for provider in &params.exclude {
            let provider = match provider.as_str() {
                "claude" => mercury_usage::Provider::Claude,
                "codex" => mercury_usage::Provider::Codex,
                _ => return encode_error(id, "invalid_route_request", "invalid excluded provider"),
            };
            if excluded.contains(&provider) {
                return encode_error(id, "invalid_route_request", "duplicate excluded provider");
            }
            excluded.push(provider);
        }
        let request = RouteRequest {
            prompt: &params.prompt,
            tags: &tags,
            project: route_project,
        };
        let limits = self.usage.route_limits();
        let now = OffsetDateTime::now_utc();
        let records = if self.usage.route_budgets().is_empty() {
            Vec::new()
        } else {
            match self.usage_records() {
                Ok(records) => records,
                Err(()) => {
                    return encode_error(
                        id,
                        "route_usage_unavailable",
                        "usage snapshot unavailable",
                    )
                }
            }
        };
        let budget_excluded = match budget_exclusions(
            self.usage.route_budgets(),
            &records,
            &limits,
            budget_project.as_deref().or(request.project),
            now,
            self.route.limit_ttl_seconds,
        ) {
            Ok(excluded) => excluded,
            Err(reason) => return encode_error(id, "route_budget_exceeded", reason),
        };
        for provider in &budget_excluded {
            if !excluded.contains(provider) {
                excluded.push(*provider);
            }
        }
        let decision = match self
            .route
            .decide_excluding(&request, &limits, now, &excluded)
        {
            Ok(decision) => decision,
            Err(error) => return encode_error(id, "route_unavailable", error),
        };
        let observed_at = limits
            .iter()
            .find(|snapshot| snapshot.provider == decision.provider)
            .and_then(|snapshot| snapshot.observed_at)
            .map(|at| (at.unix_timestamp_nanos() / 1_000_000) as i64);
        let route = RoutePreviewInfo {
            provider: decision.provider.as_str().into(),
            headroom: match decision.headroom {
                Headroom::Available => "available",
                Headroom::Exceeded => "exceeded",
                Headroom::Unknown => "unknown",
            }
            .into(),
            model: decision.model,
            effort: decision.effort,
            matched_rule: decision.matched_rule,
            fallback_from: decision
                .fallback_from
                .map(|provider| provider.as_str().into()),
            limits_observed_at_unix_ms: observed_at,
            budget_excluded_providers: budget_excluded
                .into_iter()
                .map(|provider| provider.as_str().into())
                .collect(),
            codex_limit_refresh: codex_limit_refresh.map(str::to_owned),
        };
        encode_success(id, ResponseResult::RoutePreview { route })
    }
}

fn route_metadata_valid(params: &RoutePreviewParams) -> bool {
    params.tags.len() <= 32
        && params
            .tags
            .iter()
            .all(|tag| !tag.is_empty() && tag.len() <= 128)
        && params
            .project
            .as_ref()
            .is_none_or(|project| project.len() <= 512)
}

fn route_exclusions_valid(params: &RoutePreviewParams) -> bool {
    params.exclude.len() <= 2
        && params
            .exclude
            .iter()
            .all(|provider| provider == "claude" || provider == "codex")
        && params
            .exclude
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            == params.exclude.len()
}

fn budget_exclusions(
    budgets: &[Budget],
    records: &[UsageRecord],
    limits: &[ProviderLimits],
    project: Option<&str>,
    now: OffsetDateTime,
    ttl_seconds: i64,
) -> Result<Vec<Provider>, String> {
    let mut excluded = Vec::new();
    for budget in budgets {
        if budget.on_exceed == OnExceed::Notify {
            continue;
        }
        let scope = match &budget.scope {
            Scope::Global => None,
            Scope::Workspace(workspace) if Some(workspace.as_str()) == project => None,
            Scope::Provider(provider) => Some(*provider),
            _ => continue,
        };
        if matches!(budget.window, UsageWindow::Provider { .. }) {
            // A provider window without a provider scope is ambiguous. Stale
            // account data must not become a blocking budget decision.
            let Some(provider) = scope else { continue };
            let Some(observed_at) = limits
                .iter()
                .find(|limit| limit.provider == provider)
                .and_then(|limit| limit.observed_at)
            else {
                continue;
            };
            let age = now - observed_at;
            if age.is_negative() || age > time::Duration::seconds(ttl_seconds) {
                continue;
            }
        }
        let status = budget.evaluate(records, limits, now);
        if status.level != BudgetLevel::Exceeded
            || status.resets_at.is_some_and(|reset| reset <= now)
        {
            continue;
        }
        if let Some(provider) = scope {
            if !excluded.contains(&provider) {
                excluded.push(provider);
            }
        } else {
            // Switching provider cannot avoid a global/workspace budget.
            return Err(format!("budget {} is exceeded for this route", budget.name));
        }
    }
    Ok(excluded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::{Method, Request};
    use crate::app::AppPolicy;
    use crate::config::Config;

    #[cfg(unix)]
    fn fake_codex_binary(script: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!(
            "mercury-route-limits-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn deferred_preview_coalesces_fresh_codex_limit_read_without_threads() {
        let binary = fake_codex_binary("#!/bin/sh\nread init\nprintf '%s\\n' '{\"id\":1,\"result\":{}}'\nread initialized\nread limits\nprintf '%s\\n' '{\"id\":2,\"result\":{\"rateLimits\":{\"primary\":null,\"secondary\":null,\"planType\":\"business\"}}}'\n");
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = Config::default();
        config.route.enabled = true;
        config.route.default_provider = Provider::Codex;
        let mut app = App::new(
            &config,
            AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.usage.set_route_codex_binary_for_test(binary.clone());
        let (first_tx, first_rx) = std::sync::mpsc::channel();
        let (second_tx, second_rx) = std::sync::mpsc::channel();
        let request = |id: &str| Request {
            id: id.into(),
            method: Method::RoutePreview(RoutePreviewParams {
                prompt: "review".into(),
                tags: Vec::new(),
                project: None,
                workspace_id: None,
                exclude: Vec::new(),
            }),
        };
        app.handle_deferred_route_api_request(request("first"), first_tx);
        app.handle_deferred_route_api_request(request("second"), second_tx);
        assert_eq!(app.pending_route_previews.len(), 2);
        assert!(app.route_refresh_in_flight);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(7);
        let event = loop {
            if let Ok(event) = app.event_rx.try_recv() {
                break event;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "route refresh timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        app.handle_internal_event(event);
        for receiver in [first_rx, second_rx] {
            let response: serde_json::Value = serde_json::from_str(
                &receiver
                    .recv_timeout(std::time::Duration::from_secs(1))
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(response["result"]["route"]["provider"], "codex");
            assert_eq!(response["result"]["route"]["headroom"], "unknown");
            assert_eq!(
                response["result"]["route"]["codex_limit_refresh"],
                "refreshed"
            );
            assert!(response["result"]["route"]["limits_observed_at_unix_ms"].is_number());
        }
        assert!(!app.route_refresh_in_flight);
        assert!(app.pending_route_previews.is_empty());
        let (third_tx, third_rx) = std::sync::mpsc::channel();
        app.handle_deferred_route_api_request(request("third"), third_tx);
        let third: serde_json::Value = serde_json::from_str(&third_rx.recv().unwrap()).unwrap();
        assert_eq!(third["result"]["route"]["provider"], "codex");
        assert!(third["result"]["route"]["codex_limit_refresh"].is_null());
        assert!(!app.route_refresh_in_flight);
        std::fs::remove_file(binary).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn failed_refresh_keeps_unknown_headroom_and_enters_cooldown() {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = Config::default();
        config.route.enabled = true;
        config.route.default_provider = Provider::Codex;
        let mut app = App::new(
            &config,
            AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.usage.set_route_codex_binary_for_test(
            std::env::temp_dir().join("nonexistent-mercury-route-codex-binary"),
        );
        let request = |id: &str| Request {
            id: id.into(),
            method: Method::RoutePreview(RoutePreviewParams {
                prompt: "review".into(),
                tags: Vec::new(),
                project: None,
                workspace_id: None,
                exclude: Vec::new(),
            }),
        };
        let (first_tx, first_rx) = std::sync::mpsc::channel();
        app.handle_deferred_route_api_request(request("first"), first_tx);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(7);
        let event = loop {
            if let Ok(event) = app.event_rx.try_recv() {
                break event;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "route refresh timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        app.handle_internal_event(event);
        let first: serde_json::Value = serde_json::from_str(&first_rx.recv().unwrap()).unwrap();
        assert_eq!(first["result"]["route"]["headroom"], "unknown");
        assert_eq!(
            first["result"]["route"]["codex_limit_refresh"],
            "unavailable"
        );
        assert!(app.route_refresh_retry_at.is_some());
        let (second_tx, second_rx) = std::sync::mpsc::channel();
        app.handle_deferred_route_api_request(request("second"), second_tx);
        let second: serde_json::Value = serde_json::from_str(&second_rx.recv().unwrap()).unwrap();
        assert_eq!(second["result"]["route"]["codex_limit_refresh"], "cooldown");
        assert!(!app.route_refresh_in_flight);
    }

    #[test]
    fn route_preview_is_read_only_and_does_not_echo_prompt() {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = Config::default();
        config.route.enabled = true;
        let mut app = App::new(
            &config,
            AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        let before = app.state.workspaces.len();
        let raw = app.handle_api_request(Request {
            id: "route-test".into(),
            method: Method::RoutePreview(RoutePreviewParams {
                prompt: "secret task text".into(),
                tags: vec!["review".into()],
                project: None,
                workspace_id: None,
                exclude: Vec::new(),
            }),
        });
        let response: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(response["result"]["route"]["provider"], "claude");
        assert_eq!(response["result"]["route"]["headroom"], "unknown");
        assert!(!raw.contains("secret task text"));
        assert_eq!(app.state.workspaces.len(), before);
    }

    #[test]
    fn route_preview_rejects_invalid_exclusion_without_mutation() {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = Config::default();
        config.route.enabled = true;
        let mut app = App::new(
            &config,
            AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        let before = app.state.workspaces.len();
        let raw = app.handle_api_request(Request {
            id: "route-test".into(),
            method: Method::RoutePreview(RoutePreviewParams {
                prompt: "private".into(),
                tags: Vec::new(),
                project: None,
                workspace_id: None,
                exclude: vec!["other".into()],
            }),
        });
        let response: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(response["error"]["code"], "invalid_route_request");
        assert!(!raw.contains("private"));
        assert_eq!(app.state.workspaces.len(), before);
    }

    #[test]
    fn route_preview_rejects_unknown_workspace_id() {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = Config::default();
        config.route.enabled = true;
        let mut app = App::new(
            &config,
            AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        let raw = app.handle_api_request(Request {
            id: "route-test".into(),
            method: Method::RoutePreview(RoutePreviewParams {
                prompt: "private".into(),
                tags: Vec::new(),
                project: None,
                workspace_id: Some("w99".into()),
                exclude: Vec::new(),
            }),
        });
        let response: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(response["error"]["code"], "invalid_route_request");
        assert!(!raw.contains("private"));
    }

    #[test]
    fn exceeded_budget_routes_provider_but_pauses_matching_workspace() {
        let now = OffsetDateTime::now_utc();
        let record = UsageRecord {
            at: now,
            provider: Provider::Claude,
            session_id: "session".into(),
            workspace: Some("project".into()),
            agent: None,
            pane: None,
            tokens: mercury_usage::TokenCounts {
                input: 110,
                ..Default::default()
            },
            cost_usd: None,
        };
        let mut budget = Budget {
            name: "claude-tokens".into(),
            scope: Scope::Provider(Provider::Claude),
            window: UsageWindow::Rolling { minutes: 60 },
            limit: mercury_usage::Limit::Tokens {
                max: 100,
                count: vec![mercury_usage::TokenKind::Input],
            },
            warn_at: vec![70],
            on_exceed: OnExceed::Route,
        };
        assert_eq!(
            budget_exclusions(
                std::slice::from_ref(&budget),
                std::slice::from_ref(&record),
                &[],
                Some("project"),
                now,
                60
            )
            .unwrap(),
            vec![Provider::Claude]
        );
        budget.scope = Scope::Workspace("other".into());
        budget.on_exceed = OnExceed::Pause;
        assert!(budget_exclusions(
            std::slice::from_ref(&budget),
            std::slice::from_ref(&record),
            &[],
            Some("project"),
            now,
            60
        )
        .unwrap()
        .is_empty());
        budget.scope = Scope::Workspace("project".into());
        assert!(budget_exclusions(&[budget], &[record], &[], Some("project"), now, 60).is_err());
    }

    #[test]
    fn stale_or_reset_provider_budget_never_blocks_route() {
        let now = OffsetDateTime::now_utc();
        let budget = Budget {
            name: "claude-window".into(),
            scope: Scope::Provider(Provider::Claude),
            window: UsageWindow::Provider {
                key: "five_hour".into(),
            },
            limit: mercury_usage::Limit::Percent(100.0),
            warn_at: vec![70],
            on_exceed: OnExceed::Pause,
        };
        let mut limits = vec![ProviderLimits {
            provider: Provider::Claude,
            limits: vec![mercury_usage::ProviderLimit {
                key: "five_hour".into(),
                used_percent: 100.0,
                window_minutes: Some(300),
                resets_at: Some(now + time::Duration::hours(1)),
                used_usd: None,
                limit_usd: None,
                period: None,
            }],
            credits: None,
            plan_type: None,
            limit_reached_type: None,
            observed_at: Some(now - time::Duration::seconds(61)),
        }];
        assert!(
            budget_exclusions(std::slice::from_ref(&budget), &[], &limits, None, now, 60)
                .unwrap()
                .is_empty()
        );
        limits[0].observed_at = Some(now);
        assert_eq!(
            budget_exclusions(std::slice::from_ref(&budget), &[], &limits, None, now, 60).unwrap(),
            vec![Provider::Claude]
        );
        limits[0].limits[0].resets_at = Some(now);
        assert!(budget_exclusions(&[budget], &[], &limits, None, now, 60)
            .unwrap()
            .is_empty());
    }
}
