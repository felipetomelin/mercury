# Fase 2 — Arquitetura de Usage (gauges por pane + aba Usage)

Status: design validado contra o código atual (2026-10-06). Fundação implementada:
crate `crates/mercury-usage` (tipos, parsers, janelas, orçamentos). Passos 1-7 da
integração servidor/cliente feitos (gauges, aba Usage, config `[usage]`, orçamentos,
alertas, statusline bridge, ledger de custo e Codex app-server). Auditoria final
da fase pendente; o scan histórico D9 está implementado.

## 1. Como a UI funciona hoje (mapa para quem for implementar)

| Peça | Onde | Observação |
|---|---|---|
| Superfície de panes | servidor: `src/ui/tab_surface.rs`, `src/ui/panes.rs` | renderizada no servidor e enviada como frame de células |
| Chrome (sidebar, tab bar, mode bar, overlays, toasts) | cliente: `src/client/shell/` | desenhado localmente por cima do frame em `composition.rs::compose` |
| Estado do chrome | `ClientShellSnapshot` em `src/protocol/wire.rs` | JSON; montado em `src/server/client_shell.rs::snapshot_with_completions` |
| Quando o snapshot é reenviado | `src/server/headless/render.rs` (~l.555) | refeito a cada render e comparado com `PartialEq`; muda → nova `revision` (e a superfície é re-renderizada junto) |
| Compatibilidade do snapshot | `src/protocol/endpoint.rs` | regra do contrato: campo novo precisa de `#[serde(default)]`; enum novo precisa de fallback `Unknown` |
| Linhas de agente na sidebar | `src/client/shell/agent_sidebar.rs::agent_row` → `crate::ui::sidebar_agent_rows` (`src/ui/sidebar/tokens.rs`) → `resolved_token_spans` (`src/ui/sidebar.rs`) | layout configurável por tokens: `[ui.sidebar.agents] rows = [[...]]`, `rows_by_agent` (`src/config/sidebar.rs`, `AgentSidebarToken`). Linha cujos tokens resolvem todos para `None` some sozinha |
| Overlays modais | `ClientShellOverlay` (`src/client/shell/state.rs`), render em `render.rs`, input em `overlay_input.rs` | centrados/modais |
| Keybindings | `KeysConfig` (`src/config/model.rs`) → `Keybinds` (`src/config/keybinds.rs`) → `KeybindAction` (`src/input/keybindings.rs`) → `src/client/shell/actions.rs`; ajuda em `src/input/keybind_help.rs` | `prefix+u` está livre |
| Tema | `Palette` (`src/app/state.rs`) | cores semânticas `green/yellow/red/peach/overlay0…` |
| Timer de repaint do cliente | `ClientShellState::timer_delay` (`src/client/shell/state.rs`) | agrega deadlines; é onde entra o "tick" de 1s da aba Usage |
| Sessão do agente por pane | hooks `src/integration/assets/{claude,codex}/mercury-agent-state.sh` → `pane.report_agent_session` (`session_id` + `transcript_path`) → `TerminalState.persisted_agent_session` / `hook_authority` (`src/terminal/state.rs`) | para claude/codex só o **id** é guardado (`agent_resume::session_ref_from_report` descarta o path) |
| Trabalho em background | padrão `src/app/git_refresh.rs` | `std::thread::spawn` + `event_tx.blocking_send(AppEvent::…)`; nada de I/O no caminho de render (há teste de arquitetura: `scripts/test_ui_hot_path_architecture.py`) |

## 2. Decisões

**D1 — Usage roda no servidor.** O servidor é dono dos panes e sobrevive a
detach; o cliente só recebe números prontos. Novo `src/app/usage.rs` com um
`UsageService` dentro de `App`, alimentado por uma thread de leitura (padrão
`git_refresh.rs`) que devolve `AppEvent::UsageRefreshed { sessions, limits }`.
Toda a lógica de formato/janela vive no crate `mercury-usage` (sem tipos do
servidor), testável isoladamente.

**D2 — Pane → arquivo de usage via hooks, nunca scrape.** O id que os hooks já
reportam + o cwd do pane resolvem o arquivo: `mercury_usage::locate::{claude_transcript_path,
codex_rollout_path}` (com fallback por varredura). Para Claude também somamos
`<sessão>/subagents/*.jsonl` (tokens, não contexto). Pane sem hook instalado →
sem gauge ("n/a"), nunca número inventado. Não mexemos em `session_ref_from_report`
(isso é semântica de resume); o `UsageService` lê o id de `persisted_agent_session`.

**D3 — Detecção de mudança por polling de `len/mtime`, sem dependência nova.**
O PLAN falava em FSEvents; para não adicionar o crate `notify` agora, o serviço
faz `stat` dos arquivos mapeados: 1 s para panes `working`, 10 s idle, nada se
não houver agentes. Gatilho imediato em `AgentSessionReported`, `HookStateReported`
e transição `working → idle/done` (releitura final do turno, PLAN 3.1).
`JsonlTail` lê só o que foi anexado. FSEvents fica como otimização se o custo
aparecer.

**D4 — Wire: campos opcionais no snapshot, fora da API pública por enquanto.**
- `ClientShellAgent.usage: Option<ClientShellAgentUsage>` (`#[serde(default)]`):
  `model`, `context_permille: Option<u16>`, `context_exact: bool`,
  `context_window: Option<u64>`, `tokens_in/out/cache_read/cache_write: u64`,
  `cost_micros_usd: Option<u64>`, `updated_at_unix_ms: Option<i64>`.
  Inteiros de propósito: os tipos do snapshot derivam `Eq`, e arredondar
  (permille, micro-USD) evita reenviar snapshot por variações irrelevantes.
- `ClientShellSnapshot.usage: Option<ClientShellUsageSummary>` (`#[serde(default)]`)
  com `providers` (limites reportados ou ausentes) e `budgets` (resultado de
  `Budget::evaluate`: nome, escopo, usado/limite, nível, `resets_at`,
  `forecast`, `partial`). A idade dos dados ("há 12s") é calculada no cliente.
- Throttle: no máximo 1 atualização/s por sessão no servidor (cada snapshot novo
  re-renderiza a superfície).
- Não entra em `src/api/schema` agora → não mexe em
  `docs/next/api/mercury-api.schema.json` nem nos digests de
  `tests/fixtures/endpoint-method-shapes-v1.json`. A API `usage.*` vem quando o
  roteador (Fase 5) precisar de headroom. Exceção: o statusline bridge (D8)
  precisa de um método novo.
- Custo de mudança conhecido: os testes que constroem `ClientShellAgent { .. }`
  literal (`src/client/shell/{notifications,endpoint_agent_state}.rs`,
  `src/client/shell/tests/{copy,mobile,agents_worktrees_notifications}.rs`)
  ganham `usage: None`.

**D5 — Gauge por pane = tokens novos da sidebar de agentes.** Reusa o motor de
tokens existente em vez de inventar um widget paralelo:

| Token | Render | Fonte |
|---|---|---|
| `context` | `▰▰▰▱▱ 43%` (5 células + %), verde < 60%, amarelo < `compact_at`, vermelho ≥ `compact_at`; `~` antes do % quando estimado | `context_permille`, `context_exact` |
| `context_pct` | `43%` | idem |
| `tokens` | `↑1.2k ↓22k ⟳97%` (in, out, cache hit) | `tokens_*` |
| `cost` | `$3.41` | `cost_micros_usd` |
| `model` | `opus-5.5` (prefixo `claude-` removido) | `model` |

- `AgentSidebarToken` ganha as variantes (+ nomes no parser de `src/config/sidebar.rs`),
  `AgentTokenContext` ganha `usage: Option<&ClientShellAgentUsage>`,
  `ResolvedTokenKind` ganha `ContextGauge { permille, exact }` (largura fixa em
  `resolved_token_spans`) e os demais viram texto.
- Default do Mercury em `AgentsSidebarConfig::default()` ganha a 3ª linha
  `["model", "context", "cost"]`; como tokens sem valor resolvem para `None`,
  a linha não aparece para panes sem usage — zero regressão visual para shells comuns.
- Alerta de contexto: `[usage] compact_at = 80` (por agente via
  `[usage.agents.<nome>] compact_at`); ao cruzar, toast "contexto 82% — considere /compact"
  usando o caminho de notificação existente (`src/client/shell/notifications.rs`).
  **Implementado no passo 3 com `compact_at` hardcoded em 80%** (constante
  `CONTEXT_ALERT_PERMILLE`, `src/client/shell/endpoint_agent_state.rs`; o config real é
  passo 5). Plugado em `EndpointAgentPresentation::context_alerts`, chamado de
  `ClientShellState::cache_endpoint_snapshot_with_surface`
  (`src/client/shell/endpoints.rs`) — dedupe por pane via `HashSet`, só refaz o alerta após
  cair abaixo do limiar. Cache hit rate do token `tokens` = `cache_read / (cache_read +
  tokens_in)`. Detalhe completo e desvios (efeitos de som/Terminal/System do toast não são
  propagados ainda) em `PLAN.md`, Fase 2 → "Passo 3 feito nesta rodada".
- Gauge na borda do pane fica para depois: com um pane só as bordas costumam estar
  ocultas e a borda é render do servidor (`src/ui/panes.rs`).
- Cores: `palette.green/yellow/red` agora; Fase 3 cria `gauge_ok/warn/crit` no `Palette`.

**D6 — Aba Usage = "view" do cliente que substitui a área de panes.**
- `ClientShellView { Terminal, Usage }` em `ClientShellState` (futuramente
  `Crons`, `History`, `Learn` — PLAN seção 3). Alterna com `prefix+u`
  (`KeysConfig.usage`, `KeybindAction::Usage`) e clique num chip `usage` no lado
  direito da tab bar (`src/client/shell/tabs.rs::render_tab_bar_status`).
- Em `compose`, depois do blit da superfície: se `view == Usage`, desenha o painel em
  `layout.pane_surface`, cobre `occlusion` (gráficos kitty) e limpa
  `hits.panes/pane_splits/popup` — exatamente o padrão já usado por
  `render_mobile_switcher`. Sidebar e tab bar continuam vivas.
- Input com a view aberta: `Esc`/`q`/`prefix+u` volta; `j/k/↑/↓` rolam; `Enter`
  numa linha de agente foca o pane e volta para Terminal. Atalhos com prefix continuam.
- Por que não um overlay: overlays são modais e centrados; a aba é um modo de
  navegação persistente que convive com a sidebar.
- Repaint: enquanto `view == Usage`, um deadline de 1 s em `timer_delay` atualiza
  as idades; fora dela, nenhum custo.

Layout (larguras se adaptam; tudo truncado com `truncate_end`):

```
 Usage                                                   dados de há 12s
 ─ Provedor ──────────────────────────────────────────────────────────
 claude   5h        n/a  (plano não reporta rate_limits)
 claude   semanal   n/a
 codex    primary   n/a  · créditos: ok · plano business
 ─ Orçamentos ────────────────────────────────────────────────────────
 claude-mensal  ▰▰▰▱▱▱▱▱▱▱  $250 / $1000   25%  reset 01/11  estoura ~21/10
 reviewer-3h    ▰▰▰▰▰▰▰▱▱▱  1.5M / 2M tok  75% ⚠
 ─ Agentes ───────────────────────────────────────────────────────────
 ● mercury · opus-5.5    ctx ▰▰▰▱▱ 43%   ↑1.2k ↓22k ⟳97%   $3.41
 ● api · gpt-5.6         ctx ▰▰▰▰▱ 63%   ↑1.3M ↓137k ⟳96%  n/a
```

**D7 — Config.** Em `config.toml`, parse no crate principal (`src/config/model.rs`,
com `serde_ignored` como o resto); o shape de cada orçamento é
`mercury_usage::BudgetConfig`:

```toml
[usage]
enabled = true
poll_active_ms = 1000
poll_idle_ms = 10000
compact_at = 80

[[usage.budget]]
name = "claude-mensal"
scope = { provider = "claude" }          # ou { workspace = "x" } / { agent = "reviewer" } / omitido = global
window = { kind = "calendar", period = "monthly", reset_day = 1, utc_offset = "-03:00" }
limit = { usd = 1000 }
warn_at = [70, 90]

[[usage.budget]]
name = "reviewer-3h"
scope = { agent = "reviewer" }
window = { kind = "rolling", hours = 3 }
limit = { tokens = 2_000_000 }           # count = ["input","output","cache_read","cache_write"] opcional
on_exceed = "route"                      # notify | route | pause

[[usage.budget]]
name = "claude-5h"
scope = { provider = "claude" }
window = { kind = "provider", key = "five_hour" }   # limite em percent, default 100
```

Fuso: só offset fixo (`utc_offset`). `tz = "America/Sao_Paulo"` é rejeitado com
mensagem pedindo `utc_offset` — evita trazer base tz agora; São Paulo não tem
horário de verão desde 2019, então `-03:00` é exato.

**D8 — Origem de cada número (sem tabela de preços).**

| Número | Claude | Codex |
|---|---|---|
| tokens | transcript JSONL (exato, dedupe `message.id`+`requestId`) | rollout `token_count` (exato, deltas do total cumulativo) |
| % contexto | statusline (exato); sem statusline: estimativa do último request (`~`) com janela inferida (200k; 1M se modelo `[1m]` ou prompt > 200k) | `last_token_usage.total_tokens / model_context_window` (os números da TUI do Codex) |
| custo | statusline `cost.total_cost_usd` | `codex app-server` (`estimatedUsageUsdMicros`, quando disponível) |
| janelas do provedor | statusline `rate_limits` (só Pro/Max/gateway) | rollout `rate_limits.primary/secondary` (`null` no business) + `credits` |

Orçamentos em USD só somam custo reportado; se há tokens no escopo vindos de
sessão sem fonte de custo, o status sai `partial` (PLAN seção 6).

**D9 — Persistência.** Tokens de qualquer janela são recomputáveis: no startup a
thread varre transcripts/rollouts com `mtime` ≥ início da maior janela
configurada (limitado a 35 dias) — inclusive sessões rodadas fora do Mercury,
atribuídas por provedor e por workspace via cwd. **Atualização (passo 1,
2026-10-05):** a varredura global de sessões *não* anexadas a um pane
(necessária para orçamentos agregados) foi adiada para o passo 5 — hoje não
há nenhum consumidor para esses dados (o `UsageService` só mantém um mapa
`TerminalId → SessionUsage`, D1), então armazená-los seria inventar a
arquitetura de orçamentos sem spec. **Atualização (2026-10-06):** a varredura
foi especificada em `docs/dev/usage-history-scan-spec.md` e implementada em
background, com deduplicação frente aos tails ativos. O que o passo 1 implementou: o primeiro
poll de qualquer sessão recém-anexada já é uma leitura completa do arquivo
(o `JsonlTail` começa no offset 0), cobrindo a necessidade real do gauge por
pane. Detalhe em `PLAN.md` (Fase 2, "Passos 1-2 feitos nesta rodada").
Custo **não** é recomputável
(não está no JSONL), então cada delta de `total_cost_usd` vindo do statusline é
anexado em `~/.local/share/mercury/usage/cost-ledger.jsonl`
(`{at, provider, session_id, cwd, cost_delta_usd}`), relido no startup.

**D10 — Statusline bridge.** `mercury statusline`: lê o JSON do stdin, envia ao
servidor via método novo `usage.report_statusline { pane_id (de MERCURY_PANE_ID), payload }`,
e executa o statusLine anterior do usuário com o mesmo stdin, repassando a saída
(saída idêntica). Instalação opt-in em `mercury integration install claude`
com backup do `settings.json` (padrão de `src/integration/claude_settings.rs`).
Esse método entra no schema da API (regenerar `docs/next/api/mercury-api.schema.json`
e conferir os digests de contrato).

## 3. Fluxo

```
hooks (session id) ─┐
statusline bridge ──┼─► App (servidor) ──► UsageService ──spawn──► thread de leitura
                    │        ▲                                      (JsonlTail + parsers
                    │        └──── AppEvent::UsageRefreshed ◄────── do mercury-usage)
                    │
                    └► client_shell::snapshot ─► ClientShellAgent.usage / snapshot.usage
                                                   │ (JSON, diff por PartialEq)
                                                   ▼
                              cliente: tokens da sidebar (gauge) + view Usage
```

## 4. Fundação já implementada — `crates/mercury-usage`

| Módulo | Conteúdo |
|---|---|
| `model` | `Provider`, `TokenCounts` (input não-cacheado normalizado entre provedores), `UsageEvent`, `ContextUsage` (`permille()`), `SessionUsage`, `ProviderLimit(s)`, `Credits` |
| `jsonl` | `JsonlTail`: leitura incremental, segura linha parcial, detecta truncamento (`reset`) |
| `claude` | `ClaudeTranscript` (dedupe last-wins, ignora `<synthetic>`, sidechain não conta p/ contexto), `merge_session` (subagentes), `context_window_for_model` |
| `codex` | `CodexRollout` (session_meta/turn_context/token_count, deltas, reset de contador, `rate_limits` nulos → lista vazia + créditos) |
| `statusline` | `StatuslineSnapshot::parse` (contexto exato, custo, `rate_limits` quando houver, cache hit), `apply_to(SessionUsage)` |
| `window` | `UsageWindow::{Rolling, Calendar, Provider}`, `CalendarWindow` diário/semanal/mensal com offset fixo, `bounds`, `next_reset` |
| `budget` | `Scope`, `Limit`, `Budget::evaluate` → `BudgetStatus` (nível, `warn_threshold` p/ dedupe de alerta, `partial`, previsão linear de estouro), `BudgetConfig` (TOML acima) |
| `locate` | homes (`CLAUDE_CONFIG_DIR`/`CODEX_HOME`), slug de projeto do Claude, busca de transcript/subagentes/rollout, validação de id contra path traversal |

Fixtures anonimizadas em `crates/mercury-usage/tests/fixtures/` (shape real,
valores sintéticos). Validado também contra os arquivos reais desta máquina
(teste descartável, não versionado): sessão Claude com 428 requests únicos e
Codex com totais idênticos ao último `total_token_usage` do rollout.

## 5. Plano de implementação (ordem, cada passo verificável)

1. **Server: `UsageService`** (`mercury-dev`) — `src/app/usage.rs`: mapa
   `TerminalId → SessionUsage` + tails por arquivo; thread de leitura; polling
   adaptativo (D3); `AppEvent::UsageRefreshed`; scan de startup (D9, só tokens).
   Dependência `mercury-usage = { path = "crates/mercury-usage" }` no `Cargo.toml` raiz.
   Testes: unit do serviço com dirs temporários.
2. **Wire** (`mercury-dev`) — `ClientShellAgentUsage`/`ClientShellUsageSummary`
   em `src/protocol/wire.rs`, preenchidos em `src/server/client_shell.rs`; teste
   de tolerância no estilo `snapshot_json_tolerates_future_fields_and_command_actions`.
3. **Gauges na sidebar** (`mercury-dev`) — tokens do D5 + default row + render
   do `ContextGauge`; testes em `src/ui/sidebar/tokens.rs` e
   `src/client/shell/tests/`. **Primeira entrega visível.**
4. **View Usage** (`mercury-dev`, revisão `mercury-architect`) — D6 + keybind
   `prefix+u` + help + chip na tab bar; teste de composição como os de
   `src/client/shell/tests/`. **Feito** (`src/client/shell/usage_view.rs`,
   `src/client/shell/tests/usage_view.rs`). Desvios do D6 original:
   - O chip da tab bar é **sempre** reservado (mesmo sem segmentos em
     `tab_bar_right`), não só quando há conteúdo — um chip clicável permanente
     é mais previsível do que um que aparece/desaparece. Encolhe a faixa de
     tabs em 8 colunas em qualquer layout (ajustado em
     `chrome_context::focused_last_overflow_tab_shows_its_full_label`).
   - O `reset <data>` da seção Provedor é renderizado como duração relativa
     ("em 3h"), igual ao cabeçalho de idade, em vez de data de calendário
     (`01/11` do mockup) — evita puxar formatação de fuso/calendário para um
     número que hoje só existe para janelas `provider`/`rolling` (sem reset
     fixo de calendário); quando os orçamentos de janela `calendar` (passo 5)
     entrarem, essa seção pode reaproveitar a mesma função ou ganhar uma
     formatação de data própria.
   - Navegação por teclado usa um cursor sobre as linhas de agente (`j/k`/
     setas movem o cursor, a lista rola para mantê-lo visível) em vez de um
     scroll "cego"; o D6 descrevia só "rolar a lista", mas sem um cursor não
     haveria uma linha definida para `Enter` focar.
5. **Config `[usage]` + orçamentos + alertas** (`mercury-dev`) — D7, `warn_at` via
   toasts, `compact_at`; agendar recomputação em `next_reset` (virada de janela).
   **Feito** (`src/config/usage.rs`, `src/app/usage.rs`,
   `src/client/shell/endpoint_agent_state.rs`, `src/client/shell/usage_view.rs`).
   Desvios do passo 5 original, detalhados em `PLAN.md`:
   - Na entrega original do passo 5, a avaliação usava só as sessões rastreadas.
     A varredura histórica D9 foi implementada depois, com orçamento mensal
     agregado através de reinícios e deduplicação frente a panes ativos.
   - Custo é sempre ausente nos registros de orçamento (o ledger de custo é D9/D10,
     passo 6): um orçamento em USD nunca soma nada ainda, e sai `partial = true`
     sempre que há tokens em escopo — correto dado que nenhuma sessão rastreada tem
     fonte de custo até o statusline bridge existir.
   - `warn_at` dispara o toast **no cliente**, não no servidor: o servidor só expõe
     `level`/`warn_threshold` já calculados no snapshot; `EndpointAgentPresentation::
     budget_alerts` (ao lado de `context_alerts`) é quem decide quando notificar,
     com dedupe por nome do orçamento + limiar (mesmo princípio do alerta de
     contexto do passo 3, adaptado porque um orçamento pode cruzar vários limiares).
   - `compact_at`/`[usage.agents.<id>].compact_at` substituem a constante
     `CONTEXT_ALERT_PERMILLE` do passo 3; o valor resolvido chega ao cliente via
     `ClientShellConfig.usage` (mirror do `[usage]` do config local, mesmo padrão de
     `ui.*`), não via snapshot do servidor — o alerta de contexto já era 100%
     client-side desde o passo 3.
   - `on_exceed = "route"/"pause"` faz parse e nada mais: ambos continuam no-op até
     o roteador da Fase 5 existir, documentado em código para não falhar
     silenciosamente nem fingir que a ação roda.
6. **Statusline bridge + ledger de custo** — D9/D10. **Feito.** O comando
   `mercury statusline` encadeia o comando anterior via arquivo protegido;
   instalação opt-in com backup e restauração no uninstall. O método
   `usage.report_statusline` está no schema público. O servidor confere o ID
   da sessão do hook antes de aplicar o relatório. O ledger persiste deltas
   cumulativos e os relê no restart.
7. **Codex app-server** (custo/limites ao vivo) — **feito**; implementação
   especificada em `docs/dev/codex-app-server-usage-spec.md`. Schema v2 gerado localmente
   em 2026-10-06 com Codex CLI 0.160.1; leitura real validada fora do sandbox.
   Métodos confirmados: `account/rateLimits/read` (snapshot),
   `account/usage/read` com `threadId` opcional (`threadUsage.estimatedUsageUsdMicros`
   pode ser `null`), notificações `account/rateLimits/updated` (esparsas,
   exigem merge ou refetch) e `thread/tokenUsage/updated`. Na conta business
   testada, `primary`/`secondary` foram `null`, enquanto `credits`/`planType`
   vieram preenchidos; consulta sem `threadId` retornou `threadUsage: null`.
   Não inferir USD de `individualLimit` (strings sem unidade explícita no
   schema) e não transformar `null` em zero. Integração pendente.
