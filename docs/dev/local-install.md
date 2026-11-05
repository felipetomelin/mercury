# Install the `mercury` command

The source installer supports macOS and Linux with Bash 3.2+. macOS is the
locally validated platform; Linux still needs platform qualification.
Windows has a separate experimental distribution path.

## Install once, run from anywhere

Install Rust through rustup (the repository pins the toolchain), Zig 0.16 and
your platform's native build tools. Install and authenticate Codex CLI and/or
Claude Code separately. Put Zig on PATH or point `ZIG` at its executable.

```sh
git clone https://github.com/felipetomelin/mercury.git
cd mercury
bash scripts/install-local.sh
mercury --version
mercury config check
mercury
```

The installer compiles a locked **release** host build. The first build can take
several minutes. It does not download a prebuilt Mercury release or install
dependencies for you. Cargo/rustup can fetch the pinned toolchain and crates.
There is no published package-manager installer or automatic update feed yet.

The default command is `~/.local/bin/mercury`, linked to an independent copy at
`~/.local/lib/mercury/mercury`. Moving the checkout or running `cargo clean`
does not remove the installed executable. No `sudo` is needed.

If your shell does not find the command, use the exact `export PATH=...` line
printed by the installer. For the default prefix:

```sh
export PATH="$HOME/.local/bin:$PATH"
command -v mercury
```

To keep this for future terminals, add that line once to your shell startup
file, such as `~/.zshrc` for interactive zsh or `~/.bashrc` for interactive Bash
(login Bash may instead read `~/.bash_profile`). The installer never edits shell
files. An existing alias/function can also hide a command; inspect `type -a
mercury`. After changing an installation, `hash -r` (Bash) or `rehash` (zsh)
refreshes a cached command path if needed.

## Preserve an existing development setup

Debug and release builds intentionally use separate config/state namespaces:

| Installer profile | Data namespace | Default Unix config |
| --- | --- | --- |
| `release` (default) | `mercury` | `~/.config/mercury/config.toml` |
| `dev` | `mercury-dev` | `~/.config/mercury-dev/config.toml` |

XDG directory variables, session selection and `MERCURY_CONFIG_PATH` continue
to follow the application's existing rules. Installation does not copy, merge,
delete or migrate config, notes, history, crons or state.

If you previously ran `target/debug/mercury`, install **dev** to keep that setup:

```sh
bash scripts/install-local.sh --profile dev
mercury config check
mercury
```

Already have a local build? Skip compilation:

```sh
bash scripts/install-local.sh --profile dev --no-build
```

`--no-build` first uses `target/debug/mercury` (dev) or
`target/release/mercury` (release). If that conventional artifact is absent, it
uses `target/<host>/<profile>/mercury`, the directory used by this installer;
finding that fallback requires `rustc`. The selected binary must execute on the
current machine. Cross-compilation via `CARGO_BUILD_TARGET` is refused; normal
builds explicitly select `rustc --print host-tuple`, including when Cargo config
sets a different default target.

## Update

From your checkout, pull the latest code and rerun the installer, keeping your
chosen profile:

```sh
git pull --ff-only
bash scripts/install-local.sh --profile dev
mercury --version
mercury status
```

For a release installation, omit `--profile dev`. Reinstallation atomically
replaces the managed copy; it does not stop a running Mercury server or agent.
An existing server can still be running the previous binary. If `mercury status`
reports a stale server, save work and detach the client first, then explicitly
use the existing handoff procedure:

```sh
mercury server live-handoff --import-exe "$HOME/.local/lib/mercury/mercury"
mercury status
```

Use the corresponding `lib/mercury/mercury` path for a custom prefix. Handoff is
an explicit operation; the installer never initiates it. If handoff fails, inspect
status/logs rather than killing server processes blindly. `mercury update` still
has no published Mercury feed and is not the update path for this source install.

## Custom prefix and conflicts

```sh
bash scripts/install-local.sh --prefix "/absolute/path/with spaces"
```

The prefix must be absolute, without colons/newlines, and cannot be `/`.
`bin` and `lib/mercury` must not be redirected through symlinks. An existing
`bin/mercury` file, directory or foreign symlink is preserved and causes an
error. Choose another prefix or resolve that installation yourself; the
installer has no force-overwrite mode.

## Uninstall

From a checkout:

```sh
bash scripts/install-local.sh --uninstall
# For a custom prefix, use the same prefix again:
bash scripts/install-local.sh --prefix "/absolute/custom/prefix" --uninstall
```

Only the installer's command link and managed executable are removed. Running
processes continue; config, state, notes, other files and shell configuration
are preserved. Repeating the command is safe. A foreign installation is refused.

## Em português

Depois da instalação, abra qualquer terminal e execute `mercury`.
Se já usava `target/debug/mercury`, instale com `--profile dev` para continuar
com suas configurações, memória e agendas atuais. Para atualizar, faça
`git pull --ff-only` e repita a instalação com o mesmo perfil. Para remover
somente o comando, use `--uninstall`; suas notas e configurações permanecem.

O diretório `~/.local/bin` precisa estar no PATH. Se não estiver, siga a linha
`export PATH=...` mostrada pelo instalador e adicione-a ao arquivo de inicialização
do seu shell para os próximos terminais. O instalador não altera esse arquivo.
