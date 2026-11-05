# Pesquisa brownfield — 2026-10-29

## Áreas relevantes

- `src/app/usage.rs`: estado de provider limits, ledger, sessões/statusline;
  Codex já é sondado sem pane. `start_codex_usage_refresh_if_due` usa processo
  em background; erros atualmente retiram amostra viva e não chegam como
  diagnóstico legível na projeção da tela.
- `src/app/usage_codex_app_server.rs`: initialize + account/rateLimits/read e
  account/usage/read por thread. Parser em
  `crates/mercury-usage/src/codex_app_server.rs` aceita primary/secondary e plano.
- `src/client/shell/usage_view.rs`: provider_lines escreve n/a quando não há
  janela; não expõe origem detalhada da falha. Não assumir que Usage é só sessão.
- `src/client/shell/crons_view.rs`: wizard reaproveita overlay Rename, agenda
  livre, TargetKind em 3/6 exige claude|codex|pane|script. Claude/Codex pulam
  endereço; script pula prompt, mas contador continua fixo. offset=None no envio.
- `src/cli/cron.rs`, `src/app/api/cron.rs`, `src/api/schema/cron.rs`: rm e edit
  já existem; não criar segunda implementação de persistência para a TUI.
- `crates/mercury-cron/src/{schedule,scheduler,storage}.rs`: duração simples
  é ocorrência única; every é intervalo; cron cinco campos usa offset fixo.
- `src/app/state.rs::Palette`, config/theme e render/shell: cores semânticas
  já existem, devem ser reaproveitadas. Testes de tela estão em shell/tests.
- `docs/knowledge/README.md`, spec 008: Knowledge e Learn são recursos distintos.

## Evidências da instalação

Servidor atual respondeu knowledge.request e protocolo 22, versão 0.9.3.
Config debug ativo em ~/.config/mercury-dev/config.toml, sem usage/knowledge;
Usage é enabled por default; Learn estava desligado. Vault informado existe
e está vazio. Falta de conteúdo não é falha do índice.
Logs históricos do coletor Codex registram timeout e “application network policy
is unavailable” em 20/10; isso não comprova a causa do n/a em 29/10.
session.snapshot público não contém Usage; não usar esse endpoint como prova
de que não há métricas no snapshot privado da TUI.

Sondagem read-only real em 29/10 pelo CLI Codex: initialize funcionou e
account/rateLimits/read respondeu plano business, primary=null, secondary=null,
individualLimit.remainingPercent=23, resetsAt=1793491200. O parser atual ignora
individualLimit deliberadamente por não haver moeda declarada, mas também perde
o percentual restante explícito. Isso comprova um defeito de cobertura do parser
para a conta atual, independentemente dos erros históricos de rede. Não registrar
accountId, tokens ou resposta bruta com identidade nos artefatos do projeto.

## Referência externa primária

- Vorssaint: https://github.com/vorssaint/vorssaint-utils/blob/main/docs/PRIVACY.md
  consultas pelos CLIs autenticados, sem copiar credenciais; contas por perfil,
  cache/timestamp e backoff. Desktop history não identifica de modo suficiente
  contas Claude individuais; não adotar fallback sem identidade compatível.
- Codex: https://learn.chatgpt.com/docs/app-server
- Claude: https://code.claude.com/docs/en/interactive-mode

## Riscos e restrições

Versões de CLI e planos expõem dados diferentes; confirmar contratos por
sondagem de metadados sem prompt antes de concluir adapter. Não obter cotas
inventadas a partir do ledger local. Timeout atual não vira prova de logout.
Nova UI exige integração em overlay/input/mouse/endpoints, não só desenhar cards.
Delete remove agenda, não significa cancelar processo iniciado. O código atual
de cron não deve receber calendário IANA implícito.

## Esclarecimentos resolvidos por contexto

Alvos iniciais: contas atuais Codex/Claude; sem novo login ou varredura global.
Fuso visível com offset editável; presets cobrem tarefas comuns e mantêm avançado.
Memória local significa Knowledge, sem ligar Learn/edição automática de regras.
Sem dúvidas funcionais bloqueantes. Compatibilidade de sondagem é investigação
técnica da implementação; ausência de suporte precisa de mensagem específica.
