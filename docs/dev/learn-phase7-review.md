# Fase 7 — revisão e gate local

Estado vigente em 2026-10-07: **T001–T021 implementadas localmente** (inclusive T019b e T020a). T022 exige somente a inspeção visual e os smokes reais assumidos pelo usuário. T023 foi dispensada pelo usuário quanto a SonarQube/Veracode/Wiz e cobertura quantitativa: **não executados, não PASS**. O histórico abaixo registra resultados incrementais, inclusive afirmações que foram superadas por seções posteriores. O núcleo reversível, gatilhos, deduplicação exata e amostras observadas foram exercitados em fixtures temporárias; não houve commit, reviewer de modelo real nem escrita automática em arquivos reais. Teams bidirecional está no backlog e não há entrega por e-mail.

## Grupo revisado: contrato e política de alvos

`mercury-learn` entrou no workspace sem ser conectado ao servidor. A política recebe um `enabled` explícito, identifica o alvo somente por ID exato pré-configurado, limita conteúdo e metadados, exige arquivo regular existente e rejeita symlink no alvo ou em qualquer ancestral. O modelo não fornece um path executável ao writer. Substituição de `config.toml` inteiro é recusada até existir um merger restrito a seções permitidas.

Revisão consolidada: **PASS para continuar T003–T005**, não para ativar Learn. Não foi encontrado defeito bloqueante no contrato isolado. Riscos que exigem o próximo grupo: a checagem atual é somente pré-validação, portanto o writer deve revalidar identidade/hash imediatamente antes do rename; ledger, backup, recuperação e undo ainda não existem. A corrida residual com outro processo permanece conforme a spec. O valor padrão de `[learn]` continua aguardando confirmação, mas não afeta este núcleo com flag explícita.

Testes unitários de `policy.rs`: 8/8 PASS, incluindo alvo exato sem escrita, kill-switch, ID desconhecido, política malformada/duplicada, limites de tamanho, symlink no alvo e ancestral e bloqueio de config inteiro. `cargo clippy --offline -p mercury-learn --all-targets -- -D warnings`, `rustfmt --edition 2021 --check` nos arquivos da crate e `git diff --check` passaram. Não há DB/API neste grupo; testes integrados de storage/undo são obrigatórios em T005. Um rerun amplo do workspace será necessário quando a crate for integrada ao servidor.

## Grupo revisado: backup e ledger (T003)

`storage.rs` cria diretórios privados, banco SQLite versionado com `synchronous=FULL` e backup `create_new`/0600 sincronizado antes da inserção de um item `prepared`. O ledger guarda IDs, hashes, origem, motivo e estados de recuperação; `read_backup` verifica nome, tipo, tamanho e hash. A revisão achou dois pontos antes do fechamento: entrada direta do storage sem limites para metadados e consulta SQL com tupla excessivamente complexa. Foram corrigidos com limites explícitos, path UTF-8 e `RawChangeRecord`; o acesso por symlink em ancestral também foi bloqueado antes da criação de diretórios. **PASS para prosseguir para T004**, não para aplicar automaticamente: ainda faltam revalidação de hash/identidade, rename atômico, reconciliação de crash e undo.

`cargo test --offline -p mercury-learn`: 8 testes de política + 7 testes de integração de storage passaram, incluindo restart, integridade, permissões, symlink e transições. `cargo clippy --offline -p mercury-learn --all-targets -- -D warnings` passou após correção; rustfmt limitado à nova crate e `git diff --check` passaram. Sem medição quantitativa de cobertura. O próximo grupo requer fault injection no writer e testes de rollback.

## Grupo revisado: aplicação e undo atômicos (T004, T005 parcial)

`changes.rs` exige `Policy::validate(enabled, proposal)` antes da escrita. Lê arquivo regular com `O_NOFOLLOW`, limita bytes, guarda identidade de dispositivo/inode, cria backup+ledger `prepared`, escreve temporário no mesmo diretório, sincroniza, revalida bytes/identidade e renomeia. Undo lê backup validado, exige hash aplicado, registra `undo_prepared` antes de restaurar e preserva edições humanas divergentes. Operações pendentes são reconciliadas por hash em restart/próxima aplicação, sem escrever durante a recuperação; schema v1 migra para v2. Somente Unix é habilitado: não se afirma segurança equivalente de ACL/identidade em Windows.

Revisão: um pending anterior poderia coexistir com uma nova aplicação; apply/undo agora reconciliam pendências antes de começar. O leitor de backup passou a abrir com `O_NOFOLLOW` e conferir inode, e diretórios do ledger são sincronizados. **PASS local para seguir à matriz de falhas T005**, não para conectar a gatilhos reais. Riscos residuais: não existe CAS de filesystem contra outro processo entre a última checagem e o rename; ACLs/xattrs específicos do alvo não são preservados, embora os bits Unix sejam; falhas pós-rename deixam estado pendente para reconciliação. O runtime Learn ainda não está integrado e config inteiro continua bloqueado.

Última rodada: `cargo test --offline -p mercury-learn` 33/33 PASS (12 unitários, 12 integração de changes, 9 de storage); Clippy `-D warnings` e `git diff --check` PASS. Testes cobrem ciclo apply/undo com restart e bytes/permissões exatos, kill-switch, conflito humano, backup ausente, reentrada pendente, migração v1→v2, edição/troca de inode após temporário sincronizado e falhas de backup, inserção no ledger, criação de temporário e etapa de rename injetada. Um undo interrompido ou concorrente também é reconciliado ou marca conflito sem sobrescrever edição humana. A revisão corrigiu backup órfão após erro de inserção no ledger. **PASS local da foundation**; ainda faltam reviewer/gatilhos/superfícies, smoke integrado e cobertura quantitativa.

## Grupo revisado: transcript conversacional (T006)

`transcript.rs` lê cabeçalho limitado e tail JSONL de até 4 MiB por padrão (máximo configurável de 8 MiB), exige ID esperado confiável no conteúdo e, para Claude, também no nome do arquivo; rejeita symlink e mudança de identidade do arquivo durante a leitura. Retém no máximo 64 mensagens e 64 KiB de texto por padrão, com teto por linha/mensagem. Apenas blocos textuais de usuário/assistente entram; tool result/output, thinking, telemetry, sidechains e mensagens sintéticas são excluídos. Streaming repetido é deduplicado; truncamento ou linha malformada anterior desativa contagem exata de turnos, e última linha JSON incompleta falha fechada.

Revisão consolidada: **PASS local para seguir ao runner T007**, sem envio a modelo real. A primeira versão não reaplicava o limite total quando uma mensagem duplicada crescia e podia tratar texto por mensagem truncado como contagem completa; ambos os pontos foram corrigidos e testados. Um arquivo ativo com JSONL válido ainda pode ser lido: o gatilho T009–T010 deve provar término/estado elegível antes do review. T007 deve evitar logs do transcript, limitar saída/custo e definir tratamento de segredos em conteúdo conversacional antes de transmissão ao Claude CLI.

`cargo test --offline -p mercury-learn`: 44/44 PASS (33 foundation + 11 transcript); `cargo clippy --offline -p mercury-learn --all-targets -- -D warnings` PASS. Fixtures cobrem Claude/Codex, papéis, dedup, identidade ausente/trocada, symlink, tail grande, linha oversized, Unicode/control chars, limites de texto/mensagens e JSONL final incompleto. Nenhum transcript real foi lido neste grupo.

## Grupo revisado: runner isolado do revisor (T007)

Revisão consolidada: **PASS local para seguir à integração**, sem chamada a modelo real nem escrita de arquivo de usuário. O runner executa `claude -p` com ferramentas/MCPs desabilitados, diretório privado, entrada por stdin, schema de saída, timeout, limites de entrada/saída e orçamento; valida envelope, custo e IDs antes de produzir propostas para a política determinística. Uma primeira falha de fechamento do pipe de stdin causou timeout no teste de sucesso e foi corrigida. Também foram reforçados limites na fronteira pública e a rejeição de diretório com ancestral symlink. Risco residual: a compatibilidade exata dos flags/schema com uma chamada real ao Claude CLI, a eficácia semântica da detecção de segredos e o isolamento efetivo do processo requerem smoke/manual review antes de ativação; nenhum deles é tratado como gate PASS.

`cargo test --offline -p mercury-learn`: 49/49 PASS (44 anteriores + 5 de reviewer); `cargo clippy --offline -p mercury-learn --all-targets -- -D warnings` e `git diff --check` PASS. Os testes de contrato cobrem proposta válida, origem derivada pelo Mercury, não exposição do texto em argv, resposta fora do orçamento, alvo desconhecido, entrada sensível, stdout excessivo, timeout e workdir symlink. O gate SonarQube/Veracode/Wiz/cobertura permanece FAIL/não verificável; sem commit.

## Grupo revisado: configuração `[learn]` (T008)

Revisão consolidada: **PASS local para seguir aos gatilhos**, sem worker conectado. A seção requer opt-in explícito, alvos exatos, limites válidos e IDs/caminhos/tipos aceitos pela política do core. Se `[learn]` falha no reload, só essa seção volta ao default desligado e as seções irmãs permanecem válidas. `config-section` é rejeitado porque o merger de seções ainda não existe; não há falsa autorização para sobrescrever `config.toml` inteiro. O default `enabled=false` continua uma hipótese de segurança reversível aguardando resposta do usuário.

`cargo test --offline -p mercury --bin mercury config::`: 195/195 PASS, incluindo cinco casos novos de parsing/reload; `cargo clippy --offline -p mercury --bin mercury -- -D warnings` e `git diff --check` PASS. O teste inicial `--lib` não era aplicável (o pacote `mercury` só tem binário); o comando correto foi executado em seguida. Gate externo segue FAIL/não verificável; nenhum commit.

## Grupo revisado: dedup durável e fila limitada (T009)

Revisão consolidada: **PASS local do componente para seguir à ligação dos sinais T010**. O worker aceita eventos apenas quando habilitado, exclui origens Learn/cron, usa fila limitada e não bloqueante e reivindica a chave no ledger SQLite antes de chamar o handler. A reivindicação sobrevive ao restart: evento interrompido fica `claimed` (indeterminado), sem replay automático e sem duplicar revisão. Falhas terminadas ficam `failed`; diagnósticos do worker não incluem transcript/segredos. A fila ainda não recebe sinais reais e o handler real review/apply ainda não está ligado, logo a T009 isolada não ativa escrita automática. Há `#[allow(dead_code)]` temporário no módulo até T010 consumi-lo; remover na ligação.

`cargo test --offline -p mercury-learn`: 52/52 PASS, incluindo dois testes novos de claim/estado; `cargo test --offline -p mercury --bin mercury app::learn::tests`: 2/2 PASS. `cargo clippy --offline -p mercury-learn -p mercury --all-targets -- -D warnings` e `git diff --check` PASS. O primeiro build do worker apontou uma anotação de tipo ausente, corrigida antes da rodada verde. Gate SonarQube/Veracode/Wiz/cobertura segue FAIL/não verificável, sem commit.

## Preparação dos sinais (T010 em andamento)

Inspeção da integração atual confirmou: hooks gerenciados de Claude/Codex reportam `SessionStart`, mas não fim; a migração remove hooks antigos de `Stop`/`SessionEnd`; `Idle` visual não prova fim de sessão. A [referência oficial de hooks Claude](https://code.claude.com/docs/en/hooks) confirma `SessionEnd` com `session_id` e `transcript_path`, incluindo saídas por `/clear` e troca por `/resume`, e timeout curto. O plano agora exige evento de fim explícito ou saída de processo com identidade comprovada; marco de N turnos exige contagem exata (não truncada). Construtores tipados de chaves `session_end`, `turn_milestone` e `weekly_due` foram adicionados ao worker, sem instalar hooks nem acionar revisões reais. `cargo test --offline -p mercury --bin mercury app::learn::tests`: 3/3 PASS; Clippy do binário `-D warnings` e `git diff --check` PASS. T010 permanece pendente até conectar sinais, reader e handler review/apply com testes de não recursão.

Na continuação, os gatilhos de sessão passaram a transportar o provedor, ID, caminho absoluto do transcript e, para marcos, a contagem exata de turnos. Caminho relativo ou com componentes `.`/`..` é recusado antes de entrar na fila; o leitor de transcript ainda fará a validação de identidade e symlink ao consumir o evento. `cargo test --offline -p mercury --bin mercury app::learn::tests`: 3/3 PASS. Isto é somente contrato para T010: ainda falta verificar o sinal contra a sessão observada, instalar um produtor de fim confiável e ligar o handler real. Nenhuma revisão automática foi disparada.

O core agora expõe `review_snapshots`: lê cada alvo exato com os mesmos cuidados do writer, recusa Learn desligado, alvo não textual e substituição integral de config, e associa conteúdo UTF-8 ao hash calculado antes de chamar o modelo. O handler futuro deve manter esse hash fora da resposta do modelo e passá-lo a `Changes::apply`. `cargo test --offline -p mercury-learn`: 53/53 PASS; `cargo clippy --offline -p mercury-learn -p mercury --all-targets -- -D warnings`: PASS. Ainda não há handler ativo, hook de fim nem smoke de modelo real. Uma invocação de `cargo fmt` neste grupo também formatou trechos não relacionados já modificados no workspace; a alteração é mecânica, mas o escopo do diff precisa ser conferido antes de um commit. O gate externo permanece FAIL/não verificável.

## Pipeline isolado de sessão (T010 parcial)

`pipeline.rs` compõe leitor de transcript, snapshot de alvos, revisor isolado e writer com hash pré-revisão. O chamador precisa provar que a sessão terminou; a pipeline não infere isso de estado visual. Marcos exigem contagem exata igual à do sinal, portanto transcript truncado/incompleto não autoriza revisão de marco. Sem `enabled`, não há leitura ou chamada de modelo. O retorno expõe IDs aplicados e custo observado; erro de aplicação informa quantos itens já haviam sido aplicados, sem esconder eventual resultado parcial no ledger.

Teste integrado com executável falso e arquivos temporários confirmou aplicação+undo byte a byte, supressão antes de chamar modelo para Learn desligado/marco incorreto e conflito quando uma edição humana ocorre durante a revisão. **PASS local do pipeline, não de T010**: faltam ligar o handler ao worker do servidor, autenticar sinais de fim, marcos e semanal, configurar o CLI real e fazer smoke autorizado. `cargo test --offline -p mercury-learn`: 56/56 PASS; `cargo clippy --offline -p mercury-learn -p mercury --all-targets -- -D warnings`: PASS. Nenhum alvo real foi escrito.

## Ponte do servidor e SessionEnd Claude (T010 parcial)

O worker configurado agora chama o pipeline em thread própria e reconsulta uma flag compartilhada de habilitação antes de aplicar cada proposta. O servidor só inicializa Learn quando `[learn].enabled=true`, busca o CLI Claude no PATH e exige diretório de revisão privado; se não estiver disponível, mantém a revisão inativa e registra diagnóstico. Reload que desliga Learn invalida o worker e impede nova aplicação após o retorno do modelo. Teste com executável falso e alvo temporário passou pelo worker/ledger. O hook Claude `SessionEnd` envia apenas ID e caminho; o endpoint local exige que ambos coincidam com o `SessionStart` aceito na pane e que a sessão ainda seja a identidade corrente. Hook legado de `release` é migrado, hooks de terceiros são preservados e a instalação é idempotente nos casos testados.

Evidência: `cargo test --offline -p mercury-learn` **57/57 PASS**; `app::learn::tests` **4/4**, `app::api::learn::tests` **1/1**, `integration::claude_settings::tests` **11/11** (incluindo migração de `SessionEnd` com hook de terceiros), teste de round-trip/schema **1/1**; schema API regenerado e teste sem modo de escrita PASS; `cargo clippy --offline -p mercury-learn -p mercury --all-targets -- -D warnings` PASS; `bash -n` do hook PASS. A suíte `tests/cli.rs` exclui macOS por `cfg`; um smoke equivalente do hook `SessionEnd` com socket Unix temporário passou fora do sandbox restrito. Não houve instalação em config real nem chamada ao modelo. T010/T011 **continuam pendentes**: sinal de fim Codex, marcos N-turnos, revisão semanal, não recursão ponta a ponta e smoke de provider real. A ordem real entre hook de fim e limpeza da identidade da pane também precisa de verificação manual; um evento tardio falha fechado. Gate externo SonarQube/Veracode/Wiz/cobertura segue FAIL/não verificável.

Verificação adicional do reload: 15 testes existentes de reload PASS; novo teste de desativação de Learn PASS após canonicalizar o diretório temporário da fixture (a primeira tentativa foi corretamente rejeitada por ancestral `/var` simbólico no macOS). Isso verifica a remoção do worker ao desligar a seção, enquanto o teste integrado da pipeline verifica que desabilitar durante a revisão impede a aplicação. Nenhum alvo real foi usado.

Gate PagueVeloz: **FAIL/não verificável**. Não há MCP SonarQube/Veracode/Wiz disponível neste ambiente nem cobertura quantitativa de projeto; `cargo llvm-cov`/`llvm-tools` também não estão instalados. Não declarar a fase concluída nem commitar antes de obter resultados exigidos ou exceção explícita documentada.

## Preparação do sinal Codex (T010 ainda parcial)

O hook `SessionStart` do Codex já recebia `session_id` e `transcript_path`, mas descartava o caminho ao reportar ao Mercury. Os scripts Unix e PowerShell agora enviam ambos; o servidor retém o caminho apenas após aceitar o reporte como sessão corrente e mais recente. O estado passa a indexar transcripts por pane e provedor, sem confundir Claude com Codex. Um teste verifica que um reporte Codex atrasado não substitui o caminho aceito. O detector de processo já emite `StateChanged(process_exited=true)` quando observa retorno ao shell: agora esse evento enfileira um único review somente se for Codex, posterior ao SessionStart aceito e com ID ainda corrente na pane. `Idle` visual não dispara. Teste com worker/ledger temporário passou pelos dois sinais e conferiu a chave `codex:codex-session:end`. **T010 continua parcial**: faltam N-turnos, semanal, confirmação de processo real e smoke de modelo; um evento de fim anterior ao reporte falha fechado. Nenhum modelo ou alvo real foi acionado nesta etapa.

Evidência incremental: `cargo test --offline -p mercury --bin mercury app::api::learn::tests` **3/3 PASS**, `cargo clippy --offline -p mercury -p mercury-learn --all-targets -- -D warnings` PASS, `bash -n` do hook Unix e `git diff --check` PASS. O teste do hook Unix em `tests/cli/hooks.rs` cobre o caminho reportado, mas a suíte de CLI permanece excluída em macOS; smoke equivalente de caminho Codex, hook PowerShell e processo real continuam pendentes. Gate externo inalterado: FAIL/não verificável.

## Revisão incremental: vínculo entre snapshot e aplicação (pré-T010)

A revisão do fluxo automático encontrou uma lacuna: `Changes::apply` validava o alvo no momento da escrita, mas não exigia que o conteúdo ainda fosse o mesmo enviado ao revisor. A assinatura agora exige o hash do snapshot confiável capturado antes da revisão; divergência retorna `Conflict` antes de backup, ledger ou escrita. Esse hash não pode vir da resposta do modelo. Um teste de integração altera o arquivo depois do snapshot e confirma que a edição humana é preservada. **PASS local desse controle; T010 continua pendente** até o handler obter o snapshot, transportar o hash confiável e conectar sinais reais.

`cargo test --offline -p mercury-learn`: 52/52 PASS (12 unitários, 13 `changes`, 5 reviewer, 11 storage, 11 transcript); após fortalecer a asserção do ledger, `cargo test --offline -p mercury-learn --test changes`: 13/13 PASS. `cargo clippy --offline -p mercury-learn -p mercury --all-targets -- -D warnings`, `cargo fmt -p mercury-learn --check` e `git diff --check` passaram. O `cargo fmt --all --check` do workspace ainda aponta formatação preexistente em `ghostty-vt` e `mercury-usage`; arquivos fora deste grupo não foram alterados por formatação. Smoke real e gate externo continuam pendentes.

## Preparação da inspeção/undo por data (T012–T014 em andamento)

O ledger privado agora expõe `list_recent` com limite obrigatório de 1–100 e `list_created_between` em intervalo UTC semiaberto com máximo de 1–1000. A consulta por data lê no máximo `max+1` IDs e devolve `TooLarge`, nunca uma lista parcial que poderia fazer um comando de undo por dia ignorar itens silenciosamente. Ambas retornam registros em ordem inversa de inserção, preservada na migração v1→v2; não acessam backups nem alteram arquivos. O review consolidado não encontrou bloqueador nesse núcleo; a interface pública ainda precisa definir redaction, paginação/limites e executar undo fora do loop principal. T012–T014 **não estão concluídas**.

Evidência: `cargo test --offline -p mercury-learn --test storage` **12/12 PASS**; suíte da crate **58/58 PASS**; `cargo clippy --offline -p mercury-learn --all-targets -- -D warnings` e `git diff --check` PASS. Não houve medição de cobertura. Gate SonarQube/Veracode/Wiz/cobertura: **FAIL/não verificável**, ferramentas não configuradas neste ambiente; sem commit.

## Primeira superfície de inspeção: `learn.list` (T012–T014 parciais)

O método tipado `learn.list` consulta o ledger no servidor headless por tarefa bloqueante limitada a oito requisições pendentes, sem ocupar o loop de UI. Resposta e `mercury learn list [--limit 1..100] [--json]` mostram apenas ID, alvo lógico, status e timestamp; paths, backups, motivo do modelo e conteúdo não saem no payload. Sem ledger, a consulta retorna lista vazia sem criar diretórios. Schema público foi regenerado e a ajuda da CLI atualizada. `show`, `diff`, status agregado, undo por ID/data e aba Learn ainda faltam, portanto T012–T014 e FR-007 permanecem incompletos.

Revisão consolidada: PASS local para esta superfície de listagem; o primeiro Clippy detectou apenas posicionamento do módulo de testes após funções auxiliares, corrigido antes da rodada verde. Testes: headless Learn **2/2**, contrato `learn.list` **1/1**, testes do schema **50/50**, ajuda CLI **18/18**, parser CLI **1/1**, schema regenerado e verificado sem modo de escrita **PASS**. `cargo clippy --offline -p mercury -p mercury-learn --all-targets -- -D warnings` e `git diff --check` PASS. Falta smoke de socket da nova rota e o gate externo continua **FAIL/não verificável**; sem commit.

## Show e undo por ID (T012–T014 ainda parciais)

Os métodos tipados `learn.show` e `learn.undo` e os comandos `mercury learn show/undo CHANGE_ID` foram ligados ao servidor headless. `show` devolve metadados e hashes, mas nunca caminho, backup, texto do modelo ou motivo livre (marcado `reason_redacted=true`). Undo usa o núcleo já verificado: somente registro aplicado/conflito cujo arquivo ainda tem o hash pós-aplicação é restaurado com os bytes exatos do backup; edição humana divergente retorna `learn_conflict` sem sobrescrita. Os erros de ID inválido, item ausente, estado inválido e resultado incerto são distintos. Aplicação automática e undo compartilham agora um mutex de processo sobre a transição filesystem/ledger; concorrência com escritor externo/outro processo segue risco residual já documentado. O endpoint pode ser consultado com Learn desligado, para permitir reversão após kill-switch.

Revisão/testes: fixture de API com aplicação real em arquivo temporário passou por `show`, undo exato, segunda tentativa inválida e conflito após edição humana; **4/4** testes headless Learn. `cargo test --offline -p mercury-learn` **58/58**, testes focados Learn do binário **16/16**, schema **51/51**, ajuda CLI **18/18**, Clippy `--all-targets -D warnings`, schema regenerado e `git diff --check` PASS. Não houve alteração em alvo real nem chamada a modelo. Ainda faltam diff redigido, status agregado, undo por data, CLI/TUI completos, smoke de socket e gate SonarQube/Veracode/Wiz/cobertura. **Não declarar T012–T014 concluídos nem commitar.**

## Undo por data UTC com resultados por item (T012–T014 ainda parciais)

`learn.undo_date` e `mercury learn undo --date YYYY-MM-DD` usam intervalo UTC validado. O servidor exige o conjunto completo de no máximo 1000 registros antes de qualquer restauração; acima disso retorna `learn_date_too_many` e não inicia o lote. Os registros são processados na ordem inversa de inserção do SQLite, necessária para desfazer revisões sucessivas do mesmo alvo (inclusive timestamp igual ou relógio ajustado). Cada ID retorna `undone`, `conflict`, `skipped`, `failed` ou `uncertain`; erro não recuperável interrompe o lote com `remaining_count` explícito. Conflitos humanos preservam bytes e não escondem restaurações anteriores. O lote não é transação atômica entre arquivos: resultado parcial é comunicado ao usuário; na CLI, conflito/falha gera exit code 1.

Revisão local: PASS para o contrato de undo por data, não para a fase completa. Testes headless Learn **7/7** incluem duas alterações no mesmo arquivo, conflito humano e interrupção por backup ausente com `remaining_count=1`; storage **12/12**, schema **52/52**, ajuda CLI **18/18**, parser CLI **3/3**, Clippy `--all-targets -D warnings` e `git diff --check` PASS. Schema público regenerado e verificado em modo leitura. Não houve escrita em arquivos reais, chamada a modelo ou smoke de socket. Faltam diff redigido, status agregado, aba Learn, sinais de N-turnos/semanal e gate externo SonarQube/Veracode/Wiz/cobertura, **FAIL/não verificável**.

## Status agregado do ledger (T012–T014 ainda parciais)

`learn.status` e `mercury learn status [--json]` consultam contagens de todos os estados de mudanças e gatilhos diretamente no SQLite, sem depender da página limitada de `learn.list`. A resposta inclui apenas contadores e `ledger_present`; sem ledger retorna zeros e não cria arquivos. Não expõe caminhos, texto, motivos nem backups. Testes de integração SQLite **13/13**, testes headless Learn **8/8**, contrato do schema **53/53**, ajuda CLI **18/18** e Clippy dos dois crates com `--all-targets -D warnings` passaram. O schema foi regenerado e verificado. Revisão local: **PASS para esta superfície**, não para a Fase 7. Ainda faltam `diff`, aba Learn, marcos/semanal, smoke real e gate externo.

## Diff histórico estrutural e redigido (T012–T014 ainda parciais)

Novos itens Learn agora persistem os bytes posteriores em `learn/snapshots/`, com permissões privadas, `sync_all`, hash verificado na leitura e limpeza junto com o backup se a inserção no ledger falhar. O backup anterior continua sendo a fonte do undo. `learn.diff` e `mercury learn diff CHANGE_ID [--json]` retornam somente contagens de linhas antes/depois, prefixo/sufixo iguais e linhas removidas/adicionadas; nenhum texto, motivo, caminho ou segredo entra na resposta. O resumo continua igual após undo. Se faltar o snapshot posterior (item legado ou arquivo removido), `summary=null` sinaliza indisponibilidade; snapshot adulterado falha fechado. É um diff estrutural de uma região, não um patch textual. A API pública foi regenerada.

Revisão local do grupo: **PASS** para seguir à aba Learn, ainda não para declarar T012–T014 ou Fase 7 concluídas. `cargo test --offline -p mercury-learn` **60/60**, handler Learn **10/10**, schema **54/54**, ajuda CLI **18/18**, Clippy dos dois crates `--all-targets -D warnings` e `git diff --check` passaram. Não houve smoke de socket ou modelo real, nem alteração em alvos reais. Gates externos SonarQube/Veracode/Wiz/cobertura continuam FAIL/não verificáveis; sem commit.

## Aba Learn local (T015 parcial)

A shell tem chip Learn e atalho `prefix+shift+u`. A aba consulta `learn.list` e `learn.status` assincronamente, mantém apenas respostas tipadas e redigidas em estado transitório, mostra metadados/hash/diff estrutural ao selecionar um item e exige dois `u` consecutivos no detalhe antes de chamar `learn.undo`. Esc cancela a confirmação. Resultado de undo atualiza o estado e dispara nova consulta; seleção removida durante refresh volta à lista. O pane é ocultado sem copiar bytes para `ClientShellSnapshot`. Não foi usado alvo real.

Revisão consolidada local: **PASS para continuar**, não para concluir T015/Fase 7. Testes novos da aba **4/4**; suíte shell **328 PASS, 2 ignorados**, configuração **195/195**; Clippy `-p mercury --all-targets -- -D warnings` PASS. A primeira rodada shell encontrou apenas um teste de largura fixa da barra após adicionar o chip; a expectativa foi corrigida e a suíte repetida passou. Faltam auditoria individual de gatilhos, motivo/backup com política de redação, smoke de socket, inspeção manual TUI, sinais N-turn/semanal e gate externo. Nenhum commit foi feito.

## Índice privado de sessões concluídas (pré-requisito semanal; T010/T019 parciais)

O worker agora valida o transcript delimitado e sua identidade antes de registrar um sinal de fim observado. O SQLite mantém somente provedor, ID, caminho e horário de até 64 sessões concluídas; nenhum texto conversacional é persistido nesse índice. Entrada inválida não é indexada. A futura seleção semanal deverá revalidar cada transcript antes do uso, inclusive após restart. O construtor `weekly_due` ainda não tem sessão de entrada e **não foi ligado a um temporizador**: enfileirá-lo agora marcaria falha sem revisão, portanto T010/T019 permanecem abertos.

Revisão local do grupo: **PASS para o índice**, não para a revisão semanal. Testes do crate Learn **61/61**, worker **4/4** e Clippy dos dois crates `--all-targets -D warnings` PASS. O teste de storage cobre limite, atualização, restart e entradas inválidas; o teste do worker cobre recusa de transcript cuja identidade diverge. Nenhum modelo, Teams, SMTP ou alvo real foi acionado; gate externo continua FAIL/não verificável.

## Métricas observadas por tarefa (T018; T019 pendente)

O ledger privado `task_metrics` mantém no máximo 4096 amostras. Cada amostra contém apenas ID opaco da tarefa, ID estável da observação, provedor, fase base/posterior, tokens observados, custo opcional em micros de dólar e resultado opcional. Reenvio idêntico é ignorado durante a retenção; reenvio com o mesmo ID e valores divergentes é recusado. A agregação só marca comparação como disponível quando base e posterior pertencem à mesma tarefa/provedor e todos os resultados são conhecidos; custo ausente não vira zero. Mesmo quando comparáveis, os números são descritivos, sem alegação causal de melhoria. Não há produtor ligado ao servidor nem exibição na aba Learn: T019 permanece pendente, e saída do reviewer não poderá ser tratada como métrica observada.

Revisão consolidada: **PASS local do armazenamento/agregação T018**, não da US3 ou Fase 7. Testes SQLite cobrem restart, deduplicação e conflito de replay, separação por tarefa/provedor, resultados e custo desconhecidos, validação de entrada e retenção; crate Learn **66/66**, Clippy `-p mercury-learn -p mercury --all-targets -- -D warnings` PASS. Nenhum alvo real, modelo, Teams ou e-mail foi acionado. Gate externo permanece FAIL/não verificável; sem commit.

## Smoke isolado do socket Learn (T012–T014/T016)

Um teste de integração inicia o binário Mercury em PTY com configuração, ledger, alvo e socket temporários. O alvo permitido recebe uma alteração sintética pelo núcleo reversível; chamadas reais `learn.list`, `learn.show`, `learn.diff`, `learn.undo` e `learn.status` atravessam o socket. O teste confirma redação de conteúdo, motivo e caminho, diff estrutural, restauração exata, recusa de segundo undo e preservação de edição humana com `learn_conflict`. A CLI também consulta a lista e executa undo em JSON sobre o mesmo socket. A aba Learn permanece coberta por quatro testes automatizados de listagem, detalhe, dupla confirmação e cancelamento. T012–T014 e T016 estão marcadas concluídas; a auditoria mais rica da aba em T015 e a inspeção TUI manual em T022 permanecem abertas.

O teste-base de servidor e o novo smoke não conseguiram criar sockets no sandbox restrito desta máquina; o novo smoke passou **1/1 fora dessa restrição**, usando somente fixtures em `/private/tmp`, sem modelo ou rede externa. Testes focados da aba **4/4**. Clippy do teste `server_headless` com `-D warnings` PASS. Isso não substitui o gate externo SonarQube/Veracode/Wiz/cobertura, que segue FAIL/não verificável; sem commit.

## Auditoria redigida da aba Learn (T015)

`learn.status` agora inclui no máximo 16 gatilhos recentes com tipo, provedor, estado, horários e fingerprint SHA-256 truncado; ID de sessão e caminho do transcript não saem da API. A aba alterna lista/gatilhos com `t`, permite percorrer as entradas e preserva a dupla confirmação de undo. `learn.show` verifica o backup privado e expõe apenas disponibilidade; o motivo de origem do modelo permanece redigido. Para servidores antigos, ausência do campo de backup aparece como “não informado”, não como backup ausente, e ausência da lista de gatilhos vira lista vazia. Schema público regenerado e verificado em modo leitura.

Revisão consolidada do grupo: **PASS local para T015**, sem alegar Fase 7 completa. Testes Learn crate **67/67**, focados do binário **37/37**, shell **329 PASS/2 ignorados**, schema **54/54**, smoke API/CLI pelo socket temporário **1/1** e Clippy `-p mercury-learn -p mercury --all-targets -- -D warnings` PASS. Cobertura inclui ordem/limite/restart do ledger, redação de ID de sessão, backup adulterado, compatibilidade com servidor antigo e navegação/undo da TUI. Inspeção visual/manual TUI (T022), sinais N-turn/semanal, curator e gates externos ainda pendentes; nenhum modelo ou alvo real foi acionado e não houve commit.

## Planejador de curadoria reversível (T017; execução T019 pendente)

O curator produz apenas propostas para IDs exatos de skills/memórias autorizados. Arquivamento lógico troca o conteúdo ativo por um stub e mantém os bytes originais no backup/ledger do writer. Proposta por desuso exige janela de pelo menos 30 dias, cobertura completa, zero usos observados e término recente; ausência, desatualização ou tempo futuro falham fechados. Skills idênticas podem ser deduplicadas sem copiar conteúdo; memórias distintas só podem ser combinadas no mesmo diretório, com destino primeiro e stub da fonte depois. Dois IDs que apontem para o mesmo caminho são recusados. Cada passo leva hash de um snapshot confiável e é aplicado/undo separadamente pelo writer; falha do segundo passo deixa o primeiro auditável e exige recuperação explícita em T019, não é tratada como transação atômica.

Revisão consolidada: **PASS local do planejador T017**, não da curadoria automática/US3. Testes temporários cobrem skill e memória, backup+undo exatos, conflito humano, segunda etapa parcial recuperável, alias de caminho, symlink, origem inválida, alvos fora do tipo e evidência de uso insuficiente. Crate Learn **75/75** e Clippy dos crates Learn/Mercury `--all-targets -D warnings` PASS. Nenhum arquivo real, modelo, Teams ou e-mail foi acionado. Gate externo continua FAIL/não verificável; sem commit.

## Gatilho semanal com sessão validada (T010/T019 parciais)

O scheduler interno de Learn, somente quando `[learn].enabled` e `weekly` estão ativos, consulta a cada 15 minutos o índice privado de até 64 sessões concluídas. Seleciona a mais recente da semana UTC anterior; sem candidata elegível, não enfileira nem reivindica a chave. Antes de enfileirar, revalida identidade/conteúdo delimitado e fingerprint do arquivo; o handler confere o fingerprint novamente antes da revisão. O ledger `weekly:YYYY-MM-DD` mantém deduplicação persistente. Entradas de SQLite anteriores ao fingerprint migram com campo nulo e não são usadas. O índice não armazena texto conversacional. Desativação/reload encerra o scheduler; o worker segue default-off.

Revisão local do grupo: **PASS para o subfluxo semanal, não para T010/T019 completos**. Testes cobrem seleção por semana, ausência de candidata na semana corrente, recusa de arquivo alterado, atualização/restart/limite do índice, migração fail-closed e despacho agendado com handler simulado/deduplicação após restart; crate Learn **76/76**, testes `app::learn` do binário **4/4**, Clippy `-p mercury-learn -p mercury --all-targets -- -D warnings` PASS. Riscos residuais: só uma sessão por semana é selecionada; a execução agendada ainda não foi exercitada ponta a ponta com modelo real; N-turn, curator e métricas observadas não estão conectados. SonarQube, Veracode, Wiz e cobertura continuam **FAIL/não verificáveis**; nenhum commit.

## Marcos N-turn e isolamento do revisor (T010)

O hook gerenciado `Stop` do Claude envia `learn.claude_turn_end` sem confundi-lo com `SessionEnd`. Um estado `Idle` observado do Codex serve apenas de aviso, nunca de conclusão de sessão. Em ambos os casos, o worker lê o transcript fora do loop da interface, exige identidade atual do pane, arquivo estável, contagem exata de turnos de usuário e última mensagem de assistente, e só reivindica a chave `provider:session:turn:N` se `N` for múltiplo do valor configurado. Leitura incompleta, truncada ou alterada não gera claim. O processo privado `claude -p` remove as variáveis `MERCURY_ENV`, `MERCURY_SOCKET_PATH`, `MERCURY_PANE_ID` e `MERCURY_BIN_PATH`, impedindo os hooks Mercury de se apresentarem como o pane original. Um prompt de cron para pane marca a sessão atual (ou a próxima identidade reportada, se ainda desconhecida) como inelegível para Learn até a sessão mudar. O default de Learn continua desligado.

Revisão consolidada local: **PASS para T010**, não para a Fase 7. Testes cobrem resolução de marco exato, recusa de turno incompleto/sem resposta, claim/dedup, contrato da API, SessionStart/Stop/SessionEnd separados, migração idempotente dos hooks e exclusão de cron. Smoke isolado do shell hook em socket Unix temporário confirmou `learn.claude_turn_end` (o teste `tests/cli` é desabilitado em macOS). `cargo test --offline -p mercury-learn` **77/77**, testes Learn do binário **41/41**, schema **55/55**, testes `claude_settings` **11/11** e Clippy Mercury/Learn `--all-targets -D warnings` PASS. Nenhum modelo, alvo real, Teams ou e-mail foi acionado. Restam T011 (suíte consolidada especificada), T019–T023 e os gates externos **FAIL/não verificáveis**; sem commit.

## Curadoria semanal determinística (T019/T020 parciais)

Uma chave independente `curation:weekly:YYYY-MM-DD` executa uma vez por semana quando Learn está opt-in. Ela não depende de transcript nem chama o modelo: examina até 16 alvos permitidos e arquiva no máximo quatro fontes com bytes exatamente iguais a um destino anterior na ordem da configuração. Skills idênticas podem estar em diretórios diferentes; memórias só qualificam no mesmo diretório, inclusive quando idênticas. Cada escrita usa snapshot/hash confiável, backup e ledger do mesmo writer reversível. O handler consulta o kill-switch antes de cada passo; um plano parcial retorna IDs aplicados e deixa o restante intacto para recuperação por undo. O status mostra apenas tipo `curation_weekly` e fingerprint redigido. Não há arquivamento por “desuso” porque o Mercury ainda não dispõe de cobertura observada contínua de 30 dias.

Revisão local: **PASS para a curadoria de duplicatas, não para T019/T020 completos**. `cargo test --offline -p mercury-learn --test curator` **11/11** cobre backup/undo, pares permitidos, memórias entre diretórios, plano parcial e desativação entre passos. A suíte completa Learn **80/80** e o filtro Learn do binário **43/43** passaram; Clippy Mercury/Learn `--all-targets -D warnings` PASS. O teste agendado usou somente skills temporárias e `/usr/bin/true` como executável de revisor, sem invocá-lo. Métricas de tarefas ainda não têm produtor confiável nem exibição no Learn; no-use archive segue inativo. Gate SonarQube/Veracode/Wiz/cobertura continua **FAIL/não verificável**; sem commit.

## Suíte integrada de revisão (T011)

O teste `crates/mercury-learn/tests/review.rs` exerce claim persistente, transcript completo, revisor simulado sem modelo externo, aplicação em alvo temporário, reabertura do ledger, recusa de claim repetido e undo dos bytes exatos. Casos de parser e runner inválidos permanecem nos testes dedicados `transcript.rs`/`reviewer.rs`/`pipeline.rs`; exclusão de origem recursiva e restart do worker estão em `src/app/learn.rs`. Revisão `pv-post-implement-review`: **PASS local** para o grupo T011, sem achados bloqueantes; risco residual é o smoke de modelo real ainda pendente. `pv-test-and-review`: suíte Learn **81/81**, filtro Learn do binário **43/43** e Clippy Mercury/Learn `--all-targets -D warnings` **PASS**. O gate SonarQube/Veracode/Wiz/cobertura continua **FAIL/não verificável**, então não há commit nem conclusão da Fase 7.

## Captura tipada de usage no executor headless (T019 parcial)

`CommandOutput.observed_usage` carrega tokens de entrada/saída apenas quando ambos são inteiros reportados e limitados no JSON do Claude ou no evento final `turn.completed` do Codex. Custo em micros de dólar é opcional e só existe se o campo de custo do Claude for numérico válido; scripts e pane não geram amostra. O texto final continua separado do envelope e não recebe telemetria. Ainda não existe persistência por run nem classificação base/posterior, pois isso exigiria uma fronteira confiável para cada tarefa; não se pode tratar essa captura como comparação de eficácia. A compatibilidade do parser com saídas reais de versões atuais dos CLIs ainda não foi confirmada em smoke autorizado.

Revisão local `pv-post-implement-review`: **PASS para captura tipada**, sem achados bloqueantes no escopo; limite funcional acima permanece. `pv-test-and-review`: crate Crons **33/33**, filtro Crons do binário **30/30** após repetir os quatro testes de socket fora do sandbox restrito, Clippy Mercury/Crons `--all-targets -D warnings` **PASS**. A primeira tentativa no sandbox teve quatro falhas de bind `Operation not permitted`, não de lógica. Gates externos SonarQube/Veracode/Wiz/cobertura indisponíveis; sem commit.

## Persistência de amostras observadas (T019/T020 parciais)

O ledger privado Learn passou à versão 3 com fase adicional `observed`. A migração mantém IDs, fases e valores v2; a agregação separa `observed` de baseline/after e jamais considera só essa fase uma comparação. Após persistir a conclusão de um job headless, o servidor opt-in enfileira a gravação de tokens/custo reportados e resultado classificado pelo scheduler. O ID do run torna a gravação idempotente e o ID do job é a chave opaca da tarefa. Script, pane, uso parcial e Learn desligado não geram amostra. O worker de gravação evita abrir o SQLite na UI, mas um desligamento abrupto antes de ele executar pode perder essa observação; falta reconciliação durável a partir do cron ledger. Também faltam consulta/aba Learn da série e fronteira antes/depois confiável.

Revisão `pv-post-implement-review`: **PASS local** para migração, agregação e helper de produção, com os riscos residuais acima. `pv-test-and-review`: suíte Learn **83/83**, filtro Crons do binário **31/31** (loopback simulado fora do sandbox restrito), Clippy Mercury/Learn/Crons `--all-targets -D warnings` **PASS**. Testes novos cobrem v2→v3, reinício/replay e não inferência de melhoria. Gate externo continua **FAIL/não verificável**; sem commit.

## Série observada no status e na aba Learn (T019/T020 parciais)

`Storage::recent_observed_metrics` devolve no máximo 16 linhas da fase `observed`, ordenadas por horário/ID, recusando limites inválidos e linhas corrompidas. `learn.status` adiciona `recent_metrics` com `serde(default)` para leitores de servidores antigos; o handler mantém SQLite no worker e expõe apenas chave opaca de job, provedor, horário, tokens, custo opcional e resultado. A aba Learn abre a página de métricas com `m`, usa data/hora UTC e custo em micros convertido sem arredondamento; as amostras são rotuladas como insuficientes para comparar antes/depois. Não há alegação causal nem exposição de prompt, caminho ou envelope. O schema público foi regenerado e conferido em modo leitura.

Revisão `pv-post-implement-review`: **PASS local** para consulta/API/TUI, sem achado bloqueante no grupo. `pv-test-and-review`: testes de métricas **8/8**, filtro Learn do binário **45/45**, teste do artefato de schema **1/1**, Clippy Mercury/Learn `--all-targets -D warnings` **PASS**. A TUI está coberta por teste de frame/tecla, mas ainda requer inspeção manual T022. Faltam fronteira baseline/after, evidência contínua de uso para arquivamento por desuso e reconciliação de amostra pendente após saída abrupta. Gate externo continua **FAIL/não verificável**; sem commit.

## Retry periódico do outbox Learn (T020a)

Revisão consolidada local: **PASS para a cadência de retry**, sem alterar as pendências de fronteira baseline/after. O servidor inclui o timer de cinco minutos entre seus deadlines mesmo quando nenhum cron está próximo. A tentativa abre os ledgers somente em `spawn_blocking`; se outra reconciliação já segura a trava, a nova não fica na fila. O timer é armado apenas com Crons e Learn habilitados, desarmado no opt-out e rearmado no opt-in. Um erro de destino mantém a linha durável para a próxima tentativa. Não há backoff exponencial nem alerta dedicado para falha prolongada; o limite de 4096 pendências permanece.

`pv-test-and-review`: timer/opt-out **1/1**, filtro Crons do binário **32/32** com sockets locais, Clippy Mercury/Crons/Learn **PASS**. O teste integrado anterior cobre falha de Learn, preservação do outbox e reconciliação idempotente. O gate externo segue **FAIL/não verificável**; sem commit.

## Correção do handshake da aba Learn (T016a, T022 parcial)

O smoke em PTY isolado encontrou um defeito de integração: a API/CLI aceitava `learn.list/status/show/diff/undo`, mas a lista de capacidades do transporte client-shell não anunciava esses métodos. O cliente recusava a consulta localmente e mostrava “Action unavailable”, deixando a aba em estado vazio/carregando. A lista agora inclui somente os cinco métodos usados pela aba; `learn.undo_date` e os dois métodos de hook não foram expostos. O teste de frame da aba usa as capacidades reais do servidor, de modo que uma futura omissão falhe antes do smoke.

Após rebuild, servidor e TUI com config/socket/dados isolados em `/private/tmp/mercury-learn-pty-bOrCcqC3`: `prefix+shift+u` mostrou `ledger vazio`, sem aviso de método indisponível; `m` exibiu “Nenhuma amostra observada” e “dados insuficientes para comparar”. `mercury learn status --json` no mesmo servidor retornou `ledger_present=false` e listas vazias. Cliente destacado e servidor temporário parado com exit code 0. A inspeção de ações com item real e undo ainda requer fixture TUI; este smoke não os exercitou.

O teste de digest da rota expôs uma divergência anterior em `cron.add/edit`: ambos referenciam `CronJobDefinition`, cujo `Delivery` não possui mais Email por decisão do usuário. Conferidos os contratos, atualizados somente os dois digests e congelados separadamente os cinco shapes Learn; o fixture dos métodos legados permaneceu intacto. `pv-test-and-review`: Learn **47/47**, client-shell Learn **6/6**, client commands **7/7**, contrato de shapes e ordenação/schema **PASS**. Gate externo **FAIL/não verificável**, sem commit.

## Horário de conclusão preservado no replay (T019/T020 parciais)

Revisão consolidada local: **PASS para a correção do timestamp**, sem afirmar comparação de eficácia. A consulta do outbox usa o `finished_at` durável do run; a gravação Learn usa esse instante (precisão de segundos), não o horário do worker. Replays com o mesmo ID e timestamp diferente são rejeitados; valor negativo ou overflow na conversão falham fechados. O teste integrado conclui um run em data antiga, simula indisponibilidade do Learn, reconcilia depois e confirma o timestamp original. Amostras antigas não são reescritas. O campo `skills` do agendamento só instrui o prompt, então não é evidência de uso real nem fronteira confiável baseline/after.

`pv-test-and-review`: Crons crate **37/37**, Learn metrics **8/8** e replay integrado do binário **1/1**; Clippy Mercury/Crons/Learn e `cargo fmt --all --check` **PASS**. Não há cobertura quantitativa. Riscos residuais: timestamps antigos podem representar reconciliação; a retenção de 4096 itens e ausência de retry periódico permanecem. Gate externo **FAIL/não verificável**, sem commit.

## Outbox durável de usage para Learn (T019/T020 parciais)

O ledger Crons v4 cria `learn_usage_outbox` privado. Na mesma transação que finaliza um run, ele guarda apenas run/job IDs, provedor, números reportados e resultado — sem prompt, saída, caminho, credencial ou envelope. Só há estágio quando Learn está opt-in e o alvo é headless com usage completo. Um worker serializado lê até 64 por vez, grava no Learn com ID do run e remove a pendência somente após sucesso; abertura do scheduler, novo run e reativação de Learn disparam reconciliação. Falha do destino preserva a pendência; replay é idempotente. O outbox retém no máximo 4096 linhas: se a indisponibilidade prolongada exceder esse limite, as mais antigas deixam de ser recuperáveis. Falha transitória sem novo run ou reinício não recebe retry periódico ainda.

Revisão `pv-post-implement-review`: **PASS local** para atomicidade e replay, sem achados bloqueantes no escopo; os limites operacionais acima permanecem. `pv-test-and-review`: crate Crons **36/36**, filtro Crons do binário **31/31** (sockets locais simulados), Clippy Mercury/Crons/Learn `--all-targets -D warnings` **PASS**. Testes cobrem migração v3→v4, rollback de uso inválido, sobrevivência após reinício, falha de escrita Learn que mantém o outbox e reconciliação que drena uma vez. Nenhum modelo, Teams real ou email foi acionado; gate externo **FAIL/não verificável**, sem commit.

## Revisão consolidada da suíte do workspace (2026-10-07)

`pv-post-implement-review`: **PASS local para as correções de teste**, sem achado bloqueante no código alterado. A revisão cobriu fixtures de rebrand/manifesto, expectativas da integração Claude v11, teste de notas sem `CHANGELOG.md` e isolamento de `SIGPIPE` no harness unitário. O executável de produção mantém `SIGPIPE` padrão durante saída CLI, e `tests/broken_pipe.rs` o verifica como subprocesso. O digest da exceção de publicação Grok foi recalculado para o comentário Mercury já presente no manifesto distribuído; o teste de catálogo voltou a passar. Não houve mudança de comportamento de produção nos demais ajustes.

`pv-test-and-review`: suíte Rust do workspace **exit 0**, com 3663/3663 testes executados no binário principal, 11 ignorados e 2 testes TCP de handoff filtrados; os demais 21 testes do arquivo de handoff passaram. `tests/broken_pipe.rs` passou isoladamente **3/3**, confirmando a semântica do executável real. `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo fmt --all --check`, `git diff --check` e 63 testes auxiliares aplicáveis passaram. A execução sem filtros falhou apenas nos dois testes que abrem um servidor Python HTTP em um processo e tentam conectar por TCP loopback neste ambiente. Em uma pasta temporária vazia, o servidor Python iniciou, mas `curl` em outro processo esgotou o tempo de conexão em `127.0.0.1`; por isso não há evidência de defeito no handoff nem PASS para esses dois cenários. `just test` requer `cargo-nextest`, ausente; testes TypeScript requerem `bun`, ausente. A receita completa de manutenção também requer notices/licenças ausentes e dependências Windows fora do cache offline.

`pv-security-quality-gate`: **FAIL/não verificável**. Não há SonarQube, Veracode, Wiz nem cobertura quantitativa (`cargo-llvm-cov` ausente) disponíveis; não foi feito commit. A suíte verde parcial não autoriza uso de Learn automático em alvos reais nem encerra T019–T023. Para fechar o gate, executar os dois smokes TCP e a manutenção/TypeScript em ambiente apto, restaurar ou resolver a política dos notices de terceiros, obter os relatórios de segurança/cobertura e resolver as pendências funcionais da Fase 7.

## Ajuste de segurança local-only para o primeiro uso

Revisão consolidada `pv-post-implement-review`: **PASS local**. O fork não publica manifesto de versão nem catálogo remoto próprio; `Config::default()` não deve agendar checagens de rede só por iniciar um build release. Os defaults de `version_check` e `manifest_check` agora são `false`, alinhados ao template `--default-config` e ao plano. Um usuário que configure `true` explicitamente mantém essa escolha. Não houve alteração no parser de manifesto nem em arquivos de usuário. `pv-test-and-review`: config **195/195**, reload **1/1**, build do binário, Clippy global, `cargo fmt --all --check` e `git diff --check` **PASS**. Gate externo permanece **FAIL/não verificável**.

## Item e undo reais pelo client-shell PTY (T022 parcial)

Revisão `pv-post-implement-review`: **PASS local**, sem alteração de produção. O novo teste em `tests/client_mode.rs` usa alvo, XDG e socket temporários, cria um item real via `mercury-learn::Changes`, inicia binário servidor e cliente, e reconstrói os frames do PTY pelo emulador VT. A lista mostra o alvo; o detalhe mostra backup verificado e diff estrutural sem conteúdo, caminho ou motivo privados; um `u` apenas mostra confirmação, o segundo restaura bytes exatos. `learn.show` confirma estado `undone`. O teste não requer modelo, Teams, email ou arquivos do usuário.

`pv-test-and-review`: teste focado **1/1**, arquivo `client_mode` **28/28**, Clippy global, formatação e `git diff --check` **PASS**. A primeira expectativa de título falhou porque o frame inicial apresenta as abas em minúsculas; corrigida a expectativa, sem ajuste de comportamento. O teste usa polling limitado de frame e arquivo, não sleeps fixos. Risco residual: o emulador confirma texto e fluxo, mas não substitui inspeção humana de cores/layout nem smoke com reviewer real. T022 segue parcial; gate externo **FAIL/não verificável**.

Após esse teste e a mudança dos defaults de atualização, a suíte Rust do workspace foi repetida serialmente: **exit 0**, binário principal **3663 PASS/11 ignorados**, arquivo `client_mode` **28/28**, handoff executável **21/21** e demais suites sem falha. Apenas os dois testes de HTTP loopback permaneceram filtrados por isolamento do ambiente. Clippy global, formatação e diff continuaram limpos; não há cobertura quantitativa nem resultados dos três scanners obrigatórios.

## Revisão de legibilidade da TUI a 80 colunas (T022 parcial)

O dump textual de um item real no PTY revelou que `buffer.set_style` não removia símbolos anteriores da área; um `M` do pane aparecia antes de “Learn”. As quatro abas sobrepostas (Usage, Crons, History e Learn) agora usam `Clear` antes de aplicar o estilo, prevenindo esse resíduo. Na Learn, `put_text` aplica truncamento com `…` por largura de display, o status e as teclas usam textos compactos em telas estreitas, hashes parciais são explícitos e o diff redigido ocupa duas linhas. A confirmação de undo foi movida para perto do topo e tem teste em 80×12. O frame posterior em 80×24 mostrou título limpo, resumo/teclas completos, hashes com `…`, linha do diff completa e confirmação visível; nenhum conteúdo privado foi exposto.

`pv-post-implement-review`: **PASS local**, sem achado bloqueante no grupo. A limpeza custa uma passagem pela área de cada aba por frame, proporcional ao tamanho já desenhado; não muda contratos API ou dados persistidos. `pv-test-and-review`: shell **331 PASS/2 ignorados**, client_mode **28/28** na rerodada, smoke PTY focado **1/1**, Clippy global, formatação e diff **PASS**. A primeira execução de client_mode teve timeout no cenário SSH federado; o mesmo teste passou isolado e o arquivo completo passou na repetição. Isso fica registrado como risco de timing preexistente, não como falha mascarada. A análise textual não verifica cor/contraste em terminal humano; gate externo **FAIL/não verificável**, sem commit.

## Amostra Cron observada até a TUI (T020/T022 parciais)

Um novo teste `tests/client_mode.rs` habilita Learn apenas em configuração temporária, com `weekly=false` e alvo `AGENTS.md` descartável, inicia servidor e cliente reais em PTY e cria um job headless por `cron.add`. Um executável `claude` falso devolve envelope estruturado com 120 tokens de entrada, 30 de saída e US$ 0,000125 reportados; `cron.run` conclui, o outbox entrega a amostra ao ledger, `learn.status` a expõe e a página `m` a mostra. O alvo permanece byte a byte inalterado, e o prompt não aparece no frame. A primeira captura em 80 colunas revelou que a linha única cortava custo/resultado; cada amostra agora usa duas linhas (identidade/horário e números/resultado), com seleção e scroll por amostra. Um teste unitário também exige custo visível a 80 colunas.

`pv-post-implement-review`: **PASS local** no fluxo e na alteração de layout, sem achado bloqueante; a altura da lista passa a comportar menos amostras por página, mas nenhuma é descartada e a rolagem continua por item. `pv-test-and-review`: smoke integrado **1/1**, testes unitários Learn **7/7**, arquivo `client_mode` **29/29**, Clippy global, `cargo fmt --all --check` e `git diff --check` **PASS**. Não houve chamada a provedor real, Teams, e-mail ou escrita em alvo real. O teste confirma a entrega e a leitura, não estabelece fronteira antes/depois nem cobertura de uso por 30 dias. Inspeção humana de cor/contraste, smoke de reviewer real e SonarQube/Veracode/Wiz/cobertura seguem pendentes; gate **FAIL/não verificável**, sem commit.

## Estado consolidado para conclusão (2026-10-07)

Depois desse grupo, a suíte Rust completa passou com código 0 usando data home isolado: **3664 testes do binário principal PASS, 11 ignorados**, `client_mode` **30/30** e demais arquivos sem falha; apenas os mesmos dois testes de TCP loopback foram filtrados pelo ambiente. Clippy global, `cargo fmt --all --check` e `git diff --check` passaram. O novo teste Cron em pane usa agente descartável e confirma entrega única do prompt, run e log; não substitui smoke com agente real. O harness de integração foi corrigido para isolar o data home em todos os spawns de servidor identificados, inclusive dois caminhos de `.args([..., "server"])`; o teste opcional Linux/DBus compila mas não executa no macOS.

**Pendências funcionais, sem reinterpretar como concluídas:** T019/T020 exigem fronteira confiável job↔alvo para comparação antes/depois e evidência contínua de uso antes de qualquer arquivamento por 30 dias; silêncio de hook não autoriza arquivo. T019b exige decisão sobre operações estruturadas em rota/Cron antes de permitir Learn editar config Mercury. T021 fica aberta até documentar o comportamento final dessas escolhas; T022 ainda exige inspeção humana de cor/contraste e smokes reais autorizados; T023 exige os gates. A pergunta sobre vínculo explícito job↔alvo e escopo estruturado de config foi enviada ao usuário. Até a resposta, não há comparação causal nem escrita em config real.

**Gate:** SonarQube, Veracode, Wiz e cobertura quantitativa não estão disponíveis neste ambiente, logo **FAIL/não verificável**; sem commit e sem ativação automática do Learn em alvos reais. A instalação de `cargo-llvm-cov` não está presente; há `llvm-profdata`/`llvm-cov` do Apple LLVM 21, mas o Rust usado compila com LLVM 22.1.2, sem medição confiável demonstrada aqui. Além dos smokes de provedor e dois TCP pendentes, executar os scanners/medição no pipeline adequado ou obter exceção explícita documentada é condição para declarar a fase concluída.

Atualização de cobertura em 2026-10-07: foi encontrado Homebrew LLVM 22.1.8 e a suíte completa passou instrumentada. O diagnóstico em `docs/dev/workspace-coverage-snapshot.md` mede 88,03% de linhas nos arquivos `src/` do workspace e, para Learn isolado sem aviso, 92,60%. O agregado registra 74 funções com dados incompatíveis ao combinar binários; não é resultado de gate nem substitui uma medição validada no pipeline. A frase anterior de “cobertura quantitativa não disponível” fica superada apenas para diagnóstico local. SonarQube/Veracode/Wiz e o limite Rust aprovado continuam ausentes; o gate permanece **FAIL/não verificável**.

## Handoff HTTP sem dependência de DNS reverso (2026-10-07)

Os dois testes TCP antes filtrados falhavam antes de exercitar o handoff: o
`python3 -m http.server` bloqueava em `getfqdn(127.0.0.1)` neste ambiente. Um
servidor Python temporário com apenas essa consulta substituída atendeu
`127.0.0.1` com HTTP 200. O fixture em `tests/live_handoff.rs` agora inicia
o mesmo módulo `http.server` sem DNS reverso; o Mercury de produção não mudou.
Ambos os cenários antes filtrados passaram isoladamente e o arquivo inteiro
passou **23/23** sem filtros. A suíte workspace seguinte passou sem filtros,
incluindo o binário principal **3666 PASS/11 ignorados** e `client_mode`
**31/31**; Clippy global, formatação e diff passaram.

A primeira rodada ampla após o ajuste encontrou uma falha separada no teste
de refresh de limites Codex da rota (`unavailable` em vez de `refreshed`). Ele
passou 12 vezes isolado e nas duas rodadas amplas seguintes, mas a causa da
intermitência não foi demonstrada; permanece risco de estabilidade de teste.
O gate SonarQube/Veracode/Wiz/cobertura continua **FAIL/não verificável** e
não houve commit nem ativação automática de Learn em arquivos reais.

## Metadados de vendor offline (2026-10-07)

O teste de `portable-pty` agora usa o alvo host retornado por `rustc -vV`
em `cargo metadata --offline --locked --filter-platform`. A asserção continua
exigindo o caminho exato da árvore vendorizada, mas não baixa dependências
Windows irrelevantes para esse contrato em macOS. O teste focado passou.
A suíte de manutenção executou 74 testes: **72 passaram**, um falhou pela
ausência de `vendor/portable-pty/LICENSE.md` e outro encontrou ausentes os
notices de ConPTY referenciados em `packaging/windows/conpty.json`. O gate
permanece FAIL; não se substituiu nem fabricou texto legal para fazê-lo passar.

## Fechamento funcional local após decisões do usuário (2026-10-07)

As decisões mais recentes substituem os bloqueios antigos desta página: o usuário
autorizou edição automática de Roteamento/Crons quando Learn estiver habilitado,
aprovou associação explícita job→alvo, retirou a regra de arquivamento por 30
dias, assumiu pessoalmente os smokes reais e dispensou SonarQube/Veracode/Wiz e
cobertura quantitativa nesta entrega local. Scanners dispensados são **não
executados**, não PASS. Teams por Workflows/Power Automate é backlog; SMTP/email
não faz parte do produto. `learn.enabled` continua falso por padrão.

T019/T019b/T020 locais: `[learn.job_targets]` exige UUID de job e ID de alvo
configurado. A associação ao ID e ao fingerprint do caminho do alvo é congelada no outbox Cron v5 na conclusão, preserva
amostras v4 anteriores como não associadas e não atribui histórico
retroativamente. Uma comparação só aparece se o mesmo job/provedor/alvo tiver
amostras antes do prepare e depois do apply durável de **uma única** mudança
ainda aplicada; período incerto, outcome ausente, mudança desfeita e alvo
alterado tornam os dados insuficientes. Para rota/Cron, o comparador ignora
outras seções/jobs e o cursor operacional `next_run_at`, mas rejeita mudança
semântica do job. A API e a página `m` mostram médias descritivas, sem
atribuição causal.

`config-section` expõe ao reviewer somente `[route]`, valida `RouteConfig`,
mescla apenas essa tabela e usa backup/ledger/undo. `cron-job` é um alvo cujo
ID é o UUID exato do job e cujo arquivo é o `jobs.toml` ativo; o reviewer vê
somente `schedule`, `enabled` e `timeout_seconds`, e seu patch JSON só pode
alterar esses campos. Prompt, script, destino, job ID e caminho não vêm da
resposta do modelo. Aplicação/undo e mutações do Scheduler compartilham lock;
o Scheduler relê o arquivo antes de gravar. Run reservado ou em execução
bloqueia alteração/undo do job. O servidor recebe evento para recarregar rota
ou jobs após aplicação/undo. Dois patches Cron sobre o mesmo arquivo podem ser
aplicados em sequência usando o hash após a primeira escrita, sem aceitar
hash fornecido pelo modelo.

A regra de arquivamento por desuso e seus testes exclusivos foram removidos;
curadoria semanal de duplicatas comprovadas com backup/undo permanece. A spec
e o README explicam o opt-in, a allowlist, o vínculo e os limites. Nenhum
arquivo real do usuário, modelo externo, Teams ou e-mail foi acionado neste
fechamento. A suíte workspace sem filtros terminou com exit 0 nesta rodada;
o resultado final após a última alteração está registrado abaixo.

Revisão local (`pv-post-implement-review`): não se identificou escape de path,
campo de execução controlado por reviewer nem bypass de backup/ledger nos
novos alvos. Riscos residuais: editor externo fora do lock ainda pode competir
na janela de rename documentada; a comparação não prova causalidade, amostras
antigas sem vínculo continuam não classificadas e mudança do vínculo durante
um run longo é capturada apenas na conclusão; o outbox é limitado; reload
falho exige inspeção manual do ledger/runtime. A embalagem Windows permanece
não validada pelos avisos de terceiros ausentes, sem remoção/fabricação desses
arquivos legais. Não houve commit.

## Verificação final desta entrega local (2026-10-07)

`cargo test --workspace --offline -- --test-threads=1` terminou com **código 0,
sem filtros** após as últimas alterações. O binário principal passou
**3670/3670** (11 ignorados por plataforma/escopo); `client_mode` passou
**31/31**, `live_handoff` **23/23**, e os testes de métricas Learn **10/10**,
incluindo vínculo por caminho e cursor Cron. `cargo clippy --workspace
--all-targets --offline -- -D warnings`, `cargo fmt --all --check`,
`git diff --check` e `cargo build --offline -p mercury` passaram. O executável
local respondeu `mercury 0.9.3` a `--version`. Os testes foram executados com
dados temporários isolados; a primeira tentativa no sandbox falhou em testes
de arquivo temporário de `ghostty-vt` com `Operation not permitted`, e o teste
isolado mais a suíte completa passaram fora dessa restrição. Não houve smoke
de modelo autenticado nem inspeção visual humana da TUI: ambos cabem ao
usuário conforme sua decisão. Scanners externos/cobertura continuam
**não executados por dispensa**, não PASS. Windows packaging segue não
validado pelos notices/licenças de terceiros ausentes; não houve commit.

## Retorno do primeiro smoke do usuário (2026-10-07)

`config check` passou, mas o cliente novo encontrou dois servidores iniciados
em 2026-10-05; o executável local foi recompilado em 2026-10-07. Apesar de
ambos relatarem `0.9.3` e protocolo 22, o servidor ativo não reconhecia
`cron.doctor` e seu snapshot de um agente Codex não incluía Usage. A aba Learn
ficava em “status carregando” quando métodos não eram anunciados ou retornavam
erro. O CLI agora distingue servidores anteriores pelo campo opcional
`mercury_feature_generation` no ping, marca `restart_needed` e
`server_binary_stale`, e retorna `server_restart_required` para comandos das
novas áreas. A aba Learn mostra indisponibilidade em ambos os casos, sem
spinner infinito. O schema foi regenerado e os cenários receberam testes.
O `Operation not permitted` do preview foi reproduzido ao tentar conectar ao
socket a partir do sandbox do agente; um terminal normal deve ser usado para
o smoke. O processo do usuário **não foi reiniciado nem encerrado** por esta
rodada, para preservar seus panes; o handoff local continua pendente.

Verificação da correção: a suíte workspace após o novo campo de capacidades
terminou sem filtros com código 0; após o guard CLI, o binário principal
passou **3674/3674** (11 ignorados). Os oito testes filtrados `old_server`
passaram fora da restrição de sockets. A última alteração foi apenas a
mensagem de orientação para priorizar handoff; seu teste focado passou.
Clippy global, formatação, build e `git diff --check` foram repetidos.
