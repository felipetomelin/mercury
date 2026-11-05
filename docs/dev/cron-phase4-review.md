# Fase 4 — revisão consolidada da TUI e gate

Estado em 2026-10-06: **correção local verificada; gate externo FAIL/não
verificável**. Sem commit.

Escopo corrigido pelo usuário em 2026-10-06: nenhum envio de email. O caminho SMTP e sua dependência foram removidos; jobs antigos com destino `email` continuam carregando, mas esse destino é descartado em memória e na próxima gravação. O usuário escolheu Workflows/Power Automate para Teams bidirecional; a exposição HTTPS autenticada do Mercury e o formato de interação ainda precisam ser definidos. O webhook atual só publica notificações.

## Achado e correção

Um smoke em PTY com configuração, runtime e homes de agente temporários abriu a
aba Crons, mas mostrou `Action unavailable`: `cron.list` não constava nas
capacidades anunciadas pelo canal do cliente, embora a API e a aba existissem.
O mesmo bloqueio atingia os demais comandos da aba. `src/server/client_commands.rs`
agora anuncia os oito métodos usados por listagem, assistente, edição, pause,
execução, runs e log. O teste de contrato fixa seus formatos sem reescrever o
fixture v1; também fixa os métodos aditivos anteriores de agent.start/History.

## Evidência local

- `cargo test --offline --bin mercury server::client_commands::tests -- --test-threads=1`:
  6/6 PASS, incluindo capacidade Crons e formato do endpoint.
- `cargo test --offline --bin mercury cron -- --test-threads=1`: 29/29 PASS.
- `cargo build --offline --bin mercury` e `cargo clippy --offline --all-targets -- -D warnings`:
  PASS; `git diff --check`: PASS.
- Após rebuild e restart do servidor temporário, a TUI mostrou
  `Nenhum job agendado` sem aviso de método indisponível; refresh e assistente
  de criação abriram. O rascunho foi cancelado, sem criar job. A aba Usage
  mostrou estado vazio e a History abriu no mesmo PTY. O servidor foi parado;
  `/private/tmp/mercury-tui-smoke-j7i01L` permaneceu porque a política deste
  ambiente recusou sua remoção, inclusive com escalonamento.
- Em segunda sessão isolada, com `XDG_DATA_HOME` temporário, um job script
  `/bin/echo` sem destinos externos apareceu na lista da TUI. `n` iniciou run
  manual `success`; Enter abriu runs e log com `MERCURY_TUI_SMOKE_OK`. `p` pausou
  o job (`enabled=false`) e `e` alterou a agenda de `every 1h` para `every 2h`,
  confirmado por `cron list`. O job de teste foi removido via `cron rm` e o
  servidor encerrou com exit code 0. Nenhuma entrega Teams/SMTP ocorreu.
- Em terceira sessão, o assistente da TUI criou `wizard-smoke` com `every 3h`,
  alvo `/bin/echo` e workdir temporário. `cron list` e `jobs.toml` confirmaram
  o job e o caminho; a agenda futura não foi executada. O job foi removido via
  `cron rm` e o servidor encerrou com exit code 0.

## Limites e gate

O smoke visual cobriu criação, listagem, run/log, pausa e edição com script
local; agente real em pane ainda precisa de inspeção. Notificações, entrada por
Workflows/Power Automate e smoke real de Teams ficam no backlog por decisão do
usuário e não bloqueiam esta fase; o transporte de saída passou em serviço
local simulado, mas não representa integração bidirecional. Nenhum teste SMTP
real ou envio de email será feito.
Após a correção de escopo: `cargo test --offline -p mercury-cron` 31/31 PASS;
`cargo test --offline -p mercury --bin mercury cron -- --test-threads=1`
28/28 PASS fora do sandbox restrito; schema público regenerado e teste de
artefato PASS; Clippy dos alvos Mercury/cron com `-D warnings` e
`git diff --check` PASS.
SonarQube, Veracode, Wiz e cobertura de projeto não estão disponíveis neste
ambiente. O gate PagueVeloz permanece **FAIL/não verificável**; não fazer commit
nem declarar a fase concluída.

Revisão visual adicional em 2026-10-07: a área da aba Crons agora é limpa antes de desenhar, evitando símbolos residuais do pane. Grupo conjunto das abas Usage/Crons/History/Learn: shell 331 PASS/2 ignorados, client_mode 28/28 na rerodada, Clippy global/formatação PASS. Notificações e interação Teams permanecem no backlog; gate externo FAIL/não verificável.

Smoke adicional em 2026-10-07: `tests/client_mode.rs::pane_cron_prompts_existing_fixture_agent_and_records_reply` inicia servidor e cliente em PTY, aguarda um agente Codex descartável reportar `interactive_ready`, executa um Cron no pane existente e confirma entrega única do prompt, `success` e log `CRON_PANE_OK`. O par de testes Cron verifica que logs ficam sob `XDG_DATA_HOME` temporário. Não houve modelo, Teams ou email reais. Isto cobre o fluxo de pane com fixture; a verificação com agente real continua pendente.

Incidente de isolamento: o harness anterior não definia `XDG_DATA_HOME`; cinco jobs sintéticos (`observed-fixture`/`pane-fixture`) foram gravados no diretório pessoal de dados. A lista e o ledger foram auditados, os cinco jobs removidos pela API, e os logs/banco de teste movidos para a cópia recuperável em `/private/tmp/mercury-cron-fixture-backup-Ci98gf/`. Um banco vazio recriado por testes anteriores foi auditado (0 runs/incidents/outbox) e movido para a mesma quarentena; o diretório pessoal ficou com `jobs = []` e sem logs. Todos os spawns de servidor nos testes de integração receberam data home por fixture para impedir recorrência. A suíte Rust completa foi rerodada com código 0 após a correção (3664 PASS/11 ignorados no binário; `client_mode` 30/30; dois testes TCP filtrados), e `client_mode` 30/30 foi repetido após ajuste final no helper. Clippy global, formatação e `git diff --check` PASS. O gate externo continua FAIL/não verificável.

Auditoria complementar: a busca por subprocessos com `server` encontrou dois caminhos que a primeira varredura de `.arg("server")` não capturava: o servidor nomeado do harness CLI e um smoke opcional de shutdown que usa `.args([..., "server"])`. Ambos agora recebem `XDG_DATA_HOME` isolado; os helpers CLI relacionados também herdam data home do fixture. Revisão consolidada do grupo: nenhuma escrita de produção nem mudança de contrato; o risco de dados pessoais por ambiente herdado foi reduzido. A compilação dos alvos `cli` e `host_shutdown` passou, mas ambos têm zero testes executáveis em macOS por `cfg` de plataforma; Clippy global, formatação e `git diff --check` passaram. O cenário Linux/DBus permanece pendente de execução na plataforma adequada.
