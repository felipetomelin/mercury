# Fase 2, passo 7 — usage do Codex via app-server

Status: especificação e plano técnico aprovados pelo objetivo de concluir o `PLAN.md` (2026-10-06). Análise de consistência concluída abaixo.

## Problema

O rollout JSONL do Codex já fornece tokens, contexto e alguns limites, mas não fornece custo USD por thread. A aba Usage mostra `n/a` para custo Codex e orçamentos USD ficam parciais. O app-server local do Codex 0.160.1 fornece `account/usage/read` por `threadId` e `account/rateLimits/read`.

## Solução

- O servidor Mercury consulta o app-server em uma thread de background quando há panes Codex com sessão reportada por hook. Nenhum processo, socket ou leitura de arquivo entra no caminho de renderização.
- A consulta usa JSON por linha em stdio: `initialize`, `initialized`, `account/rateLimits/read` e um `account/usage/read` por thread rastreada. O `session_meta.id` do rollout foi validado como `threadId` em consulta real.
- Uma leitura ocorre imediatamente ao detectar a primeira thread, a cada 60 s enquanto alguma trabalha e a cada 10 min quando todas estão ociosas. Mudança de sessão dispara leitura imediata. Erros e timeouts aplicam backoff de 60 s; o processo filho é encerrado ao fim de cada leitura. Notificações push ficam para otimização posterior.
- Respostas de custo estimado em micros USD atualizam o gauge da thread correspondente e entram como observações cumulativas no ledger já existente. O ledger faz os deltas e preserva orçamentos após restart.
- Limites `primary`/`secondary` e créditos/plano do snapshot do app-server substituem os do rollout quando disponíveis. Se a fonte falhar, o rollout continua como fallback. `null` significa indisponível, nunca zero. `individualLimit` não é tratado como USD: o schema não declara sua unidade.
- A consulta é local e somente de leitura. Nenhum token de autenticação é serializado ou persistido pelo Mercury.

Fora deste passo: tabela de preços, escrita no app-server, scan histórico de sessões sem pane, processos persistentes e ações `route`/`pause`.

## Critérios de aceite

1. Dado um pane Codex com hook e rollout, quando o app-server retorna custo para o mesmo `threadId`, então o gauge mostra o custo reportado e o ledger registra somente o delta positivo.
2. Dado `estimatedUsageUsdMicros: null` ou `threadUsage: null`, o custo segue `n/a`; orçamento USD permanece parcial.
3. Dado limites `primary`/`secondary` nulos, a aba mostra `n/a` e ainda mostra créditos/plano quando reportados.
4. Dado pane removido ou sessão substituída durante a leitura, nenhuma resposta antiga é aplicada ao pane novo.
5. Dado processo ausente, erro ou timeout, o Mercury continua responsivo; volta a tentar após backoff e usa os dados de rollout disponíveis.
6. A atualização do snapshot respeita o protocolo atual (campos opcionais já existentes), sem alterar o contrato público da API.

## Trade-offs

- Processo curto por ciclo simplifica recuperação e timeout, ao custo de startup a cada 60 s; push contínuo fica adiado. A cadência é suficiente para a definição atual de usage ao vivo e não afeta polling JSONL de 1 s.
- Custo é estimativa fornecida pelo Codex e pode ser ausente conforme a rota de cobrança; não inferir valor a partir de tokens.
- Primeiro total visto pelo ledger é atribuído ao momento da primeira observação, pois o app-server não fornece timestamps para cada delta anterior.

## Dependências

- Binário `codex` no PATH e app-server com `--stdio` (validado na versão 0.160.1).
- Sessão `AgentSessionRefKind::Id` já reportada pelo hook Codex.
- `mercury-usage` para tipos neutros e parser tolerante; `UsageService` e `CostLedgerHandle` para aplicação.

## Plano técnico e tarefas

1. Adicionar parser puro de resposta camelCase em `crates/mercury-usage/src/codex_app_server.rs`, com fixtures de `null`, limites, créditos e custo em micros.
2. Adicionar cliente stdio com timeout em `src/app/usage_codex_app_server.rs`. Respostas são associadas por ID de requisição e thread; processo filho termina em sucesso e timeout.
3. Agendar leituras somente para threads Codex rastreadas em `UsageService`; enviar resultado por `AppEvent` e ignorar respostas de sessão obsoleta.
4. Sobrepor custo/limites no snapshot e enviar custo cumulativo ao ledger. Preservar fallback do rollout e a renderização existente.
5. Testar parser, timeout/falha e aplicação do resultado; rodar build, Clippy, testes de Usage e contratos.

## Análise de consistência

- O snapshot já contém custo opcional por agente e limites/créditos por provedor; nenhum campo de wire novo é necessário.
- O ledger já aceita `Provider::Codex` e transforma total cumulativo em delta; não há nova persistência.
- O render compara snapshots por `PartialEq`; a aplicação do evento precisa marcar render dirty somente quando os números mudarem.
- Leitura de rollout pode chegar após o app-server; a sobreposição de custo deve ser reaplicada a cada refresh para não perder o valor reportado.
- As respostas reais nesta conta business têm `primary`/`secondary: null` e créditos presentes; o parser deve preservar exatamente esse estado.
