# Pesquisa

- `src/app/usage.rs`: cache por TerminalId, resolução via
  `persisted_agent_session`, polling em background e aplicação de resultados.
- `apply_usage_refreshed`: o ramo Claude aceita qualquer session_id quando o
  provedor corresponde. O ramo Codex exige igualdade do ID. Defeito confirmado
  por leitura; relação com o relato ainda não reproduzida.
- Atualizações sem usage não carregam identidade suficiente para validar a
  sessão de origem; avaliar a corrida de resposta vazia após troca de sessão.
- `apply_statusline_report` já exige igualdade de provedor e session_id.
- `crates/mercury-usage/src/locate.rs`: procura arquivos pelo ID exato, inclusive
  no fallback por diretórios. Não seleciona simplesmente o transcript mais novo.
- `src/ui/sidebar/tokens.rs`: apresenta context_permille recebido na usage.

Riscos: sessões atribuídas incorretamente antes do polling, respostas atrasadas,
cache de sessão anterior e confusão entre contexto local e cotas da conta.
Não foram lidos transcripts privados nem alteradas sessões em execução.

## Títulos do histórico

- `src/persist/snapshot.rs`: label manual e identidade de sessão já coexistem no
  snapshot. `src/persist/writer.rs` preserva o nome no índice no checkpoint,
  independentemente de a conversa já ter sido pesquisada.
- `src/server/headless.rs`: captura labels ativos; worker de histórico persiste
  e consulta nomes sem leitura de arquivos na renderização.
- `session_index.jsonl` do Codex: confirmadas somente as chaves `id`,
  `thread_name` e `updated_at` por inspeção local, sem imprimir valores privados.
- Claude: leitor suporta eventos `custom-title` e índice de projetos com
  `customTitle`/`summary`. Índices ausentes ou malformados permitem fallback.
- `HistoryHitInfo.title` é opcional; a UI mostra o título e mantém a resolução
  pelo ID. CLI textual também prioriza o título; JSON continua incluindo o ID.
