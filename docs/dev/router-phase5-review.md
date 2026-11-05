# Fase 5 — revisão, testes e gate consolidado

Estado em 2026-10-06: **implementação local em verificação; gate externo FAIL/não verificável**. Sem commit.

## Escopo revisado

`mercury-router`, config `[route]`, `route.preview` com limites/orçamentos, CLI
`route submit` e seus fluxos de pane/worktree, `agent.start/get/prompt`, teste
novo `tests/route_submit.rs` e schema existente. Revisão do grupo por correção,
idempotência, isolamento de prompt, estados de erro, performance e testes.

## Veredicto local e achados

**PASS local para os fluxos de envio isolados.** Os testes com servidor/socket reais
e `codex` falso confirmaram regra `default_provider=codex`, criação de pane,
readiness via estado reportado, submissão e recebimento de prompt sintético
uma única vez. Com `--wait`, o agente falso reportou working→idle, o CLI
retornou `completed` com resumo e sem ecoar o prompt. Com `--worktree`, um
repositório Git temporário originou checkout e workspace distintos, o agente
usou o root pane e recebeu o prompt uma única vez. Na falha de startup marcada
como `blocked`, o checkout foi removido antes do fallback, sem envio do prompt;
somente a branch residual foi reportada para limpeza explícita. O app-server de limites
também foi simulado, sem conta externa.
O agente falso e o prompt são criados em diretório privado temporário, com
shell não-login/PATH controlado; a fixture encerra o servidor e remove seus
arquivos ao fim.

Nenhum achado bloqueante novo no código revisado. O dispatch continua evitando
retry depois de `agent.prompt` incerto, preserva pane/worktree quando limpeza
não é confirmada e não põe prompt nos argumentos de inicialização. A resposta
de `--wait` e o fallback pré-envio têm cobertura unitária; a integração cobre
submissão simples, `--wait`, `--worktree` e limpeza após falha pré-envio, mas não
execução por provedor real.

## Evidência

- `cargo test --offline --test route_submit -- --test-threads=1`: 9/9 PASS,
  incluindo 4 testes de envio/espera/worktree/limpeza via socket/pane/agente falso e
  5 do suporte.
- Testes anteriores: 25 filtrados por `route`, 2 do leitor Codex, schema,
  build e worktree API conforme `PLAN.md`.
- `cargo clippy --offline --all-targets -- -D warnings`: PASS após o teste novo.
- O smoke real anterior chegou a `needs_interaction` no prompt de confiança do
  Codex sem submeter a tarefa; não foi tratado como sucesso de envio.

## Riscos e gate

- Ainda faltam envio/resultado com agente real já autorizado e inspeção manual
  TUI. O texto direto em argv permanece
  pendente de decisão; `--file`/`--stdin` continuam disponíveis.
- SonarQube quality gate/cobertura/hotspots, Veracode e Wiz estão indisponíveis
  neste ambiente, inclusive sem plugin identificado na busca anterior. Portanto
  o gate PagueVeloz é **FAIL/não verificável** e a Fase 5 não está concluída.
