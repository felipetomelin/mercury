//! Synthetic-only live verification. Never reads a personal Obsidian vault.
use mercury_knowledge::{backend::NativeModel, Config, Filter, Index};
use std::{
    fs,
    sync::atomic::AtomicBool,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().canonicalize()?.join(format!(
        "mercury-native-smoke-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(&root)?;
    }
    let vault = root.join("vault");
    fs::create_dir(&vault)?;
    fs::write(vault.join("Alpha.md"),"# Synthetic engineering fixture\nAlpha depends on Beta. Beta provides queue persistence for Alpha. ADR-17 chose Beta because replay is required. Incident INC-12 affected Alpha and was resolved by enabling idempotent consumers. These are synthetic facts for testing, not real services.\n")?;
    let config = Config {
        enabled: true,
        vault,
        allow_external: true,
        allowed_providers: vec!["claude".into(), "codex".into()],
        timeout_secs: 90,
        max_model_calls: 32,
        ..Config::default()
    };
    let cancel = AtomicBool::new(false);
    let mut index = Index::open(&root.join("data"))?;
    index.sync(&config, &cancel)?;
    let model = NativeModel::new(
        &config,
        &root.join("model"),
        &mercury_router::RouteConfig::default(),
        &[],
        None,
        &cancel,
    )?;
    println!(
        "{}",
        serde_json::json!({"fixture":root,"diagnostic":model.doctor()?})
    );
    let start = Instant::now();
    let derived = index.build_graph_verified(&model, &cancel, &config)?;
    println!(
        "{}",
        serde_json::json!({"stage":"index","entities":derived.entities.len(),"relations":derived.relations.len(),"communities":derived.summaries.len(),"elapsed_ms":start.elapsed().as_millis()})
    );
    for mode in ["local", "global"] {
        let answer = index.ask(
            "What depends on Beta, why was it chosen, and how was INC-12 resolved?",
            mode,
            &Filter::default(),
            &model,
            &cancel,
            config.max_context_chars,
        )?;
        println!("{}", serde_json::json!({"stage":"answer","answer":answer}));
    }
    println!(
        "{}",
        serde_json::json!({"observed":model.observations(),"fixture":root})
    );
    Ok(())
}
