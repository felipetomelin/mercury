# Auditoria da preparação — Mercury — 2026-10-29

Atualização posterior: a fase 8 foi aprovada e implementada; evidências atuais em
`knowledge-phase8-review.md`. Este documento preserva o escopo da preparação.

## Escopo e situação

Revisão do estado local e das pendências existentes; não executou agentes/modelos
reais, alterou configuração pessoal, reiniciou servidores pessoais ou escreveu no
vault Obsidian. O status inicial tinha 1.313 entradas; alterações preexistentes
foram preservadas. Sem commit/push.

## Correções realizadas

1. **Contratos de manutenção quebrados por arquivos ausentes**: restaurados
   `vendor/portable-pty/LICENSE.md` e dois arquivos em `packaging/windows/licenses/`.
   Fontes autênticas e hashes em `third-party-notices-restoration-20261029.md`.
   Nenhum hash esperado, requisito ou teste foi relaxado.
2. **Fixture de socket excedia limite Unix no TMPDIR macOS**: `unique_test_path`
   em `src/api/server.rs` limita o prefixo descritivo a seis caracteres, mantendo
   pid e nanos para identidade. Somente código `cfg(test)` mudou; três cenários
   existentes de espera de status passam no TMPDIR original.
3. **Spec/checklist fase 7 desatualizados**: `specs/007-auto-improvement/`
   reconciliado com decisões confirmadas em PLAN.md de 2026-10-07. Edições
   estruturadas de config, associação job/alvo e comparação descritiva já existem.
   Regra de arquivamento por desuso de 30 dias foi retirada do escopo, não é bug
   pendente. Exceção histórica de scanners foi preservada só para a entrega antiga.

## Revisão consolidada das correções

**Veredito local**: apto à verificação, sem achado bloqueante no grupo corrigido.
Prefixo encurtado não remove unicidade; os testes continuam exercitando o mesmo
socket/servidor e as mesmas asserções. Nomes temporários não são contratos de
produção. Licenças preservam bytes originais e metadados existentes. Documentos
marcam implementação existente, sem converter smoke simulado em aceite real.
Não foram criados testes que apenas repetem a implementação: os três cenários
reais que detectaram a falha e os testes de manutenção existentes verificam o grupo.

## Pendências remanescentes e classificação

| Item | Estado e ação |
| --- | --- |
| Knowledge/Obsidian/GraphRAG | Especificado em `specs/008-knowledge-graphrag/`; aguardando aprovação antes de produção. |
| Smoke com Claude/Codex reais | Não executado nesta auditoria; foi atribuído ao usuário na entrega anterior. Usar alvo descartável e manter separado de mocks. |
| Inspeção humana da TUI | Layout textual coberto por fixtures não prova cores/contraste; aceite humano permanece. |
| Teams bidirecional | Backlog confirmado fora do aceite local; requer gateway HTTPS, identidade e ações permitidas antes de implementar. |
| Windows/Linux | Compilação/execução e qualificações específicas não substituídas pelo teste macOS; notices restaurados não validam distribuição Windows completa. |
| Uso simultâneo de várias contas Claude | Cache de limites por provedor é limitação conhecida; melhoria de modelo de identidade/conta, não correção implícita. |
| Cobertura/scanners da nova feature | Sem MCP de SonarQube/Veracode/Wiz entre ferramentas disponíveis; sem resultado não há PASS. Dispensa antiga não cobre 008. |
| Ferramentas opcionais | `bun` e `cargo-nextest` não encontrados no PATH inspecionado; suíte Rust usa cargo test, assets JS não são declarados validados nesta rodada. |

## Validação e limites

- Baseline no sandbox: 52 PASS/17 FAIL/1 ignorado em ghostty; falhas por
  PermissionDenied ao criar fixtures em `/var/tmp`, não classificadas como regressão.
- Baseline com permissões adequadas: ghostty 69 PASS/1 ignorado; binário Mercury
  3.681 PASS/2 FAIL/11 ignorados. Dois FAIL reproduziram socket path longo.
- Manutenção após restauração: **80/80 PASS**, incluindo arquitetura de hot path.
- Regressão socket no TMPDIR original: **3/3 PASS**.
- Clippy workspace/all-targets offline/locked com `-D warnings`: **PASS** antes
  do ajuste final do prefixo; rerodada incremental registrada abaixo.
- Formatação e diff check: resultados finais registrados abaixo.
- Suíte workspace com TMPDIR curto: execução completa e resultado registrados
  abaixo; não omitir falhas nem ignorados.

Logs desta sessão em `/private/tmp/mercury-*-20261029.log`; nomes detalhados:
`mercury-baseline`, `mercury-baseline-full`, `mercury-baseline-shorttmp`,
`mercury-maintenance-fixed`, `mercury-socketfix`, `mercury-clippy`,
`mercury-fmt-final`, `mercury-diffcheck-final`.

## Gate de segurança

**Nova feature 008: não verificável nesta fase; nenhum PASS externo declarado.**
Somente documentos de planejamento foram criados para ela. Correções desta
auditoria limitam-se a fixtures, documentação e arquivos de terceiros.
Nenhum achado de scanner foi inventado nem dispensa nova presumida.

### Verificações finais do grupo corrigido

- `cargo fmt --all --check`: PASS.
- `git diff --check`: PASS.
- Clippy incremental após ajuste do harness: PASS workspace/all-targets,
  offline/locked, `-D warnings`.
- Estrutura da spec: 140 linhas, FR-001–015, SC-001–007, sem marcadores de
  template ou esclarecimentos pendentes; aprovação humana ainda necessária.
- Nenhum subagente foi utilizado.

### Resultado final workspace

`cargo test --workspace --offline --locked -- --test-threads=1`, com
`TMPDIR=/private/tmp`, permissões adequadas e caches locais: exit 0,
**4091 PASS / 0 FAIL / 13 ignorados**, sem filtros. Inclui client_mode 32/32 e live_handoff 23/23.
Esta rodada foi iniciada antes do ajuste do prefixo do harness; o ajuste foi
validado adicionalmente pelos três testes de regressão no TMPDIR original e
pelo Clippy incremental. Não houve alteração de produção. Os ignorados e os
alvos com zero testes por cfg de plataforma não são qualificação multiplataforma.

Correções reproduzíveis desta auditoria estão verificadas localmente. Permanecem
aceites reais/manuais e backlogs descritos acima. A feature 008 está especificada,
não implementada; não há aprovação presumida nem PASS de scanner externo.
