# Validação e revisão

## Escopo implementado

Isolamento das medições por terminal, provedor e sessão. Respostas de background
agora carregam a identidade consultada inclusive quando a leitura falha. A
aplicação confere também a identidade do payload. O snapshot da sidebar rejeita
cache de outra sessão; o polling remove esse cache e sua limitação de frequência.
Snapshots antigos de statusline também são removidos na troca de sessão.

O vínculo inicial continua vindo do hook com pane_id e da sessão persistida;
o leitor procura o arquivo pelo ID exato, sem selecionar a conversa mais recente
do diretório. A statusline já validava a sessão e mantém essa proteção.

Os títulos do histórico foram implementados após a aprovação adicional;
evidências específicas estão no complemento abaixo.

## Revisão consolidada

PASS para a revisão local do grupo: `src/app/usage.rs`, `src/events.rs` e
`src/server/client_shell.rs`. Nenhum bloqueador identificado por inspeção.
Sem nova dependência, I/O na renderização, leitura de conversas reais ou mudança
de protocolo público. O envelope de identidade é interno ao servidor.

Os testes de regressão cobrem dois chats no mesmo diretório, provedores Claude
e Codex, payload incorreto, resposta atrasada com e sem dados, ocultação/invalidação
do cache antigo e resultado vazio legítimo da sessão atual. As fixtures Claude
agora carregam o ID efetivo do arquivo, em vez do ID genérico anterior.

## Evidências

- `cargo test -p mercury app::usage::tests --locked --offline`: 21 PASS.
- `cargo test -p mercury-usage --locked --offline`: 48 PASS.
- Manutenção e arquitetura: 80 PASS.
- `cargo fmt --check`, `git diff --check`: PASS.
- `cargo clippy -p mercury --all-targets --locked --offline -- -D warnings`: PASS.
- `cargo build -p mercury --locked --offline`: PASS.
- Suíte principal: execução no sandbox teve 3448 PASS / 267 FAIL / 14 ignorados,
  incluindo falhas PermissionDenied ao abrir sockets/PTYs. Reexecução com essas
  permissões: **3715 PASS / 0 FAIL / 14 ignorados** (50,50s).
  Comando: `cargo test -p mercury --bin mercury --locked --offline -- --test-threads=2`.

Ambiente: Rust 1.96.1; Zig 0.16.0 oficial em `/private/tmp`, cache Cargo/Zig
no workspace. O Zig padrão 0.17 não é compatível com o componente vendorizado.

## Gate de segurança

Revisão local sem achados bloqueadores. SonarQube, Veracode, Wiz e cobertura
quantitativa: dispensados/não executados conforme `.specify/memory/constitution.md`.
Não representam gates externos aprovados.

## Limitações

Versão instalada e sessões em execução não foram alteradas. O incidente real
do chat “CI/CD” precisa de smoke após ativação da versão corrigida; os testes
automatizados demonstram o defeito corrigido, sem comprovar a causa completa
do incidente ao vivo.


## Complemento: títulos do histórico (2026-11-09)

Implementado conforme aprovação: nome manual do Mercury, título do provedor,
primeira mensagem do usuário ou “Conversa sem título”. Claude lê títulos do
transcript/índice do projeto; Codex lê o índice separado de nomes, inclusive
renomeações sem alteração do transcript. O leitor de metadados Codex agora aceita
`payload.id` além do campo legado `session_id` usado nas fixtures antigas.

O índice SQLite recebe migração transacional, mantém o FTS e atualiza os títulos
antigos mesmo com os arquivos sem alteração. Labels do Mercury são preservados
nos checkpoints de sessão e nas consultas ao histórico, indexados por provedor e
ID. O último nome manual conhecido continua disponível após fechar o painel.
Nomes ausentes/malformados dos provedores permitem fallback. Limites de leitura,
normalização de controles/espaços e limite de comprimento protegem a exibição.

API recebe campo opcional `title`, com compatibilidade com respostas anteriores;
schema público regenerado e verificado. A lista exibe o título, reserva espaço
para metadados e preserva o trecho da busca em telas estreitas. O ID continua
sendo usado para reabrir a sessão correta, inclusive quando há títulos iguais.
A CLI textual também prioriza o título e conserva o ID como informação auxiliar.

Revisão consolidada local: PASS, sem achados bloqueadores. Nenhuma nova dependência
nem leitura de metadados na renderização. O armazenamento de nomes manuais usa o
worker de checkpoint existente. Gates externos/cobertura seguem dispensados.

Validação:

- Suíte principal: **3720 PASS / 0 FAIL / 14 ignorados**.
- `cargo test -p mercury-history --locked --offline`: **12 PASS / 1 ignorado**.
- `cargo test -p mercury --test history_open --locked --offline -- --test-threads=1`:
  **10 PASS**, em servidor/PTYs isolados e com provedor simulado.
- Manutenção/arquitetura: **80 PASS**.
- Schema gerado: PASS; compatibilidade sem campo title: PASS na suíte principal.
- Clippy com `-D warnings`, build, formatação e `git diff --check`: PASS.

A primeira execução da integração expôs contexto herdado do Mercury em execução,
causando workspace extra e alvo incorreto. As fixtures agora removem essas
variáveis de seus processos filhos; a reexecução isolada passou. Dois avisos de
estilo do Clippy foram corrigidos sem mudar comportamento; o módulo de histórico
foi revalidado após esses ajustes. Não foram executados benchmarks de conversas
reais nem migração no perfil instalado. Instalação e ativação continuam pendentes.


## Ativação local (2026-11-18)

Solicitada explicitamente pelo usuário: “Reinicie e atualize”. Instalado o
artefato dev já validado com `scripts/install-local.sh --profile dev --no-build`;
nenhuma entrada de compilação estava mais nova que o executável. Config check OK.
O executável anterior foi guardado em arquivo temporário privado para rollback.
SHA-256 do artefato e da cópia instalada:
`754b8e04e6f6e6430c1145db4d167428f5221a35dca75ac08df6d02c88ddf71f`.

Reinício feito por `server live-handoff --import-exe` apontando para o executável
instalado. Status posterior: servidor em execução, protocolo compatível,
`restart_needed=false`, `server_binary_stale=false`. Os processos Mercury
observados após a transferência carregam o novo executável instalado.

Inventários privados antes/depois confirmaram 1 workspace, 7 painéis e 1 cron,
com os IDs preservados e os mesmos 7 PIDs de shell. Não se afirma igualdade de
todos os metadados: um painel apresentou mudança do campo value da identidade
de sessão entre as coletas; os demais campos dessa identidade permaneceram
iguais. Não foram enviados prompts nem encerrados os processos dos chats.
A validação do aumento de contexto exclusivamente no chat em uso continua sendo
um smoke interativo, distinto da confirmação do reinício e da preservação dos
processos.

Smoke de `history.search` após ativação: a consulta inicial excedeu o timeout de
50 segundos. Isso não foi registrado como PASS; a reconstrução/releitura inicial
do índice roda no worker de histórico. A atualização e o handoff foram
confirmados separadamente, sem depender desse smoke.
