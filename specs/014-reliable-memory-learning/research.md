# Pesquisa brownfield e diagnóstico — 2026-11-26

## 1. Áreas relevantes

- `src/app/api/panes.rs`: vincula transcript ao receber pane.report_agent_session.
- `src/app/api.rs`: sinais Codex de saída do processo e idle/turn probe.
- `src/app/learn.rs`: registro em memória, milestones, worker e curadoria semanal.
- `src/app/learn_native.rs`: revisão roteada pelos transportes Codex/Claude.
- `src/app/api/learn.rs`: sinais Claude Stop/SessionEnd e checagem de vínculo.
- `crates/mercury-learn/src/transcript.rs`: identidade, tail, clipping e turnos.
- `src/server/headless/knowledge/automation.rs`: captura/journal/monitor/retry.
- `src/server/headless/knowledge.rs`: captura, status e busca/hybrid/ask.
- `crates/mercury-knowledge/src/{automation,capture,graph,store}.rs`: persistência,
  publicação, orçamento, contexto e grafo.
- `src/integration/assets/{codex,claude}/`: contratos dos hooks instalados.
- `src/persist/snapshot.rs`, `src/server/headless/lifecycle.rs`: handoff/restart.

## 2. Evidência real, somente leitura

Servidor instalado responde no protocolo 23, sem restart_needed/stale. A árvore
Git estava limpa antes desta investigação. Configuração do perfil dev aponta
para o vault pessoal esperado; o diretório existe, não é symlink e contém uma
nota Markdown: Mercury/LearnMemory.md, 1080 bytes, alterada em 2026-11-10.

Learn: enabled=true, allow_external=true, zero alvos humanos explícitos.
Knowledge: enabled=true, allow_external=true, Codex/Claude permitidos;
automação/captura/publicação/grafo ligados; teto diário de 64 reservas.
allow_route_external=false permanece configurado.

Estado persistido e API:

| Sinal | Observação |
| --- | --- |
| Learn changes | 1 applied; nenhum failed/conflict/undone |
| Learn triggers | 5 succeeded: 1 session_end Codex e 4 curation_weekly |
| Último session_end | 2026-11-10, aproximadamente 12:40 UTC |
| completed_sessions | 1 registro |
| Knowledge auto_capture | 1 failed, attempts=1, sem proposal_id |
| Erro da captura | source changed; rebuild or review again |
| Propostas Knowledge | 0 |
| Índice Knowledge | 1 nota, 2 chunks, 12 entidades, 5 relações inferidas, 2 resumos |
| Índice atualizado | 2026-11-10 |
| Orçamento automático no dia | 0 reservas observadas |
| Monitor | watching com timestamp recente |

A captura falha referencia um arquivo existente de cerca de 14,7 MB, modificado
novamente após a data da captura. Seu header contém id e session_id coerentes.
Isso confirma origem mutável; não comprova sozinho em qual escrita ocorreu a
divergência original. As três sessões Codex atuais inspecionadas também têm ambos
os campos e todas excedem 4 MiB. Nos tails há marcadores estruturados task_complete
e task_started. Não foi impresso texto de mensagens/instruções.

Configurações globais de hooks inspecionadas:

- Claude settings.json: integração Mercury apenas em SessionStart.
- Codex hooks.json: integração Mercury apenas em SessionStart.
- A entrada hooks.state no config.toml Codex não contém Mercury.
- A precedência completa de plugins/configuração por projeto ainda deve ser
  auditada na implantação; não inferir que não exista qualquer outro hook.

CLIs locais respondem: Codex 0.172.0; Claude Code 2.1.289. As opções usadas pelo
transporte estão anunciadas no help local. **Não** foi verificada autenticação
por chamada de modelo. Não foi consumido orçamento Learn/Knowledge nesta etapa.
Resultados brutos ficaram em diretório temporário privado, fora do repositório.

## 3. Falhas reproduzidas / confirmadas no código

### R1 — Marcos param silenciosamente em conversas longas (P0)

transcript.rs limita a amostra a 64 mensagens/4 MiB de tail e define
user_turns=None quando truncated=true. resolve_turn_probe usa snapshot.user_turns?
e descarta o probe. Esse descarte ocorre antes de claim_trigger, sem registro
de falha no ledger. Não basta aumentar o tamanho da amostra: a identidade do
checkpoint precisa ser independente do contexto enviado ao revisor.

Probe sintético vinculado à biblioteca compilada, sem modelo:

    40 synthetic turns: retained_messages=64, truncated=true, user_turns=None

As três sessões atuais já excedem a janela de tail, condição que produz esse
descarte. O evento específico de cada sessão não foi rastreado retroativamente.

### R2 — Binding não sobrevive ao handoff (P0)

App::new inicializa learn_transcripts e learn_excluded_sessions vazios.
A inserção do vínculo ocorre em handle_pane_report_agent_session e exige ID e
path no mesmo report. O hook Codex atende somente SessionStart. O snapshot de
handoff preserva a referência da sessão, mas não restaura esse registro de Learn.
Processos preservados podem continuar sem novo SessionStart. A ausência de um
novo binding impede os gatilhos; exclusões de automação também exigem reconciliação
durável, sem permitir loops após restart.

### R3 — Captura pressupõe encerramento de processo (P0 / novo requisito)

Knowledge é acionado por saída Codex reconhecida e SessionEnd Claude. Turn probes
alimentam somente Learn. Chats mantidos vivos não geram notas por conclusão de
tarefa. Isso corresponde à spec 011 anterior, mas conflita com o comportamento
agora confirmado pelo usuário. Idle isolado não é conclusão confiável.

### R4 — Crescimento do arquivo vira conflito terminal (P0)

O fingerprint inclui device/inode/tamanho/mtime/ctime. capture_transcript exige
o mesmo fingerprint antes/depois do modelo; a publicação também o verifica.
fail_capture trata Conflict como failed imediatamente, mesmo em attempts=1.
A proteção evita texto inconsistente, mas não oferece snapshot estável de tarefa
nem distingue append legítimo de substituição da origem.

retry_automation zera tentativas e reapresenta a captura com o fingerprint antigo.
Se a origem já mudou, um rebuild por si só não corrige esse conflito.

### R5 — Compatibilidade Codex incompleta (P1)

O leitor Learn exige payload.session_id no header e na validação do tail. O leitor
History já aceita payload.id. Probe sintético, mesmos dados e apenas chave trocada:

    header=session_id: OK, user_turns=Some(1)
    header=id: MissingIdentity

É uma falha de compatibilidade comprovada. Não é atribuída como causa direta das
três sessões atuais: todas têm os dois campos nos headers observados.

### R6 — Saúde/sucesso não comprovam aprendizado (P1)

worker_running é calculado por self.app.learn.is_some(), não por heartbeat/handle
saudável. run_weekly_curation retorna true inclusive com zero planos; quatro
sucessos semanais não significam quatro novos aprendizados. Erros nos probes
podem desaparecer antes do ledger. watching descreve o monitor, não a captura.

### R7 — Contexto compacto ainda não usa a expansão do grafo (P1)

Index::context usa search textual, até 20 fontes e limite acumulado de 24.000
bytes; não tem orçamento por tokenizer nem expansão do grafo nessa função.
GraphRAG existe para ask e derivados, com cache de extração. Busca hybrid cria
NativeModel e usa expansão de consulta; sua repetição não deve ser o caminho
obrigatório de recuperação local econômica. É necessário separar retrieval de
síntese, escopar cache e medir qualidade/custo.

## 4. Padrões a preservar

SQLite/journal privado; fontes Markdown; notas proposed; escrita reversível;
conflitos humanos; escopo por projeto; leitores limitados; identidade explícita;
processamento fora da UI; budget conservador; consentimento por destino.
As specs 007/008/011 e seus testes sintéticos são base reaproveitável, não prova
de aceite operacional do fluxo pessoal observado em novembro.

## 5. Perguntas respondidas / limites

O usuário confirmou checkpoints com chat aberto e apenas memória/notas, com
recuperação econômica por grafos. Não há dúvida impeditiva para o plano.
Não foram feitos retry/rebuild, alteração de configuração, publicação, inferência
real, instalação ou restart nesta investigação. Autenticação real, escrita atômica
no vault e qualidade de recuperação ficam no aceite controlado da implementação.
