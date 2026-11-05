# Fase 7 — Auto improvement (spec, esclarecimento, plano e análise)

Estado: **esclarecida para o fechamento local**, 2026-10-07. Base: `PLAN.md` itens 20–24, §5–7 e `learn-phase7-research.md`. O usuário autorizou aplicação automática em Roteamento/Crons e vínculo explícito job→alvo Learn; retirou a regra de arquivamento por 30 dias. `[learn]` permanece opt-in (`enabled=false` por padrão), e quando habilitado a aplicação é automática, sem aprovação por item. Testes não devem ativar escrita em arquivos reais.

## Problema

Aprendizados das sessões hoje se perdem, e ajustes repetidos em instruções, skills, memória, roteamento e crons dependem de intervenção manual. O usuário decidiu aplicar melhorias automaticamente, mas exigiu backups, auditoria, undo e kill-switch. O processo lê texto de conversas e gera escrita em arquivos de alto impacto; conteúdo de transcript e resposta do reviewer não podem virar autoridade para executar comandos ou escolher qualquer path.

## Solução e fluxos

Com `[learn]` ativo, Mercury detecta fim de sessão/turno e marcos de N turnos, além de uma revisão semanal. Um worker lê um snapshot limitado da conversa principal Claude/Codex, excluindo tool output/thinking por padrão, e chama `claude -p` com modelo barato configurável, teto de custo, timeout, ferramentas desabilitadas (`--tools ""`), prompts de permissão negados e saída estruturada. Sessões do próprio Learn/crons não disparam revisão. Falha de auth/limite/reviewer não aplica nada e registra diagnóstico redigido.

O reviewer propõe operações estruturadas pequenas, com motivo e evidência resumida. Um validador determinístico, e não o modelo, decide allowlist, limites de tamanho, caminho canônico, symlink, extensão, conteúdo e conflito de hash. Alvos: `CLAUDE.md`/`AGENTS.md` nos projetos permitidos, skills nas raízes configuradas, arquivos de memória, e operações estruturadas allowlisted de Roteamento/Crons. O reviewer nunca fornece TOML completo nem escolhe caminho de arquivo. Roteamento só pode mudar a seção `[route]` do `config.toml`, preservando as demais seções e validando a configuração resultante. Cron só pode alterar um job existente identificado por UUID: o alvo `cron-job` usa esse UUID como ID, aponta ao `jobs.toml` ativo e apresenta ao reviewer apenas agendamento, habilitado e timeout. O patch JSON aceita só esses três campos; o writer mescla os bytes completos sob um lock compartilhado com o Scheduler, com hash, backup, ledger e undo. O Scheduler recarrega o arquivo antes de qualquer mutação e após notificação de Learn/undo. Job com run ativo não deve ser alterado; o writer precisa consultar o ledger de execuções antes da substituição. Não pode criar script, prompt, destino Teams ou caminho arbitrário. Demais campos exigem nova especificação. Não executar shell, não alterar hooks/credenciais/arquivos fora da lista, não commit/push. A aplicação faz snapshot privado **antes** da escrita, registra ledger durável por item e grava atomicamente; se alguma etapa falhar, preserva o estado anterior ou deixa recuperação explícita no ledger. Arquivos Git ficam alterados e não commitados.

`mercury learn list/show/diff/undo <id>` e undo por data inspecionam ledger e revertem cada item somente quando o hash atual corresponde ao conteúdo aplicado; conflito exige resolução humana. A aba Learn mostra gatilhos, motivo, alvo, diff redigido quando necessário, backup e status de undo. `learn.enabled=false` impede novos reviews e aplicações, sem apagar ledger/backups. Curator mescla apenas duplicatas comprovadas de skills/memórias por operações reversíveis; não existe regra de arquivamento por desuso. Métricas por tarefa usam tokens/custo realmente reportados e estado de conclusão identificável. O job headless pode declarar um `learn_target_id` allowlisted; a associação ao ID e ao fingerprint do caminho do alvo é capturada na conclusão do run e persiste no outbox. Mudança posterior da associação não reatribui a amostra já persistida, mas uma mudança durante a execução longa pode mudar o vínculo capturado na conclusão. Sem associação explícita, amostras continuam `observed` e a UI mostra “dados insuficientes”. Com associação, a fronteira baseline/after é o item `applied` durável do ledger para o mesmo alvo, e só amostras posteriores a ela são `after`; a comparação é descritiva, nunca causal.

## Critérios de aceite

1. Um fim de sessão e um marco de N turnos disparam no máximo um review por chave estável, inclusive após restart; cron semanal roda sem recursão. Disparos sem sessão/transcript confiável não aplicam mudança.
2. Reviewer recebe texto somente por stdin, sem conteúdo em argv/log, com ferramentas desabilitadas, timeout/limite de output/custo. Saída inválida, auth/limite ou prompt injection não escrevem arquivo.
3. Proposta allowlisted válida cria backup privado e registro de auditoria antes de alterar alvo; o alvo é escrito atomicamente, permanece não commitado em Git e é visível em CLI/aba Learn.
4. `learn undo <id>` e undo por data restauram exatamente o backup quando o hash aplicado ainda confere. Alteração humana detectada antes da substituição, symlink, path traversal, backup ausente e erro de disco falham fechados sem sobrescrever dados; a corrida microscópica de escritor externo entre a última checagem e o rename é risco residual documentado.
5. Kill-switch para novos disparos e aplicação; config inválida/allowlist vazia não gera escrita. Limites de tamanho, frequência, concorrência e orçamento têm testes.
6. Curator mescla apenas duplicatas comprovadas de skills/memórias de modo reversível, sem apagar fontes silenciosamente; métricas distinguem dados reais, parciais e ausentes.
7. API/CLI/schema, testes de crate e integração com fixtures, smoke isolado sem credenciais reais, revisão consolidada e validação local passam. O usuário fará os smokes com modelos reais e inspeção manual; SonarQube/Veracode/Wiz e cobertura quantitativa foram dispensados pelo usuário nesta entrega, sem declaração falsa de PASS. Não commitar automaticamente.

## Trade-offs e fora de escopo

- Prioridade a reversibilidade e escopo estrito: propostas fora da allowlist são rejeitadas, mesmo quando parecem úteis. Reviewer sem ferramentas perde contexto do filesystem, mas não ganha execução arbitrária.
- Não inferir qualidade a partir de custo isolado; só comparar tarefas com baseline/resultado conhecido. Sem alteração automática de código-fonte, pipelines, segredos ou permissões.
- `learn.enabled` é opt-in por padrão. A aplicação automática quando ativa está decidida; o opt-in impede escrita surpresa na primeira execução.
- Sem distribuição Windows validada enquanto os notices/licenças de terceiros estiverem ausentes. A dispensa de testes externos não é dispensa de obrigações de distribuição.

## Plano de implementação e dependências

O valor padrão de `learn.enabled` é opt-in. Todas as etapas
abaixo usam configuração explícita nos testes e no smoke; nenhuma escrita
automática em arquivos reais do usuário será feita para confirmar o default.

**Grupo A — núcleo reversível, sem modelo nem gatilhos.** Fixar um formato de
proposta estrito (`target` escolhido de IDs previamente configurados,
`operation`, conteúdo completo limitado, razão e referência da sessão). Criar
validação de caminhos sobre a allowlist do usuário: caminho absoluto, raiz e
alvo existentes/canônicos quando aplicável, nenhum componente symlink, arquivo
regular, extensão/tipo esperado, tamanho máximo e hash do estado lido. Guardar
backup privado com `sync_all` e registrar intenção no ledger antes de escrever;
escrever temporário no diretório do alvo, conferir hash/identidade de novo,
renomear atomicamente e registrar conclusão. O ledger deve distinguir
`proposed`, `prepared`, `applied`, `conflict`, `failed` e `undone`, para que uma
interrupção entre passos seja recuperável/visível. Undo reutiliza a mesma
validação e recusa hash divergente. Nenhuma operação roda shell ou Git.

**Grupo B — extração e reviewer isolado.** Leitor limitado de transcripts
Claude/Codex que retorna somente turnos conversacionais necessários e um
marcador de truncamento. O `claude -p` recebe o snapshot por stdin, sem
ferramentas, sem persistência de sessão, com orçamento/tempo/output limitados
e schema JSON; o processo inteiro e filhos devem ser encerrados no timeout.
Resposta inválida ou incompleta não chega ao núcleo de escrita. O reviewer
seleciona apenas IDs de alvo da allowlist; ele nunca fornece caminhos que o
executor aceite diretamente.

**Grupo C — gatilhos e dedup.** Associar sessão reportada ao pane, registrar
marcos de N turnos e término sem depender exclusivamente de um hook Claude;
Stop/SessionEnd, quando instalados, são sinais adicionais idempotentes. O
worker tem fila e concorrência limitadas, chaves persistentes
`provider/session/reason/turn_mark` e backoff de falha. Review semanal usa um
gatilho interno, não cria um cron normal que possa disparar a si próprio.
Sessões Learn e cron são excluídas por origem, não apenas pelo nome do pane.

**Grupo D — superfícies e curator.** Expor list/show/diff/undo e rollback por
data em API/CLI/aba Learn com dados redigidos; regenerar schema. Curator apenas
propõe mescla reversível de duplicatas sobre IDs allowlisted. Métricas guardam
custo/tokens observados e vínculo explícito job→alvo, com marcador de
qualidade/baseline indisponível quando não houver comparação defensável.
Editar Roteamento/Crons exige operação tipada, validação determinística e o
mesmo contrato de backup/ledger/undo; o modelo não fornece arquivo TOML.

**Grupo E — revisão e validação.** Revisão consolidada dos grupos, testes de
crate/integrados com diretórios temporários, falhas injetadas em cada passo de
backup/ledger/rename/undo e smoke sem credenciais reais. Inspeção TUI e smoke
real com Claude/Codex ficam a cargo do usuário. Scanners externos e cobertura
quantitativa ficam como exceção documentada, não como PASS.

### Matriz de análise antes de implementar

| Invariante | Evidência mínima | Falha esperada |
|---|---|---|
| Backup e ledger precedem cada escrita | teste de interrupção após prepare | alvo original intacto ou recuperação explícita |
| Reviewer não define autoridade | proposta com path traversal, symlink, alvo ausente e operação extra | rejeição sem escrita |
| Conteúdo concorrente é preservado | editar alvo entre leitura e aplicação/undo | conflito, nunca overwrite silencioso |
| Trigger não duplica nem recursa | eventos repetidos + restart + sessão Learn/cron | uma revisão ou nenhuma, conforme chave |
| Limites contêm custo e recursos | transcript/saída grandes, timeout, fila cheia | truncar/rejeitar sem travar servidor |
| Ledger permite auditoria e undo | aplicação + restart + undo por ID/data | bytes originais restaurados se hash conferir |

Risco residual: conferir hash imediatamente antes do rename reduz, mas não
elimina, corrida com um editor externo que escreva no mesmo instante; não
declarar garantia de compare-and-swap universal de filesystem. Exigir testes
de conflito e documentar esse limite no review.

### Sequência de tarefas

1. Definir config `[learn]`, allowlist, limites, saída estruturada e chave de dedup; testes de validação e de parser de transcript.
2. Criar `mercury-learn`: backup privado, ledger transacional, aplicação allowlisted, conflito e undo com testes de falha injetada.
3. Runner `claude -p` sem tools, com stdin, timeout/custo/output limitados; revisão simulada primeiro, sem chamada real em CI.
4. Gatilhos de sessão/turnos/semana e dedup persistente no servidor, com worker e sem bloquear PTY/TUI; testes de restart e não recursão.
5. API/CLI/aba Learn, curator reversível, métricas de eficácia, schema e documentação.
6. Revisão e testes consolidados locais; roteiro de smoke real para o usuário e exceções externas documentadas. Nenhum commit automático.

Dependências: Claude CLI local autenticado para smoke real do usuário; transcripts Claude/Codex acessíveis; diretório de dados privado e gravável; API Mercury ativa. Scanners externos não são dependência desta entrega por decisão do usuário.

## Análise de consistência

- O plano exige Stop/SessionEnd, N turnos e cron semanal, mas a integração Claude atual remove hooks Mercury legados desses eventos. Será preciso novo hook de Learn ou dedução idempotente via estado/parse, sem quebrar hooks de terceiros. Codex não tem contrato equivalente no repo; não assumir evento nativo.
- `mercury-history` indexa mensagem textual, não guarda role/contador de turnos na API. Learn precisa de leitor próprio limitado ou extensão tipada da crate, sem exportar transcrição inteira a clientes.
- Cron fornece padrão de executor e agendamento, não ledger de mudanças; reutilizar suas salvaguardas de subprocesso, mas não criar jobs normais auto-recursivos.
- Alterações automáticas em config podem disparar reload e outro trigger. A gravação precisa ser idempotente, com origem Learn no ledger e supressão de auto-review do próprio trabalho.
- `config.toml` e `jobs.toml` têm proprietários de escrita diferentes. Edição Cron usa lock em processo compartilhado com Scheduler e refresh-before-mutate; não escrever `jobs.toml` sem verificar que o job não tem run ativo. Undo também usa o lock e sinaliza reload. Falha de reload após escrita deixa estado degradado visível, sem alegar aplicação efetiva.
- A conclusão do run deve congelar ID do alvo e fingerprint do caminho no outbox. Alterações posteriores da associação e migrações não podem atribuir retroativamente observações antigas; sem vínculo ou fronteira `applied` única, a fase segue `observed`.
- Exceção de validação externa (decisão do usuário, 2026-10-07): SonarQube, Veracode, Wiz, cobertura quantitativa e smokes reais não serão executados pelo Codex. Risco aceito: vulnerabilidades/erros de integração podem não ser detectados localmente; registrar como não executado, sem inventar PASS.
