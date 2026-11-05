# Development and local operation

Terminal agent controller for Claude Code + Codex CLI, with crons, usage/cost tracking,
model routing, and auto-improvement. Public source preview. Build and run locally; packaged releases and a hosted
update feed are not available yet.

See [`PLAN.md`](./PLAN.md) for the plan, decisions, and current phase.

For the connected-account Usage dashboard, guided Cron editor and local memory,
see the [Usage and Crons guide](./docs/dev/usage-cron-guide.md).


For local use after building, run `target/debug/mercury` from this checkout.
Usage, themes, crons, routing, history, and the opt-in Learn flow are implemented
locally. Real Claude/Codex smokes and visual inspection are left to the user;
SonarQube/Veracode/Wiz and quantitative coverage were explicitly waived for
this local delivery, not reported as passing. Keep Learn off while trying it
on temporary targets first. Teams interaction is in the backlog; Mercury does
not send email. Do not use `mercury update` yet: this local fork has no
published Mercury release feed. Windows distribution still requires platform qualification; pinned third-party
notices/licenses were restored, as documented in the local phase8 review.

To try the local build without explicitly scheduling or submitting a new task:

```bash
target/debug/mercury config check
target/debug/mercury
```

Keep the Mercury UI running, then use a second terminal for server-backed
commands such as:

```bash
target/debug/mercury history search --query "your search terms"
target/debug/mercury cron doctor
```

`config check` works without a server. `history search` and `cron doctor` return
`server_not_running` if the UI/server is not running. The UI opens or restores
a persistent terminal session, including any saved session. Its Usage, Crons,
History, and Learn tabs can be inspected locally; Learn review remains off
unless you explicitly enable and configure it. `route preview --stdin` is
read-only; `route submit` and `cron run` intentionally start agents/jobs.

If `mercury status` says `restart_needed: yes` or
`server_binary_stale: yes`, the CLI is reaching an older server process even
when both report version 0.9.3. Save work and detach the UI (`Ctrl+B`, then
`q`); from a normal terminal, run
`target/debug/mercury server live-handoff --import-exe "$PWD/target/debug/mercury"`.
Check `target/debug/mercury status` again before trying Crons, Route, Usage,
or Learn. Handoff is intended to preserve panes; if it fails, do not kill
server processes blindly—inspect the status and server log first. A Codex
agent sandbox may reject the local Mercury socket with `Operation not
permitted`; run manual smoke commands in your own terminal.

For a disposable Cron check after the server is current, create a script job
with `target/debug/mercury cron add --name smoke-local --schedule "every 1h"
--script /usr/bin/true`. Copy the returned job ID, then run
`target/debug/mercury cron run JOB_ID`, inspect
`target/debug/mercury cron runs JOB_ID`, and remove it with
`target/debug/mercury cron rm JOB_ID`.
The simple CLI form enables local desktop notification on completion and the
job remains scheduled until removed; it does not send email.
`target/debug/mercury cron list` shows existing IDs.

Learn inspection is available through `mercury learn list|status|show|diff|undo`
and the local Learn tab (`prefix+shift+u`). The tab shows redacted audit data;
undo requires a second `u` on the same item and refuses to replace later human
edits. Automatic review remains opt-in through `[learn].enabled = true`.
The Learn tab's `m` page shows recent provider-reported tokens/cost from
opted-in headless cron runs. An explicit `[learn.job_targets]` mapping links a
job UUID to one allowlisted Learn target. When exactly one applied change is
still intact and runs exist on both sides, the tab shows descriptive before/
after averages; this is not proof that Learn caused an improvement. Otherwise
it says data is insufficient.
Each sample uses two lines so its tokens, cost, and outcome remain legible in
an 80-column terminal with the sidebar open.
The sample time is the cron run's persisted completion time, even after a
delayed restart replay. Failed Learn writes are retried every five minutes
while Learn is enabled. Older samples are not retroactively corrected.

To allow Learn to edit routing, configure a `config-section` target whose path
is the active Mercury `config.toml`; only `[route]` is shown to the reviewer or
merged. To allow a Cron edit, configure a `cron-job` target whose ID is the
existing job UUID (`mercury cron list`) and whose path is the active
`jobs.toml`. Its reviewer patch may change only `schedule`, `enabled`, and
`timeout_seconds`; an active run blocks edit and undo. `max_bytes` must cover
the full file and cannot exceed 65536. A sample layout, with placeholders to
replace before enabling, is:

```toml
[learn]
enabled = false

[[learn.target]]
id = "routing"
path = "/absolute/path/to/config.toml"
kind = "config-section"
max_bytes = 65536

[[learn.target]]
id = "123e4567-e89b-12d3-a456-426614174000"
path = "/absolute/path/to/jobs.toml"
kind = "cron-job"
max_bytes = 65536

[learn.job_targets]
"123e4567-e89b-12d3-a456-426614174000" = "routing"
```

The mapping can point to a different configured target; historical samples
without an explicit mapping remain unclassified. There is no automatic
30-day unused-skill archive rule. Weekly cleanup only merges proven duplicates
with backup and undo.

## development

```bash
cargo build
cargo test --workspace -- --test-threads=1  # Rust tests without cargo-nextest
just test        # full recipe; requires cargo-nextest and bun
just check       # formatting, tests, and maintenance checks
```

See [`CLAUDE.md`](./CLAUDE.md) for build/toolchain notes and development conventions.

## Knowledge and GraphRAG

Local Markdown/Obsidian indexing and source-grounded GraphRAG use the authenticated
Codex/Claude CLIs through Mercury routing. The feature is default-off; the real
vault is not modified by enabling search. See [setup and commands](docs/knowledge/README.md)
and [approved specification](specs/008-knowledge-graphrag/spec.md).

External SonarQube/Veracode/Wiz and quantitative coverage gates are waived for
Mercury by explicit user decision on 2026-10-29. Local tests, review, build,
Clippy, formatting and maintenance checks remain required; see
[project policy](.specify/memory/constitution.md).
