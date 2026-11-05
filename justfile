# mercury task runner
set windows-shell := ["cmd.exe", "/d", "/s", "/c"]

python := if os() == "windows" { "python" } else { "python3" }

# Run tests
test:
    cargo nextest run --locked --status-level fail --final-status-level fail --failure-output final --success-output never
    just maintenance-test
    just ui-hot-path-architecture-test
    just integration-assets-test

# Run repository maintenance contract tests
maintenance-test:
    {{python}} -m unittest scripts.test_agent_detection_manifest_check scripts.test_hermes_integration_asset scripts.test_package_windows_conpty scripts.test_vendor_libghostty_vt scripts.test_vendor_portable_pty scripts.test_windows_cross scripts.test_windows_input

# Local interactive Windows Terminal input qualification (never runs in normal CI).
[windows]
test-windows-input *args:
    pwsh -NoProfile -File scripts/test_windows_input.ps1 -AllowInputInjection -ClearClipboard {{args}}

# Run one nextest filter, e.g. `just test-one codex_stale_working`
test-one filter:
    cargo nextest run --locked "{{filter}}" --status-level fail --final-status-level fail --failure-output final --success-output never

# Enforce deterministic UI hot-path architecture boundaries
ui-hot-path-architecture-test:
    {{python}} -m unittest scripts.test_ui_hot_path_architecture

# Run fast local lint checks
[unix]
lint:
    cargo fmt --check
    cargo clippy --all-targets --locked -- -D warnings

[script("powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File")]
[windows]
lint:
    & .\scripts\windows_check.ps1 -Mode lint

# Run PR CI checks
ci filter='all()': lint
    just ci-tests "{{filter}}"

# Keep the test build independently configurable from clippy in CI.
ci-tests filter='all()':
    cargo nextest run --locked -E "{{filter}}" --status-level fail --final-status-level slow --failure-output final --success-output never
    just maintenance-test
    just ui-hot-path-architecture-test
    just integration-assets-test

# Download the Windows SDK once (requires xwin; prompts for Microsoft's SDK license)
[unix]
setup-windows-cross *args:
    {{python}} scripts/windows_cross.py setup {{args}}

# Run Windows target lint from Unix/macOS to catch cfg(windows) compile and clippy failures before CI
[unix]
windows-lint:
    {{python}} scripts/windows_cross.py lint

# Check formatting + run unit tests + Windows target lint
[unix]
check: ci windows-lint

[script("powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File")]
[windows]
check:
    & .\scripts\windows_check.ps1 -Mode check

# Install repo-local git hooks
install-hooks:
    git config core.hooksPath .githooks
    chmod +x .githooks/pre-commit
    chmod +x .githooks/commit-msg
    @echo "installed git hooks from .githooks"

# Build release binary
build:
    cargo build --release --locked

# Non-gating full-render scaling profile for background workspaces and active panes
bench-render-scale:
    cargo test --release --locked --bin mercury render_scale_profile -- --ignored --nocapture --test-threads=1

# Profile terminal target name resolution at increasing pane counts.
bench-terminal-targets:
    cargo test --release --locked --bin mercury terminal_target_lookup_profile -- --ignored --nocapture --test-threads=1

# Profile BSP split collection and construction with balanced and skewed trees.
bench-bsp-layout:
    cargo test --release --locked --bin mercury bsp_layout_profile -- --ignored --nocapture --test-threads=1

# Profile full and retained text, static-image, and unchanged-image updates.
bench-retained-graphics:
    cargo test --release --locked --bin mercury render_scale_profile_retained_graphics -- --ignored --nocapture --test-threads=1

# Profile first-batch latency and aggregate drain cost for external API bursts.
bench-api-fairness:
    cargo test --release --locked --bin mercury external_api_burst_profile -- --ignored --nocapture --test-threads=1

# Test bundled agent integration assets
integration-assets-test:
    bun test src/integration/assets/mercury-agent-state.test.ts
    bun test src/integration/assets/opencode/mercury-agent-state.test.ts
    bun test src/integration/assets/opencode/mercury-tui-session.test.ts

# Regenerate the C API bindings with bindgen-cli 0.72.1
libghostty-bindings *clang_args:
    bash scripts/generate_libghostty_bindings.sh {{clang_args}}

# Build the vendored libghostty-vt source dist
build-libghostty-vt:
    scripts/build_vendored_libghostty_vt.sh

# Print default config
default-config:
    cargo run --release --locked -- --default-config
