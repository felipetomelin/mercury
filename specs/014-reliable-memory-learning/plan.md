# Plano técnico — memória e aprendizado confiáveis

Status: concluído — 518008c publicado, testes locais e CI Linux/macOS PASS. Contrato: spec.md; evidências: research.md.

## G1 — Diagnóstico que explique a inatividade (P0)

- Criar modelo comum de estágio/resultado: observado, elegível, persistido,
  em execução, aplicado/publicado, no_change, skipped com motivo, bloqueado,
  retry, failed, superseded. Separar API requests de fila de aprendizado.
- Adicionar heartbeat e estado real dos workers; contar probes recebidos e
  rejeitados antes do ledger. Preservar motivos sanitizados sem prompts/IDs crus.
- Status/doctor deve mostrar configuração efetiva, vault efetivo, binding,
  hooks compatíveis, último checkpoint/captura/nota/memória, budget e recuperação.
- Auditar instalação efetiva de hooks globais/projeto/plugins e caminho dos CLIs.
  Distinguir executável presente, contrato suportado e autenticação validada.

## G2 — Leitor e continuidade da sessão (P0)

- Unificar resolução/validação de identidade Codex id/session_id, com rejeição
  de divergência. Reutilizar conceitos de identificação de sessão principal da
  013; não promover subagente a principal pela presença de mensagem user.
- Separar cursor/contador/evento de conclusão da amostra de mensagens. Ler
  incrementalmente, persistir offsets/IDs e tratar truncamento/rotação/compactação.
- Reconstruir bindings após startup/handoff/resume/reload a partir de IDs e
  proveniência confiáveis, localizando o transcript por ID exato. Nunca usar
  “arquivo mais recente do diretório”. Restaurar também exclusões de automação.
- Codex: eventos estruturados de task_complete/turn identificável; Claude:
  Stop/checkpoint compatível mais SessionEnd. Inspecionar versões/capacidades
  antes de instalar hooks. Fallback explícito para checkpoint manual quando
  não houver sinal nativo confiável. Não usar idle isolado como autorização.

## G3 — Checkpoint durável independente do arquivo vivo (P0)

- Persistir evento, identidade do projeto/sessão principal, ID do turno e revisão
  antes de confirmar o recebimento. Worker consome a fila durável com lease.
- Capturar snapshot privado e limitado do trecho concluído, com hashes,
  proveniência e marcador de limite. Validar consistência durante leitura;
  append depois do limite não invalida o snapshot já aceito.
- Não copiar a transcrição inteira a cada turno. Reutilizar chunks por hash e
  materializar apenas a janela necessária; retenção/TTL/limite de bytes para
  snapshots e dados intermediários, sem apagar auditoria necessária ao undo.
- Coalescer eventos repetidos do mesmo checkpoint e não confirmar evento que
  foi descartado por fila cheia. Exclusão de subagentes/origens automáticas em
  produtor e consumidor, inclusive depois de recuperar estado persistido.
- Evoluir a fila antiga com migração versionada. Identificar capturas obsoletas;
  não trocar silenciosamente fingerprints para abranger conversa ainda ativa.

## G4 — Learn → notas → índice, com retomada focal (P1)

- Learn e Knowledge compartilham o checkpoint, sem disputar leituras do arquivo
  mutável. Cada etapa tem chave idempotente e posição durável de retomada.
- Não produzir nota em ausência de aprendizado novo. Dedupe por evidência e
  conteúdo, preservando lições distintas; registrar no_change de forma explícita.
- Manter LearnMemory.md como síntese de até 8 KiB e links para notas gerenciadas.
  Atualizações atômicas só em alvos gerenciados; preservar writer/backup/undo.
- Publicar notas por aprendizado/projeto, com proveniência de checkpoint e
  status proposed. Separar persistência de nota, indexação e grafo; uma falha
  posterior não deve perder uma publicação concluída nem chamar o modelo de novo.
- Recuperação focal por captura/etapa; classificar timeout/budget como retomável
  e conflito de origem como nova validação necessária. Para a captura real falha,
  apresentar prévia e novo checkpoint elegível em vez de repetir fingerprint velho.
- Preservar rotas/skills/código humanos e allow_route_external=false.

## G5 — Recuperação compacta com custo local previsível (P1)

- Separar endpoints/modos de retrieval local, expansão no grafo e síntese LLM.
  Retrieval local não instancia NativeModel nem aciona query expansion externa.
- Candidatos por FTS/vetores locais; ranking por relevância/projeto/status/
  atualidade, remoção de duplicatas e diversidade. Grafo expande evidências
  relevantes em 1 salto por default, com teto de candidatos e relações.
- Montador final respeita até 8 fontes e 8 KiB, incluindo metadados e citações.
  Contador exato quando houver tokenizer compatível; caso contrário, bytes
  estritos e tokens explicitamente estimados. Orçamento entra na chave do cache.
- Cache por consulta normalizada/projeto/endpoint/revisão/confiança. Sínteses
  GraphRAG sob demanda têm cache de entrada/modelo/prompt/revisão; não reutilizar
  resultado de outro projeto ou com fontes revogadas.
- Reusar extrações por chunk e resumos de comunidades intactas. Agrupar dirty
  revisions e atualização do grafo; quando o grafo estiver atrasado, permitir
  recuperação textual consistente com aviso e sem LLM obrigatório.
- Contexto sob demanda com fontes/status e opção explícita de uso por destino;
  nunca anexar vault inteiro nem repetir automaticamente o mesmo pacote em
  todos os turnos. Propostas e inferências permanecem sinalizadas.
- Manter o teto pessoal atual, exibindo reservas/tentativas/chamadas e tokens
  observados separadamente. Priorizar captura/publicação sem deixar grafo consumir
  todo o budget; alteração de limites depende da configuração de ativação.

## G6 — Aceite operacional e ativação (P1)

1. Regressões dos probes documentados antes das correções.
2. Testes isolados dos grupos G1–G5 com contratos reais de eventos/CLIs simulados;
   fluxo completo com vault temporário, chamadas contadas e falhas injetadas.
3. Integração com chats longos e processo preservado por handoff, sem novo
   SessionStart. Assertar nota e contexto recuperado, não apenas status enabled.
4. Avaliação offline: baseline atual versus compacto; fontes corretas,
   relevância, insuficiência, tamanho do pacote, latência e chamadas.
   Incluir 100 consultas locais repetidas, mudanças de nota e projetos homônimos.
5. Smoke com modelo real em dados sintéticos e vault temporário; verificar
   autenticação, formato, publicação, recuperação, cache e custos observados.
6. Ativação pessoal revisável: backup consistente da config e ledgers, versões
   nativas/hook auditadas, config check, instalação e handoff preservando processos.
7. Validar checkpoints pessoais escolhidos pelo usuário, com nota visível no
   vault correto, LearnMemory alterada/auditada, fontes recuperáveis e restart.
   Não varrer sessões antigas automaticamente. Documentar rollback e recuperação.

## Verificação e integração

Uma revisão e um ciclo de testes consolidados por grupo, com repetição focal
quando houver correção. Testes Learn/Knowledge, API/schema, hooks e handoff;
full suite se contratos compartilhados forem alterados; manutenção/hot-path,
fmt, Clippy e build. Verificar evolução do protocolo/JSON e clientes antigos.
Scanners externos e cobertura quantitativa seguem dispensados pela constituição;
resultados indisponíveis não são PASS. Não houve delegação a agentes nesta etapa.

## Portabilidade aprovada

Todos os diretórios vêm da configuração/namespaces do Mercury. Registro de
sessões e cursores ficam no ledger privado, com schema aditivo idempotente.
Testes usam raízes temporárias, provedores sintéticos e vaults independentes.
Linux/macOS usam os controles privados existentes; plataformas sem escrita
privada suportada devem explicar indisponibilidade, sem relaxar permissões.
O usuário autorizou implementação e publicação no pedido de 2026-11-26.

## Refinamentos de implementação

O diário compartilhado usa o SQLite privado de Learn e snapshots JSONL
normalizados. Cada consumidor possui estado independente; Learn repete falhas
com backoff limitado e exige recuperação explícita de claims interrompidos.
O watcher/índice local independe de inferência. Notas referenciam LearnMemory
quando disponível, permitindo navegação bidirecional no grafo Obsidian.
A recuperação local usa FTS + vetores hash + até dois vizinhos em um salto;
respostas GraphRAG opcionais têm cache por grafo/modelo/filtro/revisão/orçamento.
Hook readiness inspeciona registros globais; overrides de projeto/plugins são
exibidos como limitação explícita, sem afirmar um estado efetivo não observado.
Não há deploy nem alteração do vault pessoal no aceite da entrega central.
