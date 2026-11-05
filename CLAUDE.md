# Mercury

Terminal agent controller for Claude Code + Codex CLI, written in Rust, modified freely with no obligation to any upstream project. The plan, decisions and phases live in `PLAN.md` — read it before starting work.

## Build

- Rust toolchain pinned in `rust-toolchain.toml` (1.96.1); Zig 0.16.0 on `PATH` for `crates/ghostty-vt`.
- `cargo build`, `cargo test`, `cargo clippy`.

## Project validation policy

User decision, 2026-10-29: external SonarQube/Veracode/Wiz gates and quantitative
coverage are waived for Mercury, including subsequent work, until revoked.
Record them as waived/not executed, not passing. Local review, relevant tests,
build, Clippy, formatting and maintenance checks remain required. See
`.specify/memory/constitution.md` for the scope, accepted risk and source.

## Model routing for development

Save tokens: use the cheapest model that can do the job; escalate only when needed.

| Model | Agent | Use for |
|---|---|---|
| Sonnet 5 | `mercury-mechanic` | renames/rebrand, moving files, boilerplate, docs/config edits, fixtures, running builds/tests, trivial compile fixes |
| Sonnet 5.5 | `mercury-dev` | default for implementing features with a clear spec: crates, parsers, UI views, API methods, tests |
| Opus 5.5 | `mercury-architect` | architecture, cross-module design, PTY/render/concurrency bugs, hard async/lifetime issues, security-sensitive code, end-of-phase review |

Rules:
- Start at the lowest tier that fits; escalate after a failed attempt or when the task turns out to need design judgement.
- The orchestrating session plans and splits work; delegate execution to the agents above.
- Never pass large file dumps between agents — pass paths and specs.
