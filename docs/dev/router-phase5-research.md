# Fase 5 — pesquisa do roteamento

Estado: descoberta brownfield para especificação; 2026-10-06.

## Contratos existentes

- `mercury-usage` já normaliza `Provider::{Claude,Codex}`, `ProviderLimits`, `ContextUsage` e `BudgetStatus`. Valores de limite podem estar ausentes na conta business; ausência não pode ser interpretada como folga.
- `UsageService::summary_wire` projeta limites e orçamentos para a shell, mas as decisões de roteamento precisam de dados tipados do servidor, com timestamp/frescura. `on_exceed = route | pause` hoje só faz parse e é no-op explícito; a Fase 5 deve ligar essas ações sem depender do cliente TUI.
- `agent.start` inicializa Claude/Codex num pane shell existente, enquanto `agent.prompt` envia texto a um agente existente. `pane.split` cria novo pane, e a API já suporta métodos assíncronos e respostas JSON. O CLI `agent` fornece padrões de parsing e chamada à API.
- `worktree.create` recebe `workspace_id` e cria checkout, workspace e `root_pane` em uma operação assíncrona (`src/api/schema/worktrees.rs`, `src/app/api/worktrees/deferred.rs`). O resultado fornece os IDs do novo workspace/pane; `worktree.remove` recebe o ID desse workspace e, sem `force`, preserva um checkout que ficou sujo. A remoção do checkout não apaga automaticamente a branch criada. O CLI de rota pode usar a API tipada para manter compatibilidade com endpoints remotos, sem executar `git` na máquina cliente.
- `route.preview` hoje é síncrono (`src/app/api/route.rs`), mas o servidor já desvia operações demoradas para handlers diferidos antes de chamar `handle_api_request_after_internal_events_drained` (`src/server/headless.rs`). `AppEvent` entrega resultados de threads ao dono do estado (`src/app/api.rs`). O leitor de `codex app-server` (`src/app/usage_codex_app_server.rs`) consulta `account/rateLimits/read` mesmo sem IDs de thread e já limita inicialização/leitura por timeout; pode ter um wrapper de timeout curto exclusivo para rota. O polling periódico de Codex (`src/app/usage.rs`) só roda quando existe thread rastreada, portanto não supre rotas de sessões novas. Claude não tem leitor de limite sob demanda no projeto: seus limites chegam via statusline; dado ausente continua `unknown`.
- Nenhuma crate de roteamento nem comando `route` existe. O `Config` carrega seções por `serde(default)`; uma seção `[route]` pode ser introduzida sem quebrar configs antigas. CLI e servidor devem compartilhar a decisão, sem regras duplicadas.
- A Fase 4 executa headless Claude/Codex e scripts em scheduler próprio. Não misturar a seleção de agente para tarefa interativa com a execução de cron antes de definir explicitamente o contrato de integração.

## Riscos que a spec precisa resolver

1. Regras por padrão/tag/projeto e precedência determinística quando mais de uma combina.
2. Critério de frescura dos limites e de contexto; sem fonte ou após timeout, não inventar headroom.
3. Erros seguros para fallback (429, 5xx, 401/403, limites) sem repetir uma tarefa que pode já ter produzido efeitos.
4. `route` deve escolher, criar o pane, iniciar o agente e enviar o prompt sem vazar o prompt em argv, log ou shell history.
5. O resultado/"resumo" exige um limite de leitura e timeout; não confundir confirmação de envio com conclusão da tarefa.
6. Worktree é opcional e cria estado no disco; só criar quando o usuário pedir explicitamente.
7. Múltiplas prévias simultâneas não devem gerar tempestade de processos `codex app-server`; uma atualização diferida deve agrupar pedidos com fila limitada. Falha/timeout nunca pode virar 0% nem expor mensagem bruta do provedor.

## Referências locais

`PLAN.md` §3.1/§4/§5; `src/app/usage.rs`; `src/config/usage.rs`; `src/api/schema/agents.rs`; `src/app/api/agents.rs`; `src/app/api/panes.rs`; `src/cli/agent.rs`; `crates/mercury-usage/src/model.rs`.
