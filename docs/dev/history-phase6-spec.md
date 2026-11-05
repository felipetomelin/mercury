# Fase 6 — History (spec, esclarecimento, plano e análise)

Estado: pronto para implementação incremental em 2026-10-06. Base: `PLAN.md` §4 item 25, §5 e `history-phase6-research.md`.

## Objetivo e contrato

Encontrar uma conversa antiga de Claude Code ou Codex por texto e reabri-la no CLI nativo, em um pane novo no projeto original. A fonte é local: `CLAUDE_CONFIG_DIR`/`CODEX_HOME` (ou homes padrão). “Todos” significa todos os arquivos de sessão descobertos, sem o corte de 35 dias do Usage. O índice cobre mensagens conversacionais de usuário e assistente; ignora thinking, argumentos/resultados de ferramentas, telemetria e subagentes, para não replicar conteúdo operacional sensível ou confundir sessões principais. Se um transcript não informa `cwd` confiável, ele ainda pode aparecer na busca, mas não pode ser reaberto até haver diretório válido.

O serviço mantém índice SQLite FTS5 privado e incremental. Ao pesquisar, sincroniza em worker, não no loop de TUI; detecta arquivos alterados e removidos; troca os registros de cada sessão atomicamente. Linhas JSONL incompletas em arquivo ativo são ignoradas, sem descartar mensagens válidas anteriores. Erros de leitura/SQL retornam diagnóstico redigido, sem texto de transcript. Arquivos Claude identificam sessão pelo nome seguro; rollouts Codex por `session_meta.session_id` seguro. Não seguir symlinks no scanner.

`history.search` recebe texto, filtros opcionais de projeto (`cwd` absoluto exato), intervalo de data e agente (`claude`/`codex`), mais limite limitado. Retorna IDs, provedor, projeto, data e trecho curto escapado/limitado; não retorna a conversa inteira nem paths de arquivo. Busca vazia lista sessões recentes, sem `MATCH` inválido. `history.resolve` revalida sessão, fonte e `cwd` antes da ação de reabrir; o cliente cria pane no workspace escolhido e inicia `claude --resume ID` ou `codex resume ID` pelos contratos existentes. Para o CLI ligado a um servidor headless sem workspaces, criar um workspace no `cwd` histórico e usar seu root pane; com workspaces, usar o focado (ou o único) e falhar sem mutação se a seleção for ambígua. Não responde automaticamente a prompts de confiança/autenticação e não envia texto de usuário. Paginação fica para uma iteração posterior; a primeira versão limita a 100 resultados por consulta.

A aba History apresenta campo de busca e filtros de projeto/data/agente, estados de carregamento/erro/vazio, resultados e ação de reabrir. CLI de busca/reabertura é desejável para teste e acessibilidade, mas nunca deve colocar texto sensível de transcript em argv; consulta de busca digitada pelo usuário é uma decisão explícita. A API deve ser o único leitor do índice para clientes.

## Critérios de aceite

1. Um chat antigo Claude e um Codex aparecem por texto, inclusive fora da janela de Usage; duplicatas de streaming não aparecem duas vezes. Resultados ignoram tool output/thinking e não cruzam projetos homônimos.
2. Filtro de provedor, `cwd` absoluto e intervalo de data inclusivo funciona, incluindo o dia inteiro informado em `until`; limite e busca FTS malformada são tratados sem panic nem SQL injection. A data filtra a última atividade da sessão.
3. Alterar, truncar ou remover JSONL atualiza o índice sem resultados órfãos; arquivo parcialmente escrito preserva mensagens válidas.
4. DB, WAL e diretório são privados; snippets limitados; nenhum transcript cru em logs, argv ou erros. Testar FTS5 do SQLite bundled nesta plataforma.
5. Aba History consulta o servidor sem congelar o terminal, permite buscar/filtrar e reabrir em pane novo com `cwd` original; sessão ou diretório ausente tem diagnóstico e não inicia agente.
6. Schema e documentação atualizados; testes de parser/índice/API/UI, smoke local com fixtures, revisão consolidada, testes e gate externo de segurança/qualidade antes de declarar fase concluída.

## Plano em grupos

1. Criar `mercury-history` com descoberta limitada, parse Claude/Codex, índice FTS5 transacional e testes de fixture/segurança/incrementalidade.
2. Integrar serviço ao servidor com trabalho diferido, API tipada `history.search`/`history.open` (ou composição equivalente sem enviar prompt), contrato/schema e testes.
3. Criar aba History e controles de filtro/reabertura, CLI quando coerente com padrão do projeto, testar TUI e socket real com fixtures.
4. Revisão consolidada, suíte local, análise de segurança e gates externos. Não marcar pronto enquanto um gate obrigatório estiver pendente.

## Análise de consistência e riscos

- O scanner Usage agrega tokens, não texto, e tem horizonte de 35 dias: History exige índice próprio e varredura sem esse limite.
- FTS5 pode não estar habilitado em todo build de SQLite; o primeiro teste deve criar/consultar tabela FTS5. Falha deve ser explícita, não fallback silencioso para `LIKE`.
- Um scan completo por pesquisa pode ser caro; cache de metadados e sincronização serializada com limites devem evitar trabalho repetido e bloquear o loop de servidor. Uma consulta pode usar snapshot anterior se a atualização falhar, informando frescor.
- O índice é uma cópia pesquisável de conversa privada. Aplicar permissões restritas inclusive ao WAL, não sincronizar/exportar automaticamente, e permitir purga/rebuild local.
- Retomada não é replay: `agent.start` apenas inicia CLI com ID validado no `cwd`; o provedor pode pedir confiança/autenticação, a cargo do usuário.
