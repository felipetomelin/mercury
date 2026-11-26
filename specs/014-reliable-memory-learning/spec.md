# Memória confiável no Obsidian e aprendizado com contexto econômico

Data: 2026-11-26
Status: aprovada em 2026-11-26; implementada e validada localmente; publicação autorizada.
Escopo confirmado pelo usuário nesta investigação.

## Problema

Knowledge/Learn aparecem habilitados, mas o uso cotidiano não produz um fluxo
contínuo de memória reutilizável. No ambiente inspecionado há uma nota gerenciada,
uma revisão Learn de sessão e uma captura Knowledge que falhou antes de gerar
proposta. Chats longos, retomados e preservados por handoff são o caso principal.
O usuário quer salvar aprendizados sem fechar o chat e recuperar só o contexto
relevante, evitando custo recorrente e enchimento de contexto dos agentes.

## Solução

1. Capturar tarefas/checkpoints concluídos de sessões principais Claude/Codex,
   mantendo a conversa aberta. Reconciliar vínculos e progresso após restart,
   reload, resume e handoff, sem depender de novo SessionStart.
2. Registrar checkpoints duráveis e snapshots privados, limitados e imutáveis,
   antes do processamento. Tratar crescimento posterior do chat sem invalidar
   uma captura já consistente; substituição de origem continua sendo conflito.
3. Aplicar Learn somente à memória gerenciada e publicar aprendizados úteis em
   notas Markdown no vault configurado, com origem, projeto e status proposed.
4. Disponibilizar busca/contexto local por índice e grafo limitado; produzir um
   pacote pequeno, deduplicado, com fontes e orçamento explícito. Reutilizar
   extrações e respostas em cache, em vez de chamar modelos a cada recuperação.
5. Exibir progresso e motivos reais de bloqueio por etapa; permitir recuperação
   focal sem reconstruir todo o grafo nem repetir chamadas já concluídas.

## Esclarecimentos confirmados

- Gatilho desejado: tarefas/checkpoints concluídos, mesmo com o chat aberto.
- Alvos: **apenas memória gerenciada e notas**. Sem alterações automáticas em
  código, skills, AGENTS.md, instruções humanas, roteamento ou crons.
- Solução portátil do repositório: caminhos derivados dos diretórios de dados,
  vault e executáveis configurados por instalação; nenhuma dependência de HOME,
  identidade, sessão ou credencial pessoal do ambiente investigado.
- Validar em instalações temporárias isoladas; não alterar o vault pessoal.
- Publicação no repositório autorizada após revisão e testes locais.
- Guardar conhecimento reaproveitável, não uma cópia das conversas. Sessões de
  subagentes e da própria automação não alimentam aprendizado recursivo.
- Busca/contexto deve priorizar eficiência local. Síntese por modelo é opcional,
  sujeita a consentimento, cache, orçamento e disponibilidade.
- Não coletar histórico indiscriminadamente; recuperação de sessões antigas
  será uma prévia selecionável, se necessária, e nunca um backfill global tácito.

## Critérios de aceite

### A. Eventos e continuidade

- A1: uma tarefa principal com conclusão verificável gera checkpoint persistido
  sem encerrar o CLI/chat. Idle, tool result ou texto alegando conclusão isolados
  não são suficientes. Existe checkpoint explícito de fallback com diagnóstico.
- A2: repetir eventos, reiniciar ou fazer handoff não duplica captura, nota ou
  aplicação. A confirmação do evento implica persistência recuperável.
- A3: sessões já abertas recuperam vínculo por ID/provedor/proveniência; IDs
  incompatíveis, origem automática e subagentes são rejeitados com motivo visível.
- A4: leitores aceitam formatos Codex id e session_id, rejeitam divergência entre
  ambos, suportam Claude e continuam avançando em chats maiores que 4 MiB/64
  mensagens. Contagem/checkpoint não depende de uma amostra truncada do contexto.

### B. Aprender e publicar

- B1: somente novos checkpoints elegíveis entram no processamento, com janela
  limitada e informação de truncamento. O crescimento normal do transcript após
  o checkpoint não invalida seu snapshot; conteúdo reescrito gera conflito.
- B2: uma conclusão útil resulta em nota proposed no diretório Mercury/ do vault,
  com links/origem/projeto. Sem novidade, registrar no_change sem nota redundante.
- B3: LearnMemory.md permanece um resumo limitado a 8 KiB, interligado às
  notas por links explícitos; não cresce indefinidamente nem substitui conteúdo humano divergente.
- B4: escrita tem journal, auditoria, backup/undo e detecção de conflito. Falhas
  entre modelo, proposta, publicação e indexação retomam da etapa durável correta.
- B5: a captura falha observada tem diagnóstico e recuperação focal segura;
  não basta zerar tentativas mantendo um fingerprint já obsoleto.
- B6: watcher reflete criar/editar/renomear/remover notas e invalida derivados;
  graph pendente não bloqueia publicação nem a busca textual já consistente.

### C. Contexto econômico e grafo

- C1: busca e context build locais não iniciam LLM. Usam ranking lexical/vetorial
  local e expansão de relações relevantes já existentes, com fontes verificáveis.
- C2: default proposto: até 8 trechos, 1 salto no grafo e pacote final de até
  8 KiB UTF-8, incluindo metadados. Objetivo aproximado: 2 mil tokens; sem
  tokenizer compatível, informar estimativa, nunca uma contagem exata inventada.
- C3: projeto/endpoint, revisão das fontes, consulta, orçamento e política de
  confiança fazem parte da chave do cache. Uma mudança invalida só o necessário.
- C4: 100 recuperações locais sobre fontes intactas consomem zero chamadas de
  modelo. Síntese GraphRAG repetida e idêntica reutiliza cache válido; geração
  incremental não reextrai chunks intactos nem recalcula comunidades intactas.
- C5: avaliação com corpus versionado mede relevância, fontes corretas, tamanho,
  latência e chamadas. O modo compacto preserva a qualidade do baseline registrado
  e respeita o teto; redução percentual só é afirmada depois de medida.
- C6: pacotes separam decisões validadas, notas propostas e inferências; nada é
  promovido a validado automaticamente. Sem evidência, retornar insuficiência.
- C7: não injetar automaticamente em todo prompt. Preservar permissões por
  destino; allow_route_external=false continua valendo até ativação específica.

### D. Operação e validação

- D1: CLI/TUI mostram último checkpoint, última captura/publicação/aplicação,
  heartbeat real, fila durável, no_change, skips, falhas e próximo retry por etapa.
  Curadoria sem alterações não conta como novo aprendizado aplicado.
- D2: opt-out, falta de autenticação, timeout, modelo indisponível, orçamento e
  proveniência inválida têm estados distintos. Não ocultar falhas antes do ledger.
- D3: orçamento diário e por tarefa, limites de armazenamento e backoff continuam
  impostos. Coalescer grafo e checkpoints repetidos, sem descartar silenciosamente
  um checkpoint confirmado. Não aumentar os limites pessoais automaticamente.
- D4: testes isolados cobrem o fluxo inteiro, chats longos, concorrência, fontes
  append-only/reescritas, falhas, restart e handoff com os CLIs permanecendo vivos.
- D5: o aceite da entrega central inclui smoke real sintético e testes de
  instalações temporárias independentes: nota, memória, busca, contexto, auditoria,
  restart/handoff e undo. O pedido posterior do usuário prioriza portabilidade;
  ativação no vault pessoal fica fora da publicação. Habilitado não prova operação.

## Trade-offs

Snapshots privados limitados e cursores persistidos têm custo de armazenamento,
mas evitam exportação inconsistente e chamadas repetidas. Grafo local limita
amplitude para manter baixo custo; síntese global fica sob demanda. Falhas de
proveniência continuam conservadoras. Não alterar arbitrariamente limites do
leitor para simplesmente carregar conversas inteiras. Migrações são versionadas
e compatíveis com o estado existente, preservando notas e auditoria.

## Tarefas e dependências

Grupos detalhados em tasks.md: diagnóstico observável; leitores e continuidade;
checkpoints duráveis; publicação/recuperação; recuperação econômica por grafo;
aceite integrado e ativação. Reutilizar mercury-learn, mercury-knowledge,
mercury-usage, SQLite, hooks, native transport e políticas de consentimento.
Sem banco vetorial externo, Neo4j, Ollama ou plugin Obsidian obrigatório.
