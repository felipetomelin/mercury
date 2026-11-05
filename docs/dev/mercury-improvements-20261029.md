# Melhorias propostas para o Mercury

Data: 2026-10-29. Sugestões de roadmap; somente knowledge/GraphRAG e correções
do baseline estão no pedido atual. Cada expansão precisa de spec própria.

| Prioridade | Melhoria | Motivo e critério de sucesso |
| --- | --- | --- |
| P1 | `mercury doctor --json` consolidado | Hoje config check, status e cron doctor fragmentam o diagnóstico. Mostrar build exato cliente/servidor, socket, migrações, agentes, modelos e falhas de hooks, com ações propostas sem executá-las automaticamente. |
| P1 | Recuperação e backup verificáveis | Ensaio isolado de export/import de configurações, jobs e ledgers; rollback deve preservar edições humanas e panes. |
| P1 | Identidade de contexto por projeto | Projeto, repo/worktree, sessão e destino de modelo explícitos; evitar recuperar dados de projeto errado ou enviar contexto local a destino não autorizado. Parte atendida pela feature 008. |
| P1 | Suite reproduzível por plataforma | Pré-requisitos diagnosticáveis, paths temporários curtos, isolamento de XDG/config/vault; smoke macOS real separado de Linux/Windows. |
| P2 | Observabilidade dos próprios jobs | Duração, tempo em fila, cancelamento, timeout, retry e falha por subsistema; conteúdo de prompts fora dos logs. |
| P2 | Avaliação contínua de contexto | Corpus/perguntas reais anonimizadas e comparação FTS/híbrida/GraphRAG; rastrear acerto, fontes, latência e tokens. Incluído no piloto 008. |
| P2 | Retenção e compactação | Política explícita por índice/ledger/backup, preview do que será removido e teste de recuperação antes de compactar. |
| P2 | Custos e limites por conta | Usage/Route já existem; melhorar distinção de contas do mesmo provedor e visibilidade de limite desconhecido, sem scrape de TUI. |
| P3 | Contrato MCP para conhecimento | Expor consultas/contextos limitados após estabilizar a API; evitar implementação duplicada e escrita genérica no vault. |
| P3 | Teams bidirecional | Retomar backlog somente com gateway HTTPS, identidade, ações permitidas e dedup definidos. Não é bug da versão local atual. |

Evitar abrir todos esses trabalhos juntos. Priorizar recuperação confiável,
diagnóstico e qualidade de contexto antes de novos canais externos.
