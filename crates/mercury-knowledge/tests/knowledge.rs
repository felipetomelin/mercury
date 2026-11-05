use mercury_knowledge::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    config: Config,
    index: Index,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "mk-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let vault = root.join("vault");
        fs::create_dir(&vault).unwrap();
        let index = Index::open(&root.join("data")).unwrap();
        let config = Config {
            enabled: true,
            vault,
            ..Config::default()
        };
        Self {
            root,
            config,
            index,
        }
    }
    fn note(&self, name: &str, text: &str) {
        fs::write(self.config.vault.join(name), text).unwrap();
    }
    fn sync(&mut self) {
        self.index
            .sync(&self.config, &AtomicBool::new(false))
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn note_offsets_unicode_and_frontmatter_are_real_source_ranges() {
    let text="---\nid: payments\nproject: PV\nstatus: validated\ndepends_on:\n  - '[[Queue]]'\ncustom: !untrusted\n---\n# Investigação\nA solução usa [[Queue|fila]]. 🐶\n";
    let n = parse_note("Payments.md", text, 1).unwrap();
    assert_eq!(n.id, "payments");
    assert_eq!(n.status, "validated");
    assert_eq!(n.links.len(), 2);
    for c in n.chunks {
        assert_eq!(c.text, &text[c.start..c.end]);
    }
    assert!(parse_note("bad.md", "---\nid: x", 0).is_err());
}
#[test]
fn changes_deletions_and_cancel_keep_consistent_snapshots() {
    let mut f = Fixture::new();
    f.note("A.md", "# Alpha\nrare-solution");
    f.sync();
    let rev = f.index.status().unwrap().revision;
    f.note("A.md", "# Alpha\nnewer-content");
    assert!(f.index.sync(&f.config, &AtomicBool::new(true)).is_err());
    assert_eq!(f.index.status().unwrap().revision, rev);
    f.sync();
    assert!(f
        .index
        .search("rare-solution", &Filter::default(), 10)
        .unwrap()
        .is_empty());
    fs::remove_file(f.config.vault.join("A.md")).unwrap();
    f.sync();
    assert_eq!(f.index.status().unwrap().notes, 0);
}
#[test]
fn unambiguous_rename_preserves_identity_and_reopen_results() {
    let mut f = Fixture::new();
    f.note("A.md", "# Shared\ncontext");
    f.sync();
    let id = f.index.snapshot().unwrap().unwrap().notes[0].id.clone();
    fs::rename(f.config.vault.join("A.md"), f.config.vault.join("B.md")).unwrap();
    f.sync();
    assert_eq!(f.index.snapshot().unwrap().unwrap().notes[0].id, id);
    let reopened = Index::open(&f.root.join("data")).unwrap();
    assert_eq!(
        reopened.search("context", &Filter::default(), 10).unwrap()[0].path,
        "B.md"
    );
}
#[test]
fn ambiguous_links_do_not_choose_an_arbitrary_note() {
    let mut f = Fixture::new();
    f.note("A.md", "---\ntitle: Common\n---\nfirst");
    f.note("B.md", "---\ntitle: Common\n---\nsecond");
    f.note("C.md", "[[Common]]");
    f.sync();
    let s = f.index.snapshot().unwrap().unwrap();
    assert!(s.relations.is_empty());
    assert!(!s.diagnostics.is_empty());
}
#[cfg(unix)]
#[test]
fn symlinks_and_public_storage_are_rejected() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let mut f = Fixture::new();
    symlink("/etc/passwd", f.config.vault.join("secret.md")).unwrap();
    assert!(f.index.sync(&f.config, &AtomicBool::new(false)).is_err());
    fs::set_permissions(f.root.join("data"), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Index::open(&f.root.join("data")).is_err());
}
#[test]
fn filters_apply_before_result_limit_and_fts_input_is_inert() {
    let mut f = Fixture::new();
    f.note("A.md", "---\nproject: alpha\n---\nneedle");
    f.note("B.md", "---\nproject: beta\nstatus: validated\n---\nneedle");
    f.sync();
    let hits = f
        .index
        .search(
            "needle",
            &Filter {
                project: Some("beta".into()),
                ..Filter::default()
            },
            1,
        )
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "B.md");
    assert!(f
        .index
        .search("\" OR 1=1; DROP TABLE chunks", &Filter::default(), 10)
        .is_ok());
}
#[test]
fn thousand_note_corpus_is_rebuildable_and_searchable() {
    let mut f = Fixture::new();
    for i in 0..1000 {
        f.note(
            &format!("N{i}.md"),
            &format!("---\nid: n{i}\n---\n# Note {i}\nunique{i} migration evidence"),
        );
    }
    f.sync();
    assert_eq!(f.index.status().unwrap().notes, 1000);
    let revision = f.index.status().unwrap().revision;
    f.sync();
    assert_eq!(f.index.status().unwrap().revision, revision);
    let mut timings = Vec::new();
    for _ in 0..20 {
        let start = std::time::Instant::now();
        let h = f.index.search("unique987", &Filter::default(), 10).unwrap();
        assert_eq!(h[0].note_id, "n987");
        timings.push(start.elapsed());
    }
    timings.sort();
    assert!(
        timings[18] < std::time::Duration::from_millis(500),
        "p95 {:?}",
        timings[18]
    );
}
struct Fake;
impl backend::Model for Fake {
    fn fingerprint(&self) -> String {
        "fixture-v1".into()
    }
    fn embed(&self, _: &str) -> Result<Vec<f32>> {
        Ok(vec![1.0, 0.5])
    }
    fn structured(
        &self,
        task: &str,
        input: &serde_json::Value,
        _: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        use serde_json::json;
        if task.starts_with("extract") {
            let id = input["chunk_id"].as_str().unwrap();
            Ok(
                json!({"entities":[{"name":"Alpha","kind":"service","evidence":[id]},{"name":"Beta","kind":"service","evidence":[id]}],"relations":[{"source":"Alpha","target":"Beta","kind":"depends_on","evidence":[id],"quote":"Alpha depends on Beta"}]}),
            )
        } else {
            let id = input["chunks"]
                .as_array()
                .and_then(|v| v.first())
                .and_then(|v| v["id"].as_str())
                .or_else(|| {
                    input["sources"]
                        .as_array()
                        .and_then(|v| v.first())
                        .and_then(|v| v["chunk_id"].as_str())
                })
                .or_else(|| {
                    input["allowed_evidence"]
                        .as_array()
                        .and_then(|v| v.first())
                        .and_then(|v| v.as_str())
                })
                .unwrap();
            Ok(
                json!({"insufficient":false,"claims":[{"text":"Alpha depends on Beta","evidence":[id]}]}),
            )
        }
    }
}
#[test]
fn graphrag_extracts_summarizes_answers_and_invalidates_on_change() {
    let mut f = Fixture::new();
    f.note("A.md", "Alpha depends on Beta");
    f.sync();
    let cancel = AtomicBool::new(false);
    let d = f.index.build_graph(&Fake, &cancel).unwrap();
    assert!(!d.entities.is_empty());
    assert!(!d.summaries.is_empty());
    assert!(d.relations.iter().all(|r| r.origin == "inferred"));
    for mode in ["local", "global"] {
        let a = f
            .index
            .ask("Alpha", mode, &Filter::default(), &Fake, &cancel, 32_000)
            .unwrap();
        assert!(!a.insufficient);
        for c in a.claims {
            assert!(c
                .evidence
                .iter()
                .all(|id| a.sources.iter().any(|h| h.chunk_id == *id)));
        }
    }
    f.note("A.md", "Changed facts");
    f.sync();
    assert!(f
        .index
        .ask("Alpha", "local", &Filter::default(), &Fake, &cancel, 32_000)
        .is_err());
}
#[test]
fn model_cannot_promote_unknown_evidence_or_partial_graph() {
    struct Bad;
    impl backend::Model for Bad {
        fn fingerprint(&self) -> String {
            "bad".into()
        }
        fn embed(&self, _: &str) -> Result<Vec<f32>> {
            Ok(vec![1.0])
        }
        fn structured(
            &self,
            _: &str,
            _: &serde_json::Value,
            _: &serde_json::Value,
        ) -> Result<serde_json::Value> {
            Ok(
                serde_json::json!({"entities":[{"name":"Alpha","kind":"service","evidence":["invented"]}],"relations":[]}),
            )
        }
    }
    let mut f = Fixture::new();
    f.note("A.md", "Alpha depends on Beta");
    f.sync();
    assert!(f.index.build_graph(&Bad, &AtomicBool::new(false)).is_err());
    assert!(f.index.derived("bad").unwrap().is_none());
    assert!(!f
        .index
        .search("Alpha", &Filter::default(), 10)
        .unwrap()
        .is_empty());
}
#[test]
fn native_model_requires_explicit_provider_egress() {
    let f = Fixture::new();
    let cancel = AtomicBool::new(false);
    assert!(backend::NativeModel::new(
        &f.config,
        &f.root.join("model"),
        &mercury_router::RouteConfig::default(),
        &[],
        None,
        &cancel
    )
    .is_err());
}
#[test]
fn capture_accept_is_idempotent_and_human_edits_block_undo() {
    let f = Fixture::new();
    let p = f
        .index
        .propose(
            "codex:ended-session",
            "Runbook",
            "PV",
            "Use [[Queue]] after verification.",
        )
        .unwrap();
    assert_eq!(p.state, "proposed");
    assert_eq!(
        f.index
            .propose(
                "codex:ended-session",
                "Runbook",
                "PV",
                "Use [[Queue]] after verification."
            )
            .unwrap()
            .id,
        p.id
    );
    let accepted = f.index.review(&f.config, &p.id, "accept").unwrap();
    assert_eq!(accepted.state, "accepted");
    assert_eq!(
        f.index.review(&f.config, &p.id, "accept").unwrap().state,
        "accepted"
    );
    let path = f.config.vault.join("Mercury").join(format!("{}.md", p.id));
    fs::write(&path, "human edits").unwrap();
    assert!(f.index.review(&f.config, &p.id, "undo").is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), "human edits");
}
#[test]
fn rejected_proposal_never_publishes_and_undo_preserves_audit() {
    let f = Fixture::new();
    let p = f.index.propose("s1", "Title", "PV", "Evidence").unwrap();
    f.index.review(&f.config, &p.id, "reject").unwrap();
    assert!(f.index.review(&f.config, &p.id, "accept").is_err());
    let p = f.index.propose("s2", "Title", "PV", "Evidence").unwrap();
    f.index.review(&f.config, &p.id, "accept").unwrap();
    let undone = f.index.review(&f.config, &p.id, "undo").unwrap();
    assert_eq!(undone.state, "undone");
    assert!(!f
        .config
        .vault
        .join("Mercury")
        .join(format!("{}.md", p.id))
        .exists());
    assert_eq!(
        f.index.proposal(&p.id).unwrap().unwrap().markdown,
        p.markdown
    );
}
#[test]
fn context_is_project_scoped_and_egress_and_staleness_are_checked() {
    let mut f = Fixture::new();
    f.note("A.md", "---\nproject: PV\n---\nEvidence");
    f.sync();
    let b = f.index.context("Evidence", "PV").unwrap();
    assert!(!b.sources.is_empty());
    assert!(f.index.route_bundle(&b.id, "PV", false).is_err());
    assert!(f.index.route_bundle(&b.id, "other", true).is_err());
    assert!(f.index.route_bundle(&b.id, "PV", true).is_ok());
    f.note("A.md", "changed");
    f.sync();
    assert!(f.index.route_bundle(&b.id, "PV", true).is_err());
}
#[cfg(unix)]
#[test]
fn native_codex_runner_uses_stdin_and_isolated_flags_and_parses_jsonl() {
    use backend::Model;
    use std::os::unix::fs::PermissionsExt;
    let mut f = Fixture::new();
    let executable = f.root.join("fake-codex");
    let args_file = f.root.join("argv");
    let stdin_file = f.root.join("input");
    let script=format!("#!/bin/sh\nif [ \"$1\" = --version ]; then printf 'codex fixture\\n'; exit 0; fi\nprintf '%s\\n' \"$@\" > '{}'\ncat > '{}'\nprintf '%s\\n' '{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"{{\\\"ok\\\":true}}\"}}}}' '{{\"type\":\"turn.completed\",\"usage\":{{\"input_tokens\":10,\"output_tokens\":3}}}}'\n",args_file.display(),stdin_file.display());
    fs::write(&executable, script).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    f.config.allow_external = true;
    f.config.allowed_providers = vec!["codex".into()];
    f.config.executables.insert("codex".into(), executable);
    let cancel = AtomicBool::new(false);
    let model = backend::NativeModel::new(
        &f.config,
        &f.root.join("model"),
        &mercury_router::RouteConfig::default(),
        &[],
        None,
        &cancel,
    )
    .unwrap();
    let result = model
        .structured(
            "answer fixture",
            &serde_json::json!({"secret":"fixture-only-secret"}),
            &serde_json::json!({"type":"object"}),
        )
        .unwrap();
    assert_eq!(result["ok"], true);
    let args = fs::read_to_string(args_file).unwrap();
    assert!(args.contains("--ignore-user-config"));
    assert!(args.contains("shell_tool"));
    assert!(!args.contains("fixture-only-secret"));
    assert!(fs::read_to_string(stdin_file)
        .unwrap()
        .contains("fixture-only-secret"));
    assert_eq!(model.observations().len(), 1);
}
#[cfg(unix)]
#[test]
fn native_timeout_kills_the_process_and_reports_no_private_output() {
    use backend::Model;
    use std::os::unix::fs::PermissionsExt;
    let mut f = Fixture::new();
    let executable = f.root.join("fake-codex");
    fs::write(
        &executable,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo fixture; exit 0; fi\nexec /bin/sleep 30\n",
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    f.config.allow_external = true;
    f.config.allowed_providers = vec!["codex".into()];
    f.config.timeout_secs = 1;
    f.config.executables.insert("codex".into(), executable);
    let cancel = AtomicBool::new(false);
    let model = backend::NativeModel::new(
        &f.config,
        &f.root.join("model"),
        &mercury_router::RouteConfig::default(),
        &[],
        None,
        &cancel,
    )
    .unwrap();
    let start = std::time::Instant::now();
    assert!(model
        .structured("answer", &serde_json::json!({}), &serde_json::json!({}))
        .is_err());
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}
#[test]
fn job_polling_does_not_reinitialize_or_lock_the_active_index() {
    let mut f = Fixture::new();
    for i in 0..1000 {
        f.note(&format!("N{i}.md"), &format!("polling evidence {i}"));
    }
    let id = hash(b"concurrent-job");
    f.index
        .save_job(&id, &serde_json::json!({"state":"running"}))
        .unwrap();
    let root = f.root.join("data");
    std::thread::scope(|scope| {
        let poller = scope.spawn(move || {
            for _ in 0..30 {
                let index = Index::open(&root).unwrap();
                assert_eq!(index.job(&id).unwrap().unwrap()["state"], "running");
            }
        });
        f.index.sync(&f.config, &AtomicBool::new(false)).unwrap();
        poller.join().unwrap();
    });
    assert_eq!(f.index.status().unwrap().notes, 1000);
}
#[test]
fn unchanged_chunks_reuse_extraction_and_rebuild_discards_cache() {
    struct Counting(AtomicUsize);
    impl backend::Model for Counting {
        fn fingerprint(&self) -> String {
            "counting".into()
        }
        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            backend::Model::embed(&Fake, text)
        }
        fn structured(
            &self,
            task: &str,
            input: &serde_json::Value,
            schema: &serde_json::Value,
        ) -> Result<serde_json::Value> {
            if task.starts_with("extract") {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
            backend::Model::structured(&Fake, task, input, schema)
        }
    }
    let mut f = Fixture::new();
    f.note("A.md", "Alpha depends on Beta");
    f.sync();
    let model = Counting(AtomicUsize::new(0));
    let cancel = AtomicBool::new(false);
    f.index.build_graph(&model, &cancel).unwrap();
    assert_eq!(model.0.load(Ordering::Relaxed), 1);
    f.index.build_graph(&model, &cancel).unwrap();
    assert_eq!(model.0.load(Ordering::Relaxed), 1);
    f.index.rebuild(&f.config, &cancel).unwrap();
    f.index.build_graph(&model, &cancel).unwrap();
    assert_eq!(model.0.load(Ordering::Relaxed), 2);
    f.note("A.md", "Alpha depends on Beta after a verified change");
    f.sync();
    f.index.build_graph(&model, &cancel).unwrap();
    assert_eq!(model.0.load(Ordering::Relaxed), 3);
}
#[test]
fn invalid_answer_citations_get_one_bounded_repair_then_are_checked() {
    struct Repair(AtomicUsize);
    impl backend::Model for Repair {
        fn fingerprint(&self) -> String {
            "repair-fixture".into()
        }
        fn embed(&self, t: &str) -> Result<Vec<f32>> {
            backend::Model::embed(&Fake, t)
        }
        fn structured(
            &self,
            task: &str,
            input: &serde_json::Value,
            schema: &serde_json::Value,
        ) -> Result<serde_json::Value> {
            if task.starts_with("answer") {
                self.0.fetch_add(1, Ordering::Relaxed);
                let id = if task.contains("repair invalid citations") {
                    input["allowed_evidence"][0].as_str().unwrap()
                } else {
                    "invented"
                };
                Ok(
                    serde_json::json!({"insufficient":false,"claims":[{"text":"Alpha depends on Beta","evidence":[id]}]}),
                )
            } else {
                backend::Model::structured(&Fake, task, input, schema)
            }
        }
    }
    let mut f = Fixture::new();
    f.note("A.md", "Alpha depends on Beta");
    f.sync();
    let model = Repair(AtomicUsize::new(0));
    let cancel = AtomicBool::new(false);
    f.index.build_graph(&model, &cancel).unwrap();
    let a = f
        .index
        .ask("Alpha", "local", &Filter::default(), &model, &cancel, 32000)
        .unwrap();
    assert_eq!(model.0.load(Ordering::Relaxed), 2);
    assert!(a.claims.iter().all(|c| c
        .evidence
        .iter()
        .all(|e| a.sources.iter().any(|h| h.chunk_id == *e))));
}
#[test]
fn context_separates_reviewed_decisions_from_pending_and_deprecated_sources() {
    let mut f = Fixture::new();
    for (name, status) in [
        ("Approved", "validated"),
        ("Pending", "proposed"),
        ("Old", "deprecated"),
    ] {
        f.note(
            &format!("{name}.md"),
            &format!("---\nproject: PV\ntype: decision\nstatus: {status}\n---\nshared evidence"),
        );
    }
    f.sync();
    let b = f.index.context("shared", "PV").unwrap();
    assert_eq!(b.validated_decisions.len(), 1);
    assert_eq!(b.pending_sources.len(), 1);
    assert_eq!(b.deprecated_sources.len(), 1);
    assert!(b.inferred_relations.is_empty());
    assert!(b.validated_decisions.iter().all(|id| b
        .sources
        .iter()
        .any(|h| h.chunk_id == *id && h.status == "validated")));
}
