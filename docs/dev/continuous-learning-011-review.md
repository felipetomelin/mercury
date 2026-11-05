# 011 — Continuous learning: review and validation

Date: 2026-11-05. User authorization: enable Learn/Knowledge by default, automate
session capture through graph generation, test, validate and publish.

## Verdict

**Local review: PASS.** No blocking findings remain. Scope: defaults/privacy,
managed target resolution, event provenance, routed review, durable capture
journal, budget/retry behavior, publication, monitoring, status/API/UI, reload
and handoff. Existing explicit false settings remain respected by the defaults.
The user separately authorized enabling the personal configuration in this task.

External SonarQube/Veracode/Wiz and quantitative coverage remain **waived / not
executed** under the Mercury constitution. No external scan is claimed as passing.

## Behavior and safeguards

- Learn/Knowledge are default-on with local managed scope; model processing is
  still explicit through `allow_external`. Invalid startup config fails closed.
- No configured targets means only managed `LearnMemory.md` (8 KiB). Existing
  user instruction files, routing and Cron edits require exact explicit targets.
- Observed current Claude/Codex session completion feeds capture independently
  of Learn review. In-progress turn milestones only feed Learn. Existing origin
  exclusions prevent automatic sessions from recursively teaching themselves.
- Captures persist identity, proposal association, retry state and sanitized
  errors. Existing proposals are reused after restart without repeating the LLM.
- New notes publish as `status: proposed`. The managed-memory writer also forces
  learning/proposed metadata. Publication acceptance is distinct from human
  validation, and inferred graph relations remain separate.
- Publication stages/syncs complete bytes, then uses atomic create-new hard-link
  semantics. It never renames over another editor's file; failure cleans owned
  staging. Original proposal journal/undo/conflict protections are retained.
- The monitor polls every 30 seconds with a five-second debounce. Unchanged
  revisions reuse derived data/extractions. Lexical reads do not wait for an LLM.
- A persistent daily budget conservatively reserves two provider attempts per
  structured call, covering fallback; routed Learn and automatic Knowledge share
  it. Recognizable credential patterns block automatic input/output processing;
  this screening is deliberately limited, not exhaustive secret detection.
- Native review supports Codex/Claude routing, observed quota updates, cancellation
  and tool-free subprocesses. Unknown Codex dollar costs remain absent.
- Cancellation, bounded stop and a file lease coordinate reload/handoff generations.
  Already persisted steps remain auditable; cancellation is distinct from undo.

## Evidence

| Check | Result |
| --- | --- |
| Full workspace | 4,156 passed, 0 failed, 16 ignored |
| Final Knowledge crate tests after atomic-publication/status refinements | 30 passed |
| Final server Knowledge/API/worker scope | 9 passed, 1 explicit live smoke ignored |
| Maintenance/installer/vendor/packaging/hot-path checks | 66 passed |
| Clippy, workspace/all targets, warnings denied | PASS |
| Locked offline build and formatting | PASS |
| Generated API schema, Markdown local links and diff checks | PASS |
| Real synthetic Learn review through Codex | One change applied; undo restored original bytes; cost unknown |
| Final real synthetic capture → note → index → GraphRAG | 3 notes, 9 entities, 5 inferred relations, 2 community summaries; 14 conservative call reservations |

The real Knowledge smoke completed in about 77 seconds; this is one synthetic
run, not a performance/quality guarantee. An earlier live run exercised rejection
and retry of invalid extraction evidence. Evidence IDs are now constrained by
the extraction schema in addition to deterministic validation; unsupported claims
are still rejected. The explicit live tests are ignored during normal runs and
require their `MERCURY_*_LIVE_SMOKE=1` opt-in. They use only temporary synthetic
sessions, targets and vaults, never personal conversations.

The full-suite run used the required filesystem/socket permissions. Initial
failures revealed old default-off assumptions, a test-background isolation gap
and UI status hiding historical counts; all were corrected and rerun successfully.
Atomic publication and status parsing received the final focused test cycle.

Local logs: `/private/tmp/mercury-011-*.log`. Reproduction of live tests:

```sh
MERCURY_LEARN_LIVE_SMOKE=1 cargo test --bin mercury \
  live_routed_learn_smoke_with_synthetic_memory_only -- --ignored --nocapture
MERCURY_KNOWLEDGE_LIVE_SMOKE=1 cargo test --bin mercury \
  live_automation_smoke_with_synthetic_session_only -- --ignored --nocapture
```

## Personal activation

Created private original/prepared config backups and consistent SQLite backups;
backup paths and raw inventories remain private, outside the repository.
Applied and validated explicit model consent and Codex-first automation rules,
retaining the existing vault and unrelated settings. Installed the dev binary
and completed live handoff.

Before/after verification confirms the same public pane ID, shell PID and
foreground process group, with byte-identical Cron configuration. Internal
terminal IDs were remapped by handoff; they are not process identities. The
server is current, with neither restart-needed nor stale-binary flags. Learn
reports enabled/external-allowed/worker-running; Knowledge reports enabled,
automation enabled, external allowed, monitor watching and graph current.
The initial managed memory is present. A private non-Markdown probe confirmed
atomic-publication filesystem support and was removed immediately.

## Limits

Signal replay is durable after journal persistence, not before the bounded input
queue is drained. Source identity changes fail conservatively; human edits are
not replaced. Graph retry exhaustion requires inspection/rebuild; budget exhaustion
waits for a later UTC day. Failed Learn triggers remain recorded rather than being
silently resent. Source/model availability and account quotas still affect work.

The monitor performs bounded scans rather than using a native filesystem watcher.
It does not retroactively collect arbitrary provider history. Context insertion
into Route remains explicitly configured. macOS is validated here; this work does
not qualify Linux/Windows or publish prebuilt releases/update feeds.
