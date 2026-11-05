# Mercury — Plano e Levantamento

## Usage / Crons / visual — concluído localmente, 2026-10-29

Novo pedido especificado em `specs/009-usage-cron-ux/`: cotas das contas atuais,
agenda visual com prévia/fuso, criação/edição/exclusão guiadas e auditoria visual
transversal. Pesquisa, esclarecimentos, plano e análise concluídos; spec009 aprovada pelo
usuário. U/C/V implementados e validados localmente. Guia de operação em
`docs/dev/usage-cron-guide.md`; auditoria em `docs/dev/visual-audit-009.md`.
Workspace: 4.131 PASS/0 FAIL/13 ignorados; Cron final 50/50; manutenção80/80;
Clippy/build/fmt/diff PASS. Live handoff aplicado, painéis/PIDs/crons preservados,
Usage real Business/Enterprise conferido. Evidências e limites:
`docs/dev/usage-cron-009-review.md`. Reabrir cliente para carregar o frontend novo.
A pesquisa identificou a causa do n/a Codex Business: percentual em
individualLimit.remainingPercent, antes ignorado e agora suportado.

Memória local da feature008 ativada por solicitação explícita: config debug com
backup privado e recarga sem reinício; índice e busca concluídos. Vault vazio,
0 notas. Saída externa e contexto Route continuam desativados. Evidências em
`docs/dev/local-memory-activation-20261029.md`. Este registro supera a indicação
histórica abaixo de que a configuração pessoal ainda não havia sido ativada.

## Política vigente de validação — decisão do usuário em 2026-10-29

Gates externos SonarQube/Veracode/Wiz e cobertura quantitativa dispensados para
Mercury, incluindo trabalhos seguintes, até nova orientação. Fonte: instrução
explícita do usuário nesta conversa. Risco aceito e escopo registrados em
`.specify/memory/constitution.md`; testes e revisão locais continuam exigidos.
Checks externos não executados não são PASS. K023 da fase 008 foi encerrado
por dispensa; implementação e validação local da fase 8 concluídas.


## Knowledge / GraphRAG — implementação local em 2026-10-29

Spec/plano 008 aprovados pelo usuário. Em esclarecimento posterior, o usuário
rejeitou Ollama e escolheu Codex/Claude autenticados com roteamento Mercury.
Implementados índice Markdown privado, cache incremental, busca híbrida,
GraphRAG local/global com fontes, jobs/cancelamento, aba Knowledge, captura
revisável com undo e contexto Route com autorização por destino.

Validação: workspace 4.112 PASS/0 FAIL/13 ignorados; núcleo 19/19 após ajustes;
manutenção 80/80; fluxo completo em fixture; Clippy/formatação/diff PASS.
Avaliação real sintética: 19/20 perguntas conhecidas e 5/5 sem resposta; detalhes,
versões, limites e rerodadas em `docs/dev/knowledge-phase8-review.md`.

Guia: `docs/knowledge/README.md`. Feature default-off; vault real, credenciais e
configuração pessoal não foram ativados/modificados pelos testes. Sem commit.
Gates externos da fase 8 dispensados pela decisão de projeto registrada acima;
K023 encerrado sem alegar execução ou PASS dos scanners.


## Retomada 2026-10-29 — planejamento inicial (superado pelo registro acima)

- Nova camada Knowledge/Obsidian/GraphRAG especificada em
  `specs/008-knowledge-graphrag/spec.md`, com plano, tarefas e análise. GraphRAG
  local/global integra este ciclo; produção aguarda aprovação da spec.
- Vault confirmado pelo usuário: `/Users/c12103q/Documents/Obsidian Vault/AI PV Graph`;
  processamento local por padrão e serviços externos somente com configuração
  explícita. Não houve escrita no vault nem ativação de modelos.
- Pendências existentes e validações atuais:
  `docs/dev/mercury-current-audit-20261029.md`.
- Restauradas licenças/notices autênticos de portable-pty e ConPTY; hashes
  esperados preservados e manutenção 80/80 PASS. Fixture de socket Unix teve
  prefixo encurtado; regressão 3/3 PASS no TMPDIR original do macOS.
- `specs/007-auto-improvement/` reconciliado com decisões de 2026-10-07;
  requisitos removidos não voltam ao backlog. Histórico abaixo preserva datas.
- Verificação workspace em 2026-10-29: 4091 PASS / 0 FAIL / 13 ignorados (exit 0, sem filtros, TMPDIR curto); Clippy/formatação/diff e manutenção 80/80 PASS. Detalhes e limites na auditoria.
- Sugestões priorizadas: `docs/dev/mercury-improvements-20261029.md`.


> Controlador de agentes de IA no terminal (Claude Code + Codex CLI), com crons,
> roteamento, auto improvement, temas e painel de usage/contexto.

Status (2026-10-07): **Fases 0–1 concluídas**; **Fases 2 (Usage), 3 (Temas) e 6 (History) implementadas e verificadas localmente, ainda sem gate externo**; **Fases 4 (Crons) e 5 (Roteamento) com núcleo local implementado, pendentes de smokes com agente real e gate**; **Fase 7 (Learn) com núcleo reversível, gatilhos, curadoria de duplicatas e métricas observadas implementados, mas T019–T023 ainda abertos**. O arquivo segue como histórico incremental; os registros mais recentes abaixo prevalecem sobre pendências antigas. A suíte Rust do workspace passou **sem filtros** após remover do fixture HTTP a dependência de DNS reverso; Clippy/formatação passaram. Há um diagnóstico quantitativo local de cobertura em `docs/dev/workspace-coverage-snapshot.md`, mas ele tem aviso de funções incompatíveis e não substitui o gate. SonarQube/Veracode/Wiz indisponíveis: gate **FAIL/não verificável**, sem commit ou ativação automática em arquivos reais. Email/SMTP foi removido; notificações e interação Teams via Workflows/Power Automate ficam no backlog, fora do critério desta entrega. Learn continua opt-in (`enabled=false` por padrão). Os bloqueios funcionais principais são comparação baseline/after defensável, evidência contínua de uso para a regra de 30 dias, inspeções/smokes reais autorizados e o gate externo.

Decisões do usuário (2026-10-07; **prevalecem sobre os critérios antigos acima**): autoriza edição automática de Roteamento/Crons quando Learn estiver explicitamente habilitado; aprova associação explícita job→alvo Learn; **remove** a regra de arquivamento por desuso de 30 dias; dispensa SonarQube/Veracode/Wiz e cobertura quantitativa para esta entrega; fará pessoalmente os smokes com modelos reais e inspeção manual. Não pediu envio de email, nem restauração de notices/licenças ausentes. Teams por Workflows/Power Automate permanece backlog. A dispensa de scanners não é um PASS técnico nem autoriza afirmar que distribuição Windows com notices ausentes está validada.

Fechamento funcional local (2026-10-07; **estado vigente, substitui as pendências históricas de T019–T023 descritas abaixo**): Fases 2–7 estão implementadas para uso local, com Learn desligado por padrão. T019/T019b/T020/T021 foram implementadas e documentadas: revisão automática opt-in, alvo estruturado de rota e job Cron, associação explícita job→alvo/caminho persistida no outbox, comparação antes/depois apenas descritiva e curadoria semanal de duplicatas com undo. A regra de 30 dias foi retirada do escopo. T022 fica sob responsabilidade do usuário para inspeção visual e smokes reais com Claude/Codex; os testes automatizados usam fixtures/agentes descartáveis. T023 foi dispensada **somente** quanto a scanners externos e cobertura quantitativa: não executados, sem alegar PASS. Teams bidirecional por Workflows/Power Automate é backlog; não há SMTP nem envio de email. Pacote Windows permanece não validado pelos notices de terceiros ausentes. A validação final da suíte Rust, Clippy e formatação desta rodada é registrada em `docs/dev/learn-phase7-review.md`; não houve commit, alteração de configuração pessoal, modelo real ou entrega externa.

Refinamento Crons (2026-10-08): clique/Enter no job abre definição do alvo e histórico de até 50 execuções; clique/Enter no run abre o log, e Esc retorna ao detalhe. O assistente passa a mostrar confirmação após `cron.add` + `cron.list` e erro persistente quando o ID confirmado desaparece da listagem. Testes unitários da aba (11/11) e teste PTY integrado de criação/persistência/listagem (1/1) passaram em ambiente isolado; binário local recompilado. O sumiço relatado na sessão pessoal **não foi reproduzido nem declarado resolvido**: durante a inspeção não havia servidor Mercury ativo para comparar a instância real, e o usuário fará o smoke manual. Nenhum cron/modelo real foi executado.

Verificação consolidada posterior (2026-10-08): foi adicionado teste explícito de clique no job e na execução, levando os testes focados da aba Crons a **12/12**. A primeira suíte ampla expôs duas premissas antigas dos fixtures PTY: a asserção de Usage exigia um rótulo `rate_limits` que a tela de conta ainda indisponível não mostra, e o executável Codex falso recebia também a sondagem `app-server`, registrando o handshake como prompt. Os fixtures foram corrigidos sem alterar o fluxo de produção. A suíte `cargo test --workspace --offline --quiet -- --test-threads=1` foi repetida **sem filtros e saiu com código 0**: binário principal 3681 PASS/11 ignorados, `client_mode` 32/32 e `live_handoff` 23/23. `cargo clippy --workspace --all-targets --offline -- -D warnings`, formatação e `git diff --check` passaram. Isso não comprova o smoke de criação na sessão pessoal nem substitui o aceite manual do usuário; scanners externos e cobertura quantitativa continuam dispensados, sem alegar PASS técnico.

Revisão pós-implementação da Fase 4 (2026-10-08): foi identificada uma corrida possível na TUI — respostas `cron.list` antigas podiam chegar após a atualização de um `cron.add` e substituir a lista nova. O cliente agora aceita somente a resposta da requisição de lista mais recente; respostas antigas, inclusive erros de uma abertura anterior da aba, não alteram a tela nem geram aviso. A especificação F4 recebeu critério de aceite e análise desse caso; testes focados Crons **14/14**, smoke PTY isolado de criação/persistência/listagem **1/1**, Clippy global, formatação e build passaram. Essa corrida é uma causa plausível, **não uma causa comprovada** do sumiço na sessão pessoal: o socket pessoal continuava sem servidor ativo ao ser consultado. O smoke real permanece com o usuário; nenhum cron/modelo real foi executado.

Primeiro smoke do usuário (2026-10-07; pendência operacional): `config check` passou, mas dois servidores antigos de 2026-10-05 continuam em execução enquanto o binário foi recompilado em 2026-10-07. O servidor ativo, ainda sem Crons/Usage/Learn, retornou `unknown variant cron.doctor`; um agente Codex no sandbox recebeu `Operation not permitted` ao tentar acessar o socket. O CLI/TUI foram corrigidos para diagnosticar servidor sem as funcionalidades novas e encerrar o spinner de Learn; os testes locais estão em `docs/dev/learn-phase7-review.md`. O servidor do usuário **não foi encerrado nem reiniciado** para preservar panes. Antes de retomar o aceite real de T022, fazer live handoff seguro para o binário atual em terminal normal e confirmar `restart_needed: no`.

Checkpoint intermediário já superado (2026-10-07): a spec F7 foi ajustada antes do código. `[learn.job_targets]` aceita somente UUIDs de job e IDs de alvo allowlisted. O outbox Cron v5 congela o ID do alvo na conclusão; migração v4→v5 preserva amostras anteriores como não associadas. A chave de métrica inclui job e alvo, sem atribuição retroativa nem comparação causal. Learn aceita alvo `config-section` somente para `config.toml`: o reviewer vê apenas `[route]`, a policy valida `RouteConfig`, mescla a seção sem substituir o resto do arquivo e aplica via backup/ledger/undo; proposta com outras tabelas falha fechada. A API de planejamento da regra de 30 dias e seus testes exclusivos foram removidos; deduplicação permanece. `mercury-learn` passou localmente (incluindo teste de rota/undo), Crons crate passou 38/38 e os 12 testes `app::cron` passaram com sockets isolados. Naquele checkpoint ainda faltavam edição de jobs Cron com lock/undo, fronteira baseline/after e reload de rota; todos foram implementados no fechamento funcional local acima. Os smokes reais ficaram com o usuário. Nenhum alvo real/modelo externo foi acionado; sem commit.

Verificação incremental da Fase 7 (2026-10-07): T012–T014 e T016 foram confirmadas por smoke real de API/CLI no socket local com fixtures temporárias e por 4 testes automatizados da aba Learn. Undo exato e conflito com edição humana passaram. A inspeção visual/manual da TUI, revisão semanal, smoke de modelo autorizado e gates externos continuam pendentes.

Auditoria incremental da Fase 7 (2026-10-07): T015 concluiu a visão redigida de gatilhos recentes, verificação de disponibilidade de backup no detalhe e navegação da aba; schema, testes de shell e smoke de socket passaram. A inspeção visual/manual da TUI continua em T022, assim como sinais N-turn/semanal, integração do curator, smoke de modelo autorizado e gates externos.

Atualização posterior da Fase 7 (2026-10-07; prevalece sobre o status semanal acima): o índice de sessões concluídas guarda fingerprint de arquivo; entradas antigas são migradas como inelegíveis. Um temporizador de 15 minutos seleciona somente uma sessão validada da semana UTC anterior e enfileira revisão com deduplicação persistente. Arquivo alterado, sessão inválida ou ausência de candidato não gera claim. O gatilho N-turn, a integração do curator e das métricas, o teste ponta a ponta do agendador e os gates finais continuam pendentes. Testes locais Learn: 76/76 após migração; Clippy dos crates Mercury/Learn passou. Nenhum modelo ou alvo real foi acionado.

Atualização de gatilhos da Fase 7 (2026-10-07; prevalece sobre a pendência N-turn acima): T010 ligou Claude Stop, Codex idle como aviso, e semanal à mesma fila durável. O worker só reivindica marco N-turn após validar transcript íntegro, identidade atual, contagem exata e resposta do assistente; o revisor remove variáveis de conexão Mercury e sessões atingidas por cron em pane ficam excluídas. Testes Learn crate 77/77, testes Learn do binário 41/41, schema 55/55, instalação de hooks 11/11 e Clippy Mercury/Learn passaram. Ainda faltam T011 (suíte consolidada), T019–T023 (curadoria/métricas conectadas, documentação final, TUI/smokes e gates externos). Nenhum modelo real, alvo real, Teams ou email foi acionado.

Atualização de curadoria da Fase 7 (2026-10-07): T019/T020 avançaram com um gatilho semanal independente, deduplicação automática limitada a quatro pares byte-idênticos de alvos permitidos, backup/ledger/undo e recusa de memórias entre diretórios. O desligamento entre passos é respeitado e resultados parciais continuam auditáveis. A regra de arquivamento por desuso permanece desligada por falta de cobertura observada de 30 dias; produtor de métricas e UI ainda faltam, portanto T019/T020 não estão concluídas. Testes curator 11/11, suíte Learn 80/80, filtro Learn do binário 43/43 e Clippy Mercury/Learn passaram. Gates externos permanecem FAIL/não verificáveis; sem commit.

Atualização da suíte integrada da Fase 7 (2026-10-07; prevalece sobre T011 pendente acima): T011 está concluída com teste de claim durável, revisão simulada, deduplicação após reinício e undo byte a byte em `crates/mercury-learn/tests/review.rs`. Parser, runner e exclusão de origens recursivas também estão cobertos pelas suítes adjacentes. Suíte Learn 81/81, filtro Learn do binário 43/43 e Clippy Mercury/Learn passaram. T019–T023 seguem pendentes, inclusive métricas observadas, inspeção TUI e gate externo; sem commit.

Atualização de métricas da Fase 7 (2026-10-07): o executor headless de Crons agora captura contagens completas e limitadas de tokens somente quando o envelope JSON do Claude ou o evento `turn.completed` do Codex as reporta. Custo opcional vem apenas do campo reportado; saída textual, scripts, pane, stream malformado e dados parciais não viram métricas. O valor ainda não é persistido nem exibido como antes/depois: falta uma fronteira confiável entre amostras, portanto T019 permanece aberta. Testes Crons crate 33/33, filtro Crons do binário 30/30 (sockets simulados fora da restrição de bind do sandbox) e Clippy Mercury/Crons passaram. Nenhum Teams real, email ou modelo externo foi acionado; gate externo segue FAIL/não verificável.

Atualização posterior das métricas da Fase 7 (2026-10-07; prevalece sobre “não é persistido” acima): quando Learn está opt-in, a conclusão persistida de um job headless enfileira uma amostra `observed` no ledger privado Learn. O ID do run deduplica replays; o ID do job identifica a tarefa sem prompt ou conteúdo. A migração SQLite v2→v3 preserva amostras anteriores e a comparação permanece “dados insuficientes” sem baseline/after legítimos. A gravação ocorre em worker bloqueante para não travar a UI; uma saída abrupta antes de o worker concluir ainda pode perder a amostra, e a aba Learn ainda não exibe a série. Suíte Learn 83/83, filtro Crons do binário 31/31 e Clippy Mercury/Learn/Crons passaram. T019/T020 seguem abertos; nenhum serviço externo foi acionado e não há commit.

Atualização da exibição de métricas da Fase 7 (2026-10-07; prevalece sobre a pendência de UI acima): `learn.status` agora retorna no máximo 16 amostras `observed` recentes, com tarefa opaca, provedor, horário, tokens, custo opcional e resultado, sem prompt, caminho ou envelope bruto; cliente antigo sem o campo segue compatível. A aba Learn abre a página por `m`, mostra custo sem arredondamento e indica que faltam dados comparáveis antes/depois. A consulta ocorre no worker do endpoint. Testes de métricas 8/8, filtro Learn do binário 45/45, schema regenerado/verificado e Clippy Mercury/Learn passaram. T019/T020 ainda dependem da fronteira baseline/after confiável, evidência de desuso e teste completo do caminho de entrega; gate externo segue FAIL/não verificável, sem commit.

Atualização de durabilidade das métricas da Fase 7 (2026-10-07; prevalece sobre a janela de perda acima): a versão 4 do ledger Crons possui outbox privado e limitado. Conclusão do run e amostra numérica opcional entram na mesma transação; quando Learn está opt-in, um worker copia a amostra para o ledger Learn e só então confirma a retirada. Reinício, novo run ou reativação de Learn repetem o envio pendente sem duplicar o ID. Falha de Learn mantém a pendência; nenhuma mensagem/prompt entra no outbox. O limite de 4096 pendências pode descartar as mais antigas se Learn ficar indisponível por tempo prolongado. Testes Crons crate 36/36, filtro Crons do binário 31/31 e Clippy Mercury/Crons/Learn passaram. T019/T020 continuam abertos pela comparação baseline/after e evidência confiável de desuso; gate externo FAIL/não verificável, sem commit.

Atualização de horário das métricas da Fase 7 (2026-10-07): o replay agora lê o horário persistido de conclusão do run no ledger Crons e o grava na amostra Learn, mesmo quando a reconciliação ocorre depois de um reinício. Repetir o mesmo ID com horário diferente é rejeitado; horário negativo ou conversão com overflow falham fechados. Amostras históricas anteriores não são reescritas, pois o horário original pode ser irrecuperável. A lista `skills` de um job é apenas texto do prompt e não comprova uso da skill nem permite atribuir causalidade/baseline-after. Testes dos crates Crons/Learn, teste integrado de replay do binário, Clippy e `cargo fmt --check` passaram. Continuam abertos comparação confiável, evidência de desuso, smokes reais permitidos, inspeção manual e gate externo; sem commit.

Atualização de retry das métricas da Fase 7 (2026-10-07): pendências do outbox são retentadas a cada cinco minutos enquanto Crons e Learn permanecem habilitados, mesmo sem novo job. O trabalho fica fora do loop de UI e tentativas sobrepostas são descartadas, não enfileiradas; o próximo tick tenta de novo. Opt-out desarma o timer. Testes do timer e filtro Crons do binário 32/32, Clippy limpo. T020a concluída localmente; comparação baseline/after, evidência de desuso, smokes manuais e gates externos seguem abertos.

Pesquisa de cobertura de uso da Fase 7 (2026-10-07): `docs/dev/learn-phase7-usage-evidence-research.md` mapeia os mecanismos disponíveis. Claude separa invocação de `Skill` pelo modelo e comando `/skill`; `InstructionsLoaded` não cobre todo `SKILL.md`. O Mercury não tem evento Codex nem saúde contínua de hooks que provem cobertura de todos os usos, especialmente fora de sessões geridas. Portanto a regra de 30 dias permanece desligada; silêncio dos hooks não será tratado como evidência. A primeira janela elegível só poderá começar após instrumentação verificável, sem backfill fictício.

Correção da aba Learn em PTY (2026-10-07): um smoke real em servidor isolado encontrou que `learn.list/status/show/diff/undo` funcionavam pela API/CLI, mas faltavam no handshake de capacidades do client-shell; a TUI recusava as consultas como “Action unavailable”. Os cinco métodos foram anunciados, sem expor hooks nem undo por data. Testes da aba agora usam a lista real de capacidades. Após rebuild, a TUI mostrou ledger vazio e página `m` com dados insuficientes sem aviso; cliente e servidor temporários foram encerrados. Learn 47/47, client commands 7/7, shapes/schema passaram. O smoke de item/undo real, gates externos e demais pendências da Fase 7 continuam abertos.

Verificação ampla do workspace (2026-10-07): `cargo test --workspace --offline -- --test-threads=1` revelou fixtures herdadas desatualizadas do rebrand (hash, largura de texto, notas de release, versão da integração Claude e schema do manifesto); elas foram alinhadas aos contratos atuais. A troca global de `SIGPIPE` foi desabilitada **somente no binário de testes unitários**, mantendo o comportamento de produção coberto por `tests/broken_pipe.rs`. Nova rodada completa terminou com código 0 após filtrar apenas `live_handoff_preserves_http_servers_across_multiple_sessions` e `live_handoff_preserves_python_http_server`: o servidor Python local inicia, mas conexões TCP loopback entre processos não funcionam neste ambiente; os outros 21 testes de handoff passaram. O binário principal teve 3663 testes PASS e 11 ignorados; `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo fmt --all --check`, `git diff --check` e 63 testes auxiliares de manutenção passaram. `just test` permanece indisponível sem `cargo-nextest`; `bun` e `cargo-llvm-cov` também não estão instalados. A receita completa de manutenção ainda encontra notices/licenças ausentes em `packaging/windows/licenses` e `vendor/portable-pty/LICENSE.md`, e `cargo metadata` tenta baixar dependências Windows não presentes no cache. SonarQube/Veracode/Wiz e cobertura quantitativa seguem FAIL/não verificáveis; nenhum commit. Este resultado não conclui T019–T023 nem autoriza ativar Learn automático em arquivos reais.

Correção local-only planejada (2026-10-07): como o fork não publica feed de release nem catálogo remoto próprio, os defaults de `[update].version_check` e `[update].manifest_check` devem ser `false` também em builds de release. Configuração explícita do usuário continua respeitada; os comandos de atualização permanecem desabilitados até existir infraestrutura própria. Critério de aceite: `Config::default()` e `--default-config` não sugerem checagens automáticas ativas, e testes de config/reload continuam passando.

Correção local-only verificada (2026-10-07): os dois defaults são `false`; o template de `--default-config` mostra os valores desligados. Config explícita `true` continua sendo parseada sem alteração. Testes de configuração 195/195, reload 1/1, build do binário, Clippy global, formatação e diff passaram. `mercury --version` retorna 0.9.3. Nenhum feed externo foi consultado.

Smoke PTY com item real da aba Learn (2026-10-07): novo teste de integração cria somente fixture temporária, aplica uma alteração reversível, inicia servidor e cliente reais, inspeciona o frame por emulador VT, abre detalhe com backup verificado/diff redigido, confirma que o primeiro `u` não escreve e que o segundo restaura os bytes originais. `learn.show` confirma `undone` no ledger; conteúdo e motivo privados não aparecem na tela. Teste focado 1/1, arquivo `client_mode` 28/28, Clippy global, formatação e diff passaram. Isso fecha o smoke funcional de item/undo pela TUI, mas não a inspeção visual humana de cores/layout nem o smoke de modelo real ou gate externo.

Pesquisa da fronteira de métricas (2026-10-07): Crons já guarda `job_id`, resultado, usage e horário reais, mas o ledger Learn não associa um job ao `target_id` alterado. Texto em `skills`/prompt não é evidência de uso. `docs/dev/learn-phase7-research.md` registra o contrato e a pergunta enviada ao usuário sobre vínculo explícito job↔alvo para comparação descritiva; sem esse vínculo, baseline/after permanece corretamente indisponível.

Rerodada final da suíte local após defaults de atualização e smoke TUI (2026-10-07): `cargo test --workspace --offline --quiet -- --test-threads=1 --skip live_handoff_preserves_http_servers_across_multiple_sessions --skip live_handoff_preserves_python_http_server` saiu com código 0. Binário principal 3663 PASS/11 ignorados; `client_mode` 28/28 com novo teste Learn; handoff restante 21/21; demais crates e arquivos de integração sem falhas. Os dois testes TCP continuam não verificáveis aqui. Clippy global, `cargo fmt --all --check` e `git diff --check` também passaram. Isso não substitui cobertura quantitativa nem SonarQube/Veracode/Wiz.

Inspeção textual da TUI a 80 colunas (2026-10-07): a captura do frame Learn com item real mostrou símbolos residuais do pane sob o título e corte silencioso do resumo, hashes e diff. Learn, Usage, Crons e History agora limpam a área antes de desenhar; Learn usa resumo/instruções compactos quando necessário, sinaliza hashes parciais com `…`, divide o diff redigido em duas linhas e coloca a confirmação de undo perto do topo para terminais baixos. Captura PTY posterior mostrou título limpo, texto legível e confirmação visível; teste 80×12 protege a confirmação. Shell 331 PASS/2 ignorados, client_mode 28/28 na rerodada, Clippy global/formatação/diff PASS. A primeira rodada do arquivo client_mode teve timeout intermitente no teste SSH federado, que passou isolado e na rerodada completa; não há indício de relação com a aba. Inspeção de cor/contraste ainda depende de terminal visual externo; gate SonarQube/Veracode/Wiz/cobertura segue FAIL/não verificável.

Retomada History pela TUI verificada em fixture (2026-10-07): teste de integração abre a aba em cliente PTY, pesquisa sessão Codex temporária, pressiona Enter e confirma `codex resume` no novo pane com retorno à tela de terminal. O executável Codex é descartável e não chama modelo; a busca vazia mostra apenas sessões recentes, sem snippet, conforme o contrato. `tests/history_open.rs` 9/9, Clippy global, formatação e diff PASS. Ainda falta o smoke com provedor real e o gate externo; esta evidência não declara a Fase 6 concluída.

Auditoria de escopo Learn (2026-10-07): o item 21 deste plano inclui alterações na configuração Mercury (rota/crons), mas `src/config/learn.rs`, a policy e o writer rejeitam `config-section` integralmente. A spec técnica registrou T019b como lacuna funcional, não como exclusão aprovada. Substituição do TOML inteiro segue proibida; falta esclarecer as operações estruturadas permitidas, então nenhum código de escrita em config real foi habilitado. A pergunta foi enviada ao usuário; T019/T021–T023 e o gate permanecem abertos.

Smoke integrado de métricas Learn (2026-10-07): um job Cron headless com executável Claude descartável reportou 120→30 tokens e US$ 0,000125. A amostra atravessou conclusão/outbox/ledger/API e apareceu na página `m` do cliente PTY real; o alvo temporário não foi alterado. A primeira captura a 80 colunas mostrou custo e resultado cortados na linha única; a aba agora divide identidade e números em duas linhas por amostra. Teste focado 1/1, Learn UI 7/7, `client_mode` 29/29, Clippy global, formatação e diff PASS. Isto reforça T020/T022, mas não prova antes/depois, cobertura de 30 dias, inspeção visual de cor, smoke de reviewer real ou gate externo; T019–T023 seguem abertos.

Smoke Cron em pane e contenção de fixtures (2026-10-07): um teste PTY com servidor real e agente Codex descartável confirmou `cron run` em pane já pronta, uma única entrega do prompt e resultado/log de sucesso, sem modelo ou destinos externos. Foi detectada uma falha do harness: servidores de teste isolavam configuração e runtime, mas não `XDG_DATA_HOME`, fazendo cinco jobs sintéticos e seus logs aparecerem no diretório de dados pessoal. Os cinco jobs foram removidos pela API; logs e banco com esses runs foram preservados em `/private/tmp/mercury-cron-fixture-backup-Ci98gf/` após auditoria, e a lista ativa ficou vazia. Um banco vazio recriado por testes anteriores também foi auditado (0 runs/incidents/outbox) e movido à mesma quarentena. Todos os spawns de servidor da suíte de integração agora recebem data home isolado; os dois novos testes Cron também verificam que o caminho do log permanece nesse diretório. A suíte Rust completa após a correção saiu com código 0 (binário 3664 PASS/11 ignorados; `client_mode` 30/30; apenas os mesmos dois testes TCP filtrados); o arquivo `client_mode` foi rerodado 30/30 após ajuste final no helper. Clippy global, formatação e `git diff --check` passaram. O smoke real de agente/modelo, os gates externos e T019–T023 continuam pendentes; não houve email, Teams ou execução de modelo real.

Auditoria complementar de isolamento (2026-10-07): a busca por `.args([..., "server"])` revelou servidor nomeado no harness CLI e um teste opcional de shutdown não alcançados pela varredura anterior. Ambos agora usam data home do fixture; helpers CLI também receberam isolamento. Os alvos `cli` e `host_shutdown` compilaram, mas executam zero testes no macOS por `cfg` de plataforma; o smoke Linux/DBus continua pendente. Clippy global, formatação e diff passaram. A pergunta sobre escopo estruturado de config Learn e vínculo explícito job↔alvo foi reenviada ao usuário; nenhuma escrita em config real ou comparação causal foi inferida.

Correção incremental da Fase 2 (2026-10-07): uma statusline Claude válida sem `rate_limits` agora substitui um limite antigo por `n/a`, inclusive quando o overlay do pane está em throttle. O teste unitário cobre 61% → `n/a` e o teste PTY cobre a cadeia real CLI/socket/TUI com agente descartável. Filtro Usage **39/39**, `client_mode` **31/31** e Route **25/25** passaram; Clippy global, formatação e diff limpos. O cache de limites ainda é global por provedor; múltiplas contas Claude simultâneas não são distinguíveis. Smokes autenticados, alerta desktop e gate externo continuam pendentes; Fase 2 não foi declarada concluída.

Validação ampliada do handoff (2026-10-07; prevalece sobre a filtragem TCP acima): o servidor Python de fixture travava em DNS reverso antes do bind, como confirmado por servidor local isolado e HTTP 200 ao suprimir apenas essa consulta. O fixture passou a iniciar o mesmo módulo `http.server` sem essa dependência ambiental. Os dois testes antes filtrados passaram isolados e `tests/live_handoff.rs` passou **23/23 sem filtros**. Na primeira tentativa da suíte workspace sem filtros, um teste de refresh Codex da rota retornou `unavailable`; ele passou 12 vezes isolado, a suíte principal repetida passou **3666/3666** (11 ignorados) e a rodada workspace seguinte passou com código 0, **sem filtros**, incluindo `client_mode` **31/31** e `live_handoff` **23/23**. O primeiro resultado fica registrado como possível intermitência ainda não explicada. Clippy global, formatação e diff passaram; o gate externo permanece FAIL/não verificável.

Manutenção do vendor (2026-10-07): o teste de `cargo metadata` de `portable-pty` agora resolve somente o alvo host informado pelo `rustc` e usa `--offline --locked`, suficiente para verificar que o patch aponta à árvore vendorizada sem exigir o download de `windows_aarch64_gnullvm` em macOS. O teste focado passou. A suíte de manutenção teve **72/74** testes aprovados; as duas pendências restantes são arquivos de licença/aviso de terceiros referenciados pelo metadata mas ausentes (`vendor/portable-pty/LICENSE.md` e os notices de ConPTY). Nenhum texto legal foi inventado ou removido do metadata; obter os arquivos autênticos e validar os hashes ainda é necessário para passar esse gate. Isto não altera a decisão sobre a licença própria do Mercury.

### Fase 1 — rebrand técnico e remoção de nomenclatura legada

Mercury é um hard fork de um multiplexador de terminal de código aberto (Rust) usado como ponto de partida — não é uma evolução dele, nem reivindica parentesco. Todo o código foi renomeado/limpo para não carregar a marca original: rebrand mecânico aplicado a ~270 arquivos técnicos (código Rust, scripts de integração `.sh/.ps1/.js/.py`, manifests TOML, fixtures de teste, `justfile`, `skills/mercury/SKILL.md`, `.zed/settings.json`), renomeação de arquivos/diretórios (`*-agent-state.*`→`mercury-agent-state.*`, etc.), seguido de uma varredura completa removendo todos os vestígios restantes do nome original (ver abaixo).

**Decisões tomadas durante o rebrand (não mecânicas, precisaram de julgamento):**
1. **URLs de rede desativadas, não fabricadas.** O software base tinha 3 features que fazem fetch para um domínio hospedado que não é do Mercury: checagem de update (`src/update.rs`), sync de catálogo remoto de detecção de agentes (`src/detect/manifest_update.rs`), instalação remota automática via SSH (`src/remote/attach.rs`), além de checagem de versão via Homebrew API. Mercury não tem hosting próprio (decisão #5). Em vez de inventar um domínio fictício ou apontar pra infra real de terceiros sob a marca errada, essas três funções agora retornam erro cedo com mensagem explicando que a feature está desativada até Mercury ter infra própria — o resto da lógica (parsing de manifesto, comparação de versão, fallback pra binário local via `MERCURY_REMOTE_BINARY`) continua intacto e testável.
2. **Referências de atribuição (issues do GitHub do projeto original) removidas**, não renomeadas — renomear uma URL de issue real pra um domínio que não existe seria fabricar um link falso; o comentário/teste foi reescrito pra não citar a fonte.
3. **Texto de ajuda do CLI**: removidas as linhas do `AGENT_HELP_FOOTER` (`src/cli.rs`) e do `--help` (`src/main.rs`) que apontavam pra docs hospedadas que não existem para Mercury; teste correspondente (`tests/cli/surface.rs`) ajustado.
4. **Aviso de "external contributor policy"** removido de `build.rs` — processo de contribuição do projeto original não se aplica aqui (CLAUDE.md já diz isso).
5. **Site de documentação pública inteiro apagado** (`docs/next/website/`, `docs/preview/`, `docs/versions/` — ~800 arquivos): era o site do projeto original (changelogs/releases que nunca existiram pro Mercury); rebrandar esse texto criaria páginas falsas, então foi removido em vez de reescrito.
6. **Infra de release/distribuição removida** (não existe pra Mercury ainda, decisão #5 "sem remote"): `.github/` (CI/templates), scripts de release/changelog/preview/instalador público e seus testes, `flake.nix`/`nix/package.nix` tiveram paths e metadados consertados (sem inventar homepage/org). Recipes de release no `justfile` removidas; mantidas só as recipes de build/test/lint locais.
7. **Skills de dev-tooling do processo de release do projeto original removidas** (eram específicas do fluxo de release/triagem de issues do GitHub dele, inaplicáveis a um fork local sem repositório público).
8. **Código vendorizado com patches**: os patches de `portable-pty` (env var, nomes em comentários/docs de patch) foram atualizados mantendo a estrutura dos arquivos `.patch` intacta (mesma contagem de linhas) já que não há reaplicação automática — apenas documentação técnica. URLs de issue reais nos arquivos de patch foram removidas, não fabricadas.
9. **Logo removido** (`assets/logo.png`/`.svg`) — era a marca visual do projeto original; Mercury ainda não tem identidade visual própria.
10. **READMEs reescritos do zero**, minimalistas e honestos (sem badges/contadores/redes sociais que não existem pro Mercury).

**Verificado** (usuário, fora do sandbox, mesmo bloqueio de TLS da Fase 0): `cargo build` e `cargo clippy` limpos (pacote/binário já aparecem como `mercury`; `Cargo.lock` regravou sozinho). `cargo test --test-threads=1` só falha no mesmo SIGPIPE pré-existente de `client_writer_closes_queue_after_socket_write_failure` já documentado na Fase 0 — confirmado não ser regressão (mesmo teste, mesmo sinal, antes e depois do rebrand).

**Dois bugs de correção encontrados e corrigidos durante a verificação** (não eram apenas rebrand cosmético):
- Fixtures de sessão (`tests/fixtures/session/current-*-session.json`) tinham só o `include_str!` renomeado, não o arquivo físico — corrigido com `mv`.
- O teste de contrato `advertised_client_shell_method_shapes_stay_at_the_v1_contract` (`src/server/client_commands.rs`) quebrou de verdade: o rebrand mudou uma variante de enum (`ToastDelivery`) e um campo de wire protocol (`min_*_version`), o que muda o *shape* JSON de `pane.input.set` e `pane.split` (não é só cosmético — `description`/`title` já são normalizados fora do hash). Confirmado que são só esses 2 métodos afetados e que a mudança é a intencional do rebrand; digests do fixture `tests/fixtures/endpoint-method-shapes-v1.json` atualizados de propósito (não é um "bless cego" — qualquer app cliente externo que dependa do nome antigo do campo/variante precisaria ser atualizado, mas não há nenhum ainda).
- Um teste diagnóstico temporário (confirmando que o parser de sessão do Mercury lê corretamente um `session.json` real gerado pelo software base, já que o formato não mudou) foi removido depois de validar — tinha path fixo de máquina, não pertence à suíte permanente.
- **Dois arquivos apagados por engano na varredura de remoção de marca, depois restaurados**: `distribution/install.ps1` (compilado via `include_str!` em `src/update.rs` pro self-updater Windows) e `distribution/latest.json` (compilado via `include_str!` num teste de contrato de schema do updater). Pareciam só infra de release morta, mas são `include_str!`'d de verdade — detectado auditando todos os `include_str!`/`include_bytes!` do repo contra o que ainda existia em disco antes de pedir rebuild. `install.ps1` foi restaurado e rebrandado; `latest.json` foi substituído por um fixture sintético mínimo (domínio `example.invalid`, hashes zerados) em vez do changelog real do Herdr v0.9.1 que estava lá (285 ocorrências de "herdr" em prosa de release notes reais) — o teste só valida formato/schema, não o conteúdo.

### Fase 2 — Usage (em andamento)

Design completo, com mapa da UI atual e decisões D1–D10: **`docs/dev/usage-ui-design.md`**. Resumo:
- **Onde roda**: no servidor (`src/app/usage.rs`, a criar), leitura de arquivos em thread de background no padrão `git_refresh.rs`; o cliente só recebe números prontos via campos opcionais do `ClientShellSnapshot` (`#[serde(default)]`, inteiros: permille/micro-USD). Fora do schema da API pública por enquanto (não mexe nos digests de contrato).
- **Pane → sessão**: id reportado pelos hooks (`pane.report_agent_session`, já existente) + cwd → `mercury_usage::locate`. Sem hook = "n/a". Nunca scrape.
- **Detecção de mudança**: polling de `len/mtime` (1 s ativo / 10 s idle / nada sem agentes) + gatilho nos eventos de hook — **mudança em relação à seção 3**, que previa FSEvents: evita dependência nova agora; FSEvents vira otimização se necessário.
- **Gauge por pane**: tokens novos na sidebar de agentes (`context`, `context_pct`, `tokens`, `cost`, `model`), reaproveitando o motor de tokens configurável existente; nova linha default `["model", "context", "cost"]` que some sozinha quando o pane não tem usage. Borda do pane fica pra depois.
- **Aba Usage**: `ClientShellView { Terminal, Usage }` no cliente, `prefix+u` + chip na tab bar; desenhada sobre a área de panes (padrão do `render_mobile_switcher`), sidebar/tab bar continuam vivas. Base para as abas Crons/History/Learn.
- **Fuso dos orçamentos**: só offset fixo (`utc_offset = "-03:00"`); `tz = "<IANA>"` é rejeitado com mensagem clara (sem base tz por ora; São Paulo não tem DST desde 2019). O exemplo da seção 3.1 foi ajustado.
- **Custo**: só o reportado (statusline Claude; app-server Codex implementado no passo 7). Tokens de qualquer janela são recomputáveis varrendo JSONL no startup; custo não é, então os deltas observados vão para `~/.local/share/mercury/usage/cost-ledger.jsonl`.

**Feito nesta rodada** — crate `crates/mercury-usage` (membro do workspace; ainda não é dependência do binário):
- `model` (tipos neutros: `TokenCounts` normalizado, `SessionUsage`, `ContextUsage`, `ProviderLimits`), `jsonl::JsonlTail` (leitura incremental com linha parcial/truncamento), `claude` (dedupe `message.id`+`requestId` last-wins, `<synthetic>` ignorado, sidechain fora do contexto, subagentes somados), `codex` (deltas do total cumulativo, reset de contador, `rate_limits` nulos → vazio + `credits`), `statusline` (contexto exato, custo, `rate_limits` se houver), `window` (rolling/calendar diário-semanal-mensal/provider), `budget` (escopos, limites usd/tokens/percent, níveis, `partial`, previsão de estouro, `BudgetConfig` = formato TOML `[[usage.budget]]`), `locate` (slug de projeto Claude, busca de transcript/rollout, validação de id).
- 36 testes unitários com fixtures anonimizadas em `crates/mercury-usage/tests/fixtures/`. Validado também contra arquivos reais desta máquina (teste descartável, não versionado): Claude 428 requests únicos; Codex com totais idênticos ao último `total_token_usage`.
- Achado de infraestrutura: este crate **compila e testa dentro do sandbox** com `--offline` (as dependências já estão em `.cache/cargo`); só o binário completo precisa de rede (download do ghostty no build).

**Verificação (terminal real, na raiz do repo):**
```
export PATH=/opt/homebrew/opt/rustup/bin:$PATH ZIG=/opt/homebrew/opt/zig@0.16/bin/zig CARGO_HOME=$PWD/.cache/cargo ZIG_GLOBAL_CACHE_DIR=$PWD/.cache/zig
cargo test -p mercury-usage                      # 36 testes novos
cargo clippy -p mercury-usage --all-targets -- -D warnings
cargo fmt -p mercury-usage -- --check
cargo build && cargo clippy --all-targets -- -D warnings   # workspace inteiro: confirma que nada mais mudou
```
`Cargo.lock` já foi atualizado com o pacote `mercury-usage` (só adição; nenhuma versão existente mudou), então `--locked` funciona.

**Passos 1-2 feitos nesta rodada** (`mercury-dev`): `UsageService` (`src/app/usage.rs`)
+ wire. Detalhes:
- `UsageService` vive em `App` (mapa `TerminalId → SessionUsage`, `BTreeMap<Provider,
  ProviderLimits>`, tails incrementais atrás de `Arc<Mutex<..>>`); thread de leitura
  no padrão exato de `git_refresh.rs` (`std::thread::spawn` + `event_tx.blocking_send`);
  polling adaptativo 1s/10s por terminal (`start_usage_refresh_if_due`, chamado em
  `handle_scheduled_tasks_headless` a cada tick) + gatilho imediato (deadline zerado)
  em `HookStateReported`/`AgentSessionReported`/`StateChanged` (plugado em
  `App::handle_internal_event_with_pane_updates`). `AppEvent::UsageRefreshed { sessions,
  limits }` adicionado; aplicado com throttle de 1 atualização/s por sessão (D4).
  Resolução pane → arquivo via `mercury_usage::locate` lendo
  `TerminalState.persisted_agent_session` (sem tocar `session_ref_from_report`).
  Sessões sem hook ou com `AgentSessionRefKind::Path` (pi/omp) não são rastreadas
  ("n/a"). `mercury-usage` adicionado a `[dependencies]` do `Cargo.toml` raiz.
  5 testes unitários novos (`src/app/usage.rs`, dirs temporários, padrão de
  `git_refresh.rs`/`persist/`).
- Wire: `ClientShellAgentUsage`/`ClientShellUsageSummary` (+ `ClientShellProviderUsage/Limit/Credits`,
  `ClientShellUsageBudget` vazio) em `src/protocol/wire.rs`, ambos `#[serde(default)]`;
  preenchidos em `snapshot_with_completions` (`src/server/client_shell.rs`) a partir do
  `UsageService`. `budgets` fica sempre `Vec::new()` até o passo 5. Todos os literais
  `ClientShellAgent { .. }`/`ClientShellSnapshot { .. }` do repo (bem mais que os listados
  na D4 — ~20 sites em testes) atualizados com `usage: None`.
- **Desvio documentado do design**: D9 ("scan de startup... inclusive sessões rodadas
  fora do Mercury") pedia uma varredura global de todos os transcripts/rollouts dentro da
  janela de 35 dias. Essa varredura não tem consumidor hoje — o mapa do `UsageService` é
  por `TerminalId` (gauge por pane, D1), e o único consumidor de dados "fora do Mercury"
  seria a avaliação de orçamentos (passo 5), que ainda não existe. Implementar o
  armazenamento dessa varredura agora seria inventar a arquitetura de orçamentos sem
  spec — decisão de `mercury-architect`. Implementado em vez disso o que já tem
  consumidor: o primeiro poll de qualquer terminal rastreado já é uma leitura completa
  (o `JsonlTail` de uma sessão nova começa no offset 0), então o gauge mostra o total
  correto imediatamente ao anexar o hook, sem esperar deltas se acumularem. A varredura
  global de sessões não-anexadas fica para o passo 5, quando tiver para onde ir.
- **Achado de arquitetura em `mercury_usage::JsonlTail`**: `read_new` já entrega as
  linhas do arquivo truncado/substituído *durante* a mesma chamada em que sinaliza
  `reset = true` (ela reseta `offset` para 0 e lê até o fim antes de retornar) — um
  caller que só chama `transcript.reset()` depois de ver `result.reset` acaba
  reprocessando o conteúdo novo em cima do estado antigo (duplicando contagem). Corrigido
  comparando `std::fs::metadata(path).len()` com `tail.offset()` (método público) *antes*
  de chamar `read_new`, resetando o parser primeiro quando o arquivo encolheu. Não mexi na
  crate `mercury-usage` (fora do escopo deste passo) — só documentei o padrão de uso
  correto no caller (`src/app/usage.rs::read_claude_file`/`read_codex_file`).

**Passo 3 feito nesta rodada** (`mercury-dev`): tokens de gauge na sidebar + alerta de
contexto — **primeira entrega visível**. Detalhes:
- `AgentSidebarToken` ganhou `Context`, `ContextPct`, `Tokens`, `Cost`, `Model`
  (`src/config/sidebar.rs`; `context` entra na lista de tokens não-textuais que rejeitam
  `rules` condicionais, igual `state_icon`/`git_status`, porque não tem um único valor de
  texto). `AgentsSidebarConfig::default()` ganhou uma 3ª linha
  `["model", "context", "cost"]`.
- `ResolvedTokenKind` ganhou `ContextGauge { permille, exact }` (largura fixa,
  não-truncável, cor por faixa) e `Text(String)` (tokens textuais simples:
  `context_pct`/`tokens`/`cost`/`model`, renderizados como `Custom`).
  `resolved_token_spans` (`src/ui/sidebar.rs`) ganhou os dois casos.
  `AgentTokenContext` (`src/ui/sidebar/tokens.rs`) ganhou `usage: Option<&ClientShellAgentUsage>`,
  passado em `src/client/shell/agent_sidebar.rs::agent_row` a partir de `agent.usage.as_ref()`.
  Todo token novo resolve para `None` (linha some) quando `usage` é `None` ou o campo
  específico é `None`; `tokens` é exceção por não ter campo opcional (os contadores são
  `u64`, zero é um valor real) — resolve sempre que `usage` existe.
- **Fórmulas** (documentadas em `docs/dev/usage-ui-design.md` D5, hardcoded até o passo 5):
  cor do gauge verde `< 60%`, amarelo `< 80%`, vermelho `≥ 80%` (`CONTEXT_YELLOW_AT_PERMILLE`/
  `CONTEXT_RED_AT_PERMILLE` em `src/ui/sidebar/tokens.rs`); cache hit rate =
  `cache_read / (cache_read + tokens_in)` (razão de "tokens de entrada que vieram do
  cache"; `None`/segmento omitido quando ambos são zero); `tokens`/`cost` usam contagem
  "humanizada" (`1.2k`, `22k`, `1.3M`, sem `.0` sobrando).
- **Alerta de contexto**: `EndpointAgentPresentation::context_alerts`
  (`src/client/shell/endpoint_agent_state.rs`) compara `context_permille` por pane e
  dispara no cruzamento de 800‰ (80%, `CONTEXT_ALERT_PERMILLE`, mesmo hardcode do passo 5);
  dedupe por `HashSet<pane_id>` — só refaz o alerta depois que o pane cai abaixo do limiar
  (ou perde o usage/some do snapshot). Plugado em
  `ClientShellState::cache_endpoint_snapshot_with_surface`
  (`src/client/shell/endpoints.rs`, logo após `project_snapshot_for_generation`), que monta
  um `SemanticNotification { kind: Custom, title: "contexto {~}NN% — considere /compact", .. }`
  por pane cruzado e chama `receive_notification` — mesmo caminho usado pelos toasts
  vindos do servidor (`src/client/shell/notification_policy.rs`).
  **Desvio/limite conhecido**: o retorno de `receive_notification` (efeitos de som/
  Terminal/System) é descartado aqui — só o toast "Mercury" (banner in-app, via
  `tick_notifications` mutando `visible_notification`/`queued_notifications` como efeito
  colateral) realmente aparece para este alerta específico hoje; com
  `[client].toast_delivery = "terminal"` ou `"system"` ele não vai gerar notificação OS/terminal
  ainda. Levar os efeitos até `install_client_shell_snapshot`
  (`src/client/shell_runtime.rs`) exigiria mudar a assinatura de várias funções de cache de
  snapshot só para este caso raro — fica para quando o passo 5 (orçamentos/`warn_at`) for
  implementado, já que provavelmente vai precisar do mesmo encanamento para mais alertas.
  Toast também respeita a supressão padrão de "pane já é o foco ativo" do
  `notification_policy.rs` (mesma regra usada por `Finished`/`NeedsAttention`).
- Testes: `src/ui/sidebar/tokens.rs` (resolução de cada token novo, casos `None`/campo
  ausente, fórmulas de gauge/contagem/cache-hit, prefixo `claude-`) e
  `src/client/shell/tests/endpoints.rs` (renderização da linha de usage só quando há
  `usage`, e o ciclo completo do alerta: abaixo → cruza → não repete → cai → cruza de
  novo).

**Passo 4 feito nesta rodada** (`mercury-dev`): aba Usage dedicada (D6). Detalhes:
- `ClientShellView { Terminal, Usage }` em `ClientShellState` (`src/client/shell/state.rs`),
  default `Terminal`; `view` roteado para o render em `ShellRenderState` (`src/client/shell/render.rs`).
  Espaço deixado para `Crons`/`History`/`Learn` (não implementadas).
- Keybind `prefix+u` de ponta a ponta: `KeysConfig.usage`/`KeysConfigOverlay` (`src/config/model.rs`),
  `Keybinds.usage` (`src/config/keybinds.rs`), `KeybindAction::Usage` (`src/input/keybindings.rs`,
  tabela de `resolve_non_indexed_action`), ação em `src/client/shell/actions.rs`
  (`toggle_usage_view`, implementada em `src/client/shell/usage_view.rs`), entrada na ajuda
  (`src/input/keybind_help.rs`), amostra comentada em `src/main.rs`.
- Chip "usage" clicável no lado direito da tab bar (`src/client/shell/tabs.rs::render_tab_bar_status`,
  `hits.usage_chip`) — sempre reservado (8 colunas: `" usage "` + 1 de respiro), inclusive sem
  segmentos de status; clique alterna a view (`src/client/shell/mouse.rs`). **Desvio/impacto conhecido**:
  isso encolhe a faixa de tabs em 8 colunas em qualquer layout — um teste de largura fixa
  (`chrome_context::focused_last_overflow_tab_shows_its_full_label`) foi ajustado (107→99).
- Composição (`src/client/shell/composition.rs::compose`): quando `view == Usage`, desenha
  `usage_view::render_usage_view` por cima de `layout.pane_surface` (mesmo ponto/padrão do
  bloco do `render_mobile_switcher`: cobre `occlusion`, limpa `hits.panes/pane_splits/popup`).
  Sidebar e tab bar continuam renderizando ao redor.
- Conteúdo (`src/client/shell/usage_view.rs`): cabeçalho com "Usage" + idade do dado mais
  recente (maior `updated_at_unix_ms` entre os agentes, humanizado no cliente, "sem dados
  ainda" se nenhum); seção **Provedor** (uma linha por `ClientShellProviderLimit`, ou
  "n/a (plano não reporta rate_limits)" quando o provider não tem limite nenhum reportado —
  nunca inventa número; créditos/plano como sufixo); seção **Orçamentos** (sempre
  "nenhum orçamento configurado ainda" — `ClientShellUsageBudget` ainda não tem campos,
  passo 5); seção **Agentes** (uma linha por pane com `usage` presente, reaproveitando
  `format_model_token`/`context_gauge_text`/`format_tokens_token`/`format_cost_token` do
  passo 3, promovidas de privadas para `pub(crate)` em `src/ui/sidebar/tokens.rs` e
  reexportadas por `src/ui/sidebar.rs`/`src/ui.rs` — nada de lógica de formatação duplicada).
- Input (D6): `Esc`/`q`/`prefix+u` fecham a view; `j/k`/setas movem um cursor sobre as
  linhas de agente (a lista rola para mantê-lo visível — some via `reveal_cursor`, no mesmo
  padrão de `reveal_mobile_workspace`); `Enter` foca o pane do agente sob o cursor
  (`Method::PaneFocus`, mesmo caminho do clique de agente na sidebar) e volta para
  `Terminal`. Atalhos com prefix continuam funcionando por cima (checados antes do roteamento
  da view em `src/client/shell/input.rs`).
- Repaint: `usage_view_tick_deadline` (`src/client/shell/state.rs::timer_delay`) agenda um
  tick de 1s enquanto `view == Usage` (para a idade do cabeçalho); fica `None` fora da view.
- Testes novos: `src/client/shell/tests/usage_view.rs` (abrir/fechar por prefix+u, Esc, q e
  clique no chip; painel cobre a superfície de panes e limpa os hits; provider sem limite
  mostra "n/a" nunca um número fabricado; provider com limite mostra o percentual/janela
  reais; Enter foca o pane e sai da view; estado vazio quando nenhum agente tem usage).
- Build/lint/teste (terminal real, mesmo comando da seção "Verificação" acima): `cargo build`,
  `cargo clippy --all-targets -- -D warnings` e `cargo fmt --check` limpos nos arquivos
  tocados (há drift de formatação pré-existente em partes não relacionadas do repo, não
  tocado); `cargo test --bin mercury client::shell::` → 345 passaram, 0 falharam (inclui os
  7 testes novos da aba Usage).

**Passo 5 feito nesta rodada** (`mercury-dev`): config `[usage]` real + orçamentos +
alertas (D7). Detalhes:
- `[usage]` (`src/config/usage.rs`, novo): `enabled`, `poll_active_ms`/`poll_idle_ms`
  (default 1000/10000, substituem os antigos hardcodes `POLL_ACTIVE_INTERVAL`/
  `POLL_IDLE_INTERVAL` do passo 1 — `UsageService::poll_interval` agora lê
  `self.poll_active`/`self.poll_idle`, setados em `UsageService::new`/`apply_config`),
  `compact_at` (default 80, substitui `CONTEXT_ALERT_PERMILLE` do passo 3),
  `[usage.agents.<id>] compact_at` (override por agente, chave validada com o mesmo
  `crate::detect::parse_canonical_agent_label` de `ui.sidebar.agents.rows_by_agent`),
  e `[[usage.budget]]` — cujo shape é `mercury_usage::BudgetConfig` consumido direto
  (sem duplicar campos). Tolerância a chaves desconhecidas via o mesmo padrão
  `#[serde(default)]`/`serde_ignored` do resto do arquivo; `"usage"` virou uma seção
  conhecida em `src/config/io.rs` (`KNOWN_TOP_LEVEL_CONFIG_KEYS` + `load_live_section`
  pro reload ao vivo).
- Orçamentos inválidos (`tz` sem `utc_offset`, escopo ambíguo, etc.) **não** derrubam a
  seção inteira: `UsageConfig::resolved_budgets()` descarta só a entrada ruim e devolve
  um diagnóstico por entrada (`Config::collect_diagnostics`/reload ao vivo), igual ao
  padrão de outras seções tolerantes do arquivo.
- `UsageService` (`src/app/usage.rs`) ganhou `budgets: Vec<mercury_usage::Budget>` e, a
  cada `usage_summary_wire()` (chamado do snapshot, `src/server/client_shell.rs`),
  monta `UsageRecord`s a partir dos eventos (`ClaudeTranscript`/`CodexRollout::events()`,
  já expostos pelo crate) de **sessões atualmente rastreadas** (`self.usage.tails`) e
  roda `Budget::evaluate` por orçamento configurado. Atribuição: `agent` =
  `terminal.agent_name` (nome custom do pane, ex. "reviewer"), `pane` = id do terminal
  (cobre `scope = { agent = "<pane-id>" }`), `workspace` = label do workspace que contém
  o pane (`Workspace::display_name_from_terminals`). `cost_usd` é sempre `None` por
  enquanto — o ledger de custo (D9/D10) é passo 6; por isso um orçamento em USD com
  tokens de sessões sem custo reportado sai corretamente como `partial = true` em vez de
  inventar um valor.
- **Desvio documentado do design (D9)**: a varredura histórica de sessões *não*
  anexadas a um pane continua adiada — este passo só precisava de um consumidor real
  pros dados "fora do Mercury" (orçamentos agregados), e o spec do passo 5 (ver tarefa)
  pediu explicitamente avaliação "contra as sessões rastreadas", não um scan global.
  Efeito prático: um orçamento mensal só reflete uso de sessões que estiveram
  anexadas a um pane *durante a execução atual* do Mercury, não o mês inteiro se o
  Mercury foi reiniciado no meio dele. Retomar isso é um projeto à parte (startup scan +
  atribuição de workspace sem pane) — não implementado para não inventar arquitetura
  sem spec, mesmo princípio do desvio do passo 1.
- Wire: `ClientShellUsageBudget` (`src/protocol/wire.rs`) saiu do placeholder vazio e
  ganhou os campos reais de `BudgetStatus`: `name`, `scope` (string legível:
  `"global"`/`"provider: X"`/`"workspace: X"`/`"agent: X"`), `level`
  (`ClientShellBudgetLevel::{Ok,Warn,Exceeded,Unavailable,Unknown}`), `used_permille`,
  `used_scaled`/`limit_scaled` (inteiros escalados por `unit`:
  `ClientShellBudgetUnit::{Usd→micro-USD,Tokens→contagem,Percent→permille}`, mesma razão
  de design dos outros campos do snapshot — `Eq` sem jitter de float),
  `warn_threshold`, `resets_at_unix_ms`, `forecast_exhausted_at_unix_ms`, `partial`.
  Enums novos com fallback `#[serde(other)] Unknown` (regra de contrato do
  `src/protocol/endpoint.rs`); não entrou no schema da API pública (mesma exceção de D4
  — só o statusline bridge do passo 6 precisa).
- Recomputação em `next_reset`: `UsageService::next_budget_reset_deadline` usa
  `UsageWindow::next_reset` (já pronto no crate) pra achar o próximo instante em que um
  orçamento de calendário vira, e isso entra no `usage_next_poll_deadline()` existente —
  mesmo mecanismo de wakeup adaptativo do passo 1, sem timer novo.
- Alertas `warn_at`: ficaram **no cliente**, não no servidor — o servidor só evolui
  `level`/`warn_threshold` no wire a cada avaliação; `EndpointAgentPresentation::budget_alerts`
  (`src/client/shell/endpoint_agent_state.rs`, ao lado de `context_alerts`) dispara um
  `SemanticNotification` por orçamento que cruza um `warn_at` novo, com dedupe por
  `HashMap<nome_do_orçamento, limiar>` (mesmo princípio do `HashSet<pane_id>` do passo 3,
  adaptado pra guardar *qual* limiar já foi alertado, já que orçamento pode cruzar 70 e
  depois 90 e cada cruzamento deve notificar). `context_alerts` deixou de usar a
  constante `CONTEXT_ALERT_PERMILLE` e passou a receber `&UsageConfig`
  (`compact_at_permille_for(agent)`), resolvido no cliente a partir do `[usage]`
  espelhado em `ClientShellConfig.usage` (`src/client/shell/config.rs`, mesmo padrão de
  `apply_live_config`/`from_config` das outras seções client-owned).
- `on_exceed` (`notify`/`route`/`pause`): só `notify` tem efeito (cai no alerta acima);
  `route`/`pause` fazem parse normalmente (`mercury_usage::OnExceed`) mas são **no-op**
  documentado em código (`endpoint_agent_state.rs::budget_alerts`) e aqui — ação real
  depende do roteador geral da Fase 5, que ainda não existe. Não falha nem inventa
  comportamento.
- Aba Usage (`src/client/shell/usage_view.rs`): seção **Orçamentos** agora renderiza a
  lista real — barra de 10 células (`▰▰▰▱▱▱▱▱▱▱`), `usado / limite` formatado por
  unidade (`$`, contagem humanizada de tokens via `format_count` — promovida a
  `pub(crate)` e reexportada por `src/ui/sidebar.rs`/`src/ui.rs`, mesmo padrão do passo 4
  — ou `%`), percentual, `⚠`/`✗` por nível, `reset <relativo>`, `estoura <relativo>`
  (quando há previsão) e badge "· parcial". `n/a` explícito (nunca um número) quando o
  nível é `Unavailable`.
- Testes novos: parse de config (`src/config/usage.rs`: defaults, exemplo completo do
  design doc, `tz` sem `utc_offset`/escopo ambíguo rejeitados por entrada sem derrubar o
  resto, `compact_at_permille_for` com/sem override), avaliação de orçamento
  ponta-a-ponta contra uma sessão sintética (`src/app/usage.rs`), recomputação em
  `next_reset` (janela diária), reload ao vivo de `[usage]` (`src/app/mod.rs`), alerta
  `warn_at` com dedupe por orçamento+limiar incluindo re-cruzamento após cair
  (`src/client/shell/endpoint_agent_state.rs`), `compact_at` por agente
  (idem), render da aba Usage com orçamentos reais incluindo o caso `partial`
  (`src/client/shell/tests/usage_view.rs`), e tolerância do wire a `level`/`unit`
  desconhecidos (`src/protocol/endpoint.rs`).
- Build/lint/teste (terminal real): `cargo build`, `cargo clippy --all-targets --offline
  -- -D warnings` e `cargo fmt --check` limpos nos arquivos tocados (mesmo drift
  pré-existente em partes não relacionadas do repo, não tocado). Testes filtrados nas
  áreas tocadas: 249 passaram (`usage`/`config::`/`endpoint_agent_state`/`usage_view`/
  `protocol::endpoint`/`client::shell::config`/`app::tests::reload`/`server::client_shell`)
  + 36 em `mercury-usage` (inalterado) — 0 falhas.

**Passo 6 feito nesta rodada (Codex):** `mercury statusline` recebe o mesmo stdin do Claude Code, reporta o JSON ao servidor com timeout curto e executa o comando de statusLine anterior com stdin/stdout preservados. `mercury integration install claude` instala a bridge sob opt-in, salva a configuração anterior em backup e um arquivo de encadeamento; uninstall restaura o statusLine anterior. O método `usage.report_statusline` está no schema da API. O servidor aceita o relatório apenas quando o ID de sessão coincide com o hook do pane; contexto, custo e limites passam para o snapshot. Custos cumulativos viram deltas no ledger privado em background, com replay no restart para orçamentos USD. Verificação local: `cargo build --offline`, 42 testes da crate, 7 de statusline, 30 filtrados de usage, 5 de instalação Claude, 1 de desinstalação Claude, testes de contrato do snapshot e do schema da API e `cargo clippy --all-targets --offline -- -D warnings` passaram. O schema da API foi regenerado. O gate externo SonarQube/Veracode/Wiz não está configurado neste ambiente; não houve commit.

**Pesquisa para o passo 7:** `codex app-server generate-json-schema` funcionou neste ambiente (Codex CLI 0.160.1). O schema v2 confirma `account/rateLimits/read`, `account/usage/read`, `thread/tokenUsage/updated` e `account/rateLimits/updated`; custo estimado da thread aparece em `threadUsage.estimatedUsageUsdMicros` e pode ser `null`. Um app-server local iniciado fora do sandbox respondeu a `initialize`, `account/rateLimits/read` e `account/usage/read`. Nesta conta business, `primary`/`secondary` vieram `null`, `credits` e `planType` vieram preenchidos, e `threadUsage` foi `null` na consulta sem `threadId`. O ciclo de vida e o tratamento de dados ausentes estão especificados e implementados no passo seguinte.

**Passo 7 implementado (Codex):** especificação, plano e análise em `docs/dev/codex-app-server-usage-spec.md`. O servidor consulta `codex app-server --stdio` em background, com processo de vida curta, timeouts e encerramento; lê `account/rateLimits/read` e `account/usage/read` por thread identificada pelo hook. O parser preserva limites nulos/créditos e só aplica custo USD quando o `threadId` coincide. A cadência é imediata para nova thread, 60 s enquanto há agente trabalhando, 10 min em idle, com retry de 60 s após erro. O custo cumulativo entra no ledger existente; limites do rollout são fallback quando o app-server não reporta. Sem alteração no wire público. Testes: parser (3), transporte simulado incluindo respostas fora de ordem e timeout (2), troca de sessão/overlay/fallback no serviço e `cargo clippy --all-targets --offline -- -D warnings`. Uma leitura real do app-server com `threadId` foi validada fora do sandbox e retornou custo USD não nulo. O gate externo SonarQube/Veracode/Wiz não está configurado, portanto não houve commit.

**Varredura histórica D9 implementada:** especificação e análise em `docs/dev/usage-history-scan-spec.md`. O startup varre JSONL recentes de Claude/Codex em background, retém apenas eventos do horizonte de 35 dias e combina os registros com os tails ativos sem duplicar sessão. Sessões externas entram em orçamentos globais/de provedor; workspace só é atribuído quando o `cwd` coincide inequivocamente com um workspace aberto. Custo continua exclusivo do ledger. Teste integrado mostra 510 tokens após scan, durante pane rastreado e após detach, sem duplicação; fixture da crate cobre Claude, Codex, subagente, evento antigo e arquivo inválido. `cargo clippy --all-targets --offline -- -D warnings`, 46 testes da crate e 13 testes do módulo Usage passaram. Ainda falta verificação manual na TUI e gate externo antes de encerrar a fase.

**Próximos passos (ordem; detalhes na seção 5 do design doc):**
- [x] `UsageService` no servidor + `AppEvent::UsageRefreshed` + scan de startup (tokens,
      com o desvio documentado acima) — `mercury-dev`.
- [x] Campos `usage` no snapshot (`ClientShellAgent.usage`, `ClientShellSnapshot.usage`) — `mercury-dev`.
- [x] Tokens de gauge na sidebar + linha default — **primeira entrega visível** — `mercury-dev`.
- [x] View Usage (`prefix+u`, chip na tab bar, help) — `mercury-dev` com revisão `mercury-architect`.
- [x] `[usage]` no config.toml, avaliação de orçamentos, alertas `warn_at`/`compact_at`, recomputação em `next_reset` — `mercury-dev`.
- [x] Statusline bridge (`mercury statusline`, método `usage.report_statusline`, encadeamento do statusLine do usuário, instalação opt-in com backup) + ledger de custo.
- [x] Codex app-server (custo/limites ao vivo) — schema v2 e leitura real validados em 2026-10-06; implementação e testes locais acima.

**Smoke TUI local de Usage (Codex):** com `XDG_*`, `CODEX_HOME` e config
temporários, um rollout sintético de 1.200 tokens de entrada e 300 de saída
alimentou orçamento Codex de 2.000 tokens em janela de 24 h. A aba Usage
mostrou `1.5k / 2k`, `75%` e `⚠`; a lista de agentes ficou vazia sem pane
vinculado. `mercury config check` passou, assim como 46/46 testes de
`mercury-usage` e 35/35 testes filtrados `usage` do binário. Evidência e
limites em `docs/dev/usage-phase2-review.md`. Ainda faltam validação
operacional de statusline/alertas em pane real e o gate externo
SonarQube/Veracode/Wiz/cobertura; nenhum commit.

Validação integrada posterior da Usage (2026-10-07): um pane PTY com Claude
descartável, sessão de hook vinculada, transcript e `mercury statusline` reais
no socket mostrou na TUI contexto exato de 85%, `five_hour` de 61% e alerta
de orçamento 510/1000 tokens. O smoke encontrou e corrigiu a perda de limites
do provedor quando o statusline chegava durante o throttle do poll; a taxa do
pane permanece limitada. Teste unitário de regressão, grupo Usage 38/38,
`client_mode` 31/31, Clippy global, formatação e diff passaram. Detalhes em
`docs/dev/usage-phase2-review.md`. Falta smoke com Claude autenticado e
alerta desktop, além do gate externo; sem commit.

### Fase 3 — Temas (implementação local em verificação)

Spec, plano, esclarecimentos e análise: `docs/dev/themes-phase3-spec.md`. O catálogo agora inclui `mercury`/`mercury-light`, sem alterar o tema padrão. `Palette` tem `gauge_ok/warn/crit` independentes das cores de estado; todos os temas antigos mantêm os valores visuais anteriores e `[theme.custom]` permite sobrescrever os tokens (inclusive nos blocos light/dark). O picker Settings existente faz preview imediato, Cancel restaura a paleta, Confirm persiste o nome.

`[[theme.import]]` carrega temas locais Base16 YAML plano ou Ghostty (subconjunto de cores) por nome, caminho absoluto e formato explícito. O conteúdo é validado e limitado antes de entrar na paleta; não há rede nem execução de opções Ghostty. Imports válidos aparecem no picker e funcionam no reload. Se o arquivo selecionado falhar durante um reload, a paleta anterior fica ativa com diagnóstico; no startup há fallback built-in.

Verificação local: `cargo test --bin mercury --offline theme -- --test-threads=1` fora do sandbox: **47/47 passaram** (dois testes de socket falham apenas dentro do sandbox com `Operation not permitted`, confirmados passando fora); `cargo clippy --all-targets --offline -- -D warnings` e `scripts/test_ui_hot_path_architecture.py` (6/6): limpos. Smoke em PTY isolado confirmou picker `mercury`/`mercury-light`, preview imediato, Esc restaurando paleta/config, Confirm persistindo e retorno a Mercury. Import Ghostty local apareceu no picker, aplicou `#112233` no preview e persistiu após Confirm; o tema ativo foi restaurado no fixture. Evidências e limites em `docs/dev/themes-phase3-review.md`. Pendente: gate externo SonarQube/Veracode/Wiz e cobertura; nenhum commit foi feito.

### Fase 4 — Crons (implementação local em verificação)

Atualização de escopo: o usuário escolheu **Workflows/Power Automate** para Teams bidirecional, em vez de um bot próprio, mas colocou **notificações e interação Teams no backlog**, fora do critério de conclusão desta entrega. O webhook de saída atual não cumpre a interação; não será feito smoke real de Teams agora. Para o backlog, ainda faltam definir a conectividade HTTPS autenticada do Flow ao Mercury e o formato dos comandos. A documentação do [conector Teams](https://learn.microsoft.com/en-us/connectors/teams/) informa que respostas em threads de canal não disparam o gatilho de nova mensagem; [Adaptive Cards com espera de resposta](https://learn.microsoft.com/en-us/power-automate/overview-adaptive-cards) são opção para ações vinculadas a um run. Ação HTTP para endpoint externo pode exigir [Power Automate Premium](https://learn.microsoft.com/en-us/power-platform/admin/power-automate-licensing/types).

Pesquisa, spec, esclarecimentos, plano e análise: `docs/dev/cron-phase4-research.md` e `docs/dev/cron-phase4-spec.md`. A crate `mercury-cron` cobre agendas relativas, intervalo, cron de cinco campos e RFC3339; jobs TOML atômicos; ledger SQLite WAL versionado; reserva única e avanço durável antes do dispatch; lock de scheduler; catch-up; runs manuais; repeat; e incidents agrupados. O executor headless/script usa processo sem shell, prompt via stdin, timeout e saída limitada. O servidor despacha no máximo quatro runs em paralelo e preserva excedentes. O alvo pane usa `agent.prompt` com espera e leitura limitada. API/CLI `cron.add/list/edit/pause/run/rm/runs/log/doctor` compartilham o mesmo estado; `cron.log` limita leitura e rejeita symlinks.

Destinos local, pane e Teams HTTPS de **saída** estão integrados após a persistência do run. `[SILENT]` suprime entregas; incidents recorrentes só avisam no primeiro evento ou lembrete; falha em um destino não impede os demais. Credenciais vêm de variáveis de ambiente e não entram em argv ou mensagens de erro. Teams usa payload JSON limitado e não segue redirecionamentos. Por correção de escopo do usuário, o destino SMTP foi removido; destinos `email` de jobs antigos são ignorados ao carregar, sem interromper os demais jobs, e eliminados na próxima gravação. A interação de entrada pelo Teams via Workflows/Power Automate ainda não está implementada.

A aba Crons usa a API do endpoint ativo: lista com próximo horário/último status, runs, logs, run now, pause/resume e edição da agenda com leitura da definição completa; não persiste logs no snapshot. `prefix+shift+c` abre a aba sem conflitar com `prefix+c` (nova aba). A tecla `a` abre assistente de criação para alvo Claude, Codex, pane ou script, com validação de campos e workdir, envio único por `cron.add` e descarte do rascunho ao cancelar/trocar de endpoint. A skill `skills/mercury/SKILL.md` orienta a criação de jobs por linguagem natural via CLI, sem execução implícita. A saída headless extrai somente `result` de Claude e a última `agent_message` concluída do Codex, ignorando eventos/telemetria intermediários e stderr bruto; envelope inválido/falho marca o run como falho.

Verificação atual: **31 testes da crate e 28 testes `cron` do binário passaram** após remover SMTP; schema regenerado/testado; Clippy `--all-targets -D warnings` e `git diff --check` limpos. Os testes de socket do grupo `cron` exigem execução fora do sandbox restrito a bind local. Smoke com servidor/socket real executou `/bin/echo` e confirmou ledger/log; outro smoke em sessão isolada executou `codex exec --json` num repositório Git temporário e confirmou `success`/`MERCURY_CRON_SMOKE_OK` em `cron runs`/`cron log`. Codex fora de Git recebe diagnóstico específico em `cron doctor` e antes do processo, sem desabilitar a proteção do CLI. A TUI foi testada em PTY isolado para lista, assistente, run/log, pausa e edição; detalhes em `docs/dev/cron-phase4-review.md`. Ainda faltam teste real com agente em pane e gate externo SonarQube/Veracode/Wiz. Teams (inclusive smoke) fica no backlog e não bloqueia esta fase; não haverá smoke SMTP nem envio de email. O gate externo está **FAIL/não verificável**: SonarQube, Veracode, Wiz e cobertura de projeto não estão configurados/disponíveis neste ambiente; nenhum commit será feito antes de evidência do gate.

### Fase 5 — Roteamento (em implementação)

Pesquisa, spec, esclarecimentos, plano e análise: `docs/dev/router-phase5-research.md` e `docs/dev/router-phase5-spec.md`. A crate `mercury-router` foi criada com regras ordenadas por padrão/tag/projeto, validação de config e seleção de Claude/Codex por headroom tipado. Um limite ausente, obsoleto ou já vencido por `resets_at` permanece `unknown`, não é interpretado como 0%; limite excedido seleciona fallback antes do envio. `[route]` já é carregado, validado e recarregado pela configuração, desabilitando regras inválidas com diagnóstico. `route.preview` é uma API somente de leitura que usa limites observados pelo servidor, explica a escolha sem retornar o prompt e não cria pane; schema público regenerado. `mercury route preview --file PATH|--stdin` expõe a prévia sem prompt em argv.

O comando `mercury route submit --file PATH|--stdin` agora usa as APIs tipadas para selecionar o workspace, criar pane, iniciar Claude/Codex e enviar o prompt uma única vez. `--wait` aguarda com timeout e retorna trecho limitado da leitura; resposta incerta após `agent.prompt` nunca faz retry/fallback. Falha de startup antes do prompt fecha só o pane criado pela rota e pode tentar o fallback configurado. O prompt não entra nos argumentos de inicialização e é redigido do trecho retornado. Se o Codex pedir confiança na pasta, a rota preserva o pane, retorna `needs_interaction` e não concede confiança automaticamente nem envia o prompt. Testes unitários da seleção, do handler e do dispatch passaram; build e `cargo clippy --all-targets --offline -- -D warnings` passaram. Smoke real em repositório Git temporário confirmou seleção de Codex, pane e processo iniciados; após o ajuste, retornou `needs_interaction` com ID do pane em cerca de 3 segundos, sem conceder confiança e sem enviar o prompt. Portanto **não** confirmou envio/completação real da tarefa. O servidor temporário foi encerrado. A opção de prompt diretamente no argv ainda depende da decisão do usuário; por ora apenas `--file`/`--stdin` são aceitos.

`route.preview` agora incorpora orçamentos `on_exceed=route|pause`: orçamento excedido de provedor exclui esse provedor e permite fallback fora do escopo; orçamento global/workspace excedido bloqueia tarefa nova, pois trocar provedor não reduz esse consumo. Dado de janela de provedor obsoleto ou já resetado não bloqueia. A resposta explicita provedores excluídos pelo orçamento. No envio, a prévia recebe o ID estável do workspace e resolve seu rótulo de usage, evitando bypass por divergência entre o rótulo exibido e o histórico. Testes cobrem escopos, fallback, dado obsoleto, reset e ID inválido; schema regenerado, Clippy limpo.

`mercury route submit --worktree` é opt-in: `worktree.create` cria checkout/workspace e o agente usa o root pane. Em falha antes do prompt, o CLI só faz fallback após `worktree.remove` sem force confirmar limpeza; checkout sujo ou remoção incerta ficam preservados com ID/caminho para inspeção. O resultado informa branches geradas que podem sobrar após remoção de checkout. Smoke real em repositório Git temporário confirmou criação, root pane e `needs_interaction` de Codex sem envio da tarefa; em seguida o worktree foi removido sem force, a branch de teste foi excluída e o servidor temporário encerrado. Testes simulados cobrem criação, fallback condicionado à limpeza e falha segura quando a limpeza não é possível. Verificação consolidada: 8 testes de `cli::route`, 30 testes da API de worktrees (com XDG temporário gravável para hooks de plugin), `cargo clippy --all-targets --offline -- -D warnings` e `git diff --check` passaram. Sem XDG temporário, dois testes de plugin não iniciam hooks por bloqueio de escrita no home do sandbox; não é falha de roteamento.

`route.preview` passou a atualizar limites Codex sob demanda no servidor quando o cache está ausente/obsoleto/resetado, inclusive sem pane Codex ativo. A consulta read-only ao app-server tem timeout curto em worker; prévias simultâneas compartilham uma consulta em fila limitada. Falha mantém headroom `unknown` e entra em cooldown, sem mostrar erro bruto do provedor. Para Claude, não existe fonte suportada de consulta sob demanda nesta integração: os limites vêm da statusline, com TTL e `unknown` para ausência/obsolescência. Testes com app-server simulado cobrem leitura sem thread, coalescência, cache e falha/cooldown. Smoke real em sessão isolada retornou `codex_limit_refresh=refreshed`, headroom `unknown` fiel ao `rateLimits` nulo desta conta e timestamp; segunda prévia usou cache sem nova consulta. Servidor temporário encerrado. Verificação após o grupo: 25 testes filtrados por `route`, 2 do leitor Codex, teste do schema, build, Clippy `--all-targets -D warnings` e `git diff --check` passaram.

`tests/route_submit.rs` passou com servidor/socket reais e Codex falso: a regra escolheu Codex, criou pane e entregou prompt sintético uma única vez; `--wait` acompanhou working→idle, retornou `completed` e omitiu o prompt do resumo. `--worktree` criou checkout e workspace separados em repositório Git temporário, iniciou o agente no root pane e entregou o prompt uma única vez. Falha de startup `blocked` removeu o checkout antes do fallback, não enviou prompt e informou a branch residual (9/9 testes do arquivo, sem conta externa). A revisão consolidada e os riscos estão em `docs/dev/router-phase5-review.md`; Clippy global passou após os quatro testes. Ainda faltam envio/resultado com agente real já autorizado e inspeção manual TUI. SonarQube/Veracode/Wiz e cobertura de projeto permanecem indisponíveis, logo o gate externo está FAIL/não verificável e não houve commit.

### Fase 6 — History (implementada localmente, em verificação)

Pesquisa brownfield e contrato com esclarecimentos, plano e análise em `docs/dev/history-phase6-research.md` e `docs/dev/history-phase6-spec.md`; revisão consolidada em `docs/dev/history-phase6-review.md`. A crate `mercury-history` indexa texto conversacional Claude/Codex em SQLite FTS5 privado, sem corte por idade, com atualização incremental, filtros e deduplicação. Ignora tool output/thinking; `resolve_resume` revalida ID, fonte e projeto. As APIs executam scan/consulta em worker serializado; CLI e aba History buscam e reabrem via resume nativo em pane novo, sem conceder confiança automaticamente. A TUI e o CLI esperam de forma limitada quando o shell do pane ainda está ocupado. O clique só seleciona; Enter reabre. A suíte local inclui 8 testes da crate (mais 1 benchmark manual ignorado), 3 do handler, 7 do CLI e 7 da aba; a suíte da shell anterior teve 323 PASS e 2 ignorados, e a rodada filtrada de History teve 78/78 PASS após o novo teste. Build, Clippy `--all-targets -D warnings` e `git diff --check` passaram. Smoke por socket com fixtures encontrou chats Claude/Codex e recusou projeto ausente; integração isolada abriu a sessão por CLI em pane existente e em workspace criado no headless, ambos com executável Codex falso (7/7 testes do arquivo). Smoke manual em PTY isolado encontrou sessão sintética na TUI, validou filtros de provedor/data e busca; um `cwd` longo escondia o snippet a 80 colunas e a linha foi corrigida para priorizar o trecho, confirmado após rebuild. Atualização: a retomada pela TUI com provedor falso passou no teste integrado (9/9); ainda faltam retomada com provedor real e gate externo. SonarQube/Veracode/Wiz e cobertura não estão disponíveis, portanto gate FAIL/não verificável e nenhum commit. As compilações usam `CARGO_INCREMENTAL=0` após `cargo clean -p mercury` remover somente artefatos regeneráveis.

O CLI `mercury history open` espera por até 3 segundos somente quando `agent.start` retorna `agent_pane_busy`, sem criar outro pane nem mudar a identidade do agente. Em servidor sem workspace, cria um no `cwd` histórico; com vários, exige seleção focada não ambígua. Resposta de `agent.start` sem confirmação tipada é indeterminada, nunca sucesso presumido. O filtro `until` inclui todo o dia final; a API rejeita datas impossíveis e intervalo invertido. Benchmark manual com os homes locais (~1,1 GiB Codex + 34 MiB Claude) mediu 17,90–22,66 s na primeira sincronização e 21,39 ms sem mudanças, abaixo dos 60 s da TUI nesta máquina; o índice temporário privado foi removido pelo teste. Em 2026-10-06, o teste da shell passou a verificar também que uma resposta `agent_started` tipada após Enter/resolução/pane/resume devolve a TUI ao terminal (7/7 testes de `history_view`); isso não substitui a retomada com provedor real, que ainda não foi verificada.

### Estado atual / próximos passos
- [x] Base copiada na raiz e rebrandada; sem cópia dos docs/notas de contribuição do projeto original no repositório (removidos, ver Fase 1).
- [x] Arquivos de licença removidos (`Cargo.toml` e `.gitattributes` ajustados).
- [x] Agentes de desenvolvimento em `.claude/agents/` (cópia em `docs/dev/agents/`).
- [x] Toolchain: Rust 1.96.1 via `/opt/homebrew/opt/rustup/bin/rustup` + `zig@0.16` (Zig 0.17 **não** serve: versão minor exata).
- [x] `git init` + commit base (o sandbox bloqueia escrita em `.git` — commits são feitos pelo usuário).
- [x] Fase 0: build. O sandbox do Claude Code intercepta TLS para `crates.io`/`deps.files.ghostty.org` com certificado não confiável (proxy de rede do sandbox) — `cargo build`/`cargo test`/`cargo clippy` têm que rodar no terminal do usuário, fora do sandbox, com:
  `PATH=/opt/homebrew/opt/rustup/bin:$PATH ZIG=/opt/homebrew/opt/zig@0.16/bin/zig CARGO_HOME=$PWD/.cache/cargo ZIG_GLOBAL_CACHE_DIR=$PWD/.cache/zig cargo build` (zig 0.16.0 confirmado em `/opt/homebrew/opt/zig@0.16/bin/zig`).
  `cargo build` e `cargo clippy` passam limpos. `cargo test` tem falhas conhecidas **pré-existentes do software base** (não introduzidas pelo fork), de isolamento entre testes rodando em paralelo — não bloqueiam a Fase 0:
  - `config::io`, `detect::manifest_update`, `persist::plugin_registry`, `app::api::plugins`: usam `std::env::set_var` em vars globais (`XDG_CONFIG_HOME`, `CONFIG_PATH_ENV_VAR`, `CATALOG_URL_ENV`) sem lock — racem entre threads de teste paralelas (confirmado: passam 100% isolados/`--test-threads=1`).
  - `platform::remote_bridge_tests::bridge_child` e qualquer teste de socket: `src/platform/unix_common.rs:244` (`begin_cli_output`/`end_cli_output`) alterna `SIGPIPE` entre `SIG_DFL`/`SIG_IGN` **no processo inteiro** (intencional, pra `mercury | head` funcionar); como é global e não por thread, pode matar o binário de teste inteiro com SIGPIPE real quando corre em paralelo com testes de socket.
  - Atualização de 2026-10-07: a troca de `SIGPIPE` foi isolada do harness unitário; o executável real mantém a semântica, confirmada por `tests/broken_pipe.rs` 3/3. Testes com variáveis globais continuam serializados com `--test-threads=1` nesta verificação.
- [x] Fase 0: spike dos leitores de usage — **concluído**, schemas reais validados nesta máquina:
  - **JSONL do Claude Code** (`~/.claude/projects/<slug>/<sessionId>.jsonl`): confirmado `message.usage` com dedupe por `(message.id, requestId)` — testado no próprio histórico desta sessão (59 mensagens únicas de 113 linhas `assistant`, 54 duplicatas de streaming corretamente ignoradas).
  - **Statusline bridge**: schema real capturado ao vivo (statusLine temporário apontando pra um script de debug, revertido depois). Estrutura real:
    `session_id, transcript_path, cwd, model{id,display_name}, workspace, cost{total_cost_usd,total_duration_ms,total_api_duration_ms,total_lines_added/removed}, context_window{total_input_tokens,total_output_tokens,context_window_size,current_usage{...},used_percentage,remaining_percentage}, exceeds_200k_tokens, prompt_cache{warm,ttl,expires_at,requests,misses,hit_ratio,cache_write_tokens,miss_causes,...}, fast_mode, thinking{enabled}`.
    **Achado importante**: nesta conta (Experian/enterprise via Claude Code, não é Pro/Max pessoal) **não existe o campo `rate_limits`** — confirma o risco já previsto na seção 6 (janelas de 5h/semanal só em Pro/Max); o painel de Usage do Mercury vai mostrar "n/a" pras janelas por hora nessa conta. `prompt_cache` é mais rico do que o documentado (hit_ratio, TTL, causas de miss) — útil pro painel de cache.
  - **Rollouts do Codex** (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`): confirmado evento `token_count` com `info.total_token_usage`/`last_token_usage`/`model_context_window` e `rate_limits.primary/secondary=null` + `credits{has_credits,unlimited,balance}` + `plan_type:"business"` — bate exatamente com o documentado na seção 1.3.
  - **`codex app-server`**: a tentativa original no sandbox do Claude Code falhou com `Operation not permitted`. Em 2026-10-06, no Codex CLI 0.160.1, o schema foi gerado e o servidor local respondeu às leituras de conta fora do sandbox; a integração no Mercury foi implementada e testada no passo 7 acima.
  - Nota de infraestrutura: `~/.codex` nesta máquina tem armazenamento híbrido (SQLite `thread_history_1.sqlite`/`goals_1.sqlite`/`memories_1.sqlite` além dos rollouts JSONL) — os parsers do Mercury devem usar só os rollouts JSONL documentados, não presumir acesso ao SQLite interno (formato não documentado/instável).

**Fase 0 concluída.** Build limpo, clippy limpo, leitores de usage validados com dados reais desta máquina.

---

## 1. Levantamento

### 1.1 Software base (referência principal — ponto de partida do fork)

| Item | Dado |
|---|---|
| Licença | Apache-2.0 no upstream — **arquivos de licença removidos da base do Mercury** (decisão do usuário, uso não comercial) |
| Stack | Rust · ratatui 0.30 · crossterm · tokio · portable-pty · interprocess (unix socket) · serde/toml/schemars · **libghostty-vt** vendorizado (emulação de terminal) |
| Tamanho | ~11 MB de Rust, ~3.4k arquivos, projeto open-source estabelecido |
| Cadência (upstream) | estável a cada 1–2 semanas + preview quase diário |
| Contribuição (upstream) | **PRs não solicitados são fechados** → fork seria hard fork |
| Arquitetura | servidor headless dono dos PTYs + clientes TUI que fazem attach/detach; sessões nomeadas; `--remote` via SSH |
| API | socket JSON em `~/.config/mercury/mercury.sock`, 117 métodos (`workspace.*`, `tab.*`, `pane.*`, `agent.prompt/wait/read`, `events.subscribe`, `pane.report_metadata`, `plugin.*`) |
| Estado de agente | `idle / working / blocked / done / unknown` — para Claude/Codex via **manifests TOML de screen-scrape** (`src/detect/manifests/claude.toml`, `codex.toml`) |
| Persistência | `~/.config/mercury/session.json` v3 (workspaces → tabs → layout tree → panes com cwd) + resume nativo (`claude --resume`) + replay de scrollback |
| Temas | 11 built-in (catppuccin, gruvbox, tokyo-night, dracula, nord, one-dark, solarized, kanagawa, rose-pine, vesper, terminal), auto light/dark, `[theme.custom]` por token |
| Outros | worktrees, notificações (toast/OS/som), keybindings com prefix, custom commands (popup/pane/shell), plugins (`mercury-plugin.toml`), skill para agentes |
| **Não tinha (motivou o fork)** | crons, usage/tokens/custo, % de contexto, rate limits, roteamento automático, busca em histórico de chats, auto improvement |

### 1.2 Agente de referência pesquisado (design de crons, roteamento, auto improvement)

Projeto de código aberto (MIT, Python 3.11+) usado só como referência de design pros recursos de crons/roteamento/auto-improvement abaixo — Mercury não reusa nem embute esse código.

**Crons**
- Agenda: one-shot relativo (`30m`), intervalo (`every 2h`), cron 5 campos (`0 9 * * *`), ISO timestamp; também por linguagem natural (agente chama tool `cronjob_manage`).
- Storage: `~/.<agente-referência>/cron/jobs.json` (escrita atômica) + ledger SQLite `executions.db` + `output/`.
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
- Nudges: a cada N turnos (default 10) um **agente de revisão em background** relê o transcript e decide se grava memória ou cria/edita skill (`~/.<agente-referência>/skills/<nome>/SKILL.md`). Prioriza corrigir skill existente a criar nova; "lições, não logs".
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
| **A. Plugin sobre o software base + daemon `mercuryd`** | entrega rápida; reaproveita 100% do software base | UI limitada a panes/overlays e tokens de sidebar; sem aba de crons nativa; depende do roadmap alheio |
| **B. Hard fork do software base (tag v0.9.3)** | PTY, ghostty-vt, persistência, detecção, temas, remote já prontos; UI nativa (gauges, aba de crons) | 11 MB de Rust para dominar; upstream rápido → rebase caro (mitigável) |
| **C. Do zero em Rust** | arquitetura 100% sua, código enxuto | refazer emulação de terminal, PTY, persistência, detecção — meses de trabalho só para chegar no nível atual do software base |

**Decidido: B — hard fork como base.** Código copiado da tag `v0.9.3` para esta pasta e modificado livremente (sem obrigação de acompanhar o upstream, sem vínculo de marca — ver Fase 1). Features novas em crates próprios (`mercury-usage`, `mercury-cron`, `mercury-router`, `mercury-learn`, `mercury-history`) por organização, não por compatibilidade com upstream.

---

## 3. Arquitetura proposta

```
┌──────────────── mercury (cliente TUI) ─────────────────┐
│ Sidebar: workspaces/tabs/agentes + gauges ctx/limites  │
│ Abas: Terminal | Crons | Usage | History | Learn       │
└─────────────────────────┬──────────────────────────────┘
                          │ unix socket (API herdada + mercury.*)
┌─────────────────────────▼──────────────────────────────┐
│ mercury server                                         │
│  ├─ núcleo herdado: PTYs, layout, sessões, detecção    │
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
| `calendar` | **US$ 1000/mês**, US$ 50/dia, 2M tokens/semana | dia do mês / dia da semana / hora + offset UTC configuráveis |
| `provider` | `five_hour`, `seven_day`, `spend_limit` (Claude), `primary/secondary/credits` (Codex) | `resets_at` reportado |

**Escopo** — cada janela se aplica a um nível:
`global` → `provedor` (claude/codex) → `workspace/projeto` → **`agente`** (um pane/sessão específica, ou um nome de agente como `reviewer`).

**Unidade do limite** — `usd` (do custo reportado), `tokens` (in/out/cache configurável), `percent` (só janelas do provedor).

Exemplo de config:
```toml
[[usage.budget]]
name = "claude-mensal"
scope = { provider = "claude" }
window = { kind = "calendar", period = "monthly", reset_day = 1, utc_offset = "-03:00" }  # offset fixo; nomes IANA ainda não suportados
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

### P0 — Base (paridade com o software base)
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

### P2 — Crons (aba dedicada)
11. Definição: cron 5 campos, `every 2h`, `30m`, ISO; via aba, CLI (`mercury cron add/list/edit/pause/run/rm/runs`) ou linguagem natural (skill para o agente criar jobs).
12. Alvo de execução: `claude -p` / `codex exec --json` headless **ou** prompt num agente/pane existente (`agent.prompt`) **ou** script puro (`no_agent`).
13. Campos: workdir, agente/modelo, skills/prompt files, `repeat`, timeout, entrega.
    Destinos desta entrega: **notificação local (toast/macOS) + log na aba Crons** e **pane/agente existente**. **Email/SMTP removido do escopo por correção do usuário.** Teams bidirecional (receber resposta e executar tarefas autorizadas) via **Workflows/Power Automate**, inclusive notificações e smoke real, fica no backlog a pedido do usuário; o webhook de saída já existente não é prova desse requisito futuro.
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
| 0. Spike (1–2 dias) | build da base nesta pasta, statusline bridge, parsers JSONL/rollout e chamada ao app-server com dados reais desta máquina | números batem com `/cost`/`/status` do Claude e `/status` do Codex |
| 1. Fork + rebrand | `mercury` rodando com paridade do software base, importando `session.json` dele | sessão atual do usuário reabre igual no Mercury |
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
1. **Abordagem: hard fork de um software base v0.9.3** (não é evolução/parentesco, ver Fase 1), modificado livremente; features em crates separados.
2. **Roteamento: os quatro modos**, todos no escopo — headroom de limite, regras por tarefa, fallback em erro e delegação entre agentes (P2, itens 16–19).
3. **Entrega de crons:** notificação local + log, pane/agente existente e Teams bidirecional; nenhum email/SMTP (correção do usuário em 2026-10-06).
4. **Auto improvement: totalmente automático**, com backup, auditoria e rollback (item 22).

5. **Repositório:** esta pasta, tudo local por enquanto (sem remote).
6. **Prioridade do roteamento:** fallback > headroom > regras > default.
7. **Usage:** janelas por hora, janelas de calendário (incl. mensal em USD), orçamento por agente, fetch orientado a eventos; sem tabela de preços própria.
8. **Licença:** arquivos de licença removidos.
9. **Desenvolvimento:** roteamento Sonnet 5 / Sonnet 5.5 / Opus 5.5 (seção 5.1).


## Instalação local do comando — spec 010 (2026-11-05)

Implementado `bash scripts/install-local.sh`: build release por padrão, perfil
`--profile dev` para preservar mercury-dev, prefixo local sem sudo, cópia
independente do checkout, diagnóstico PATH e uninstall restrito à instalação
gerenciada. Conflitos/redirects recusados; atualização por rename sem reiniciar
servidor. Nesta máquina o comando `mercury` está instalado com o build dev
existente; configuração e inventário preservados. README EN/PT e guia atualizados.

29 testes do instalador, checks de manutenção selecionados, builds reais dev e
release, smoke dos dois perfis, Clippy e formatação validados. Não há release
binária/feed novo, nem qualificação Linux/Windows adicional. Evidências e limites
em `docs/dev/local-install-010-review.md`; spec/plano em `specs/010-local-cli-install/`.


## Aprendizado contínuo — spec 011 (2026-11-05)

Learn/Knowledge default-on com escopo gerenciado e consentimento explícito para
modelos. Automação de sessão encerrada → captura persistente → nota proposed →
índice → GraphRAG; polling/debounce de alterações, orçamento diário, backoff,
replay e publicação atômica sem sobrescrever edição humana. Learn usa roteamento
Codex/Claude com custo desconhecido preservado como ausente. Estados e contadores
ficam visíveis no CLI/TUI; schema de status atualizado de forma opcional.

Validação: 4.156 testes workspace, checks focados finais e manutenção passaram,
assim como Clippy/build/fmt. Smokes reais sintéticos comprovaram Learn/apply/undo e
captura/grafo (9 entidades, 5 inferências, 2 resumos no corpus). Ativação pessoal
com backup, config check, instalação dev e handoff preservou pane/PIDs/crons/vault.
Detalhes e limites em docs/dev/continuous-learning-011-review.md e guia em
docs/knowledge/continuous-learning.md. Gates externos continuam dispensados.
