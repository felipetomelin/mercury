# Fase 6 — pesquisa brownfield de History

Estado: descoberta para spec, 2026-10-06. Fonte: `PLAN.md` §4 item 25 e §5.

## Áreas relevantes

- `crates/mercury-usage/src/history_scan.rs` descobre JSONL de Claude em `projects/<slug>/<session>.jsonl` e rollouts Codex em `sessions/YYYY/MM/DD/rollout-*.jsonl`, mas extrai apenas eventos de tokens dentro de um horizonte de 35 dias. Não há índice de texto nem busca. `crates/mercury-usage/src/locate.rs` fornece homes configuráveis por `CLAUDE_CONFIG_DIR`/`CODEX_HOME`, validação de ID e localização por sessão.
- `src/agent_resume.rs` já forma comandos nativos seguros para Claude (`claude --resume ID`) e Codex (`codex resume ID`) e valida IDs/argv. `src/api/schema/agents.rs` e `src/app/agents.rs` iniciam um agente em pane shell existente; `src/api/schema/panes.rs` permite criar pane com `cwd` específico. O CLI de rota (`src/cli/route/dispatch.rs`) mostra como aguardar readiness sem reenviar um prompt incerto.
- `src/client/shell/state.rs` define `ClientShellView::{Terminal,Usage,Crons}` e prevê History. `src/client/shell/crons_view.rs` e `usage_view.rs` fornecem padrão de render sobre o pane surface, estados transitórios, hit map, comandos da API e teclas. `src/client/shell/tabs.rs`, `composition.rs`, `input.rs`, `mouse.rs` e `endpoints.rs` concentram o encanamento da aba.
- `crates/mercury-cron` já usa `rusqlite` bundled e ledger SQLite WAL. O workspace tem a dependência disponível offline. Não há crate `mercury-history` ainda.

## Padrões e convenções

- I/O pesado do servidor sai do hot path para worker e retorna por `AppEvent` (`src/app/usage.rs`, `src/app/api/worktrees/deferred.rs`, `src/events.rs`). APIs de leitura demorada são diferidas antes do handler síncrono em `src/server/headless.rs`.
- API pública usa structs tipadas `serde`/`schemars`; o artefato `docs/next/api/mercury-api.schema.json` é regenerado e testado. Views do cliente consultam o endpoint ativo; não devem ler `~/.claude`/`~/.codex` diretamente.
- Conteúdo de transcript é sensível: não usar argv, logs de diagnóstico, snapshot de UI persistido ou telemetria para texto bruto. Dados de busca devem ter limite de tamanho e permissões locais restritas.

## APIs e contratos encontrados

- Claude: sessão pelo nome do JSONL e `sessionId`; mensagens `user`/`assistant` em `message.content`, com blocos de texto e eventos auxiliares. O fixture `crates/mercury-usage/tests/fixtures/claude-session.jsonl` confirma mensagens duplicadas de streaming e `tool_result` que não devem virar resultados conversacionais duplicados.
- Codex: `session_meta.payload.session_id/cwd`, `event_msg` e `response_item` em `crates/mercury-usage/tests/fixtures/codex-rollout.jsonl`. `response_item.payload.type=message` tem `role` e blocos `output_text`; eventos de tokens/telemetria não são mensagens.
- `agent.start` aceita `args` validados e `pane_id`; `pane.split` aceita `cwd`. O History pode reabrir via comandos nativos existentes após validar a sessão selecionada e o diretório. Nenhum método `history.*` existe ainda.

## Riscos e restrições

1. O índice FTS5 replica texto que pode conter segredos; diretório/DB/WAL devem ser privados do usuário, sem network, snippets limitados e exclusão quando a fonte some. Não indexar output de ferramentas por padrão evita armazenar logs grandes/segredos acidentais sem perder a conversa principal.
2. Um arquivo ativo cresce e pode terminar em linha parcial ou JSON inválido. Indexar incrementalmente por `(path, tamanho, mtime)` e trocar mensagens da sessão em transação, preservando o índice anterior se a leitura falhar. Não bloquear TUI/API durante o scan.
3. Codex/Claude podem ter cópias de uma mesma sessão; deduplicar por provedor+ID e preferir fonte mais nova, sem perder contexto de `cwd`. Nunca confiar só no basename de projeto para filtro/reabertura.
4. IDs, caminhos e texto vêm de arquivos externos mutáveis; validar sessão e `cwd` novamente antes de reabrir, usar parâmetros SQL para `MATCH`/filtros e não interpolar conteúdo em shell. Se a fonte não existe mais, impedir reabertura com diagnóstico.
5. `rusqlite` bundled não prova por si só que FTS5 está habilitado; criar uma tabela FTS5 em teste de integração antes de depender dela.

## Questões em aberto / pressupostos para a spec

- Interpreto “todos os transcripts” como sessões Claude/Codex descobertas nos homes configurados, sem limite de 35 dias; indexar texto de mensagens `user`/`assistant`, não payloads de ferramenta nem thinking, para evitar amplificação de dados sensíveis. Confirmar esse recorte no contrato da Fase 6.
- Filtro de projeto deve usar `cwd` absoluto exato na API; o rótulo curto da UI pode ser só apresentação, para não misturar projetos de mesmo nome.
- Reabrir deve lançar o CLI nativo em um pane novo com `cwd` da sessão e deixar prompts de confiança/autenticação para o usuário, sem responder a eles automaticamente.
