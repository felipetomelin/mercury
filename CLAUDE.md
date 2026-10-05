# Mercury

Terminal agent controller for Claude Code + Codex CLI. Codebase starts from Herdr v0.9.3 (Rust) and is modified freely. The plan, decisions and phases live in `PLAN.md` — read it before starting work. Herdr's original contributor notes are in `docs/herdr-base/` for reference only; their release/contribution rules do not apply here.

## Build

- Rust toolchain pinned in `rust-toolchain.toml` (1.96.1); Zig 0.16.0 on `PATH` for `crates/ghostty-vt`.
- `cargo build`, `cargo test`, `cargo clippy`.

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
