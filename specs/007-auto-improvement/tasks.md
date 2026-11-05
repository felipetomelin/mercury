# Tasks: Mercury Auto Improvement

**Current reconciliation**: 2026-10-29; PLAN.md confirmed decisions supersede historical open scope.

**Input**: `spec.md`, `plan.md`, `docs/dev/learn-phase7-research.md`
**Execution**: grouped sequentially in this workspace; `[P]` means file-level
independence, not authorization to spawn agents or bypass a gate.

## Phase 1 — Foundation

- [x] T001 Create `crates/mercury-learn/Cargo.toml` and register the crate in `Cargo.toml`.
- [x] T002 [P] Add exact target-policy and structured proposal contracts in `crates/mercury-learn/src/policy.rs` (FR-004, FR-010). Config section editing remains explicitly unsupported until the section-aware merger is implemented; no writer is connected.
- [x] T003 [P] Add private ledger schema, backup path validation, and recovery states in `crates/mercury-learn/src/storage.rs` (FR-005). Crash reconciliation and target replacement remain in T004–T005.
- [x] T004 Implement hash/identity-checked atomic apply and undo in `crates/mercury-learn/src/changes.rs`, reexported by `lib.rs` (FR-001, FR-005, FR-006). Unix-only until Windows ACL/identity semantics are specified.
- [x] T005 Add fault-injection, conflict, symlink, size, and restart tests in `crates/mercury-learn/tests/changes.rs`, `tests/storage.rs`, and internal writer tests (SC-001, SC-002). External-writer race after final check remains documented.

## Phase 2 — US1: review after sessions (P1)

**Independent test**: a fixture session proposes a valid allowlisted change;
an invalid/prompt-injected response writes nothing.

- [x] T006 [P] [US1] Add bounded Claude/Codex conversational transcript reader in `crates/mercury-learn/src/transcript.rs` (FR-003). Caller must prove session completion before invoking the reader; that trigger gate is T009–T010.
- [x] T007 [P] [US1] Add simulated reviewer contract and real `claude -p` runner in `crates/mercury-learn/src/reviewer.rs` (FR-003). Fake-executable contract tests pass; live model smoke remains pending.
- [x] T008 [US1] Integrate validated `[learn]` config and default-off kill-switch in `src/config/model.rs` and `src/config/learn.rs` (FR-001, FR-004). No runtime worker is enabled yet.
- [x] T009 [US1] Add persistent trigger dedup and bounded server worker in `src/app/learn.rs` (FR-002). Signal producers and review/apply handler are connected in T010; no live dispatch yet.
- [x] T010 [US1] Connect session/turn/week signals without recursion in `src/app/api.rs`, `src/app/api/learn.rs`, `src/app/learn.rs`, and `src/integration/claude_settings.rs` (FR-002). Claude Stop and Codex idle are probes only; the worker validates exact completed turn counts before claiming, and cron pane prompts exclude their session. Live model smoke remains T022.
- [x] T011 [US1] Add parser, runner, dedup/restart, and non-recursion tests in `crates/mercury-learn/tests/review.rs` and `src/app/learn.rs` (SC-003). The consolidated integration test covers one durable claim, simulated review, restart, and exact undo; parser/reviewer and recursive-origin cases are covered by the adjacent suites.

## Phase 3 — US2: inspect and undo (P1)

**Independent test**: applied change is listed, diffed, and undone by ID/date;
human-edited target yields conflict and remains unchanged.

- [x] T012 [P] [US2] Add typed Learn API and regenerated schema in `src/api/schema/learn.rs` and `docs/next/api/mercury-api.schema.json` (FR-007).
- [x] T013 [US2] Add server handlers for list/show/diff/undo in `src/server/headless/learn.rs` (FR-006, FR-007).
- [x] T014 [P] [US2] Add `mercury learn` CLI in `src/cli/learn.rs` (FR-006, FR-007).
- [x] T015 [US2] Add Learn tab and keyboard actions in `src/client/shell/learn_view.rs` (FR-007). Manual TUI inspection remains T022.
- [x] T016 [US2] Add API/socket, CLI, and TUI tests in `tests/` and `src/client/shell/tests/learn_view.rs` (SC-001, SC-004). Automated smoke uses a temporary server; manual TUI inspection remains T022.
- [x] T016a [US2] Advertise and allow the five Learn methods used by the TUI through the client-shell transport; test with negotiated capabilities and repeat the PTY smoke.

## Phase 4 — US3: weekly curation and metrics (P2)

**Independent test**: weekly due key runs once; curator archives/merges an
allowlisted item reversibly; no baseline yields “insufficient data”.

- [x] T017 [P] [US3] Add reversible skill/memory curator in `crates/mercury-learn/src/curator.rs` (FR-008). Trusted usage evidence, scheduled application, and partial-plan recovery orchestration remain T019.
- [x] T018 [P] [US3] Add observed task metric storage/aggregation in `crates/mercury-learn/src/metrics.rs` (FR-009). Trusted sample producers and the UI remain T019.
- [x] T019 [US3] Weekly exact-duplicate curation, durable observation replay, explicit job-target mapping and descriptive comparison are implemented. Evidence: `src/app/learn.rs`, `crates/mercury-learn/src/metrics.rs`, `tests/metrics.rs`, `src/client/shell/learn_view.rs`. No 30-day unused archive requirement remains (PLAN.md decision 2026-10-07).
- [x] T019b [US1] Structured route/cron edits, validation, backup/undo and reload are implemented. Evidence: `src/config/learn.rs`, `crates/mercury-learn/src/policy.rs`, `tests/review.rs` route/cron tests, `src/app/learn.rs::tests::route_and_cron_review_notify_app_after_durable_apply`.
- [x] T020 [US3] Restart/curation/metrics and phase-boundary tests exist in `crates/mercury-learn/tests/{curator,metrics}.rs`; no-use tests are not required after removal of that feature. New execution evidence is recorded in `docs/dev/mercury-current-audit-20261029.md`.
- [x] T020a [US3] Retry pending Learn usage periodically while opted in, without a new cron run, and test timer/opt-out behavior. Preserve the outbox as the source of truth.

## Phase 5 — Consolidated verification

- [x] T021 README, PLAN and technical phase-7 spec document current behavior and risks. Specs/tasks reconciled again on 2026-10-29; historical reports retain their original evidence dates.
- [ ] T022 Run grouped review, tests, schema/build/Clippy, isolated socket smoke, and manual TUI; record evidence in `docs/dev/learn-phase7-review.md` (SC-004).
- [x] T023 Explicit exception recorded in PLAN.md on 2026-10-07 for local phase-7 delivery (scanners/quantitative coverage only). Those tools did not pass; this does not waive the new feature 008 gates or manual/real-provider acceptance.

## Dependencies and execution order

T001–T005 form the reversible foundation. T006–T011 depend on it. US2 may
start after foundation and before live reviewer integration; US3 depends on
trigger ownership and the same core. T021–T023 follow all user stories. Tasks
marked `[P]` touch distinct files and can be prepared independently, but the
single-review/single-test-cycle policy applies per coherent group.
