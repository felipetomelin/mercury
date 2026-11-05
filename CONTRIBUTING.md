# Contributing to Mercury

Bug reports, documentation improvements and focused pull requests are welcome.
For a substantial feature, open an issue to align the scope before implementation.

## Development

Install Rust through rustup and Zig 0.16.0, then run:

```sh
cargo build --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
```

Use temporary fixtures for sessions, account responses, vaults and scheduled jobs.
Tests must not require personal credentials, call a paid model or execute a real
user's scheduled task. Add focused regression coverage for behavior changes.

Describe the problem, resulting behavior and validation in a pull request.
For visual changes, include screenshots with fictional data; see
[README asset generation](assets/mercury/README.md). Keep changes scoped and use
Conventional Commit messages where practical.

Preserve upstream attribution and vendored notices. See [LICENSE](LICENSE) and
[NOTICE](NOTICE). The detailed project workflow is in [CLAUDE.md](CLAUDE.md).

## Reports

Include the operating system, terminal, revision, reproduction steps and expected/
actual behavior. Remove tokens, account identifiers, private paths and project
content from logs/screenshots. Report vulnerabilities through the private contact
in [SECURITY.md](SECURITY.md).
