# Fase 2 — varredura histórica para orçamentos (D9)

Status: especificação, esclarecimentos e plano técnico aprovados pelo objetivo de concluir o `PLAN.md` (2026-10-06). Análise de consistência abaixo.

## Problema

Hoje os orçamentos somam tokens apenas das sessões com pane e hook nesta execução. Depois de reiniciar o Mercury, um orçamento mensal ignora transcripts antigos, inclusive sessões que rodaram fora do Mercury. O ledger de custo já sobrevive ao restart, mas os registros de tokens não.

## Escopo e esclarecimentos

- Varredura no startup, em background, de transcripts Claude e rollouts Codex sob os diretórios configurados. Somente arquivos JSONL com `mtime` dentro dos últimos 35 dias entram na leitura; eventos são filtrados pela janela de 35 dias para não contar conteúdo antigo de arquivos ativos.
- Custo continua vindo exclusivamente do ledger. Nenhum custo é inferido de tokens ou dos JSONL. Orçamentos USD seguem `partial` quando há tokens de sessões sem custo reportado.
- Identidade de deduplicação: `(provider, session_id)`. O conjunto histórico exclui sessões atualmente rastreadas pelos panes, cujos tails incrementais são a fonte autoritativa. Quando um pane passa a rastrear uma sessão histórica, ela deixa de contribuir pelo snapshot histórico; quando deixa de ser rastreada, volta a contribuir pelo snapshot histórico até a próxima varredura. Não somar main + subagente duas vezes.
- Escopos `global` e `provider` sempre recebem eventos históricos. Para `workspace`, usar o `cwd` declarado pelo transcript/rollout somente quando ele coincide de forma inequívoca com a identidade de um workspace persistido/aberto; caso contrário `workspace = None`. Sessões sem pane não recebem `agent` ou `pane` inventados.
- A varredura não escreve nos diretórios de Claude/Codex e não lê SQLite interno. Erros por arquivo são isolados; um arquivo inválido não derruba a varredura inteira.
- Fora desta entrega: indexação FTS5 (Fase 6), atualização contínua de sessões externas enquanto o Mercury roda, recuperação de custo histórico anterior ao ledger e atribuição retroativa de workspaces que já não existem.

## Critérios de aceite

1. Depois do startup, um orçamento global/provedor inclui tokens de transcript/rollout recente não anexado a pane.
2. Um arquivo antigo ou evento fora do horizonte não contribui; arquivos malformados são ignorados sem travar o servidor.
3. Uma sessão presente no scan e no tail de pane aparece uma vez só em cada orçamento; subagentes Claude contam uma vez.
4. O uso histórico sem custo deixa orçamento USD parcial; o ledger continua sendo a única fonte de custo.
5. A varredura nunca bloqueia renderização e o snapshot recebe atualização quando ela termina.
6. `workspace` só é atribuído por correspondência inequívoca de `cwd`; `agent`/`pane` históricos seguem ausentes.

## Plano técnico

1. Acrescentar um scanner puro à crate `mercury-usage` que enumera os dois formatos e devolve eventos agrupados por sessão, com `cwd` opcional. Testar com fixtures temporários, subagentes, corte temporal, arquivos inválidos e deduplicação.
2. Agendar a leitura única de startup numa thread do `UsageService`, com `AppEvent` próprio. Guardar os registros históricos em memória de forma limitada ao horizonte e reagir à conclusão com render dirty.
3. Mesclar os registros em `collect_usage_records` excluindo as sessões dos tails ativos. Mapear `cwd` para workspace só quando houver uma identidade inequívoca.
4. Adicionar teste integrado de orçamento mensal após scan e teste de transição histórico ↔ pane rastreado. Executar build, Clippy, testes de Usage/contrato e teste da arquitetura de hot path.

## Análise de consistência

- `ClaudeTranscript::events()` e `CodexRollout::events()` já fornecem `UsageEvent`; o scanner não precisa calcular tokens de novo.
- `CodexRollout::cwd()` já expõe o diretório; o Claude transcript guarda `cwd` no JSONL, mas o parser atual não o expõe, então o scanner precisa capturá-lo de forma tolerante sem alterar a contagem.
- `UsageService::usage_summary_wire()` já monta `UsageRecord` em memória; a mescla ocorre ali, sem I/O na renderização.
- `Workspace::identity_cwd` e `display_name_from_terminals()` oferecem uma identidade conservadora para orçamento por workspace; nunca inferir o nome pelo slug do diretório Claude.
- O ledger replay é independente da varredura de tokens; a mudança não altera seu formato nem o wire público.
