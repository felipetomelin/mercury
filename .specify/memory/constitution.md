# Mercury Constitution

This file records existing project decisions from `PLAN.md` and the PagueVeloz
workflow supplied for this repository. It does not supersede user instructions.

## Core principles

### I. Spec before implementation

For a new feature, complete research, specification, clarification, technical
plan, consistency analysis, and executable tasks before production code. Do not
silently decide an unresolved product choice; record assumptions and return to
clarification if they materially change the requested behavior.

### II. Safe local control

Mercury manages local agent sessions and user files. Treat transcripts, model
output, prompts, config, and files as sensitive and untrusted. Never put private
prompt text in agent launch arguments or diagnostic logs. Do not grant folder
trust, run arbitrary shell commands from model output, commit, or push on behalf
of an automatic feature unless explicitly authorized by its contract.

### III. Reversible automatic changes

Auto improvement applies changes automatically only when enabled. Every write
must be constrained by an explicit allowlist, private backup, durable audit
record, size/frequency limits, and conflict-aware undo. Failure must preserve
data or expose a recoverable state. `learn.enabled=false` is a kill-switch.

### IV. Server-owned state, bounded workers

Persistent state and sensitive I/O belong to the Mercury server. Clients use
typed APIs. Long-running scans, subprocesses, and database operations must not
block PTY rendering or the TUI event loop; concurrency, time, and output are
bounded. Prefer existing project crates and contracts over duplicated logic.

### V. Evidence-based validation

Test the smallest relevant scope first, then the affected integration and schema
surfaces. Batch related implementation, review, and test work. Manual TUI and
real-provider smokes remain distinct from simulated tests. Do not describe a
phase as complete from local unit tests alone.

## Quality and security gate

Project-specific exception approved explicitly by the user on 2026-10-29 in
this conversation: external gates are waived for Mercury. This covers SonarQube
Quality Gate/hotspots, Veracode, Wiz and the quantitative coverage gate for
current and subsequent Mercury work, until the user changes this instruction.
It does not apply to other projects or change global PagueVeloz rules.

Accepted risk: external scanning and quantitative coverage may not reveal
vulnerabilities, dependency issues or insufficient coverage. These checks remain
not executed/waived, never reported as PASS. Reference: explicit user decision
in this conversation; no external ticket was provided.

Local code/security review, relevant tests, build, Clippy, formatting and
repository maintenance checks remain required. Known findings still require
assessment; the exception is not permission to ignore errors or fabricate
validation. Committing, merging, releasing or activating personal configuration
still requires its own task authorization.

## Scope and governance

`PLAN.md` is the feature-scope source for phases 0–7; `AGENTS.md` and global
PagueVeloz skills govern execution. Existing dirty-worktree changes are user
work and must be preserved. Amend this constitution only to reflect a confirmed
project decision, with its source and date.

## Continuous-learning default decision

User decision on 2026-11-05 (this conversation, spec 011): Learn and Knowledge
are enabled by default. With no explicit targets/vault, writes are confined to
Mercury-managed memory/notes. Human instruction files, routing and crons still
require explicit exact targets. External processing remains off in new defaults;
the user authorized explicit Codex/Claude processing for this personal activation.
Generated notes retain proposed status and cannot impersonate human validation.
The user's false settings remain kill-switches. This supersedes default-off
statements in historical feature reports, without relaxing the writer policy.

**Version**: 1.2.0 | **Recorded**: 2026-10-06 | **Amended**: 2026-11-05
