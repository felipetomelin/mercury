# Fase 7 — pesquisa brownfield de Auto improvement

Estado: descoberta para spec, 2026-10-06. Fonte: `PLAN.md` §4 itens 20–24, §5 e arquitetura local.

## Áreas relevantes

- `src/integration/claude_settings.rs` instala hoje apenas `SessionStart` para a integração Claude. A migração remove hooks Mercury legados de `Stop`/`SessionEnd`, preservando hooks de terceiros. Reintroduzir gatilhos de Learn requer ação própria, opt-in/compatível e testes de install/uninstall; o contrato atual de hook não fornece “fim de sessão” para Learn.
- `src/app/actions.rs` processa `AppEvent::PaneDied`, `StateChanged`, `HookStateReported` e `AgentSessionReported`; `src/app/api/panes.rs` valida report de sessão/estado. Hooks, detecção visual e `agent.start` podem produzir eventos repetidos ou sem ID de sessão. Dedup do trigger deve ser persistente, por provider+session+motivo/turno, não por pane transitório.
- `crates/mercury-history` localiza/indexa transcripts Claude e Codex; só indexa texto conversacional e fornece `resolve_resume`. Para revisão de uma sessão, deve existir leitor limitado de transcript e contagem de turnos, sem expor tool output/thinking por padrão. `mercury-usage` tem localização de sessão e tokens, mas horizonte de 35 dias; Learn não deve confundir essas métricas com eficácia de uma tarefa.
- `src/app/cron.rs`/`crates/mercury-cron` executam jobs em worker, com timeout/output limitado, ledger SQLite e agendamento persistente. É padrão para revisão semanal, mas o scheduler atual é voltado a jobs configurados pelo usuário; Learn deve evitar criar cron editável/recursivo no fluxo normal.
- `src/config/model.rs` contém `Config { usage, route, ... }` e validação/reload; não há `[learn]`. `src/api/schema.rs` e `src/cli.rs` têm rotas tipadas para API/CLI. A shell usa abas locais `Usage`, `Crons`, `History`, com dados consultados do endpoint ativo; Learn pode seguir o mesmo padrão de UI.
- `crates/mercury-cron/src/storage.rs::save_atomic` demonstra temporário privado + `sync_all` + rename para arquivos próprios. A Fase 7 escreve arquivos do usuário, portanto precisa de backup antes da mutação, ledger durável e rollback com detecção de conflito.

## Padrões e convenções existentes

- O servidor é dono do estado e I/O pesado corre em worker; alterações de UI chegam por `AppEvent`. Config e API usam `serde`/`schemars`, schema público é regenerado e testado. O cliente TUI não deve ler/escrever transcript ou ledger diretamente.
- Headless executors (`mercury-cron`) passam prompts por stdin, não argv, limitam tempo/output e não têm permissão implícita de criar cron. O reviewer Learn deve receber snapshot de conversa por stdin, usar saída estruturada e não ganhar acesso a ferramentas/escrita arbitrária.
- Estado local por sessão está em `crate::session::data_dir()`, mas backups de Learn exigidos pelo plano ficam sob o diretório de dados do usuário `~/.local/share/mercury/learn/backups/` (respeitando XDG/Windows). O índice/ledger devem ser privados.

## APIs e contratos encontrados

- `agent.start` e `agent.prompt` são interativos; `claude -p` no executor Cron é referência mais adequada para review em background, sem pane. `claude --help` local (2.1.289, 2026-10-06) confirma `--model`, `--tools ""` (desabilita ferramentas), `--json-schema`, `--output-format json`, `--max-budget-usd`, `--no-session-persistence`, `--safe-mode`, `--restricted`, `--strict-mcp-config`, `--permission-mode dontAsk` e `--permission-prompts none`. A [receita oficial da Anthropic para reviewer agendado](https://platform.claude.com/cookbook/claude-agent-sdk-scheduled-repository-reviewer-scheduled-repository-reviewer) confirma `structured_output` no resultado e que um limite de gasto pode ser excedido pela última chamada; portanto o runner deve rejeitar resultados acima do teto reportado. Isso valida o contrato planejado, não a autenticação, o custo efetivo ou o comportamento de um review real; nenhum review foi disparado nesta pesquisa.
- `PaneReportAgentSessionParams` identifica sessão/cwd via hook; `HookStateReported` sinaliza estados, mas não substitui diretamente `SessionEnd`. `PaneDied` captura término de processo mesmo sem hook, desde que ID de sessão tenha sido reportado.
- `mercury-cron` oferece ledger de execução, mas não um ledger de mudanças em arquivos; Learn precisa registrar alvo, hash antes/depois, backup, origem, motivo, timestamp e status de undo de cada aplicação.

## Riscos e restrições

1. Transcript e saída do reviewer são não confiáveis (prompt injection). Nunca executar comandos ou aceitar paths arbitrários do modelo. Aplicar somente operações estruturadas em alvos permitidos; validar canonicalização, symlinks, limites de tamanho, conteúdo e tipo antes de qualquer escrita.
2. Aplicação totalmente automática pode sobrescrever edição humana concorrente. Guardar hash/mtime do conteúdo lido, fazer backup privado antes de escrever, escrever atomicamente e abortar se o alvo mudar. Undo deve recusar sobreposição quando o hash atual divergir do hash produzido pelo Learn, salvo opção explícita de force, se houver.
3. Fim de sessão/Stop/contagem de turnos podem disparar review duplicado ou loop (review do próprio reviewer). Registrar dedup/lease persistente; desabilitar gatilho em sessões Learn e cron; limitar frequência, concorrência e orçamento.
4. Alterar `CLAUDE.md`/`AGENTS.md`/skills/config/cron é de alto impacto. Sem commit/push automático. Mudanças em repo Git permanecem como working-tree edits; não tocar em arquivos fora da allowlist configurada. Backups/ledger têm prioridade transacional sobre a escrita.
5. Curator não pode apagar skills/memórias sem recuperação. Arquivamento/mesclagem requer backup e ledger; métricas de eficácia precisam de baseline e comparação identificáveis, nunca inferir qualidade somente de custo bruto.
6. O gate externo SonarQube/Veracode/Wiz/cobertura está indisponível, portanto conclusão e commit seguem bloqueados independentemente de testes locais. Em 2026-10-06, não havia ferramentas MCP desses três serviços, configuração de CI/análise no projeto nem `cargo llvm-cov` instalado; o Rust local também não tinha o componente `llvm-tools`. Isto é FAIL/não verificável, não uma aprovação implícita.

## Questões em aberto

- Pergunta enviada ao usuário: `[learn]` ativo por padrão ou opt-in? O plano define aplicação automática quando ativa e kill-switch `learn.enabled=false`, mas não fixa o default.
- Confirmar qual fonte é suportada para término/turnos de Codex e Claude nesta versão: hooks Claude adicionais, transições Mercury e/ou leitura do transcript. Fallback de polling deve ser explícito e testável.
- Política exata de modelo barato/review sem ferramentas, limites por sessão/semana, e forma de medir eficácia antes de automatizar curator.

# Session-end signal contract (2026-10-06)

The current managed Claude and Codex hooks report SessionStart identity only.
Visual `Idle` transitions are per-response and do not prove session completion.
The official [Claude hooks reference](https://code.claude.com/docs/en/hooks)
documents `SessionEnd` as a separate event with `session_id`,
`transcript_path`, `cwd`, and `reason`; it fires on exit, `/clear`, and session
switch via `/resume`, has no decision control, and defaults to a short 1.5 s
timeout. Therefore a future Claude end signal should do only a bounded local
enqueue and must keep existing third-party hooks intact. A `Stop` hook alone
would signal a completed response, not necessarily an ended session. Codex
has no equivalent managed end hook in this repo, so process-exit identity
must be separately proven before its transcript is reviewed.

## Métricas antes/depois — descoberta brownfield (2026-10-07)

### Áreas e contratos atuais

- `crates/mercury-cron/src/storage.rs` finaliza um run e coloca usage reportado no outbox na mesma transação. O evento durável contém `run_id`, `job_id`, provedor, tokens/custo, resultado e horário de conclusão do run. `src/app/cron.rs::record_headless_observation` entrega essa amostra ao Learn com chave opaca `cron:<job_id>` e fase `observed`; restart/replay conserva o ID e o horário.
- `crates/mercury-learn/src/metrics.rs` aceita fases `observed`, `baseline` e `after`, deduplica por provedor/ID de observação e só considera comparáveis amostras da mesma tarefa/provedor com resultado conhecido em ambas as fases. A UI expõe apenas as amostras `observed`, corretamente marcadas como insuficientes para comparação.
- `crates/mercury-learn/src/storage.rs` guarda mudanças por `target_id`, fonte e instante da aplicação. Não há contrato que associe um `target_id` de skill/memória/instrução a um `job_id` de Cron. `crates/mercury-cron/src/job.rs` tem lista `skills` dentro da definição do job, mas ela orienta o prompt e não prova carregamento nem efeito do alvo.

### Restrições e risco de inferência

Só a ordem temporal de uma mudança Learn e de um run Cron não demonstra que o run usou o arquivo alterado. Mesmo quando o mesmo job roda antes e depois, custo/tokens e resultado são descritivos, não causalidade nem qualidade. Não promover amostras `observed` existentes a `baseline`/`after` em lote, nem associar por nome de skill no prompt ou por diretório. Um vínculo explícito entre ID de job e ID de alvo poderia permitir comparação descritiva do mesmo job/provedor, preservando as amostras observadas e o horário real; mudanças concorrentes, alteração da definição do job e ausência de resultado conhecido ainda teriam de invalidar a comparação.

### Questão de esclarecimento

Perguntado ao usuário se aceita vínculo explícito job Cron ↔ alvo Learn para comparação descritiva antes/depois. Sem essa decisão, manter `dados insuficientes` é o único resultado defensável; a instrumentação atual já mede observações reais, mas não prova eficácia de uma alteração.
