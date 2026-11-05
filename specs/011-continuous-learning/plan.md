# Plano técnico

Status: spec/clarify/análise completos; implementação, testes e ativação
concluídos. Evidências: docs/dev/continuous-learning-011-review.md.

1. Defaults: enabled=true nos dois recursos; Learn.allow_external=false;
   permitir targets/vault vazios como seleção de escopo gerenciado, resolver
   somente no servidor. Preservar false explícito e falhar fechado na carga
   inválida. Criar memória gerenciada privada apenas se não há alvos explícitos.
2. Journal Knowledge em tabela aditiva: payload privado por sessão/fingerprint,
   pending/published/done/failed, proposta associada, tentativas/backoff. Publicar
   automaticamente Markdown proposed em Mercury/ usando journal/undo existente;
   aceitar/revisar explicitamente pode validar, nunca sobrescrever humano.
3. Worker único do servidor: canal limitado, persistência, polling 30s, debounce
   5s para revisões do vault. Captura usa mesma rotina validada da API, com
   proveniência observada; modelo usa regras Codex/Claude e chamadas limitadas.
   Orçamento diário persistido para automação (reservas conservadoras por chamada
   estruturada, incluindo fallback). Erros sanitizados e tentativas limitadas.
4. Compartilhar SERIAL com operações Knowledge manuais; configurações/cotas
   atualizadas por reload. Drop/disable cancela o modelo e polling sem bloquear
   UI. Handoff desativa workers antigos antes de assumir escrita no novo servidor.
5. Conectar fim Claude/Codex à captura separada de Learn; reutilizar checagens de
   sessão/transcrição corrente e exclusão de Cron/Learn. Aprendizado a cada 12
   turnos continua somente Learn. Reinício retoma journal, sem scan de históricos.
6. Status/CLI/TUI expõem enablement, bloqueio externo, contagens, erros e progresso.
   Atualizar default config, README EN/PT, guias e constituição com decisão nova.
7. Revisão consolidada, testes de config/worker/journal/fluxo com modelos simulados,
   suite workspace (mudanças transversais), manutenção, build/Clippy/fmt/schema.
   Smoke com CLI real sobre corpus sintético, sem conversas privadas nos testes.
8. Backup pessoal privado, ativar config explícita no vault atual, instalar novo
   binário dev e live-handoff com inventário antes/depois. Validar status e ciclo
   controlado. Commit/push na main; se transporte Git bloqueado, API oficial
   preservando hashes/histórico e fast-forward, como na entrega anterior.

Gates externos/coverage quantitativa dispensados pela constituição; revisão
local/testes permanecem exigidos. Nenhum serviço remoto novo além dos CLIs
explicitamente autorizados. Sem mudança de métodos API/schema se JSON genérico
existente comportar status novo; validar contratos mesmo assim.

## Ajuste por descoberta antes da implementação do revisor roteado

Claude CLI está instalado mas não autenticado (consulta somente leitura). Para
respeitar a preferência prévia Codex/Claude, Learn usará a interface de revisor
comum e NativeModel, com roteamento/fallback, stdin limitado e orçamento diário
compartilhado. O revisor Claude isolado permanece como implementação compatível
e para testes existentes. Custos não reportados por Codex serão None, nunca zero.
Entrada preserva snapshots completos dos alvos; conversa pode ser amostrada com
truncated=true para respeitar contexto. Memória padrão tem limite 8 KiB.
LearnRuntimeInfo opcional amplia o status tipado; regenerar schema/API e validar
compatibilidade. Essa mudança implementa a preferência já autorizada e não exige
novo serviço, modelo local ou alteração de política externa.
