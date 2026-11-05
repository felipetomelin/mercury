//! Opt-in documentation exporter. Uses the real renderer and fictional data;
//! never connects to a server, account, vault, or agent.
use super::*;
use crate::protocol::{
    ClientShellProviderLimit, ClientShellProviderUsage, ClientShellUsageActivity,
    ClientShellUsageSummary,
};

#[test]
#[ignore = "manual README asset generation; see scripts/render_readme_assets.py"]
fn export_public_readme_frames() {
    let root = std::path::PathBuf::from(
        std::env::var_os("MERCURY_README_FRAMES").expect("set MERCURY_README_FRAMES"),
    );
    std::fs::create_dir_all(&root).unwrap();
    let now = time::OffsetDateTime::now_utc().unix_timestamp() * 1000;
    let mut projected = snapshot();
    projected.workspaces[0].label = "mercury-demo".into();
    projected.workspaces[0].new_workspace_cwd = "/workspace/mercury".into();
    projected.panes[0].cwd = Some("/workspace/mercury".into());
    projected.worktree_directory = "/workspace/.worktrees".into();
    projected.usage = Some(ClientShellUsageSummary {
        providers: vec![
            ClientShellProviderUsage {
                provider: "claude".into(),
                status: "atualizado".into(),
                detail: "Conta conectada".into(),
                observed_at_unix_ms: Some(now),
                credits: None,
                plan_type: Some("enterprise".into()),
                limits: vec![],
            },
            ClientShellProviderUsage {
                provider: "codex".into(),
                status: "atualizado".into(),
                detail: "Conta conectada".into(),
                observed_at_unix_ms: Some(now),
                credits: None,
                plan_type: Some("business".into()),
                limits: vec![ClientShellProviderLimit {
                    key: "individual".into(),
                    used_permille: 420,
                    window_minutes: None,
                    resets_at_unix_ms: Some(now + 172800000),
                }],
            },
        ],
        budgets: vec![],
        activity: Some(ClientShellUsageActivity {
            tokens_today: 1240000,
            cache_read_today: 870000,
            input_today: 1000000,
            cost_microusd_today: Some(3750000),
            daily_tokens: vec![130000, 220000, 160000, 450000, 850000, 640000, 1240000],
            last_activity: vec![("codex".into(), now - 12000)],
        }),
    });
    let mut config = ClientShellConfig::from_config(&Config::default());
    config.palette = Palette::catppuccin();
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.view = ClientShellView::Usage;
    state.usage_view_reveal_cursor = false;
    let frame = state.compose(112, 27).expect("usage frame");
    std::fs::write(
        root.join("usage.json"),
        serde_json::to_vec(&frame.frame).unwrap(),
    )
    .unwrap();

    state.view = ClientShellView::Crons;
    let mut form =
        super::super::cron_form::CronForm::new("/workspace/mercury".into(), vec![], None);
    use super::super::cron_form::Field;
    form.set(Field::Name, "Revisão diária do projeto");
    form.set(Field::Offset, "-03:00");
    form.set(
        Field::Prompt,
        "Revise as mudanças e resuma os próximos passos.",
    );
    let offset = mercury_cron::job::parse_utc_offset("-03:00").unwrap();
    let mut at = time::OffsetDateTime::parse(
        "2026-11-05T12:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    let schedule = mercury_cron::Schedule::parse(&form.schedule().unwrap(), at, offset).unwrap();
    for _ in 0..3 {
        at = schedule.next_after(at).unwrap();
        form.preview.push(at.unix_timestamp());
    }
    form.preview_key = form.key().ok();
    state.crons_view.form = Some(form);
    let frame = state.compose(112, 32).expect("cron frame");
    std::fs::write(
        root.join("crons.json"),
        serde_json::to_vec(&frame.frame).unwrap(),
    )
    .unwrap();

    state.view = ClientShellView::Knowledge;
    state.knowledge_view.data = serde_json::json!({"enabled":true,"index":{"notes":42,"chunks":186,"relations":73,"summaries":9}});
    let frame = state.compose(112, 24).expect("knowledge frame");
    std::fs::write(
        root.join("knowledge.json"),
        serde_json::to_vec(&frame.frame).unwrap(),
    )
    .unwrap();
}
