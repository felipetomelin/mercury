# Implementation Plan: Mercury Auto Improvement

## Current decisions — reconciliation 2026-10-29

This section supersedes intermediate design/pending statements below, which
remain as implementation history. PLAN.md user decisions of 2026-10-07 confirm:
Learn defaults off; structured route/cron editing and explicit job-to-target
mapping are implemented; comparison is descriptive; the 30-day unused archival
rule is removed. The external-scanner/coverage exception applies only to that
local delivery. Human visual inspection and real-provider smokes remain separate.
Evidence: `src/config/learn.rs`, `crates/mercury-learn/src/policy.rs`,
`crates/mercury-learn/tests/{review,metrics}.rs`, `src/app/learn.rs`.
Current revalidation: `docs/dev/mercury-current-audit-20261029.md`.


**Branch**: current worktree | **Date**: 2026-10-06 | **Spec**: `spec.md`

## Summary

Implement the `PLAN.md` Phase 7 with a small Rust core for reversible changes,
then a bounded transcript/reviewer pipeline, persistent triggers, API/CLI/TUI,
curator, and metrics. No model output directly selects filesystem paths or
executes commands. Full behavior remains behind explicit `[learn]` activation
under the provisional opt-in default.

## Technical context

- **Language**: Rust 1.96.1, existing Mercury workspace.
- **Dependencies**: `rusqlite` bundled SQLite, existing `sha2`/UUID packages,
  Claude CLI for real review only; no new network service.
- **Storage**: private Mercury data `learn/` with SQLite ledger and `backups/`;
  atomic replacement in target directory with metadata/hash revalidation.
  New ledger items also keep a private hash-verified after snapshot for
  historical redacted diff; older items without it return unavailable.
- **Testing**: `cargo test` for crate, config, server, API, CLI, and TUI fixtures;
  fault injection and restart tests; isolated socket smoke.
- **Platforms**: macOS/Linux first; Windows requires explicit ACL validation
  before claiming equivalent protection.
- **Performance/limits**: server event loop never blocks on transcript/SQLite/
  reviewer; worker queue, transcript bytes, file size, process time/output, and
  review spend are bounded by validated config.

### Analyze: Learn methods in the client-shell handshake

The CLI speaks to the API socket directly, but the TUI sends endpoint requests
over the client-shell transport. `src/server/client_commands.rs` has a separate
allowlist that is also advertised at handshake; if it omits Learn methods, the
client refuses them before the API handler is reached. Add only the five
methods the tab uses (`learn.list`, `learn.status`, `learn.show`, `learn.diff`,
`learn.undo`) and test both the server allowlist and a client with the actual
advertised capabilities. Do not expose hook-only or date-range undo methods
through this surface without a TUI consumer.

## Constitution check

`spec.md` covers `.specify/memory/constitution.md`: spec-first, exact allowlist,
private backup, audit, conflict-aware undo, no arbitrary shell/Git/trust,
bounded workers, distinct local and real-provider verification, blocking
external gate. The provisional default is recorded, not silently assumed.
No production code should write real user files during development.

## Project structure

```text
crates/mercury-learn/         core policy, ledger, backup, apply/undo, reviewer
src/config/                   [learn] parsing/validation/reload
src/app/                      trigger ownership, scheduler, bounded worker
src/server/headless/          typed learn endpoint dispatch
src/api/schema/               learn request/result types
src/cli/                      mercury learn list/show/diff/undo
src/client/shell/             Learn tab/state/actions/rendering
tests/                        API/socket and integration fixtures
docs/dev/                     research, detailed design, review evidence
```

## Sequence and decisions

1. **Foundation**: Validate exact target IDs/paths/type/size; private backup,
   durable ledger, atomic replace, crash recovery, hash-checked undo. Core takes
   an explicit enable flag; no config or reviewer integration yet.
2. **Review**: Parse bounded conversational text only; read at most a small
   identity header plus a 4 MiB JSONL tail, reject oversized individual lines,
   cap retained turns/text, and mark truncation instead of claiming a complete
   turn count. Require a trusted expected session ID and matching Claude file
   stem or Codex `session_meta`; skip sidechains, tools and thinking. Then
   launch `claude -p` with no tools, JSON schema, stdin, timeout/output/spend
   limits. The stdin envelope contains the bounded transcript and bounded
   current text for exact allowlisted target IDs, never paths or credentials;
   the source identity is attached by Mercury, not supplied by the model.
   Invalid output is discarded before core. The trusted reviewer snapshot hash
   must be passed separately to apply; a file changed since that snapshot is
   a conflict, even if it is still allowlisted. The model cannot choose or
   override that expected hash.
3. **Triggers**: Use reported session IDs and transcript turns; Claude hooks
   may supply additional signals, not sole authority. Codex lacks equivalent
   native hook contract here. Dedup keys persist across restart; exclude Learn
   and cron origin; weekly due handled internally. A bounded nonblocking queue
   feeds one owner worker. It durably claims a trigger key before starting a
   review; an interrupted claim is reported as indeterminate and is not
   automatically replayed, preserving at-most-once dispatch across restart.
   Trusted completed-session signals that pass bounded transcript validation
   are also indexed in a private SQLite table (at most 64 metadata rows, no
   transcript text) so the later weekly scheduler can select candidates after
   restart. Selection must revalidate transcript identity and freshness; a
   weekly timer alone must never claim a key whose handler has no review input.
   Store a file fingerprint with each completed-session index entry, migrate
   older entries as ineligible, and compare it before and after weekly
   transcript validation. The scheduler selects a session completed in the
   preceding UTC calendar week and dispatches only when the candidate remains
   unchanged; the durable trigger ledger deduplicates across restarts.
   A visual `Idle` transition is not session completion. Session-end requires
   a trusted process-exit observation or an explicit provider end signal with
   matching session identity. Mid-session N-turn review requires a stable
   complete conversational snapshot and exact retained turn count; a truncated
   count is ineligible. Claude's managed hook reports SessionStart, Stop as a
   turn probe, and SessionEnd as a distinct completion signal; the old
   SessionEnd/release hook is removed idempotently without regressing release
   ownership. Codex's managed hook reports start identity only. For turn
   milestones, Claude's managed `Stop` hook reports an explicit end-of-turn
   signal, while Codex may use an observed idle transition only as a probe,
   never as proof of session completion. The bounded Learn worker reads the
   transcript off the UI loop, derives an exact count from a stable complete
   snapshot, and only then claims a `provider:session:turn:N` key. Changed,
   incomplete, truncated, or non-multiple snapshots produce no claim or write.
   Both signals must match the currently reported pane/session identity and
   exclude Learn/cron origins. The Learn reviewer subprocess strips pane
   authority environment variables; a cron `agent.prompt` marks its current
   pane session ineligible for Learn, including the next reported identity
   when no identity was known at submission time.
4. **Surfaces**: Config, API/schema, CLI, Learn tab; the tab stores only
   redacted typed API responses, refreshes list/status asynchronously, and
   requires a second `u` on the same selected item before undo. Curator uses
   the same reversible core; metrics distinguish real observation from inference.
   Status includes at most 16 recent trigger fingerprints, types, states and
   timestamps, never raw session IDs or transcript paths. Detail reports only
   verified backup availability; model-supplied reasons remain redacted.
   The private metric ledger retains at most 4096 samples with an opaque task
   key, stable observation ID, provider, baseline/after phase, observed tokens,
   optional cost and optional outcome. Replayed observations are deduplicated
   while retained. Comparison requires both phases for the same task and
   provider with known outcomes; it reports observed cohorts, never a causal
   improvement claim. T019 must provide trusted samples rather than accept
   reviewer-generated numbers as observations.
   As the first trusted producer, parse optional bounded token usage from the
   headless Claude result envelope or Codex `turn.completed` JSONL event in
   `mercury-cron`; never infer missing counts from text. Carry this metadata
   separately from output text to the scheduler completion path. After a
   successful cron ledger finish, enqueue an idempotent `observed` row for a
   blocking worker to write to the private Learn metric ledger when Learn is
   opted in. No sample is persisted
   from script/pane runs or malformed/partial provider usage, and a sample is
   never assigned a baseline/after phase merely from elapsed time. Migrate
   old task-metric schema without losing baseline/after rows. A later T019
   step must establish the phase boundary before displaying comparisons.
   Expose at most sixteen newest `observed` records through the existing
   redacted `learn.status` response, with an optional default-empty field for
   old servers. The Learn tab gets a read-only metrics page; this page cannot
   imply that the observed run history is an efficacy comparison.
   To close the crash gap, the cron ledger atomically stages a bounded private
   usage outbox item with run completion, only while Learn is opted in. A
   background reconciler copies pending rows into Learn by stable run ID and
   acknowledges each only after the Learn write succeeds. Restart retries
   remaining rows; no prompt, model text, or output path enters the outbox.
   Read the completed run timestamp from the cron ledger when replaying its
   outbox row and persist that timestamp in Learn; reject a replay with the
   same observation ID but a different timestamp. Existing historical samples
   retain their stored timestamps because their original completion times may
   not be recoverable. A job's requested skill names are prompt text only and
   cannot establish actual skill use or a causal before/after boundary.
   While Learn and Crons remain enabled, wake the existing background
   reconciler every five minutes. Include that timer in the headless loop's
   deadline so a system with no due jobs still retries. Skip a retry if another
   reconciliation is already running; do not queue blocking workers. The
   outbox's run-ID dedup and ack-after-write remain the correctness boundary.

### Analyze: periodic Learn usage replay

- The retry timer belongs to `App`, because its loop already combines cron
  deadlines and opt-in state. The SQLite read/write remains in `spawn_blocking`.
- A static nonblocking lock prevents overlapping startup, completion and
  periodic reconciliation jobs; a skipped attempt is recovered by the next
  tick. The durable outbox remains authoritative across process shutdown.
- The timer is armed only with both Crons and Learn enabled and disarmed when
  Learn is disabled. No network delivery or Teams behavior changes.
   Curator T017 reads exact target snapshots through the same safe reader used
   by Learn. A complete >=30-day no-use observation ending within one day of
   the current clock can produce a logical archive stub; otherwise it fails
   closed. Only same-directory memories can
   have distinct content combined; exact duplicate skills can be deduplicated
   without copying content between directories. Different IDs for the same
   path are rejected. Merge then archives the source;
   each step carries its own trusted preimage hash. No file is deleted
   or moved by the curator, and T019 owns ordered apply/recovery through the
   existing backup/ledger/undo writer.
   A separate `curation:weekly:YYYY-MM-DD` key allows deterministic duplicate
   cleanup independently of a session transcript/model review. At most four
   exact-duplicate sources are archived per run; source ordering follows the
   configured target order. Only same-directory memory duplicates qualify.
   The writer applies each step with its trusted preimage hash and leaves any
   partial result visible for explicit undo/recovery. Automatic no-use archive
   remains disabled until a complete trusted 30-day observation exists.
5. **Gates**: One review/test cycle per coherent group, isolated smoke, manual
   TUI, external SonarQube/Veracode/Wiz/coverage before commit/completion.

## Consistency analysis

- The plan's automatic application and provisional disabled default are
  compatible: explicit enablement triggers automatic writes without approval.
- The existing Claude integration removes legacy Stop/SessionEnd Mercury hooks;
  trigger implementation must add its own idempotent signal or infer from
  terminal/session transitions without disturbing third-party hooks.
- `mercury-history` is a search index, not a role-aware transcript API; Learn
  needs a separate limited reader or typed internal extension.
- Cron's executor is a pattern for subprocess limits, not a change ledger or
  safe source of recursive Learn jobs.
- Cross-process filesystem CAS is not universally available; immediate hash
  and identity rechecks reduce but cannot eliminate a concurrent external
  rename/write race. Review and docs must retain that residual risk.

## Open confirmation and gate

The user may change the `enabled` default. Real reviewer smoke requires an
authorized test target and account; external quality tools currently unavailable.
These do not relax unit/integration or backup/undo requirements.
