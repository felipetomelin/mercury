# Tarefas agrupadas

Status: concluído; implementação e validação local verificadas, gates externos dispensados explicitamente pelo usuário em 2026-10-29. Evidência: `docs/dev/knowledge-phase8-review.md`.
`[P]` indica independência de arquivos, não autorização para delegar agentes.

## G0 — Especificação e baseline

- [x] K001 Pesquisar arquitetura, histórico e fontes primárias.
- [x] K002 Especificar, esclarecer escopo e preparar plano/análise.
- [x] K003 Spec/plano aprovados explicitamente pelo usuário em 2026-10-29.
- [x] K004 Revalidar baseline e corrigir erros reproduzíveis. Relatório: `docs/dev/mercury-current-audit-20261029.md`; aceites manuais/backlogs separados.

## G1 — Dados e memória (FR-001–004, 008, 012–014)

- [x] K005 Criar crate/config, migrações, modelos e armazenamento privado.
- [x] K006 [P] Parser Markdown/propriedades/links e política de caminhos.
- [x] K007 Ingestão incremental, identidade, snapshots e invalidação transitiva.
- [x] K008 FTS5, filtros, relações explícitas e inspeção de evidências.
- [x] K009 Revisar e testar grupo: restart, rename/delete, conflitos e limites.

## G2 — Busca e GraphRAG (FR-005–008, 010, 013–015)

- [x] K010 Backend nativo Codex/Claude, diagnóstico, saída estruturada, egress e limites.
- [x] K011 Features locais versionadas, expansão semântica e ranking híbrido determinístico.
- [x] K012 Extração de entidades/relações com proveniência e inferência marcada.
- [x] K013 Comunidades versionadas e resumos com evidências, sem dados obsoletos.
- [x] K014 Query local/global e validação de citações; erro explícito sem backend.
- [x] K015 Corpus/perguntas versionados, benchmark FTS/híbrida/GraphRAG e smoke real.
- [x] K016 Revisão e teste consolidado do grupo, incluindo falhas e cancelamento.

## G3 — Fluxos Mercury e Obsidian (FR-009–011, 015)

- [x] K017 Worker/job API, CLI, schema e anúncio de capacidades compatíveis.
- [x] K018 Knowledge tab/status/fontes e teste PTY isolado.
- [x] K019 Captura privada, review, aceite, exportação de links e undo com conflito.
- [x] K020 Context bundle e Route opt-in com política de destino/egress.
- [x] K021 Documentar setup/vault/modelos, recuperação e comandos com exemplos.
- [x] K022 Revisão/testes do grupo e integração ponta a ponta.

## G4 — Fechamento

- [x] K023 Gates externos SonarQube/Veracode/Wiz e cobertura quantitativa dispensados explicitamente pelo usuário para o projeto Mercury em 2026-10-29; risco e origem registrados na constituição. Checks externos não executados, sem alegar PASS. Formatação, Clippy e testes locais passaram.
- [x] K024 Registrar aceites SC-001–007 e limitações por plataforma/modelo.
- [x] K025 Atualizar status vigente e backlog, sem declarar mocks como smoke real.

Dependências: G0 → G1 → G2 → G3 → G4; K017 pode ser preparado após contratos G1.
GraphRAG K012–015 é obrigatório neste ciclo; não encerrar somente com K008/K011.
