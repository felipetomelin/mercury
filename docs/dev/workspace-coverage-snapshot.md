# Cobertura Rust local — 2026-10-07

Estado: **diagnóstico, não quality gate PASS**. A suíte do workspace foi executada
offline com `RUSTFLAGS=-Cinstrument-coverage`, `LLVM_PROFILE_FILE` e
`XDG_DATA_HOME` temporários. Os perfis foram agregados com Homebrew LLVM
22.1.8 (`llvm-profdata`/`llvm-cov`); o compilador Rust 1.96.1 usa LLVM 22.1.2.
O diretório temporário de evidência foi
`/private/tmp/mercury-coverage-89OEvt/`.

`cargo test --workspace --offline --quiet -- --test-threads=1` terminou com
código 0 ao filtrar os dois testes de TCP loopback já incompatíveis com este
ambiente. O binário principal teve 3664 testes aprovados e 11 ignorados;
`client_mode` teve 30/30. A medição pontual anterior de `mercury-learn` passou
sem avisos do `llvm-cov`: 2340/2527 linhas (92,60%) e 3345/3754 regiões
(89,10%) dos oito arquivos `src/` da crate, sem contar arquivos `tests/`.

No workspace, três de 503 perfis `.profraw` gerados por subprocessos estavam
corrompidos e foram preservados em `invalid-profiles/`, não tratados como
sucesso. O merge dos 456 perfis válidos da rodada completa passou. O relatório
agregado considerou apenas arquivos Rust em `src/` da raiz e de `crates/*/src/`:

| Módulo | Linhas cobertas / total | Cobertura |
|---|---:|---:|
| Mercury principal | 183713 / 209214 | 87,81% |
| ghostty-vt | 4067 / 4585 | 88,70% |
| mercury-cron | 2046 / 2222 | 92,08% |
| mercury-history | 417 / 468 | 89,10% |
| mercury-learn | 2350 / 2527 | 93,00% |
| mercury-router | 321 / 333 | 96,40% |
| mercury-usage | 2189 / 2284 | 95,84% |
| **Total diagnóstico** | **195103 / 221633** | **88,03%** |

Regiões agregadas: 287185/326608 (87,93%). Esses números incluem testes
unitários que vivem dentro de arquivos `src/`, mas excluem os arquivos
`tests/`, dependências do Cargo, vendor e os dois cenários TCP filtrados. Não
há cobertura de branches reportada. Ao combinar os binários de teste, o
`llvm-cov` avisou sobre **74 funções com dados incompatíveis**; o percentual
agregado é apenas um sinal de diagnóstico até a metodologia ser validada em
pipeline, e não prova do limite de cobertura exigido pelo projeto. O relatório
isolado da crate Learn não apresentou esse aviso. Para controle, a leitura
apenas do binário de testes unitários principal também não emitiu aviso:
176998/209202 linhas (84,61%) em `src/`; ela não representa a união dos
testes de integração, portanto não substitui o agregado nem o gate.

SonarQube, Veracode e Wiz não estão configurados neste ambiente. Também falta
um limite Rust explícito de cobertura e a execução dos cenários filtrados/por
plataforma. Portanto o gate PagueVeloz permanece **FAIL/não verificável**;
nenhum commit ou ativação automática do Learn em arquivos reais é autorizada
por esta medição.
