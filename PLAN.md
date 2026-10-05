# Mercury — Plano e Levantamento

> Controlador de agentes de IA no terminal (Claude Code + Codex CLI), inspirado no Herdr,
> com crons (estilo Hermes), roteamento, auto improvement, temas e painel de usage/contexto.

Status: **Fase 0 em andamento** — base Herdr v0.9.3 copiada, licenças removidas, ainda não compilada. Data: 2026-10-05.

### Estado atual / próximos passos
- [x] Base Herdr v0.9.3 na raiz; `CLAUDE.md`/`AGENTS.md` originais em `docs/herdr-base/`.
- [x] Arquivos de licença removidos (`Cargo.toml` e `.gitattributes` ajustados).
- [x] Agentes de desenvolvimento em `.claude/agents/` (cópia em `docs/dev/agents/`).
- [x] Toolchain: Rust 1.96.1 via `/opt/homebrew/opt/rustup/bin/rustup` + `zig@0.16` (Zig 0.17 **não** serve: versão minor exata).
- [ ] `git init` + commit base (o sandbox bloqueia `.git` — rodar pelo usuário).
- [ ] Fase 0: build com `PATH=/opt/homebrew/opt/rustup/bin:$PATH ZIG=/opt/homebrew/opt/zig@0.16/bin/zig CARGO_HOME=$PWD/.cache/cargo ZIG_GLOBAL_CACHE_DIR=$PWD/.cache/zig cargo build` (o sandbox bloqueia escrita em `~/.cargo` e no cache global do Zig); o zig baixa 16 deps de `deps.files.ghostty.org`/`codeberg.org` — pode esbarrar no proxy.
- [ ] Fase 0: spike dos leitores de usage (statusline bridge, JSONL do Claude, rollouts do Codex, `codex app-server`).

---

## 1. Levantamento

### 1.1 Herdr (referência principal)

| Item | Dado |
|---|---|
| Repo | https://github.com/herdrdev/herdr (local: v0.8.2, upstream: v0.9.3) |
| Licença | Apache-2.0 no upstream — **arquivos de licença removidos da base do Mercury** (decisão do usuário, uso não comercial) |
| Stack | Rust · ratatui 0.30 · crossterm · tokio · portable-pty · interprocess (unix socket) · serde/toml/schemars · **libghostty-vt** vendorizado (emulação de terminal) |
| Tamanho | ~11 MB de Rust, ~3.4k arquivos, ~91 contribuidores, 42k★ |
| Cadência | estável a cada 1–2 semanas + preview quase diário |
| Contribuição | **PRs não solicitados são fechados** → fork seria hard fork |
| Arquitetura | servidor headless dono dos PTYs + clientes TUI que fazem attach/detach; sessões nomeadas; `--remote` via SSH |
| API | socket JSON em `~/.config/herdr/herdr.sock`, 117 métodos (`workspace.*`, `tab.*`, `pane.*`, `agent.prompt/wait/read`, `events.subscribe`, `pane.report_metadata`, `plugin.*`) |
| Estado de agente | `idle / working / blocked / done / unknown` — para Claude/Codex via **manifests TOML de screen-scrape** (`src/detect/manifests/claude.toml`, `codex.toml`) |
| Persistência | `~/.config/herdr/session.json` v3 (workspaces → tabs → layout tree → panes com cwd) + resume nativo (`claude --resume`) + replay de scrollback |
| Temas | 11 built-in (catppuccin, gruvbox, tokyo-night, dracula, nord, one-dark, solarized, kanagawa, rose-pine, vesper, terminal), auto light/dark, `[theme.custom]` por token |
| Outros | worktrees, notificações (toast/OS/som), keybindings com prefix, custom commands (popup/pane/shell), plugins (`herdr-plugin.toml`), skill para agentes |
| **Não tem** | crons, usage/tokens/custo, % de contexto, rate limits, roteamento automático, busca em histórico de chats, auto improvement |

### 1.2 Hermes Agent (crons, roteamento, auto improvement)

Repo: https://github.com/NousResearch/hermes-agent — **MIT**, Python 3.11+.

**Crons**
- Agenda: one-shot relativo (`30m`), intervalo (`every 2h`), cron 5 campos (`0 9 * * *`), ISO timestamp; também por linguagem natural (agente chama tool `cronjob_manage`).
- Storage: `~/.hermes/cron/jobs.json` (escrita atômica) + ledger SQLite `executions.db` + `output/`.
- Campos: `id, name, prompt, schedule{kind,expr,display}, skills[], deliver, repeat{times,completed}, state, enabled, next_run_at, last_run_at, last_status, model/provider, workdir, script, no_agent`.
- Execução: tick a cada 60s com lock; cada job roda em **sessão nova sem histórico**; `next_run_at` avança **antes** do dispatch (no máximo uma execução por ocorrência); `no_agent` = só script.
- Marcadores: `[SILENT]` (não entrega), `[CRON_FAILURE]` (marca falha).
- Falhas: `failure_streak`, agrupamento de erros em "incidents" (alerta 1x, lembra em 6h, `ack`), `cron doctor`, retry automático se a falha foi de rede antes da chamada ao modelo.
- Cron não pode criar cron (evita loop) e não roda auto-review.

**Roteamento**
- Não há "smart router" ativo; o roteamento é por **papéis** (modelo principal, delegação, tarefas auxiliares, pin por job) + **cadeia de fallback** (429/5xx/401/403 → próximo provider; respeita `resets_at` de rate limit; volta ao primário no turno seguinte).
- Subagentes: contexto novo, resumo volta ao pai, paralelo (máx. 10), opcionalmente 1 git worktree por filho.
- Referência externa útil: `claude-code-router` (proxy que roteia requests do Claude Code por papel).

**Auto improvement**
- Memória: `MEMORY.md` (notas do agente) + `USER.md` (perfil), com limite de caracteres, injetadas como snapshot congelado no início da sessão.
- Nudges: a cada N turnos (default 10) um **agente de revisão em background** relê o transcript e decide se grava memória ou cria/edita skill (`~/.hermes/skills/<nome>/SKILL.md`). Prioriza corrigir skill existente a criar nova; "lições, não logs".
- `write_approval: true` → mudanças ficam pendentes para `approve/reject/diff`.
- Curator: arquiva skills sem uso (stale 14d, archive 30d), mescla duplicadas, com backup/rollback.
- Busca: SQLite **FTS5** em todas as sessões (`session_search`).

### 1.3 Fontes de usage / contexto (verificado nesta máquina)

Versões: Claude Code 2.1.285 · codex-cli 0.160.0.

| Dado | Claude Code | Codex |
|---|---|---|
| Sessões vivas | `~/.claude/sessions/<pid>.json` (`sessionId, cwd, status busy/idle, startedAt`) + checar pid | `pgrep codex` + rollouts modificados recentemente |
| Tokens | `~/.claude/projects/<slug>/<sessionId>.jsonl` → `message.usage` (dedupe por `message.id`+`requestId`, ignorar `<synthetic>`) | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` → evento `token_count` (`total_token_usage`, `last_token_usage`) |
| % contexto | **exato só via statusline** (`context_window.used_percentage`, `context_window_size`); estimativa pelo JSONL | `last_token_usage.input_tokens / model_context_window` |
| Janelas por hora (5h/semana) | **só via statusline** (`rate_limits.five_hour/seven_day` → `used_percentage`, `resets_at`), só Pro/Max | `rate_limits.primary/secondary` (`used_percent`, `window_minutes`, `resets_at`) — **no seu plano business vêm `null`** |
| Janela de gasto (diária/semanal/**mensal**) | statusline `rate_limits.spend_limit` → `used_percentage`, `resets_at`, `used_usd`, `limit_usd`, `period` (`daily/weekly/monthly`) — via Claude apps gateway; os campos em USD atualizam ~a cada 5 min | `credits` (`hasCredits`, `unlimited`, `balance`) + `rate_limit_reached_type` (ex.: `workspace_member_usage_limit_reached`) |
| Custo (USD) | statusline `cost.total_cost_usd` por sessão (calculado pelo próprio Claude Code) | app-server `estimatedUsageUsdMicros` / `estimatedUsageCreditsMicros` por thread (calculado pela OpenAI) |
| Push/eventos | hooks (`SessionStart`, `Stop`, `SessionEnd`, `PreCompact`) | `codex app-server` JSON-RPC (`account/rateLimits/read`, `thread/tokenUsage/updated`) — experimental |

Observações:
- **Sem tabela de preços própria**: todo custo vem do que o Claude Code e o Codex já reportam. Sem fonte de custo → o orçamento daquela janela é medido em tokens.
- O `notify` do Codex já está em uso (Codex Computer Use) e só aceita um comando → não usar.
- Nunca fazer scrape da TUI para usage.

---

## 2. Decisão: fork × do zero

| Opção | Prós | Contras |
|---|---|---|
| **A. Plugin Herdr + daemon `mercuryd`** | entrega rápida; reaproveita 100% do Herdr | UI limitada a panes/overlays e tokens de sidebar; sem aba de crons nativa; depende do roadmap alheio |
| **B. Hard fork do Herdr (tag v0.9.3)** | PTY, ghostty-vt, persistência, detecção, temas, remote já prontos; UI nativa (gauges, aba de crons) | 11 MB de Rust para dominar; upstream rápido → rebase caro (mitigável) |
| **C. Do zero em Rust** | arquitetura 100% sua, código enxuto | refazer emulação de terminal, PTY, persistência, detecção — meses de trabalho só para chegar no Herdr atual |

**Decidido: B — Herdr como base.** Código copiado da tag `v0.9.3` para esta pasta e modificado livremente (sem obrigação de acompanhar o upstream). Features novas em crates próprios (`mercury-usage`, `mercury-cron`, `mercury-router`, `mercury-learn`, `mercury-history`) por organização, não por compatibilidade com upstream.

---

## 3. Arquitetura proposta

```
┌──────────────── mercury (cliente TUI) ─────────────────┐
│ Sidebar: workspaces/tabs/agentes + gauges ctx/limites  │
│ Abas: Terminal | Crons | Usage | History | Learn       │
└─────────────────────────┬──────────────────────────────┘
                          │ unix socket (API Herdr + mercury.*)
┌─────────────────────────▼──────────────────────────────┐
│ mercury server                                         │
│  ├─ núcleo herdr: PTYs, layout, sessões, detecção      │
│  ├─ mercury-usage   : watchers (FSEvents) JSONL/rollout│
│  │                    + statusline bridge + app-server │
│  ├─ mercury-cron    : scheduler, jobs.toml, ledger     │
│  ├─ mercury-router  : regras + headroom de limites     │
│  ├─ mercury-learn   : review em background, aprovações │
│  └─ mercury-history : SQLite FTS5 dos transcripts      │
└────────────────────────────────────────────────────────┘
Dados: ~/.config/mercury/config.toml · ~/.local/share/mercury/{mercury.db, cron/, learn/}
```

### 3.1 Modelo de usage: janelas, escopos e fetch

**Janela (`UsageWindow`)** — três tipos:
| Tipo | Exemplo | Reset |
|---|---|---|
| `rolling` (horas) | 5h do Claude, `window_minutes=300` do Codex, ou "100k tokens / 3h" definido pelo usuário | contínuo; ou `resets_at` do provedor |
| `calendar` | **US$ 1000/mês**, US$ 50/dia, 2M tokens/semana | dia do mês / dia da semana / hora + fuso configuráveis |
| `provider` | `five_hour`, `seven_day`, `spend_limit` (Claude), `primary/secondary/credits` (Codex) | `resets_at` reportado |

**Escopo** — cada janela se aplica a um nível:
`global` → `provedor` (claude/codex) → `workspace/projeto` → **`agente`** (um pane/sessão específica, ou um nome de agente como `reviewer`).

**Unidade do limite** — `usd` (do custo reportado), `tokens` (in/out/cache configurável), `percent` (só janelas do provedor).

Exemplo de config:
```toml
[[usage.budget]]
name = "claude-mensal"
scope = { provider = "claude" }
window = { kind = "calendar", period = "monthly", reset_day = 1, tz = "America/Sao_Paulo" }
limit = { usd = 1000 }
warn_at = [70, 90]

[[usage.budget]]
name = "reviewer-3h"
scope = { agent = "reviewer" }
window = { kind = "rolling", hours = 3 }
limit = { tokens = 2_000_000 }
on_exceed = "route"   # notify | route | pause
```

**Janela de contexto por agente** — cada pane mostra o % de contexto do seu próprio modelo (`context_window_size` no Claude, `model_context_window` no Codex; 200k/258k/1M conforme o modelo), com alerta configurável por agente (`compact_at = 80`).

**Quando dar fetch** — prioridade a push, pull só quando necessário:
| Gatilho | O que atualiza |
|---|---|
| Push contínuo: FSEvents nos JSONL/rollouts, statusline bridge, hooks (`Stop`, `SessionEnd`, `PreCompact`), notificações do app-server (`thread/tokenUsage/updated`, `account/rateLimits/updated`) | tokens, contexto, custo, limites |
| Mudança de estado do agente (`working → idle/done`) | releitura final do turno |
| Cruzou `resets_at` ou fronteira de calendário (virada do dia/mês) | zera/recalcula a janela e busca o valor novo no provedor |
| Dado mais velho que o TTL da fonte (ex.: `used_usd` do gateway ~5 min, limites do Codex ~60 s com agente ativo) | `account/rateLimits/read` / `account/usage/read` |
| **Antes de uma decisão de roteamento ou de disparar um cron** | fetch forçado se o dado tiver mais de N segundos |
| Startup / reattach | varredura completa |

Polling adaptativo: agentes ativos → TTL curto; tudo ocioso → nenhum polling (só os timers de `resets_at`). Toda leitura guarda `fetched_at` e a UI mostra a idade do dado.

**Statusline bridge** (Claude): binário `mercury statusline` lê o JSON do stdin, grava/manda para o socket, e **encadeia o statusline atual do usuário** (saída idêntica). Instalação opt-in via `mercury integration install claude` com backup do `settings.json`.

---

## 4. Features (priorizadas)

### P0 — Base (paridade com Herdr)
1. Rebrand: binário `mercury`, paths `~/.config/mercury`, socket, env `MERCURY_ENV`, skill própria.
2. Sessões persistentes, workspaces → tabs → panes, splits, zoom, detach/attach (herdado).
3. Histórico de espaços abertos e restauração com resume nativo (`claude --resume`, `codex resume`) (herdado + validar).
4. Detecção de estado Claude/Codex (herdado; reforçar com hooks).

### P1 — Usage & contexto (diferencial principal)
5. Gauge por pane/agente: `% contexto` do modelo daquele agente (barra), tokens in/out/cache, custo da sessão, modelo.
6. Painel **Usage** com todas as janelas da seção 3.1:
   - janelas por hora (5h e semanal do provedor, e janelas rolling definidas pelo usuário);
   - janelas de calendário (diária/semanal/**mensal**, ex.: US$ 1000/mês), com o `spend_limit` do gateway Claude quando existir;
   - orçamentos por agente, workspace e provedor;
   - burn rate e previsão de estouro ("no ritmo atual, US$ 1000 acaba dia 23").
   - "n/a"/créditos quando o provedor não reporta.
7. Alertas e ações por janela (`warn_at`, `on_exceed = notify | route | pause`); contexto > limiar do agente → sugerir `/compact`.
8. Estratégia de fetch da seção 3.1, com idade do dado visível ("há 12s").

### P1 — Temas
9. Manter os 11 temas + light/dark automático + `[theme.custom]`.
10. Tema "mercury" próprio; tokens novos para gauges (`gauge_ok/warn/crit`); picker com preview ao vivo; import de temas (ex.: base16/Ghostty).

### P2 — Crons (aba dedicada, modelo Hermes)
11. Definição: cron 5 campos, `every 2h`, `30m`, ISO; via aba, CLI (`mercury cron add/list/edit/pause/run/rm/runs`) ou linguagem natural (skill para o agente criar jobs).
12. Alvo de execução: `claude -p` / `codex exec --json` headless **ou** prompt num agente/pane existente (`agent.prompt`) **ou** script puro (`no_agent`).
13. Campos: workdir, agente/modelo, skills/prompt files, `repeat`, timeout, entrega.
    Destinos de entrega (decidido): **notificação local (toast/macOS) + log na aba Crons**, **pane/agente existente** (envia resultado ou continua a conversa), **Microsoft Teams** (Incoming Webhook / Workflows) e **e-mail** (SMTP). Múltiplos destinos por job.
14. Robustez: `next_run_at` antes do dispatch, ledger SQLite, `[SILENT]`/`[CRON_FAILURE]`, incidents, `cron doctor`, catch-up configurável ao religar.
15. Aba Crons: lista com próximo run, último status, logs do run, ações (run now/pause/edit).

### P2 — Roteamento
16. Regras declarativas em TOML: por padrão de prompt/tag/projeto → agente (claude|codex) + modelo + effort.
17. **Roteamento por headroom**: se Claude está > X% do limite 5h ou com contexto cheio, mandar para Codex (e vice-versa).
18. Fallback em erro/limite atingido; volta ao primário após `resets_at`.
19. Delegação: `mercury route "<tarefa>"` abre pane (opcionalmente em worktree), escolhe agente, envia prompt, acompanha estado e devolve resumo.

### P3 — Auto improvement
20. Gatilhos: fim de sessão (`Stop`/`SessionEnd`), a cada N turnos, ou cron semanal.
21. Revisão em background (`claude -p` com modelo barato) lendo transcript → propõe alterações em `CLAUDE.md`/`AGENTS.md`, skills (`.claude/skills`, `~/.codex/...`), memória e na própria config do Mercury (regras de roteamento, crons).
22. **Aplicação totalmente automática** (decidido), com salvaguardas obrigatórias:
    - snapshot/backup de cada arquivo antes de escrever (`~/.local/share/mercury/learn/backups/`);
    - ledger de auditoria (o quê, quando, por quê, sessão de origem) visível na aba Learn;
    - `mercury learn undo <id>` / rollback por item ou por data;
    - em repositórios git, alterações em `CLAUDE.md`/`AGENTS.md` ficam como mudança não commitada (nunca commit/push automático);
    - limites: tamanho máximo por arquivo de memória, lista de arquivos permitidos, kill-switch `learn.enabled = false`.
23. Curator: detectar skills/memórias sem uso e duplicadas.
24. Métricas de eficácia: custo/tokens por tarefa ao longo do tempo.

### P3 — Histórico de chats
25. Indexação SQLite FTS5 de todos os transcripts Claude + Codex; aba History com busca, filtro por projeto/data/agente, e "reabrir" (resume num pane novo).

---

## 5. Fases de execução

| Fase | Entrega | Critério de pronto |
|---|---|---|
| 0. Spike (1–2 dias) | build da base Herdr nesta pasta, statusline bridge, parsers JSONL/rollout e chamada ao app-server com dados reais desta máquina | números batem com `/cost`/`/status` do Claude e `/status` do Codex |
| 1. Fork + rebrand | `mercury` rodando com paridade Herdr, importando `session.json` do Herdr | sessão atual do usuário reabre igual no Mercury |
| 2. Usage | crate `mercury-usage` (janelas rolling/calendar/provider, escopos, fetch) + gauges + aba Usage + integração claude | contexto por agente, janelas por hora e orçamento mensal visíveis ao vivo; virada de janela recalcula sozinha |
| 3. Temas | tema mercury, tokens de gauge, picker com preview | troca de tema ao vivo sem reiniciar |
| 4. Crons | crate `mercury-cron` + aba + CLI | job agendado roda headless e entrega resultado; sobrevive a restart |
| 5. Roteamento | regras + headroom + `mercury route` | tarefa é enviada ao agente correto conforme regra/limite |
| 6. History | FTS5 + aba + resume | busca encontra chat antigo e reabre |
| 7. Auto improvement | review + aplicação automática com backup/auditoria/undo + curator | mudança aplicada após sessão, visível no ledger e reversível com `mercury learn undo` |

### 5.1 Roteamento de modelos no desenvolvimento

Para economizar tokens, cada tarefa vai para o modelo mais barato que dá conta (detalhes em `CLAUDE.md`):

| Modelo | Agente | Tarefas |
|---|---|---|
| Sonnet 5 | `mercury-mechanic` | rebrand, mover arquivos, boilerplate, docs/config, fixtures, rodar build/test, erros de compilação triviais |
| Sonnet 5.5 | `mercury-dev` | implementação padrão com spec clara (crates, parsers, views, API, testes) |
| Opus 5.5 | `mercury-architect` | arquitetura, bugs de PTY/render/concorrência, código sensível (escrita em configs, auto improvement), revisão de fim de fase |

Escala para o próximo nível só após falha ou quando a tarefa exigir decisão de design. Por fase: Fase 1 é quase toda Sonnet 5; Fases 2–6 Sonnet 5.5 com revisão Opus no fim; Fase 7 (auto improvement automático) tem design e revisão em Opus.

Cada fase: testes unitários nos crates novos, testes com fixtures JSONL reais (anonimizadas), e checagem manual na TUI.

---

## 6. Riscos

- **Formatos não documentados** (`~/.claude/sessions`, rollout do Codex, app-server experimental) podem mudar → parsers tolerantes + testes com fixtures por versão + detecção de versão.
- **Rate limits indisponíveis** (Codex business = `null`; Claude só Pro/Max ou gateway) → UI mostra "n/a"/créditos, nunca inventa número.
- **Custo sem fonte** (sessões antigas sem statusline, app-server indisponível) → orçamento em USD fica marcado como parcial; tokens continuam exatos.
- **Escrita em configs do usuário** (`settings.json`, `hooks.json`) → sempre opt-in, com backup e `uninstall`.
- **Custo do auto improvement** → modelo barato, limite de tokens, desligável.

---

## 7. Decisões

Tomadas em 2026-10-05:
1. **Abordagem: Herdr v0.9.3 como base**, modificado livremente; features em crates separados.
2. **Roteamento: os quatro modos**, todos no escopo — headroom de limite, regras por tarefa, fallback em erro e delegação entre agentes (P2, itens 16–19).
3. **Entrega de crons:** notificação local + log, pane/agente existente, Teams e e-mail.
4. **Auto improvement: totalmente automático**, com backup, auditoria e rollback (item 22).

5. **Repositório:** esta pasta, tudo local por enquanto (sem remote).
6. **Prioridade do roteamento:** fallback > headroom > regras > default.
7. **Usage:** janelas por hora, janelas de calendário (incl. mensal em USD), orçamento por agente, fetch orientado a eventos; sem tabela de preços própria.
8. **Licença:** arquivos de licença removidos.
9. **Desenvolvimento:** roteamento Sonnet 5 / Sonnet 5.5 / Opus 5.5 (seção 5.1).
