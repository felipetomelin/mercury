# Fase 5 — Roteamento (spec, esclarecimento, plano e análise)

Estado: especificado para implementação incremental em 2026-10-06. Base: `PLAN.md` §3.1, §4 itens 16–19 e §5, mais `router-phase5-research.md`.

## Problema e objetivo

Uma tarefa enviada a Mercury ainda depende de seleção manual de Claude/Codex. O usuário quer escolher por regras declarativas e headroom, enviar a um pane de agente, acompanhar a execução e receber um resumo; quando o primário está indisponível antes do envio, o sistema deve escolher um fallback seguro.

## Contrato e decisões esclarecidas

- `[route]` é opt-in, com lista ordenada `[[route.rule]]`. Cada regra admite padrão de texto, tag e projeto como filtros conjuntivos; a primeira regra correspondente vence. Cada regra indica provedor (`claude`/`codex`), modelo e effort opcionais, e fallback ordenado. Ausência de regra usa um default explícito; config inválida deve produzir diagnóstico, não enviar a tarefa a agente arbitrário.
- Tags vêm de `--tag` ou metadado estruturado do pedido, não de palavras adivinhadas no prompt. O projeto é o workspace selecionado ou `--project`; o match de padrão deve ser limitado em tamanho e custo. Não usar shell para avaliar regra.
- Antes de decidir, buscar limites frescos quando necessário com timeout curto. Para Codex, usar `account/rateLimits/read` em worker do servidor mesmo sem pane/thread Codex existente, compartilhando uma leitura entre prévias simultâneas e limitando a fila. Para Claude, só a statusline já integrada reporta limites; sem fonte de consulta sob demanda, dado ausente/obsoleto fica `unknown`, nunca 0%. Falha/timeout do refresh também fica `unknown`, com diagnóstico redigido e sem travar TUI. Se há orçamento `on_exceed=route`, aplicar apenas ao escopo relevante. Contexto cheio afeta um pane existente, não a capacidade de uma sessão nova.
- `mercury route` usa o endpoint ativo. A forma do prompt na CLI (texto direto ou somente `--file`/`--stdin`) aguarda resposta do usuário, pois texto direto inevitavelmente entra no argv/histórico do shell. `route preview`/`--dry-run` explica regra, dados usados/idade e escolha sem criar pane nem mandar prompt. Na execução, enviar o ID do workspace junto do rótulo na prévia, para aplicar o orçamento ao workspace certo mesmo se os rótulos usados pela API e pelo histórico de usage divergirem; criar pane novo nesse workspace, iniciar agente escolhido e enviar prompt por `agent.prompt` (não por argv do agente nem comando shell). Nome/IDs de pane e agente devem constar da resposta.
- Fallback automático só é permitido antes do aceite inequívoco do prompt pelo agente: binário indisponível, início falho, limite/indisponibilidade conhecido antes de iniciar, ou erro de transporte antes de submeter. Erro após aceite é indeterminado: não reenviar automaticamente, para evitar efeito duplicado. 401/403 requer diagnóstico de autenticação; 429/5xx só habilita fallback quando a falha ocorreu antes do envio. O primário volta a ser candidato no próximo pedido depois de `resets_at`; não há sticky fallback permanente.
- `--wait` acompanha o agente com timeout e devolve leitura limitada como resumo, sinalizando `completed`, `timed_out` ou `submitted` separadamente. Enviar prompt não equivale a concluir tarefa. Sem `--wait`, retornar após aceite do envio. `--worktree` é opt-in e deve usar os mecanismos existentes com diagnóstico/rollback caso a criação falhe antes do prompt.
- Com `--worktree` no envio, chamar `worktree.create` para o workspace selecionado, sem `force`/`trust_repository` e sem branch/path fornecidos pelo cliente; a API gera a branch e usa o diretório configurado. O root pane do novo workspace é o pane do agente (não criar um segundo split). Antes de `agent.prompt`, falha de startup só permite fallback se `worktree.remove` sem force confirmar remoção do checkout; se o checkout estiver sujo ou a remoção for incerta, preservar e retornar diagnóstico com IDs/caminho. Branch gerada pode permanecer após remoção do checkout, pois a API atual não remove branches; informar esse resíduo em vez de executar `git` no cliente remoto. Após prompt, preservar sempre o worktree.
- `on_exceed=route|pause` em orçamento de provedor excedido exclui esse provedor para tarefas novas, permitindo outro provedor fora do escopo; em orçamento global/workspace excedido, bloqueia o envio, pois mudar de provedor não reduz o consumo do mesmo escopo. Orçamentos por agente não se aplicam a um novo agente com nome único. Janela de provedor sem escopo de provedor é ambígua e não gera bloqueio; dado de provedor obsoleto ou vencido por `resets_at` não pode bloquear. Agentes já trabalhando continuam.

## Critérios de aceite

1. Uma regra correspondente escolhe provedor/modelo/effort de modo determinístico; empate e config inválida têm teste.
2. Headroom fresco evita provedor excedido e seleciona fallback; dado nulo ou obsoleto é explicitamente `unknown` e não 0%.
3. `route --dry-run` explica a escolha sem mutação. Rota real cria pane, inicia agente, envia prompt uma vez e retorna IDs.
4. Prompt não aparece em argv, logs de diagnóstico ou shell history; erros de autenticação/limite são redigidos para não vazar resposta bruta do provedor.
5. Falha antes do envio pode ir para fallback; após aceite, nenhuma repetição automática. O próximo pedido reavalia o primário.
6. `--wait` devolve resumo limitado e estado terminal/timeout inequívoco, sem bloquear TUI ou servidor.
7. `on_exceed=route|pause` sai do no-op documentado e aplica-se somente às tarefas novas em escopo.
8. API/CLI/schema, testes da crate, testes integrados de socket, verificação manual, revisão consolidada e gate externo de segurança/qualidade passam antes de declarar a fase concluída.

## Fora de escopo

Sem proxy de requests do Claude Code, migração de sessão em andamento, reexecução automática de tarefas potencialmente não idempotentes, cobrança estimada por tabela de preços, ou delegação de subagentes por `route` sem comando explícito do usuário.

## Plano por grupos

1. Crate `mercury-router`: tipos, parser/validação, precedência de regras, headroom, decisão e razão auditável; testes de limite desconhecido, stale, `resets_at`, fallback e pausa.
2. Integrar `[route]` ao config, limites tipados do `UsageService` e método `route.preview` no servidor; contrato/schema e testes. Desviar `route.preview` para refresh diferido de Codex somente quando o cache estiver ausente/obsoleto/resetado; agrupar pedidos, limitar concorrência/timeout, aplicar o snapshot no dono do estado e então responder. Dado Claude permanece derivado da statusline.
3. Orquestrar `mercury route` por operações tipadas existentes (`route.preview`, `pane.split`/`worktree.create`, `agent.start/get/prompt/wait/read`, `worktree.remove`) no CLI, garantindo envio único e fallback pré-envio. A API não precisa de um método `route.submit`: a decisão continua pertencendo ao servidor, enquanto o CLI conserva a sequência e identifica respostas indeterminadas sem retry. Testar com sockets/agentes simulados e logs redigidos.
4. Integração `on_exceed`, worktree opt-in, revisão consolidada, testes, smoke real autorizado, TUI e gate de segurança antes de commit.

## Análise de consistência e riscos

- O `UsageService` hoje produz resumo para wire, não uma API de decisão; a conversão para texto e escala inteira é inadequada para seleção. Expor snapshot tipado/idade dentro do servidor, mantendo cliente separado.
- O `route.preview` síncrono não pode fazer I/O de subprocesso no servidor; a leitura de Codex deve ser diferida via `AppEvent`. Uma resposta de refresh nula ou com erro não substitui informação fresca por número inventado; o TTL do roteador mantém dado velho como `unknown`.
- `agent.start` exige pane shell e retorna antes de a inicialização terminar. O orquestrador deve esperar readiness antes de `agent.prompt`; se o prompt foi aceito mas a espera falha, marcar estado indeterminado, não acionar fallback.
- O estado de `.git`/worktree é mutável; só usar a API de worktree existente quando opt-in, nunca inferir autorização por haver repo.
- Credenciais e prompts são sensíveis: evitar argv, tracing e respostas de erro bruto. Limitar strings e duração antes de criar qualquer pane.
- A Fase 4 deixa Teams/SMTP real pendente por escolha do usuário; isso não autoriza disparos externos durante testes da Fase 5.
