# Feature Specification: Mercury Auto Improvement

**Feature Branch**: current dirty worktree (no branch/commit until quality gate)
**Created**: 2026-10-06
**Status**: implemented for local use per PLAN.md decisions of 2026-10-07; real-provider/manual acceptance remains separate. Reconciled 2026-10-29.
**Input**: `PLAN.md` items 20–24 and `docs/dev/learn-phase7-spec.md`

## User scenarios and testing

### US1 — Learn safely from a finished session (P1)

When Learn is enabled, a completed Claude/Codex session can produce a small
allowlisted improvement that is applied automatically, backed up, audited, and
reversible. Without a reliable session identity or approved target, nothing is
written.

1. Given an enabled Learn and a permitted file, when a valid review proposes a
   change after session end, then Mercury records a private backup and audit
   item before writing, and `learn undo <id>` restores the original bytes.
2. Given a disabled Learn, invalid proposal, symlink, or target whose hash
   changed before final validation, when the same event arrives, then no file
   is overwritten.
3. Given repeated events or restart, when the same trigger key is seen again,
   then at most one review is dispatched.
4. Given a trusted completed-session signal with a transcript whose identity
   validates, Mercury retains only bounded private session metadata for later
   weekly review. Invalid or active/incomplete transcripts are not indexed;
   a weekly consumer must revalidate the transcript before reading content and
   reject a file whose identity or metadata changed after completion. A weekly
   due key without an eligible completed session is not claimed.

### US2 — Inspect and undo automatic changes (P1)

Users can inspect applied and failed changes in CLI and Learn tab and undo one
item or a date range without silently replacing their subsequent edits.

1. Given an applied item whose target still matches the applied hash, when the
   user requests undo, then the exact backup bytes are restored and the ledger
   records undo.
2. Given later human edits or a missing backup, when undo is requested, then
   Mercury reports a conflict and preserves the current file.
3. Given a new audited change, when diff is requested before or after undo,
   Mercury reports bounded line-change structure from private verified
   before/after snapshots without returning file text. For legacy changes
   without an after snapshot, it reports explicitly that the diff is
   unavailable rather than comparing against unrelated current bytes.
4. Given the Learn tab, when the user selects a record, its redacted metadata
   and structural diff are visible. Undo requires an explicit second action;
   a stale or conflicted target leaves the human file unchanged and the tab
   refreshes after a successful undo. The server MUST advertise all Learn
   methods used by the tab in the client-shell capability handshake; a direct
   CLI/API success alone does not satisfy this acceptance case.
5. Given recent trigger ledger entries, the Learn tab shows a bounded audit
   of type, status, timestamps, and stable fingerprint without exposing raw
   session IDs or transcript paths. Change detail reports whether its private
   backup can be verified; model-supplied reason remains redacted.

### US3 — Periodic review and reversible curation (P2)

Mercury reviews every configured N turns and weekly, and proposes cleanup of
exact duplicate skills/memory while preserving recovery. Effectiveness
metrics report only observed cost/tokens and comparable outcomes.

1. Given a milestone or weekly due key, when the scheduler restarts, then one
   bounded review runs without reviewing its own process or cron jobs.
2. Given an archival/merge proposal, when it targets an allowed skill/memory,
   then the original remains recoverable through backup and ledger.
3. Given insufficient task baseline, when metrics are displayed, then they say
   data is insufficient rather than imply an improvement.
4. The 30-day unused-skill archival requirement was removed by explicit user
   decision on 2026-10-07. Missing observations never authorize archival.
5. Given two exact allowlisted targets, an exact duplicate skill may be
   deduplicated, or same-directory memories may be combined. The proposal
   retains both original byte streams in the reversible ledger, archives the
   source logically, and leaves failed steps or human edits visible for
   recovery. Cross-directory non-identical content is not merged.
6. A weekly curator may automatically deduplicate only byte-identical
   allowlisted skills or same-directory memories. It must not infer a 30-day
   no-use window or merge distinct memories without trusted evidence/proposal;
   missing evidence results in no such automatic change.
7. A headless cron run may contribute a task sample only when its provider
   reports complete numeric input/output token counts. A cost is recorded
   only when explicitly reported; script/pane runs and partial or malformed
   usage contribute no token sample. The run ID is the observation identity
   and the job ID is an opaque task identity. Such runs are stored as
   `observed` unless an explicit job-to-target mapping and a single intact
   applied change establish the baseline/after boundary. Comparison requires
   samples on both sides for the same job/provider and is descriptive, never
   causal. Replaying a run after restart must not duplicate a sample.
8. Given observed headless samples, the Learn status API and tab show at most
   sixteen recent records with opaque task identity, provider, time, reported
   tokens, optional reported cost, and observed outcome. Prompt text, paths,
   raw provider envelopes, and untrusted model text never enter this surface.
   Older server responses without the field remain readable. Observed-only
   samples are labeled insufficient for before/after comparison.
9. If Mercury stops after the cron run is durably completed but before the
   Learn sample is written, restart retries the same bounded observation
   without duplicating it. A temporary Learn write failure while Mercury stays
   running is retried on a bounded interval without waiting for another run;
   retry attempts do not multiply observations or block the UI. Staging contains
   only IDs, provider, reported
   numeric usage, and outcome; no prompt or output text. The Learn sample time
   is the persisted run completion time, including after delayed replay, not
   the reconciliation time. Learn disabled does not create new staged samples.

## Edge cases

- Active/incomplete transcripts, missing session IDs, provider auth or quota
  errors, malformed reviewer JSON, partial writes, crash between backup/rename
  and ledger update, and repeated hook/visual events fail closed or recover.
- Transcript text and reviewer output may contain prompt injection; neither
  grants filesystem path, shell, Git, network, or credential authority.
- Large transcript, output, file, queue, or spend is capped before work starts.
- User or editor may change an allowed file between read and rename; recheck
  hash/identity immediately before rename and document residual filesystem race.

## Functional requirements

- **FR-001**: Learn MUST be disabled by `learn.enabled=false`; when enabled,
  valid improvements MUST apply without per-item approval.
- **FR-002**: Learn MUST review completed sessions, N-turn milestones, and
  weekly due events with persistent dedup and recursion exclusion.
- **FR-003**: The reviewer MUST receive bounded conversational transcript via
  stdin, run `claude -p` without tools/persistence, and have time, output, and
  cost limits; invalid/error output MUST NOT reach the writer.
- **FR-004**: Only exact configured target IDs and permitted operations MUST be
  accepted; path traversal, symlink, wrong type, oversized content, and config
  targets outside allowed sections MUST be rejected.
- **FR-005**: Every applied item MUST have a private pre-write backup, durable
  ledger item with source/reason/hashes, atomic target write, and recovery state.
- **FR-006**: Undo by ID/date MUST restore exact bytes only when current hash
  matches the applied state; conflict MUST preserve current bytes.
- **FR-007**: CLI/API/Learn tab MUST expose list, show, diff, status, and undo
  with bounded/redacted responses and a regenerated public schema. Diff MUST
  be based on verified private before/after bytes and return structural line
  counts only by default; it MUST NOT expose raw file content or infer old
  applied bytes from a target that may have since changed.
- **FR-008**: Curator MUST archive/merge allowed skill/memory reversibly, with
  no permanent deletion outside backup retention policy. The curator proposes
  deterministic changes only; the existing writer performs every mutation.
- **FR-009**: Metrics MUST distinguish observed cost/tokens, comparable task
  outcome, and insufficient data.
- **FR-010**: Model-proposed changes MUST NOT commit/push, auto-trust a folder,
  execute shell, alter secrets/hooks, or write unlisted code/source files.

## Key entities

- **Target policy**: Stable ID, exact path/section, type, maximum bytes.
- **Review trigger**: Provider, session, reason, turn mark, origin, dedup key.
- **Proposal**: Target ID, bounded content/operation, reason, source evidence.
- **Change item**: Ledger ID, target, hashes, backup, timestamps, status/undo.
- **Task metric**: Task identity, observed tokens/cost/outcome, comparison state.

## Success criteria

- **SC-001**: Fixture session end applies one valid change, visible in CLI/tab,
  and undo restores byte-for-byte original content.
- **SC-002**: Fault-injection tests at backup, ledger, temp write, rename,
  recovery, and undo preserve current bytes whenever hash divergence is
  observed before replacement; unavoidable external-writer race is documented.
- **SC-003**: Duplicate triggers across restart produce at most one review per
  stable key; Learn/cron-originated sessions produce none.
- **SC-004**: Local unit, integration, API/schema, isolated smoke, TUI inspection,
  and mandatory SonarQube/Veracode/Wiz/coverage gates pass before completion.

## Assumptions and dependencies

- `learn.enabled=false` is the confirmed default; explicit true opts into writes.
- Real Claude/Codex smoke and human visual inspection were assigned to the user
  in PLAN.md (2026-10-07). Automated fixtures are not equivalent acceptance.
- Source architecture: `docs/dev/learn-phase7-spec.md`; historical research
  describes intermediate stages and does not override confirmed decisions.
- Structured route edits apply only to `[route]` in the active config. Cron
  edits target an existing UUID and only schedule/enabled/timeout_seconds;
  active runs block changes. Backup, conflict-aware undo and reload apply.
- Teams bidirectional integration remains separate backlog. SMTP is excluded.
- The removed 30-day unused archival rule must not be resurrected by old tasks.
