#!/usr/bin/env bash
# Install a host build without changing Mercury data or shell configuration.
# Compatible with Bash 3.2 (macOS) and newer.
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: bash scripts/install-local.sh [options]

Install Mercury from this checkout (macOS/Linux; no sudo needed).

  --profile release|dev  Build profile; release is the default.
                         dev preserves the mercury-dev configuration namespace.
  --prefix ABSOLUTE     Install under PREFIX/bin and PREFIX/lib/mercury.
                         Default: $HOME/.local
  --no-build            Install the existing target/release or target/debug binary.
  --uninstall           Remove only this installer's managed command and binary.
  --help, -h            Show this help.

Requires Rust/rustup, Zig 0.16 and native build tools unless --no-build is used.
No server is started/stopped; config, state, notes and shell files are untouched.
EOF
}

die() { printf 'error: %s\n' "$*" >&2; exit 1; }
exists() { [[ -e "$1" || -L "$1" ]]; }
check_directory() {
    [[ ! -L "$1" ]] || die "Refusing symlink directory: $1"
    if exists "$1" && [[ ! -d "$1" ]]; then
        die "Expected a directory: $1"
    fi
}

profile=release
prefix="${HOME:?HOME must be set}/.local"
build=1
uninstall=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --profile|--prefix)
            [[ $# -ge 2 && -n "$2" ]] || die "$1 requires a value"
            if [[ "$1" == --profile ]]; then profile="$2"; else prefix="$2"; fi
            shift 2 ;;
        --no-build) build=0; shift ;;
        --uninstall) uninstall=1; shift ;;
        --help|-h) usage; exit 0 ;;
        *) die "Unknown option: $1 (see --help)" ;;
    esac
done
case "$profile" in release|dev) ;; *) die "Profile must be release or dev" ;; esac
case "$(uname -s)" in Darwin|Linux) ;; *) die "This installer supports macOS/Linux only" ;; esac
[[ "$prefix" == /* && "$prefix" != *:* && "$prefix" != *$'\n'* && "$prefix" != *$'\r'* ]] ||
    die "Prefix must be an absolute path without colons or newlines"

if [[ "$uninstall" == 1 ]] && ! exists "$prefix"; then
    printf 'No managed Mercury installation at %s\n' "$prefix"
    exit 0
fi
mkdir -p "$prefix"
prefix="$(cd "$prefix" && pwd -P)"
[[ "$prefix" != / ]] || die "Refusing to install directly under /"
bin_dir="$prefix/bin"
lib_parent="$prefix/lib"
install_dir="$lib_parent/mercury"
binary="$install_dir/mercury"
command_path="$bin_dir/mercury"
for directory in "$bin_dir" "$lib_parent" "$install_dir"; do
    check_directory "$directory"
done

managed_link=0
if exists "$command_path"; then
    [[ -L "$command_path" && "$(readlink "$command_path")" == "$binary" ]] ||
        die "Refusing to replace an existing command: $command_path"
    managed_link=1
fi
if exists "$binary"; then
    [[ "$managed_link" == 1 && -f "$binary" && ! -L "$binary" ]] ||
        die "Refusing an unmanaged or redirected binary: $binary"
fi

if [[ "$uninstall" == 1 ]]; then
    if [[ "$managed_link" == 0 ]]; then
        printf 'No managed Mercury installation at %s\n' "$prefix"
        exit 0
    fi
    rm "$command_path"
    if exists "$binary"; then rm "$binary"; fi
    rmdir "$install_dir" 2>/dev/null || true
    printf 'Removed managed Mercury command. Configuration, state and notes were preserved.\n'
    exit 0
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
artifact_dir=release
namespace=mercury
if [[ "$profile" == dev ]]; then artifact_dir=debug; namespace=mercury-dev; fi
source_binary="$repo_root/target/$artifact_dir/mercury"
[[ -z "${CARGO_BUILD_TARGET:-}" ]] ||
    die "Unset CARGO_BUILD_TARGET; this installer only installs host builds"
if [[ "$build" == 1 ]] || ! exists "$source_binary"; then
    command -v rustc >/dev/null 2>&1 || die "Rust not found; install through rustup or build target/$artifact_dir/mercury first"
    host="$(cd "$repo_root" && rustc --print host-tuple)" || die "Could not determine the Rust host target"
    [[ -n "$host" && "$host" != *[!a-zA-Z0-9_.-]* ]] || die "Invalid Rust host target"
    source_binary="$repo_root/target/$host/$artifact_dir/mercury"
fi
if [[ "$build" == 1 ]]; then
    command -v cargo >/dev/null 2>&1 || die "Cargo not found; install Rust through rustup and Zig 0.16 first"
    cargo_args=(build --locked --bin mercury --manifest-path "$repo_root/Cargo.toml" --target-dir "$repo_root/target" --target "$host")
    if [[ "$profile" == release ]]; then cargo_args+=(--release); fi
    # Override any configured cross-compilation target: the command must run here.
    (cd "$repo_root"; unset CARGO_BUILD_TARGET; cargo "${cargo_args[@]}")
fi
[[ -f "$source_binary" && -x "$source_binary" && ! -L "$source_binary" ]] ||
    die "No executable at $source_binary; build the selected profile first"

mkdir -p "$bin_dir" "$install_dir"
staging="$(mktemp -d "$install_dir/.install.XXXXXXXX")"
new_link=0
cleanup() {
    if [[ "$new_link" == 1 && -L "$command_path" ]] &&
        [[ "$(readlink "$command_path")" == "$binary" ]]; then
        rm "$command_path"
    fi
    rm -rf "$staging"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
cp "$source_binary" "$staging/mercury"
chmod 755 "$staging/mercury"
version="$("$staging/mercury" --version)" || die "The staged binary could not run on this machine"
[[ "$version" == 'mercury '* ]] || die "The staged executable did not identify itself as Mercury"
if [[ "$managed_link" == 0 ]]; then
    # ln refuses a destination created since the conflict check; never overwrite it.
    ln -s "$binary" "$command_path"
    new_link=1
fi
# Rename on the same filesystem keeps an existing running executable intact.
mv -f "$staging/mercury" "$binary"
new_link=0
printf 'Installed %s (%s profile; %s data namespace)\n' "$version" "$profile" "$namespace"
printf 'Command: %s\n' "$command_path"
printf 'Config and data were not migrated. Start with: mercury\n'

resolved="$(command -v mercury || true)"
if [[ "$resolved" != "$command_path" ]]; then
    if [[ -n "$resolved" ]]; then
        printf 'Another mercury currently has precedence: %s\n' "$resolved"
    else
        printf 'The install directory is not currently resolving mercury.\n'
    fi
    printf 'Run this in your shell, then verify with command -v mercury:\n  export PATH=%q:"$PATH"\n' "$bin_dir"
    printf 'Add that line to your shell startup file if needed for future terminals.\n'
fi
