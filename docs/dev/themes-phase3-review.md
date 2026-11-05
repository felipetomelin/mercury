# Fase 3 — revisão e verificação de temas

Estado em 2026-10-06: **implementação local verificada; gate externo
FAIL/não verificável**. Sem commit.

## Evidência local

- `cargo test --offline --bin mercury theme -- --test-threads=1`: 47/47 PASS,
  incluindo catálogo Mercury, import Base16/Ghostty, overrides, picker e reload.
- `python3 scripts/test_ui_hot_path_architecture.py`: 6/6 PASS.
- Clippy `--all-targets -D warnings` e build passaram na rodada de código
  anterior; esta rodada alterou apenas documentação e fixtures temporários.
- Smoke em PTY isolado com `[theme] name="mercury"`: o picker mostrou
  `mercury` e `mercury-light`. Mover a seleção aplicou preview imediato da
  paleta clara; Esc restaurou as cores Mercury e manteve o config. Confirmar
  gravou `mercury-light`; uma segunda confirmação restaurou `mercury`.
- Import Ghostty local `tui-ocean` apareceu no mesmo picker. O preview usou
  fundo `#112233` do arquivo importado; Esc restaurou Mercury. Confirmar
  persistiu `tui-ocean` no config temporário e o picker permitiu voltar a
  `mercury`. O servidor foi parado; nenhum config real do usuário foi tocado.

## Limites e gate

O smoke visual não cobriu todos os formatos inválidos nem auto-switch em
terminal externo; estes têm testes locais, não inspeção manual equivalente.
SonarQube, Veracode, Wiz e cobertura de projeto não estão disponíveis neste
ambiente; o gate PagueVeloz permanece **FAIL/não verificável**. A pasta
temporária `/private/tmp/mercury-tui-smoke-j7i01L` permanece porque a política
do ambiente impede sua exclusão, apesar de o servidor estar parado.
