# 011 — Aprendizado contínuo

Status: implementado, validado e ativado pessoalmente em 2026-11-05.
Evidências: docs/dev/continuous-learning-011-review.md. Publicação autorizada.

## Problema

Learn depende de habilitação/alvos manuais e captura Knowledge/indexação não
seguem automaticamente o encerramento de sessões ou mudanças no vault.

## Solução

Learn e Knowledge habilitados por padrão; false explícito continua sendo
kill-switch. Sem vault configurado, usar vault privado gerenciado por sessão.
Learn sem alvos recebe somente uma memória Markdown gerenciada, sem editar
AGENTS.md/skills/crons/config humanos implicitamente. Processamento externo
exige configuração: Learn ganha allow_external=false por padrão; Knowledge
preserva allow_external=false. O pedido autoriza habilitar ambos explicitamente
nesta máquina com os CLIs disponíveis/autenticados, mantendo o vault existente.

Sessão encerrada reconhecida pelo Mercury → fila privada persistente → captura
com modelo roteado → nota automática com status proposed no diretório Mercury/
→ indexação incremental → GraphRAG. Notas automáticas nunca são validadas como
decisões humanas. Encerramento e marcos de turnos continuam alimentando Learn;
origens automatizadas são excluídas para impedir loops.

Monitorar o vault periodicamente fora da UI, com debounce, para mudanças/remoções.
Serializar operações com a API Knowledge existente. Persistir progresso,
proposta associada, retentativas limitadas e diagnóstico sanitizado; retomar
após restart sem repetir captura/publicação já concluídas. Publicação humana
posterior pode validar uma nota proposta; undo continua conflitando com edição.

## Critérios de aceite

- AC1: defaults ligados e configurações antigas false respeitadas; configuração
  sem alvo/vault funciona com escopo gerenciado; config inválida falha fechada.
- AC2: sem consentimento externo, indexação/grafo explícito locais funcionam e
  captura/GraphRAG aguardam configuração sem consumir LLM.
- AC3: encerramentos Claude/Codex autenticados pelo pane/transcrição disparam
  captura; turnos não exportam sessões em andamento; Cron/Learn não recursam.
- AC4: duplicatas e restart preservam identidade da captura/proposta/nota.
  Falha/cancelamento não promove nota a validated nem substitui edição humana.
- AC5: criação/edição/remoção no vault atualiza índice após debounce; conteúdo
  intacto reutiliza extrações, sem rebuild pago a cada poll.
- AC6: modelo indisponível, orçamento diário e limite de tentativas deixam estado
  inspecionável; não bloqueiam UI e são recuperáveis por config/novos trabalhos.
- AC7: kill-switch cancela trabalho pendente; reload/handoff não deixa workers
  antigos escrevendo com configuração anterior; concorrência controlada.
- AC8: status Knowledge informa automação, fila, falhas, última indexação/grafo
  e necessidade de configuração; TUI mostra resumo do estado.
- AC9: testes isolados cobrem ciclo completo, restart, falhas, opt-out, proveniência,
  edições humanas e mudanças do vault; smoke real em dados sintéticos separado.
- AC10: ativação pessoal com backup/config check/handoff preserva pane IDs/PIDs,
  crons e vault; atualizar mercury instalado e publicar commit no GitHub.

## Clarificações

Nenhuma dúvida bloqueante: autorização desta mensagem abrange implementação,
publicação e ativação pessoal. Consentimento pessoal Codex/Claude será gravado
explicitamente, sem mudar o default de privacidade de novas instalações.
Autopublicar significa nota proposed; validação humana continua explícita.
Não coletar sessões históricas indiscriminadamente. Sem auto-commit/execução
arbitrária por modelo; roteamento e crons humanos exigem alvos explícitos.

## Trade-offs / dependências

Polling limitado e debounce evitam nova dependência de watcher nativo. Defaults
externos desligados requerem configuração antes do ciclo pago em novas máquinas.
SQLite/ledgers, NativeModel, política Learn e writer reversível existentes são
reutilizados. Garantia de replay começa após persistência do sinal no worker;
fila de entrada limitada expõe saturação. Linux/Windows não ganham qualificação
por testes em macOS. Vaults enormes mantêm os limites existentes de GraphRAG.

## Tarefas

Config/defaults; journal/publicação; worker/monitor; eventos/reload; UI/docs;
testes/revisão/smoke; ativação e publicação.

Revisor Learn também usa roteamento/fallback Codex/Claude; custos desconhecidos
permanecem ausentes. Status runtime é opcional para clientes antigos.
