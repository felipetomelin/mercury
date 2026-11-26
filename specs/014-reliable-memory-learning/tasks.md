# Tarefas agrupadas

- [x] Investigar código, configuração, status e ledgers sem alterar o vault.
- [x] Reproduzir bugs do leitor com dados sintéticos e sem modelo.
- [x] Confirmar checkpoints com chat aberto, memória/notas e contexto econômico.
- [x] Preparar spec, plano e análise alinhados.
- [x] Aprovação de planejamento, implementação, testes e publicação (2026-11-26).

## G1 — Diagnóstico operacional

- [x] Contar sinais recebidos/elegíveis/descartados e motivos, antes da inferência.
- [x] Expor saúde real, no_change e último resultado útil por estágio na CLI/TUI.
- [x] Inspecionar registro global dos hooks e contrato dos CLIs; explicitar limites dos overrides locais.

## G2 — Leitores e continuidade

- [x] Regressões id-only, divergência de IDs, clipping e conversas longas.
- [x] Cursores/contagem/checkpoints incrementais independentes da amostra LLM.
- [x] Rebind e exclusões após resume/reload/restart/handoff.
- [x] Eventos de conclusão confiáveis nos provedores e checkpoint explícito.

## G3 — Fila e snapshots

- [x] Evento durável antes do ack; lease, replay e dedupe por checkpoint.
- [x] Snapshot limitado de trecho finalizado, com proveniência e hashes.
- [x] Append legítimo, rotação/reescrita, retenção e migração da fila anterior.

## G4 — Memória/notas e recuperação

- [x] Dedupe de lições, no_change, síntese LearnMemory de até 8 KiB e links.
- [x] Publicação/retomada por estágio, auditoria, conflitos e undo.
- [x] Recuperação focal com prévia; fonte legada alterada exige novo checkpoint e não recebe rebase.
- [x] Indexação/monitor independente da disponibilidade do grafo/modelo.

## G5 — Contexto econômico

- [x] Retrieval local sem LLM; ranking, diversidade e expansão limitada do grafo.
- [x] Teto de 8 fontes/8 KiB, fontes/status e tokens exatos ou estimados rotulados.
- [x] Cache/invalidação por escopo e revisão; graph incremental/coalescido.
- [x] Medir baseline/qualidade/custo com corpus e 100 recuperações locais.

## G6 — Aceite e ativação

- [x] Revisão e testes consolidados por grupo; testes de integração reais de IPC.
- [x] Smokes reais sintéticos: memória/undo e nota/indexação/grafo validados.
- [x] Contratos de configuração, armazenamento privado, backups/undo e handoff via integração.
- [x] Aceite portátil em raízes temporárias independentes; sem ativação no vault pessoal.
- [x] Registrar evidências finais e preparar a publicação autorizada após gates.
