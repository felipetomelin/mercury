# Validação da entrega 013

## Escopo final aprovado

Somente sessões de agentes principais no histórico. A ampliação para todos os
subagentes foi solicitada explicitamente pelo usuário após a pesquisa Guardian.
Não há opção para incluir subagentes. As cores por diretório aparecem no histórico
e na lateral de projetos/chats, incluindo a apresentação recolhida.

## Implementação

- `mercury-history`: classifica origens Codex estruturadas e legadas de subagente,
  de qualquer papel, e sidechains Claude. Um transcript Claude com mensagens
  principais e registros sidechain continua sendo uma conversa principal.
- Migração transacional adiciona is_subagent e força backfill de arquivos
  antigos sem alteração de mtime. O filtro SQL ocorre antes de LIMIT, tanto na
  lista quanto na busca. A reabertura por ID também recusa sessões de subagentes.
- Codex interrompe a leitura ao reconhecer uma sessão de subagente no metadado,
  evitando indexar suas mensagens. Arquivos originais não são alterados.
- Marcador visual usa hash determinístico de endpoint + path lexical, com cores
  da paleta do tema. Um helper único é usado no histórico, projeto e chat; mantém
  os ícones/cores de estado e os fundos de seleção. Path ausente é neutro.
- Servidor transporta identity_cwd em project_path opcional no snapshot. Não usa
  cwd temporário de criação/foreground para determinar a cor. Protocolo binário
  privado evoluiu de 22 para 23; endpoint JSON mantém geração 1 e aceita snapshots
  antigos sem project_path. Schema e fixtures de protocolo foram atualizados.

## Revisão consolidada

Revisão local do grupo sem achados bloqueadores de correção ou segurança.
Sem nova dependência, exclusão de transcripts ou leitura de arquivos na UI.
Consultas SQL parametrizadas e limites/checagens de caminhos existentes mantidos.
Não há dependência de produção no SQLite interno do Codex: a classificação usa
metadados dos transcripts. Os avisos Clippy encontrados foram resolvidos sem
supressões, extraindo o desenho do marcador para um helper e removendo uma
referência redundante.

## Evidências

- Suíte principal: **3723 PASS / 0 FAIL / 14 ignorados**.
- Módulo de histórico: **16 PASS / 1 ignorado**.
- Integração API/ping: **12 PASS**; histórico/reabertura: **10 PASS**.
- Após os ajustes de estrutura: client-shell **398 PASS / 3 ignorados**.
- Manutenção e arquitetura: **80 PASS**.
- Clippy Mercury/mercury-history, all-targets e `-D warnings`: PASS.
- Build Mercury offline, formatação e `git diff --check`: PASS.
- Schema gerado/contratos JSON antigos: PASS na suíte principal.
- Integração de handoff: **22 PASS / 1 falha inicial por timeout de startup**.
  O único teste afetado, `live_server_holds_one_pty_master_fd_per_pane`, passou
  isoladamente (**1 PASS**, 2,17s). Nenhuma falha permanece sem reteste aprovado.
  Trata-se de uma repetição focal, não de uma execução única com 23 PASS.

As execuções de integração usam sockets/PTYs temporários e removem contexto
MERCURY_* herdado do terminal ativo. `tests/cli.rs` contém cfg que exclui macOS;
esse alvo executou zero testes e não é reportado como validado nesta plataforma.
Rust 1.96.1, Zig 0.16.0 oficial temporário e caches no workspace.

## Limites e gates

Gate local: **PASS**, considerando o reteste focal de startup descrito acima.
SonarQube, Veracode, Wiz e cobertura quantitativa: dispensados/não executados,
conforme constituição do Mercury; não representam resultados externos aprovados.

Origens legadas/desconhecidas não são inferidas pelo título ou pela ausência de
mensagens. Paleta finita permite colisões; cores não substituem título/path.
Aliases por symlink podem ter cores diferentes. Verificação visual automatizada
foi feita em buffer, em temas claro/escuro; smoke visual humano não foi executado.
Nenhum servidor instalado, chat em execução ou configuração pessoal foi alterado
nesta etapa. Para ativar, atualizar servidor e cliente juntos por causa do protocolo.


## Ativação autorizada em 2026-11-24

O usuário solicitou instalar/reiniciar a entrega (“Faça isso”). Nenhuma entrada de
compilação estava mais nova que o binário já validado. Config check PASS.
Backup privado do executável anterior e inventários antes/depois foram registrados
em pasta temporária privada. Instalado com `install-local.sh --profile dev --no-build`.
SHA-256 do artefato e da cópia instalada:
`45235d287f6615082ec84ca4120dab5eec03bc408ca9bd564a528037e924733e`.

Handoff concluído de protocolo 22 para 23, com expected-protocol=23 e
expected-version=0.9.3. Status posterior: servidor running, compatible=true,
endpoint_compatible=true, restart_needed=false e server_binary_stale=false.
Comparação antes/depois confirmou o mesmo workspace, os mesmos 6 painéis, os
mesmos 6 PIDs de shell e o mesmo cron. Nenhum prompt foi enviado aos chats.
O processo Mercury observado após a transferência carrega a imagem instalada
nova; nenhum processo Mercury com a imagem anterior foi observado nessa coleta.

A consulta inicial ao histórico ultrapassou 50s durante a releitura inicial.
Verificação somente leitura do índice confirmou 530 registros já classificados
como subagente e 453 registros ainda aguardando releitura naquele instante.
Não se reporta esse smoke de API como PASS. O worker segue processando o índice
em background; a saúde do servidor e a preservação dos processos foram conferidas
separadamente. A cor no frontend segue validada por testes de buffer, sem smoke
visual humano nesta ativação.
