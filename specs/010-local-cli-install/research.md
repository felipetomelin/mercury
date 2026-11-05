# Descoberta brownfield

- src/config/io.rs: app_dir_name usa cfg(debug_assertions), mercury-dev em debug
  e mercury em release; config/state/socket têm separação por perfil.
- DEVELOPMENT.md: execução atual via target/debug/mercury; atualização de servidor
  usa server live-handoff explícito. Preservar processos/PTYs é obrigatório.
- README.md e README.pt-BR.md: build release manual; não há feed/bins publicados.
- distribution/install.ps1: instalador Windows pré-existente, fora desta entrega.
- Cargo.toml/build.rs/crates/ghostty-vt: workspace Rust com Ghostty/Zig vendorizado;
  cargo install do registry não é um caminho qualificado para este snapshot.
- ~/.local/bin já pertence ao PATH desta máquina; mercury não existe nele.
- .specify/memory/constitution.md: gates externos/coverage quantitativa dispensados
  pelo usuário para Mercury; revisão e verificações locais permanecem exigidas.

Contrato proposto: install-local.sh [--profile release|dev] [--prefix ABSOLUTE]
[--no-build] [--uninstall]. Prefixo default ~/.local; bin/mercury é um link absoluto
para lib/mercury/mercury. Não há estado novo no servidor ou migração de config.

Riscos: sobreposição com comando existente; troca involuntária de perfil;
symlinks internos; interrupção durante atualização; PATH/shadowing; CARGO_BUILD_TARGET
redirecionando o artefato. Mitigação: conflitos antes do build, dev explícito,
recusar links internos, staging no mesmo filesystem + rename, diagnóstico PATH,
build host com --target-dir conhecido e rejeição de cross-target neste instalador.
Nenhuma dúvida de produto bloqueia o plano.
