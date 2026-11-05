//! Live rubric evaluation on versioned synthetic notes, separate from unit tests.
use mercury_knowledge::{
    backend::{Model, NativeModel},
    Config, Filter, Index,
};
use serde_json::{json, Value};
use std::{
    fs,
    sync::atomic::AtomicBool,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/evaluation");
    let resume = std::env::args().skip(1).collect::<Vec<_>>();
    let root = if resume.first().is_some_and(|s| s == "--resume") {
        let path =
            std::path::PathBuf::from(resume.get(1).ok_or("missing resume path")?).canonicalize()?;
        if !path.starts_with(std::env::temp_dir().canonicalize()?)
            || !path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .starts_with("mercury-knowledge-eval-")
        {
            return Err("resume must be an evaluation fixture".into());
        }
        path
    } else {
        std::env::temp_dir().canonicalize()?.join(format!(
            "mercury-knowledge-eval-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ))
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        if !root.exists() {
            fs::DirBuilder::new().mode(0o700).create(&root)?;
        }
    }
    let vault = root.join("vault");
    if !vault.exists() {
        fs::create_dir(&vault)?;
    }
    for entry in fs::read_dir(&source)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "md") && resume.is_empty() {
            fs::copy(&path, vault.join(path.file_name().unwrap()))?;
        }
    }
    let config = Config {
        enabled: true,
        vault,
        allow_external: true,
        allowed_providers: vec!["claude".into(), "codex".into()],
        timeout_secs: 90,
        max_model_calls: 256,
        ..Config::default()
    };
    let cancel = AtomicBool::new(false);
    let model = NativeModel::new(
        &config,
        &root.join("model"),
        &mercury_router::RouteConfig::default(),
        &[],
        None,
        &cancel,
    )?;
    let mut index = Index::open(&root.join("data"))?;
    index.sync(&config, &cancel)?;
    println!(
        "{}",
        json!({"stage":"start","fixture":root,"diagnostic":model.doctor()?})
    );
    let start = Instant::now();
    let derived: mercury_knowledge::graph::Derived = match index.derived(&model.fingerprint())? {
        Some(v) => serde_json::from_value(v)?,
        None => index.build_graph_verified(&model, &cancel, &config)?,
    };
    println!(
        "{}",
        json!({"stage":"indexed","communities":derived.summaries.len(),"elapsed_ms":start.elapsed().as_millis()})
    );
    let questions: Value = serde_json::from_slice(&fs::read(source.join("questions.json"))?)?;
    let mut results: Vec<Value> = if root.join("progress.json").exists() {
        serde_json::from_slice(&fs::read(root.join("progress.json"))?)?
    } else {
        Vec::new()
    };
    for case in questions["cases"].as_array().unwrap() {
        if results.iter().any(|r| r["id"] == case["id"]) {
            continue;
        }
        let query = case["question"].as_str().unwrap();
        let mode = case["mode"].as_str().unwrap();
        let expected: Vec<_> = case["sources"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let start = Instant::now();
        let fts = index.search(query, &Filter::default(), 20)?;
        let fts_ms = start.elapsed().as_millis();
        let start = Instant::now();
        let hybrid = index.hybrid(query, &Filter::default(), 20, &model)?;
        let hybrid_ms = start.elapsed().as_millis();
        let start = Instant::now();
        let answer = match index.ask(
            query,
            mode,
            &Filter::default(),
            &model,
            &cancel,
            config.max_context_chars,
        ) {
            Ok(answer) => answer,
            Err(error) => {
                let result = json!({"id":case["id"],"group":case["group"],"passed":false,"error":error.to_string()});
                println!(
                    "{}",
                    json!({"stage":"case","id":case["id"],"passed":false,"error":error.to_string()})
                );
                results.push(result);
                fs::write(
                    root.join("progress.json"),
                    serde_json::to_vec_pretty(&results)?,
                )?;
                continue;
            }
        };
        let text = answer
            .claims
            .iter()
            .map(|c| c.text.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        let cited_paths: Vec<_> = answer
            .claims
            .iter()
            .flat_map(|c| c.evidence.iter())
            .filter_map(|id| {
                answer
                    .sources
                    .iter()
                    .find(|h| h.chunk_id == *id)
                    .map(|h| h.path.as_str())
            })
            .collect();
        let sources_ok = if case["sources_any"] == true {
            expected.iter().any(|p| cited_paths.contains(p))
        } else {
            expected.iter().all(|p| cited_paths.contains(p))
        };
        let keywords_ok = case["terms"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| text.contains(&v.as_str().unwrap().to_lowercase()));
        let passed = if case["group"] == "unanswered" {
            answer.insufficient && answer.claims.is_empty()
        } else {
            !answer.insufficient && sources_ok && keywords_ok
        };
        let result = json!({"id":case["id"],"group":case["group"],"passed":passed,"rubric":"expected keywords + cited sources, human semantic review still required","fts_recall":expected.iter().all(|p|fts.iter().any(|h|h.path==*p)),"hybrid_recall":expected.iter().all(|p|hybrid.iter().any(|h|h.path==*p)),"fts_ms":fts_ms,"hybrid_ms":hybrid_ms,"graphrag_ms":start.elapsed().as_millis(),"answer":answer});
        println!(
            "{}",
            json!({"stage":"case","id":case["id"],"passed":passed})
        );
        results.push(result);
        fs::write(
            root.join("progress.json"),
            serde_json::to_vec_pretty(&results)?,
        )?;
    }
    let count = |group: &str| {
        results
            .iter()
            .filter(|r| r["group"] == group && r["passed"] == true)
            .count()
    };
    let known = count("text") + count("relation") + count("global");
    let report = json!({"fixture":root,"known_passed":known,"known_total":20,"relations_passed":count("relation"),"global_passed":count("global"),"unanswered_passed":count("unanswered"),"rubric_threshold_passed":known>=16&&count("relation")>=4&&count("global")>=4&&count("unanswered")==5,"human_semantic_review":"required","results":results,"observations":model.observations()});
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "{}",
        json!({"stage":"complete","report":root.join("report.json"),"rubric_threshold_passed":report["rubric_threshold_passed"]})
    );
    Ok(())
}
