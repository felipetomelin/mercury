# Plano técnico — Mercury Knowledge & GraphRAG

**Status**: aprovado pelo usuário em 2026-10-29; implementação local verificada; gates externos dispensados explicitamente pelo usuário para Mercury em 2026-10-29.
**Spec**: `spec.md`. Pesquisa: `research.md`. Consistência: `analysis.md`.

## Sequência de entrega

1. Revalidar o baseline e corrigir pendências reproduzíveis; reconciliar fase 7.
2. Implementar armazenamento, ingestão Markdown, grafo explícito e busca textual.
3. Integrar CLIs autenticados, features locais/expansão semântica e busca híbrida com proveniência.
4. Implementar GraphRAG completo local/global e avaliação comparativa.
5. Integrar captura revisável, pacote de contexto e superfícies Mercury.
6. Rodar revisão, testes, segurança e smokes por grupo; registrar limitações.

As etapas 2–5 pertencem ao mesmo ciclo solicitado. A entrega não está completa
se parar em busca textual, vetorial ou grafo explícito.

## Arquitetura proposta

- Novo crate `mercury-knowledge`: documentos, chunks, relações, propostas,
  repositório SQLite/FTS5, recuperação, ranking e pipeline GraphRAG.
- Backend de modelo isolado atrás de contrato `embed/extract/summarize/answer`;
  implementação pelos CLIs Codex/Claude autenticados, fakes determinísticos nos testes.
- Config `[knowledge]` independente de `[learn]`, default `enabled=false`.
  Campos: vault, projetos/aliases, exclusões, provedores/modelos, política de
  egress, diretório gerenciado e limites. Nenhum caminho pessoal compilado.
- Índice em `data_dir()/knowledge/`, privado e separado do vault. Backups e
  propostas ficam privados. Documentos aceitos em `Mercury/` no vault.
- Worker servidor, uma escrita por índice; fila de até 8 jobs, 1 job de
  indexação/modelo ativo, timeout e cancelamento. TUI apenas apresenta estado.

## Modelo de dados

Document(id estável, path relativo, project_id, type, status, content_hash,
revision), Chunk(id, doc_id, heading, offsets, hash), Entity(id, aliases, type),
Relation(src, kind, dst, origin, evidence_chunk_ids, revision), Embedding(chunk,
model_fingerprint, dimension, vector), Community(id, members, revision),
Summary(community, evidence, model/prompt fingerprints), Job e CaptureProposal.

Nota sem ID ganha identidade persistida no índice; renomeações só preservam ID
quando há identificador explícito ou correspondência inequívoca por hash.
Links ambíguos/não resolvidos são diagnósticos, não entidades fundidas.
Relações tipadas: `depends_on`, `decided_by`, `affected_by`, `resolved_by`,
`documented_by`, `belongs_to`. Wikilink sem tipo permanece `references`.
Inferências são `origin=inferred`; revisão explícita é armazenada separadamente.

## Ingestão e snapshots

Ler apenas arquivos regulares, caminho canônico contido no vault; excluir
diretórios internos e não seguir links externos. Limites iniciais propostos:
10.000 notas/vault, 1 MiB/nota, 256 MiB total, 1.000 tokens/chunk estimados
com ajuste ao tokenizer disponível, 100 tokens de sobreposição.
Guardar offsets reais; truncamento nunca altera a fonte nem finge cobertura.
Parser Markdown/frontmatter deve preservar campos desconhecidos e não executar
tags YAML, HTML, scripts ou embeds. Escolher dependência após verificar cache.

Hash do conteúdo determina reindexação; não confiar só em mtime/len.
Construir revisão derivada em staging e publicar transacionalmente. Mudanças
invalidam embeddings, relações inferidas, grupos e resumos dependentes. Resumo
obsoleto não entra em respostas; job falho mantém revisão anterior identificada.

## GraphRAG e ranking

1. Busca FTS5 e cosseno sobre features locais versionadas, enriquecidas por palavras-chave; expansão semântica de consultas pelo LLM; fusão determinística por
   reciprocal rank fusion, resultados limitados por projeto/status.
2. Extrair entidades/relações por JSON schema limitado; cada evidência precisa
   referir chunk da entrada. Descartar saída malformada/IDs desconhecidos.
3. Construir comunidades por agrupamento determinístico do grafo (propagação
   de rótulos com seed fixa e limite de iterações), não apenas componentes;
   versionar parâmetros e membros.
4. Gerar resumos de comunidades com citações aos chunks; invalidar por revisão.
5. Consulta local: sementes híbridas + até 2 hops, 50 entidades, 100 relações,
   máximo de 20 chunks e envelope de contexto configurável (default 8.000 tokens).
6. Consulta global: selecionar resumos, mapear respostas parciais citadas e
   reduzir em síntese, preservando fonte e estado de inferência.
7. Validar referências contra snapshot. Não confundir citação válida com
   verdade semântica; conjunto de avaliação mede suporte e respostas corretas.

Sem Neo4j ou extensão vetorial inicialmente; cosseno em vetor limitado em memória
é aceitável no corpus piloto, medindo custo antes de adotar ANN.

## CLIs autenticados e política de saída — alteração autorizada em 2026-10-29

O usuário rejeitou Ollama nesta sessão e escolheu Codex/Claude já disponíveis.
Nenhum runtime de embeddings ou modelo adicional será instalado. Vetores são
features locais de tokens/bigramas com hash; descritores extraídos pelo LLM e
expansão da consulta oferecem recuperação semântica complementar, sem alegar
que se trata de embeddings neurais. Comparar o ganho no conjunto de avaliação.

Usar as regras `[route]`, tags `knowledge` + `extract|summarize|answer`, projeto
e limites observados. Sem regra específica, Claude prioriza extração/resumos,
Codex prioriza respostas; fallback apenas entre provedores permitidos. Nomes de
modelos podem vir da regra ou de `[knowledge.models]`; vazio usa o default do CLI.
Não afirmar qual é o melhor modelo sem avaliação. Mudanças de config/versão dos
CLIs invalidam o fingerprint; modelos realmente reportados ficam na auditoria.

Config default desabilitada e `allow_external=false`. Habilitar processamento
LLM exige `allow_external=true` e `allowed_providers`. Envio a tarefas Route é
outra autorização (`allow_route_external` + `route_destinations`). Conteúdo vai
por stdin; nunca em argv. Não copiar credenciais nem substituir a autenticação.

Codex: `exec --ignore-user-config --ignore-rules --ephemeral --sandbox read-only`,
saída estruturada, hooks/plugins/apps/shell/multi-agent/browser desabilitados.
Claude: print safe-mode/restricted, MCP estrito, tools vazias, sem persistência.
Diretório de trabalho privado e descartável, sem documentos do projeto.
Subprocessos têm timeout, saída limitada, cancelamento e encerramento do grupo.
Chamadas e orçamento Claude são limitados; custo Codex não reportado é desconhecido.
Transcripts e notas não podem ampliar autoridade de execução ou rede.

## CLI/API e UX

Comandos propostos: `knowledge status|doctor|index|search|related|ask|capture|review`
e `context build`. `index` retorna job_id; `jobs show|cancel` pode ficar sob
`knowledge jobs` para não colidir com Cron. CLI humano e `--json` usam mesmo contrato.
`ask --mode local|global` imprime resposta, fontes, revisão e advertências de
dados insuficientes. Busca/ask requerem servidor conforme padrão History.
`context build` visualiza/exporta via stdout; `route ... --context <bundle-id>`
é opt-in, valida revisão e egress antes da submissão, sem duplicar o Route.
Knowledge tab oferece status, busca e detalhe de fontes; grafo visual fica no
Obsidian neste ciclo. Exportar relações confirmadas como links em notas gerenciadas.

## Captura e escrita

`capture` usa identidade de sessão concluída e leitor limitado; aceita conclusão registrada pelo Mercury ou declaração explícita `--completed` do usuário, registrada na origem, sem ativar Learn; proposta privada
com dedup por sessão/hash. Aceite explícito cria nota em diretório gerenciado;
backup+audit+expected hash previnem perda por edição concorrente. Undo restaura
somente estado esperado; nenhuma edição automática de notas humanas fora dele.
Não mudar `[learn]` ou ativar revisão automática como efeito colateral.

## Verificação

Fixtures de 1.000 notas; 20 perguntas com fontes esperadas + 5 sem resposta.
Comparar FTS, híbrida e GraphRAG (local/global) com corpus/modelo/seed registrados.
Testes de DB, parser, ranking, invalidação, dedup, migração, crash, cancelamento,
egress, symlink, conflitos e prompt injection. API/schema/handshake/client e teste
PTY isolado. Smoke real local independente; não alegar PASS por mocks.
Revisão consolidada, testes e gate de segurança conforme skills globais.
Dispensa da fase anterior não autoriza pular os gates desta feature.
