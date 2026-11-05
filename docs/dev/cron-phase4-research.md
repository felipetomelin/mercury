# Fase 4 — pesquisa brownfield para crons

Status: pesquisa (2026-10-06); insumo para a spec, não implementação.

## Áreas relevantes

- `src/server/headless.rs::handle_scheduled_tasks_headless` é o tick periódico do servidor; `src/app/runtime.rs::next_headless_loop_deadline_with_git_refresh` decide o próximo despertar sem busy loop.
- `src/events.rs::AppEvent` entrega resultados de threads/processos de background à `App`; o padrão existe em Usage e Git refresh.
- `src/api/schema.rs::Method` e `src/api/server.rs` expõem métodos locais por socket; `src/cli.rs` roteia subcomandos a módulos em `src/cli/`.
- `src/client/shell/state.rs::ClientShellView` e `src/client/shell/composition.rs` possuem a aba Usage sobre a superfície de panes; a Fase 4 pode adicionar `Crons` sem redesenhar a shell.
- `skills/mercury/SKILL.md` já ensina agentes a usar a CLI/API e pode incluir instruções para criar jobs em linguagem natural.

## Padrões e contratos existentes

- O servidor é dono de estado e execução; o cliente recebe snapshot com campos opcionais `#[serde(default)]`. Métodos API públicos exigem schema regenerado e testes de shape.
- Operações que bloqueiam rodam em thread/processo de background, e o resultado entra por `AppEvent`. Nenhum I/O de job/SQLite deve ocorrer em renderização.
- O repo não possui crate cron nem SQLite no `Cargo.lock`, nem dependência HTTP/SMTP. `/usr/bin/sqlite3` existe nesta máquina, mas o binário do Mercury deve continuar multiplataforma; usar o executável do sistema como única persistência seria frágil.
- Configuração de usuário fica sob `$XDG_CONFIG_HOME/mercury`/`~/.config/mercury`; dados mutáveis, como o ledger de custo, sob `$XDG_DATA_HOME/mercury`/`~/.local/share/mercury`.

## Riscos e restrições

- Adicionar SQLite requer dependência nova e disponibilidade de download/build. Acesso à rede é restrito neste ambiente; não substituir o requisito por JSONL sem documentar mudança de escopo.
- Jobs têm efeitos externos (processos, Teams, e-mail); testes precisam usar executores e transportes falsos. Segredos de webhook/SMTP não podem ir para logs, snapshots ou arquivos de job.
- O scheduler precisa avançar `next_run_at` de forma durável antes do dispatch, inclusive depois de crash/restart; múltiplos servidores e catch-up exigem regra clara de ownership.
- Executar script/agent ou enviar prompt a pane existente são alvos com autoridades diferentes; a configuração precisa explicitar cada um, sem interpretar prompt como shell.
- O wire/API e a UI serão interfaces compartilhadas; regressões exigem testes de contrato e suíte de cliente além dos testes do crate.

## Questões resolvidas para a spec

- O servidor será a autoridade dos jobs; CLI e aba chamam API local, não editam storage diretamente.
- SQLite será usado para ledger de execuções e incidents, conforme `PLAN.md`; `jobs.toml`/arquivo equivalente será salvo atomicamente. A dependência nova deve ser disponibilizada antes da implementação do ledger.
- Horários de cron usam offset UTC fixo por job nesta primeira versão, coerente com `[usage]`; nomes IANA exigiriam base tz nova.
- Entregas Teams/SMTP são opt-in por job e leem credenciais de variáveis de ambiente indicadas pelo usuário, nunca valores literais persistidos.
