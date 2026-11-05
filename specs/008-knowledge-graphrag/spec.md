# Mercury Knowledge & GraphRAG

**Criado**: 2026-10-29
**Status**: aprovada pelo usuário em 2026-10-29 nesta sessão; implementação e validação local concluídas; gates externos dispensados explicitamente pelo usuário para Mercury em 2026-10-29.
**Origem**: pedido do usuário nesta sessão; extensão do Mercury local existente.

## Problema

Contexto de projetos fica distribuído entre sessões, repositórios, decisões e
investigações. History recupera conversas, mas não fornece uma memória revisada
de projetos nem respostas rastreáveis a perguntas que cruzam várias fontes.
O usuário quer retomar trabalho rapidamente e reutilizar conhecimento com agentes.

## Clarificações confirmadas

- GraphRAG faz parte desta entrega, e não de um ciclo futuro.
- Vault existente: `/Users/c12103q/Documents/Obsidian Vault/AI PV Graph`.
- Indexação e armazenamento locais por padrão. Em esclarecimento posterior nesta sessão, o usuário rejeitou Ollama e escolheu os CLIs Codex/Claude autenticados com roteamento Mercury; tarefas LLM enviam trechos somente aos provedores explicitamente habilitados.
- O diretório existe e está vazio na inspeção de 2026-10-29.
- O caminho acima é uma configuração pessoal, nunca um default compilado.

## Solução e limites

Adicionar memória Markdown compatível com Obsidian, busca híbrida, grafo de
relações com evidência e GraphRAG com os CLIs Codex/Claude. O Mercury controla indexação e tarefas;
Obsidian oferece edição/navegação sem ser necessário à execução do Mercury.
O grafo e os índices são reconstruíveis. Documentos são a fonte de conhecimento;
repositórios, issues e MRs continuam sendo as fontes oficiais de engenharia.

Inclui consulta, contexto para tarefas, captura explícita de sessões concluídas,
revisão de propostas, atualização incremental e remoção de derivados obsoletos.
Não inclui sincronização em nuvem, servidor público, Neo4j obrigatório, plugin
Obsidian próprio, instalação automática de modelos ou captura global de sessões.
MCP público e ingestão contínua de GitLab/Datadog ficam em melhorias posteriores.

## Cenários de usuário e aceite

### US1 — Consultar memória no terminal (P1)

1. Dado um vault configurado, ao indexar, notas e links são consultáveis sem
   alterar os documentos originais nem exigir Obsidian aberto.
2. Dada uma busca, o usuário recebe trechos, fonte, estado de validação e data;
   resultados podem ser limitados por projeto, serviço e tipo.
3. Dado um arquivo alterado, removido ou renomeado, a próxima sincronização
   invalida seus trechos, relações, embeddings e resumos antigos.

### US2 — Responder perguntas com GraphRAG (P1)

1. Dadas fontes conectadas, uma pergunta recupera trechos e relações e produz
   resposta com referências verificáveis às fontes e às evidências das relações.
2. Dada pergunta transversal, a consulta global usa resumos de grupos conectados
   e permite rastrear cada resumo aos trechos que o originaram.
3. Dada relação extraída por modelo, ela aparece como inferida. Uma inferência
   não se torna decisão validada sem revisão explícita.
4. Sem evidência suficiente, a resposta declara a limitação; não inventa caminhos
   ou referências. Citações incompatíveis com o snapshot são rejeitadas.
5. Sem CLI/modelo disponível, busca textual e relações explícitas continuam úteis;
   GraphRAG informa indisponibilidade, sem fallback externo silencioso.

### US3 — Preparar contexto para um agente (P1)

1. Dado projeto e tarefa, o usuário gera um pacote limitado de contexto, com
   fontes, decisões aprovadas, pendências e inferências separadas.
2. Gerar ou visualizar o pacote não submete tarefa nem inicia agente.
3. Incluir o pacote em Route exige ação explícita e respeita o destino configurado:
   contexto local não pode ser enviado a agente externo sem autorização de egress.

### US4 — Registrar aprendizado (P2)

1. Dada sessão concluída e identificável, captura explícita cria proposta privada
   limitada, sem publicar a transcrição inteira no vault.
2. Ao aceitar a proposta, uma nota nasce em diretório gerenciado com origem,
   revisão e links; repetição da captura não duplica a nota.
3. Se o usuário editar o destino, aceitar/atualizar/undo detectam conflito e
   preservam o arquivo humano. Rejeitar proposta não altera conhecimento aprovado.

## Requisitos funcionais

- **FR-001**: funcionalidade desabilitada por padrão; vault e projetos configuráveis.
- **FR-002**: aceitar Markdown UTF-8, propriedades YAML limitadas e wikilinks;
  preservar conteúdo desconhecido, resolver ambiguidades sem escolher nota errada.
- **FR-003**: manter identidade estável, hash, revisão, origem e estado
  `proposed|validated|deprecated`; notas sem estado entram como `unreviewed`.
- **FR-004**: suportar projetos, serviços, decisões, incidentes, runbooks e
  aprendizados; relações tipadas têm direção, origem, trecho e revisão.
- **FR-005**: oferecer busca textual/semântica/híbrida e vizinhança limitada do grafo.
- **FR-006**: GraphRAG deve extrair entidades/relações, construir grupos de
  entidades conectadas, gerar resumos com fontes e consultar modo local/global.
  Busca vetorial isolada ou expansão de wikilinks não satisfaz este requisito.
- **FR-007**: separar fatos explícitos, relações inferidas e decisões validadas.
- **FR-008**: registrar modelo, versão do prompt, parâmetros, hash de entrada e
  proveniência dos artefatos; mudança incompatível exige reindexação dos derivados.
- **FR-009**: permitir indexar, consultar, inspecionar status, cancelar e reconstruir
  pelo CLI/API, sem trabalho pesado no loop de renderização.
- **FR-010**: não enviar conteúdo para endpoints externos ou agentes remotos sem
  configuração explícita por destino; credenciais nunca entram em notas/logs/argv.
- **FR-011**: captura e escrita no vault exigem ação explícita, backup e auditoria;
  propostas não alteram regras, skills, configuração, código ou o Learn.
- **FR-012**: ignorar `.obsidian`, `.git`, `.trash`, anexos binários e exclusões
  configuradas; recusar escape de diretório/symlink e não seguir URLs das notas.
- **FR-013**: controlar bytes, arquivos, jobs, hops, resultados, contexto, tempo e
  tokens; cancelamento e falha mantêm o último snapshot consistente identificável.
- **FR-014**: edição/remoção deve invalidar resumos dependentes antes de retorná-los;
  consulta não mistura snapshots de revisão diferentes.
- **FR-015**: incluir diagnóstico de endpoint/modelos, versão do índice, última
  sincronização, fila, falhas e limites, sem registrar conteúdo sensível.

## Critérios de sucesso

- **SC-001**: vault sintético com 1.000 notas pode ser indexado/reaberto e recuperado
  após interrupção sem perda dos documentos nem duplicação de identidades.
- **SC-002**: busca textual aquecida p95 <= 500 ms no corpus de 1.000 notas no host
  de referência registrado; latência de modelo é medida separadamente.
- **SC-003**: conjunto versionado de 20 perguntas (8 textuais, 6 de relações e
  6 transversais), com fontes esperadas; >= 16 respostas corretas e fundamentadas,
  incluindo >= 4/6 de relações e >= 4/6 transversais com modelo/provedor identificado.
- **SC-004**: toda referência retornada aponta para trecho presente no snapshot;
  perguntas sem resposta (5 casos adicionais) reconhecem insuficiência.
- **SC-005**: com `allow_external=false`, nenhuma tarefa LLM é iniciada; com autorização, somente Codex/Claude permitidos podem receber conteúdo. Contexto no Route requer autorização separada por destino; um modelo não pode ativar tools/MCP por conteúdo de notas.
- **SC-006**: testes de falha cobrem conflito humano, restart, remoção de fonte,
  JSON inválido do modelo, timeout, cancelamento e prompt injection documental.
- **SC-007**: fixture de ponta a ponta atravessa CLI/API/worker/índice/consulta;
  smoke com modelo real via CLI é registrado separadamente dos mocks.

## Trade-offs

Priorizar rastreabilidade e controle local, aceitando indexação inicial mais lenta.
Sem alegar melhora de qualidade até comparar com busca textual e híbrida no mesmo
conjunto. Regras de negócio não são inferidas pela semelhança entre notas.
Dados pessoais ficam fora de fixtures; não executar indexação no vault real antes
de concluir gates e configurar conscientemente o destino.

## Tarefas e dependências

Grupos implementáveis em `tasks.md`; arquitetura e decisões em `plan.md`.
Dependências: CLIs Codex/Claude autenticados, geração estruturada e features locais com expansão semântica pelo LLM,
armazenamento privado, vault configurado e contratos CLI/API Mercury.
Instalação/disponibilidade do runtime local será diagnosticada na implementação;
nenhum modelo foi baixado ou ativado nesta fase.
