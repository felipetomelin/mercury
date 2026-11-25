# Pesquisa brownfield

## Áreas relevantes

- `crates/mercury-history/src/parse.rs`: lê session_meta, porém não retém source.
- `crates/mercury-history/src/lib.rs`: indexa todos os transcripts Codex elegíveis
  e aplica filtros SQL/limit. Não distingue Guardian de conversas comuns.
- `crates/mercury-history/src/titles.rs`: migração e metadados de títulos da 012.
- `src/api/schema/history.rs`, `src/server/headless/history.rs`: contrato e worker.
- `src/client/shell/history_view.rs`, `state.rs`: lista, filtros e busca.
- `src/client/shell/sidebar.rs`: apresentação de projetos e agentes.
- `src/protocol/wire.rs`, `src/server/client_shell.rs`: snapshot tipado e projeção.

## Evidência local

Consulta somente leitura ao banco de metadados do Codex encontrou mais de 760
threads com título “Guardian review”. Todas as linhas consultadas tinham
`source = {"subagent":{"other":"guardian"}}` e `has_user_event = 0`.
`thread_source` variava entre guardian_review, subagent e ausente; por isso não
deve ser a única identificação. A contagem cresce durante uso normal.
Um session_meta de amostra confirmou o mesmo source no próprio transcript.
Nenhuma mensagem, instrução base ou conteúdo de ferramenta foi impresso.

## Padrões e contratos

- Preservar arquivos originais; mudanças ficam no índice privado do Mercury.
- A migração precisa invalidar a leitura incremental antiga uma única vez ou
  fazer backfill equivalente. Filtrar só linhas recebidas pela UI deixa o limit
  ocupado por sessões internas e não resolve o problema.
- Cores de estado já existem; usar um marcador separado para identidade do path.
- `ClientShellWorkspace.new_workspace_cwd` é resolvido pelo tab ativo e não
  garante identidade estável. Não usá-lo indiscriminadamente como cor de projeto.
- O workspace tem identity_cwd no servidor. O histórico já transporta cwd; o
  snapshot dos agentes permite associar pane -> workspace.

## Riscos e restrições

Título sozinho produz falsos positivos. Ausência de user event também pode ser
legítima em sessões novas. Não consultar SQLite interno do Codex como dependência
de produção: os transcripts já fornecem a origem necessária. Cores não são IDs
únicos nem podem substituir status. Alterações no wire binário exigem evolução
compatível conforme testes/protocolo do projeto.

## Perguntas resolvidas

O usuário confirmou cores no histórico e na lista lateral. Sem questões de
produto impeditivas; aprovação da especificação ainda pendente.
