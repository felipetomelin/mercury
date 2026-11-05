# 010 — Local command installation: review and evidence

Date: 2026-11-05. Scope authorized by the user: plan, implement, test and publish
the simplification from a checkout-specific binary path to `mercury`.

## Consolidated review

Local verdict: **PASS**, no blocking findings. Reviewed installer lifecycle,
build/profile selection, file ownership, failure cleanup, tests and user docs.
No Rust production code, API/schema, database or model-provider behavior changed.

The installer uses a user prefix, refuses foreign commands and redirected
internal directories/binaries, validates a staged executable and renames it on
the same filesystem. It never truncates a running executable. Arguments are
quoted; there is no eval, sudo, remote shell installer or forced overwrite.
The private install directory only contains the copied executable and temporary
staging during installation. Uninstall preserves configuration, state and notes.

The build explicitly selects the rustc host target, uses Cargo.lock, and runs
inside the checkout so rustup uses the pinned toolchain. `--no-build` supports
the conventional artifact and a host-specific fallback. Profile dev is explicit
for the existing personal setup; release remains the public default.

## Validation

| Check | Result |
| --- | --- |
| Installer subprocess tests | 29 passed, 0 failed |
| Installer plus selected vendor/packaging/hot-path maintenance checks | 66 cases validated; one metadata test rerun successfully after restoring Rust to the test environment |
| Real dev build + temporary installation | PASS, locked/offline macOS host build |
| Real default release build + temporary installation | PASS, locked/offline macOS host build |
| Both real installed binaries: version/help/config check | PASS, run from `/private/tmp` with isolated XDG directories; expected profile namespace confirmed |
| Workspace Clippy, all targets, warnings denied | PASS |
| Rust formatting, Bash syntax, Python compilation, diff check | PASS |
| Changed Markdown local links | PASS |
| Real temporary install removal | PASS, both managed temporary installations removed |

Subprocess tests cover conflicts, symlink redirection, paths with spaces, failed
builds/version checks/renames, atomic replacement, a process held across update,
missing PATH/shadowing, copying independently of the checkout, dev/release
selection, cached host artifacts, and idempotent removal without deleting data.

The first real build smoke was interrupted by editing the running Bash script;
repeated from an immutable script snapshot with the final source, successfully.
Initial fixture failures from macOS `/var` versus `/private/var` aliases were
fixed by canonicalizing the temporary test root. No unresolved failures remain.

Logs are local artifacts under `/private/tmp/mercury-install-*.log`, not required
to run Mercury. Full Rust tests were not repeated: this group changes only shell
installation and documentation; existing production Rust contracts are intact.

## Personal activation

Installed with `--profile dev --no-build` at the user's default prefix. A login
shell resolves `mercury`; `mercury --version` reports 0.9.3 from `/private/tmp`.
The installed copy matches the pre-existing debug executable byte for byte.
Before/after checks confirm the config path/hash and pane/workspace inventories
are unchanged. The same server socket remains active; both observations report
`restart_needed=false` and `server_binary_stale=false`. No handoff or restart
was performed. Raw personal inventories remain private temporary files.

## Gate status and limits

Local review/checks: PASS. SonarQube, Veracode, Wiz and quantitative coverage are
**waived / not executed**, under the user's recorded Mercury exception in
`.specify/memory/constitution.md`; no external scan is represented as passing.

macOS is validated; Linux behavior has isolated shell tests but no native Linux
qualification here. Windows remains separate/experimental. No prebuilt release,
package-manager formula or automatic update feed was published. PATH requires
the explicit setup printed by the installer if the directory is absent; shell
files and aliases are never changed. Concurrent hostile mutations by another
process of the same user are outside the installer's ownership checks.
