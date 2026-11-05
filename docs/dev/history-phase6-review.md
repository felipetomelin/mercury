# Fase 6 — revisão, testes e gate consolidado

Estado em 2026-10-06: **implementação local em verificação; gate externo FAIL/não verificável**. Sem commit.

## Escopo revisado

Crate `mercury-history`, API diferida `history.search`/`history.resolve`, CLI `mercury history`, aba History na shell cliente e schema público. Revisão de correção, segurança, performance, estados de erro e testes feita no grupo inteiro, sem delegação paralela.

## Achados e correções

1. Cópias de um mesmo ID em pastas diferentes podiam aparecer duas vezes ou resolver o caminho errado. Busca/lista/resolução agora escolhem a fonte mais recente por `(provider, session_id, modified_ns, path)`; teste de integração com cópias divergentes.
2. Um clique acidental numa linha iniciava o agente. Clique agora só seleciona; Enter reabre. Teste da hit map garante ausência de request no clique.
3. Cancelar a edição de busca deixava query e resultados inconsistentes. Esc restaura a query anterior sem enviar request; teste de regressão.
4. Shell de pane recém-criado pode não estar pronto para `agent.start`. A TUI agenda retries a cada ~100 ms com limite de 30 tentativas, sem `sleep` no loop do cliente e sem responder prompts de confiança. Pane permanece para inspeção se o início falhar; teste de busy/retry.
5. O CLI tinha a mesma corrida, mas falhava imediatamente. `history open` agora repete apenas `agent_pane_busy` por no máximo 3 segundos, reutilizando o pane e o mesmo `AgentStartParams`; teste cobre busy seguido de sucesso.
6. O limite `until=YYYY-MM-DD` excluía timestamps com horário no próprio dia final. Busca textual e listagem agora comparam a parte de data de modo inclusivo; teste cobre último segundo do dia e início do dia seguinte. A API também rejeita datas impossíveis e intervalo invertido antes de acessar homes, com dois testes de validação.
7. A execução paralela dos testes revelou colisão rara no nome de fixture temporária (mesmo timestamp). O helper agora usa sequência atômica e `create_dir` exclusivo com retry; suíte repetida com 8 PASS e 1 benchmark ignorado por padrão.
8. O CLI `history open` falhava com `pane_not_found` quando ligado a servidor headless sem workspaces. Agora cria um workspace no `cwd` validado da sessão e usa seu root pane; com workspaces existentes, seleciona o focado (ou único) e rejeita ambiguidade antes da mutação. Resposta malformada de `agent.start` não é mais rotulada como sucesso; o pane fica preservado com estado indeterminado.
9. Smoke da TUI a 80 colunas mostrou que um `cwd` longo escondia o snippet por truncamento. A linha agora prioriza o trecho encontrado antes do caminho/ID; teste de regressão exige snippet visível com projeto longo. IDs e caminhos continuam no resultado para terminais mais largos.

## Evidência local

- `cargo test --offline -p mercury-history`: 8 testes de parser, FTS5, dedupe, incrementalidade, filtros, permissões e resolve passaram.
- `cargo test --offline --bin mercury client::shell::tests -- --test-threads=1`: 323 passaram, 2 ignorados na rodada anterior; a aba History agora tem 7 testes. A rodada filtrada mais recente (`cargo test --offline --bin mercury history -- --test-threads=1`) teve 78/78 PASS, incluindo o novo caso de layout.
- Em 2026-10-06, `cargo test --offline --bin mercury client::shell::tests::history_view -- --test-threads=1` passou novamente (7/7) após estender o teste de Enter/resolução/pane/resume até a resposta `agent_started`; o teste confirma retorno à vista Terminal somente com sucesso tipado. `git diff --check` passou. `cargo fmt --all -- --check` ainda acusa formatação preexistente em outros arquivos do worktree; a alteração deste teste não apareceu no diff do rustfmt.
- Testes do contrato/schema e handler de filtros inválidos passaram. A rodada mais recente do CLI teve 7/7 PASS, cobrindo headless vazio, seleção ambígua, busy/retry, erro não-busy e resposta indeterminada.
- `cargo clippy --offline --all-targets -- -D warnings`, `cargo build --offline --bin mercury` e `git diff --check` passaram após o ajuste do filtro de datas. O CLI `mercury history --help` lista busca e reabertura.
- Smoke de API por socket com servidor/fixtures isolados retornou um resultado Claude e um Codex; `history.resolve` recusou `cwd` ausente. Integração `tests/history_open.rs` (7/7 PASS, incluindo 2 cenários History) abriu sessão Codex de fixture em pane existente e em workspace criado no headless; em ambos, um executável Codex falso recebeu `resume fixture-codex`. O teste fixa shell não-login e PATH isolado, e encerra/remove o servidor e os dados temporários.
- Smoke manual da TUI em PTY isolado, sem homes reais, encontrou uma sessão Codex sintética de 2024. O filtro de provedor alternou corretamente entre vazio (Claude) e resultado (Codex); o filtro de 30 dias ocultou a sessão antiga. A busca mostrou o snippet antes do `cwd` longo após rebuild; o CLI confirmou o mesmo hit. O servidor foi parado. A retomada via TUI não foi acionada: a política do ambiente vetou tornar executável o agente falso mesmo com escalonamento, e não se usou provedor real. O diretório temporário `/private/tmp/mercury-tui-smoke-j7i01L` permanece pela política de exclusão.
- Benchmark manual ignorado por padrão, com os homes locais (~1,1 GiB Codex + ~34 MiB Claude) e índice temporário privado: primeira sincronização em 22,66 s e 17,90 s em duas rodadas; sincronização sem mudanças em 21,39 ms. O índice temporário foi removido pelo teste; nenhum texto de conversa foi impresso. Nesta máquina a primeira varredura ficou abaixo dos 60 s do cliente TUI.

## Gate e riscos residuais

- SonarQube quality gate/coverage/hotspots, Veracode e Wiz: indisponíveis neste ambiente; a busca de plugins por esses três nomes também não retornou integração. Sem resultado, o gate PagueVeloz é **FAIL/não verificável**; não concluir fase nem commitar.
- Cobertura quantitativa de projeto não foi coletada (`cargo llvm-cov` ausente). Testes locais não substituem essa métrica.
- A primeira indexação percorre todo o histórico configurado; o benchmark local ficou abaixo de 60 s, mas homes maiores ou hardware mais lento ainda podem ultrapassar o timeout do cliente TUI. Worker preserva responsividade do servidor, porém não há progresso parcial/fatiamento para esse caso.
- Em Windows, permissões do índice ainda dependem de ACL do diretório do usuário; o contrato de privacidade 0700/0600 foi verificado em Unix. Isso exige implementação/validação específica antes de declarar suporte Windows equivalente.

Revisão visual adicional em 2026-10-07: a área da aba History agora é limpa antes de desenhar, evitando símbolos residuais do pane. Grupo conjunto das abas Usage/Crons/History/Learn: shell 331 PASS/2 ignorados, client_mode 28/28 na rerodada, Clippy global/formatação PASS. Os riscos de índice/ACL e o gate externo acima continuam abertos.

Integração TUI→servidor→pane em 2026-10-07: `tests/history_open.rs` agora abre a aba History por clique em PTY, busca `fixture-needle`, seleciona a sessão Codex sintética, pressiona Enter e confirma `codex resume fixture-codex` no novo pane, com retorno da TUI ao terminal. O executável Codex é falso e fica no diretório temporário privado; nenhum provedor/conta real foi chamado. A lista com busca vazia não promete snippet (o índice o retorna somente para busca textual), e a asserção tolera quebra visual de linha no pane estreito. Arquivo integrado **9/9 PASS**, Clippy global, `cargo fmt --all --check` e `git diff --check` PASS. Isto supera a antiga pendência de retomada via TUI com fixture acima, mas não verifica retomada com provedor real nem fecha o gate externo **FAIL/não verificável**.
