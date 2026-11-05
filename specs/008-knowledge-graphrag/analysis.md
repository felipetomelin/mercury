# Análise de consistência — 2026-10-29

**Resultado**: consistente para revisão do usuário; implementação aprovada pelo usuário.

| Requisitos | Plano | Tarefas | Aceite |
| --- | --- | --- | --- |
| FR-001–004 | Config, dados, ingestão | K005–008 | US1, SC-001 |
| FR-005 | Ranking híbrido e relações | K008, K011 | US1, SC-002 |
| FR-006–008 | Extração, comunidades, resumos, queries | K012–015 | US2, SC-003–004 |
| FR-009, 015 | Worker, CLI/API/TUI e diagnóstico | K010, K017–018 | SC-007 |
| FR-010 | Egress por endpoint e destino Route | K010, K020 | US3, SC-005 |
| FR-011 | Captura/review/escrita/undo | K019 | US4, SC-006 |
| FR-012–014 | Caminhos, limites, snapshots | K006–009, K016 | SC-001, SC-006 |

## Verificações

- GraphRAG consta no escopo, plano, tarefas e critérios mensuráveis.
- Modo local não usa credenciais/provedores Route como consentimento implícito.
- Sem fallback cloud silencioso; falha do modelo não quebra busca textual.
- Índice privado separado de notas; uma inferência não vira decisão validada.
- Invalidação de chunks propaga para relações, comunidades e resumos.
- Pipeline de modelo não executa instruções das notas, shell nem tools.
- Não há mudança de PTY ou renderização para suportar indexação.
- Critérios de avaliação incluem relações e síntese transversal, não só perguntas
  triviais que FTS já responde. Citação existente não basta para provar suporte.
- CLIs Codex/Claude verificados em smoke real sintético; modelos reportados e versões constam na evidência. Isso não qualifica Windows nem um servidor de modelos local.

## Gate

Pesquisa/spec/clarificações/plano/análise foram produzidos. A regra global
`sdd-workflow.mdc` exige spec aprovada antes de código de produção. A aprovação do usuário foi recebida em 2026-10-29; validações técnicas seguem em execução.
Não há aprovação inventada nem extensão da dispensa histórica de scanners.

Alteração de backend autorizada pelo usuário nesta sessão: Codex/Claude, sem Ollama. Política de saída explícita e autenticação nativa; nenhuma dispensa de segurança foi inferida.

## Fechamento da política de validação — 2026-10-29

Após revisar os resultados locais, o usuário dispensou explicitamente os gates
externos para o projeto Mercury. A decisão está registrada na constituição,
com risco aceito e referência à conversa; K023 foi encerrado por dispensa,
sem transformar checks não executados em PASS. Testes e revisão locais continuam
obrigatórios. Esta decisão posterior supera as restrições históricas acima.
