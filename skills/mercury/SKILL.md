---
name: mercury
description: "Control Mercury, a terminal multiplexer for coding agents. Use only when the user explicitly mentions Mercury or asks to use Mercury to inspect or control panes, tabs, workspaces, scheduled jobs, commands, or another agent. Do not use merely because a task could benefit from a background terminal, delegation, or parallel work. Requires MERCURY_ENV=1."
---

# Mercury

Mercury organizes terminals into workspaces, tabs, and panes, recognizes coding agents running inside panes, and exposes the current session through the `mercury` CLI.

Before issuing any control command, verify that this agent is running inside a Mercury-managed pane:

```bash
test "${MERCURY_ENV:-}" = 1
```

If the check fails, say that you are not running inside Mercury and stop. Do not inspect or control the focused Mercury session from outside Mercury.

When the check passes, the `mercury` binary in `PATH` talks to the current session. Use it to inspect neighboring work, create terminal layout, start agents and commands, read output, and wait for state changes.

## Learn the current CLI

The installed binary is the authority for command syntax. Start with:

```bash
mercury --help
```

Then print the relevant command group by running the group without a subcommand:

```bash
mercury agent
mercury pane
mercury workspace
mercury tab
mercury worktree
mercury terminal
mercury notification
mercury integration
mercury session
mercury machine
mercury cron help
```

Do not run bare `mercury` for discovery; it launches or attaches the TUI. Do not probe a mutating nested command by omitting arguments. Commands such as `mercury workspace create` are valid with defaults and will execute.

## Schedule a task when explicitly requested

Translate a user's natural-language scheduling request into a declarative cron job only after establishing its schedule, target (`claude`, `codex`, an existing pane, or a script), working directory, and expected result. Ask for missing choices that materially affect what will execute or where results will be delivered; do not infer a recurring command or an external recipient from context alone. Use `mercury cron help` for the installed syntax and `mercury cron list` to inspect existing jobs before adding or editing one.

For a simple job, use `mercury cron add --name NAME --schedule EXPR` with exactly one target: `--provider claude|codex --prompt-file PATH`, `--pane PANE --prompt-file PATH`, or `--script PROGRAM` with repeated `--arg ARG`. Prompts must be supplied via a file, never as a command-line argument. For advanced settings such as model, effort, repeat, catch-up, or multiple delivery destinations, prepare a complete job TOML and use `mercury cron add --file JOB.toml`; editing an existing job likewise requires `mercury cron edit JOB_ID --file JOB.toml` with the complete definition. Do not put webhook URLs, SMTP credentials, or other secrets in the TOML; configure only environment-variable names for secret fields. Do not print prompt content or secrets into logs while preparing the job.

After creation, inspect the returned job ID and verify it with `mercury cron list` and `mercury cron doctor`. `doctor` validates configuration without executing the job; `mercury cron run JOB_ID` executes immediately and needs separate user intent. To inspect outcomes, use `mercury cron runs JOB_ID` then `mercury cron log RUN_ID`. A job may have external effects, so never create, run, resume, edit, or remove one merely to explore the CLI.

Most control commands return JSON. Read identifiers and state from those responses instead of predicting them.

## Understand layout, panes, and agents

Choose the primitive that matches the job:

- Workspace, tab, and pane topology organize terminal locations.
- Pane commands control raw terminals, shells, tests, servers, input, and output.
- Agent commands control the recognized coding agent currently occupying a pane.

A pane exists whether or not it contains an agent. `agent start` requires an existing available shell pane and never creates, splits, or moves layout. Use pane commands for ordinary processes. Use agent commands when Mercury must validate agent identity or interpret `idle`, `working`, `blocked`, `done`, and `unknown` lifecycle states.

Agent commands accept either a unique live agent name or the pane ID currently hosting that agent. They do not accept terminal IDs or bare agent-kind labels. Names must match `[a-z][a-z0-9_-]{0,31}` and be unique among live agents. A name follows the current pane occupant and is cleared when that agent exits, is released, or is replaced.

`idle` and `done` both mean the agent is ready for input. The CLI/API uses the server's seen state to distinguish them; explicit focus commands mark the target seen, while reads do not. Each TUI client tracks viewed completions independently, so its Done badge can differ from the CLI or another client's badge. `blocked` means Mercury recognized an approval or question UI. `unknown` means an agent is present but Mercury cannot classify it confidently; it does not prove completion.

## Use IDs and caller context

Public IDs are opaque stable handles:

- workspace: `w1`
- tab: `w1:t1`
- pane: `w1:p1`

Closed tab and pane IDs are not reused. A pane moved into another workspace receives a new workspace-qualified pane ID. After `pane move`, continue with `.result.move_result.pane.pane_id` or the live agent name. The old value is reported as `.result.move_result.previous_pane_id`; only the moved process's inherited caller context keeps resolving that old ID, so do not use it as a general agent target.

Mercury injects the caller's context into each managed pane:

```bash
printf '%s\n' "$MERCURY_WORKSPACE_ID" "$MERCURY_TAB_ID" "$MERCURY_PANE_ID"
```

Prefer `--current` when a pane command should target the calling pane. An omitted `pane split` target uses the calling pane when `MERCURY_PANE_ID` is available, otherwise the focused pane. Other commands may use the UI-focused pane, which can belong to the user or another client.

Discover live state with:

```bash
mercury workspace list
mercury tab list --workspace "$MERCURY_WORKSPACE_ID"
mercury pane current --current
mercury pane list --workspace "$MERCURY_WORKSPACE_ID"
mercury agent list
```

Creation responses expose the IDs to use next. `workspace create` returns `.result.workspace`, `.result.tab`, and `.result.root_pane`. `tab create` returns `.result.tab` and `.result.root_pane`. `pane split` returns the new pane as `.result.pane`.

IDs and live agent names are scoped to one server. Two saved SSH machines can both have `w1:p1` or an agent named `reviewer`. Selecting a machine in the TUI does not retarget commands running in your pane: without `--machine`, they still use the inherited session and socket context.

To control a saved SSH machine, use the same global prefix for discovery and every later command:

```bash
mercury --machine <label-or-id> agent list
mercury --machine <label-or-id> pane list
mercury --machine <label-or-id> agent prompt <remote-agent-name> "Reply with your current status." --wait --timeout 120000
```

The selector must be an enabled saved profile ID or a unique, case-sensitive label, not an arbitrary SSH hostname. Commands use that profile's remote session without an open TUI. Do not combine `--machine` with `--session` or `--remote`. Discover IDs on that machine; inherited local IDs and `--current` do not identify remote panes.

Both installations must support machine API forwarding, and the remote server must already be running and API-compatible. Forwarding never installs, starts, or restarts a server and never falls back to Local. Local configuration, session management, installation commands, and interactive attachment are not forwarded. Remote worktree paths must be absolute, `~`, or start with `~/`; plugin link paths must be absolute. A connection failure does not prove a mutation was not applied: inspect remote state before retrying.

`mercury machine list` lists saved connection profiles, not a cross-machine pane inventory; add `--json` for scripts. Only add, remove, enable, or disable profiles when the user asks. Removing a profile disconnects the client but does not stop remote sessions. Interactive `machine add` discovers running remote sessions and may ask the user to pick one; non-interactive setup uses the remote default session unless `--remote-session` is explicitly supplied. Use `mercury machine status [<label-or-id>] --json` to diagnose a failing machine. `mercury machine reconnect` needs the user to complete SSH authentication in their terminal; do not run it for them. Setup asks before stopping an incompatible server and defaults to No; do not approve replacement without the user's consent. Experimental handoff is not part of `machine add`.

## Start and coordinate an agent

Default to a sibling pane in the current tab and the current working directory. Do not create a workspace, tab, worktree, or different cwd unless the user explicitly requests that topology or location.

Honor a direction requested by the user. Otherwise inspect the caller pane:

```bash
mercury pane layout --pane "$MERCURY_PANE_ID"
```

Split a wide pane to the right and a narrow or tall pane down. Avoid repeated same-direction splits that create unusably narrow columns or short rows. Keep the user's focus in the calling pane and explicitly preserve the caller's working directory:

```bash
mercury pane split --current --direction right --cwd "$PWD" --no-focus
```

Replace `right` with `down` when appropriate. Read the new pane ID from `.result.pane.pane_id`.

An available shell pane must be at its interactive prompt, with the shell itself in the foreground and no foreground command, editor, or agent running. Start a supported agent in that pane with a useful unique name:

```bash
mercury agent start reviewer --kind codex --pane <returned-pane-id>
```

Use the kind requested by the user. Run `mercury agent` to inspect the installed kind list and options. Pass native agent arguments only after `--`:

```bash
mercury agent start reviewer --kind codex --pane <returned-pane-id> -- <agent-args...>
```

A successful `agent start` returns only after Mercury detects the expected agent in the same pane and considers it ready for interactive input. If the agent is blocked during startup, the command returns `agent_not_ready` immediately but keeps the name available for `agent read` and `agent send-keys`. Wait until the agent becomes idle before prompting it. Startup defaults to a 30-second timeout.

Submit work through the agent surface:

```bash
mercury agent prompt reviewer "Review the current diff and report only actionable findings." --wait --timeout 120000
```

`agent prompt` honors the pane's live bracketed-paste mode and sends text followed by encoded Enter as one ordered submission. It reports successful submission only after both have been written; that alone does not prove the agent started a turn. For Codex on Windows, Mercury sends a paste boundary before Enter so submission does not depend on prompt size. It rejects an agent already waiting at an approval or question dialog with `agent_blocked` before sending any input. Inspect the blocked UI and ask the user before answering it. For normal agent work, `--wait` is enough: it waits for the first settled `idle`, `done`, or `blocked` state. Do not repeat those defaults with `--until`.

With `--wait`, a prompt sent from a non-working state must produce observed `working` or `blocked` activity. After submission, Mercury waits up to five seconds for that activity; unrelated `idle`, `done`, or session changes do not satisfy this gate. It returns `agent_prompt_stalled` if no activity is observed, or `timeout` if the caller's timeout expires first. The caller timeout includes submission time. Without a timeout, the settled-state wait is indefinite after activity is observed. This wait tracks lifecycle state, not an individual turn; if the agent is already working, completion of the active turn may satisfy it.

Use `--until` only for a state-specific workflow, such as waiting for an already-running agent to request input:

```bash
mercury agent wait reviewer --until blocked --timeout 120000
```

Without `--until`, standalone `agent wait` uses the same settled-state defaults as `agent prompt --wait`.

Use logical keys for interactive agent UI controls:

```bash
mercury agent send-keys reviewer esc
mercury agent send-keys reviewer ctrl+c
```

Mercury validates all keys before writing any bytes. Read the result through the resolved agent:

```bash
mercury agent get reviewer
mercury agent read reviewer --source recent-unwrapped --lines 120
```

If a wait fails or returns `blocked`, inspect `agent get` and `agent read` before deciding what input to send. A timeout or stalled response does not prove the prompt was never delivered; do not blindly submit it again. Use the pane surface only when raw terminal control is intentional.

## Run an ordinary command in another pane

Create a sibling pane with the same geometry rule, preserve the caller's working directory, and keep user focus unchanged:

```bash
mercury pane split --current --direction right --cwd "$PWD" --no-focus
```

Read the new pane ID from `.result.pane.pane_id`, then run and inspect the command:

```bash
mercury pane run <returned-pane-id> "just test"
mercury pane wait-output <returned-pane-id> --match "test result" --timeout 120000
mercury pane read <returned-pane-id> --source recent-unwrapped --lines 120
```

`pane run` atomically sends command text and Enter. `pane wait-output` searches the selected snapshot immediately, so output that already exists can match. Use `--match <text>` for a literal substring or `--regex <pattern>` for a Rust regular expression. Omitting `--timeout` allows an indefinite wait.

Use the read source that matches the task:

- `visible`: the currently rendered viewport.
- `recent`: recent rendered output, including soft wraps.
- `recent-unwrapped`: recent output with soft wraps joined; prefer it for logs and transcripts.
- `detection`: the plain-text bottom-buffer snapshot used for agent detection.

Use `--format ansi` when colors and terminal styling are evidence. Otherwise use text.

`--lines` asks Mercury for more rows from the pane's available screen and host scrollback. Alternate-screen rows do not enter ordinary host scrollback. For supported idle agents, Mercury can collect application-owned history and restore the viewport afterward, but not every application or response can be recovered this way.

If a larger recent read still does not reveal the completed response, ask the agent to write it as Markdown in a temporary directory and reply only with the file path, then read that file on the same machine. Use this only as a fallback; do not request file output in the initial prompt.

## Safety and coordination rules

- Use `--no-focus` for background work unless the user asked to switch context.
- Use `--current`, an explicit pane ID, or a unique agent name. Do not rely on another client's focused pane.
- Parse IDs from JSON responses. Do not derive them from sidebar order or examples.
- Do not close workspaces, tabs, panes, or sessions you did not create unless the user explicitly asked. `workspace close --group` closes the primary workspace and its linked worktree workspaces; never add it merely to bypass `workspace_group_close_required`.
- Use `--trust-repository` only after the user has verified the repository. It grants per-request Git trust; it is not a routine retry for a failed worktree command.
- Client and server versions can differ after an update. Check `mercury status` before relying on new server features. A missing method is not permission to stop or upgrade a server.
- Never run `mercury server stop` from an active session unless the user explicitly intends to stop the server and its pane processes.
- Never kill the main Mercury process. Use named test sessions for experiments that need an isolated server.
- CLI server errors are JSON on stderr with exit status 1. CLI syntax errors exit with status 2.
