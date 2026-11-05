# Knowledge & GraphRAG — revisão e evidências — 2026-10-29

**Estado**: concluído com validação local; gates externos dispensados explicitamente pelo usuário para Mercury em 2026-10-29.
**Spec/plano**: `specs/008-knowledge-graphrag/`. Aprovação explícita recebida nesta
sessão. O usuário substituiu Ollama pelos CLIs autenticados Codex/Claude.

## Comportamento entregue

- `mercury-knowledge`: Markdown UTF-8, propriedades escalares/listas, wikilinks,
  relações tipadas, SQLite/FTS5 privado, snapshots, renomeação por identidade/hash,
  atualização incremental e invalidação de fontes/derivados/bundles.
- Cache de extração validado por chunk/hash/fingerprint; `index --graph --rebuild`
  refaz o índice sem reutilizar extrações. Mudança de projeto invalida o cache.
- Features locais de tokens/bigramas, descritores e expansão semântica pelo LLM,
  ranking híbrido, extração de entidades/relações, comunidades, resumos e consultas
  GraphRAG local/global. Não são embeddings neurais nem Microsoft GraphRAG Python.
- Codex/Claude por stdin e CLIs autenticados, regras Mercury + estratégia por
  tarefa e limites observados; fallback restrito aos provedores permitidos.
  Executáveis, versões, modelos configurados/reportados e observações ficam
  identificados quando disponíveis. Defaults não equivalem a modelo fixado.
- API tipada `knowledge.request`, CLI, jobs persistidos, cancelamento/deadline,
  diagnóstico, aba Knowledge/kg, busca e detalhe de fonte sem iniciar agente.
- Captura de sessão concluída por registro prévio ou declaração explícita
  `--completed`. Proposta privada, aceite/rejeição, wikilinks de relações explícitas
  pertinentes oriundas de notas validadas, auditoria e undo com conflito humano.
- Contexto por projeto com fontes e IDs separados de decisões validadas,
  pendências e documentos obsoletos. Inferências não são promovidas ao bundle.
  `route --context` verifica revisão, fontes, projeto e autorização por destino.

Guia de configuração/operação: `docs/knowledge/README.md`. A configuração começa
desabilitada. Os testes não ativaram o vault real nem modificaram credenciais,
configuração pessoal ou servidores Mercury pessoais. Sem commit/push.

## Revisão consolidada

**Veredito local: PASS** para comportamento e testes do grupo, sem alegar aprovação
de scanner. Revisão executada no mesmo agente, sem delegação. Pesquisa, testes e
avaliação real foram agrupados; suíte e avaliação correram em paralelo.

| Achado durante implementação | Correção e evidência |
| --- | --- |
| Inicialização SQL concorrente com polling de jobs | Migração transacional somente quando necessária; polling não executa DDL. Teste concorrente de 1.000 notas. |
| Relações ou referências fora do contrato do modelo | Enums de relações, nomes/quotes contidos na fonte, IDs de evidência validados e uma correção limitada. Persistência só após validar o conjunto. |
| Captura com limite total menor que o limite de mensagem | Total corrigido para 16 KiB; cenário integrado cobre leitura, proposta, aceite e undo com Learn desligado. |
| Aba adicional escondia controles em terminal estreito | Rótulo kg e omissão de segmentos decorativos quando necessário; teste PTY a 80 colunas e proteção do rótulo da aba focada. |
| Contratos CLI/capacidades | Descrições de completions, ordenação da allowlist, schema regenerado e baseline de hash somente do novo método. Métodos existentes preservados. |
| Timers de jobs concluídos retinham estado | Timer abortado ao finalizar/falhar antes de executar; cancelamento/status têm capacidade independente da fila de trabalho. |
| Garantia de privacidade em Windows seria incompleta | Armazenamento recusa plataformas sem validação Unix; não afirma proteção equivalente por ACL. |

Arquivos/transcripts/modelos não podem escolher caminhos de escrita, executáveis,
hooks, MCPs, shell ou ações de Git. Os CLIs rodam em diretórios privados
descartáveis, com ferramentas/customizações desabilitadas, timeout e limites de
saída; prompts não entram em argv. Notas e transcrições continuam dados não
confiáveis. Rejeitar saída inválida não implica que uma citação existente prove
suporte semântico: isso é avaliado separadamente.

## Testes e execução

Host: **Darwin arm64**; Rust e Zig conforme o toolchain do projeto; cache local.

| Verificação | Resultado |
| --- | --- |
| `cargo test --workspace --offline --locked -- --test-threads=1` com TMPDIR curto e permissões para fixtures Unix | **4.112 PASS, 0 FAIL, 13 ignorados**, exit 0, sem filtros. |
| Núcleo Knowledge após refinamento do bundle/cache | **19/19 PASS**. Corpus de 1.000 notas, p95 de busca aquecida < 500 ms, reopen, rename/delete, cancelamento, caminhos, SQLite concorrente, egress, subprocesso/timeout e proveniência. |
| CLI/API/worker/GraphRAG/TUI/capture/review/undo final | **PASS** com servidor/PTY reais e Codex simulado; somente dados temporários. |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | PASS, com rerodada incremental após os ajustes finais. |
| `cargo fmt --all --check` e `git diff --check` | PASS. |
| Manutenção e arquitetura de hot path | **80/80 PASS**. |
| Schema e contratos existentes | Schema regenerado; novo método tem baseline explícito; contratos antigos passaram. |

A suíte ampla precede os últimos refinamentos de agrupamento do bundle,
guardas de plataforma e exportação de links; estes tiveram testes focados do
núcleo e do fluxo integrado, além de Clippy/formatação. Contagens de rerodadas
não são somadas como testes únicos. Alvos com zero testes por cfg e ignorados
não qualificam outras plataformas.

Logs locais: `/private/tmp/mercury-008-workspace-final.log`,
`mercury-008-core-final.log`, `mercury-008-e2e-final.log`,
`mercury-008-clippy-final.log`, `mercury-008-maintenance.log`.

## Avaliação com provedores reais

Smoke sintético executou extração, resumo e consultas local/global com os CLIs
autenticados. A avaliação versionada usa quatro notas de engenharia fictícias,
20 perguntas com resposta e cinco sem evidência. Relatório revisável e hashes do
corpus: `docs/dev/knowledge-evaluation-20261029.json`.

- Perguntas conhecidas: **19/20** na rubrica de termos e fontes esperadas.
- Relações: **5/6**; sínteses globais: **6/6**.
- Falta de evidência: **5/5**, sem fabricar resposta.
- Limiar da spec satisfeito pela rubrica; fontes e afirmações inspecionadas pelo
  agente. Revisão humana permanece distinta.
- O caso `relation-2` citou Queue.md, que contém explicitamente a dependência de
  autenticação, enquanto a rubrica esperava Authentication.md. A falha foi mantida;
  não foram alteradas as fontes esperadas para fazer a avaliação passar.
- Medianas no corpus pequeno: FTS < 1 ms, híbrida ~5,6 s e GraphRAG ~14,1 s;
  maior consulta GraphRAG ~58,6 s. Não generalizar para vaults maiores.

Houve retomada após uma resposta global inválida, corrigida por tentativa limitada
e revalidação. Durante a avaliação, Codex mudou de 0.164.0 para 0.165.0; o
fingerprint detectou a mudança e o grafo foi refeito. Claude reportou 2.1.289.
As respostas mantêm seus fingerprints; o relatório combina versões identificadas,
não uma comparação com modelo fixo. Observações de custo retidas cobrem somente
a retomada, não o custo total; custo Codex não reportado é desconhecido.

## Aceites e limites

- SC-001/002: corpus 1.000 notas, recuperação e busca verificados no núcleo.
- SC-003/004: rubrica real acima e verificação de referências/snapshot; sem
  alegação de qualidade universal ou de que o roteador conhece um ótimo global.
- SC-005/006: default-off, allowlist por provedor/destino, stdin, isolamento,
  symlink/conflito, invalidação, timeout e saída inválida cobertos.
- SC-007: fixture completa e smoke real registrados separadamente.

Limites residuais: operações de filesystem não oferecem CAS universal contra
outro editor do mesmo usuário; `--completed` é declaração humana, não detecção de
processo; defaults dos modelos podem mudar sem atualizar o CLI; Windows/ACLs e
smokes Linux não foram qualificados. Templates de YAML suportam subconjunto
documentado. Nenhuma inferência se torna decisão aprovada automaticamente.

## Gates externos — DISPENSADOS pelo usuário em 2026-10-29

Depois da entrega do relatório, o usuário declarou nesta conversa que não é
necessário passar pelos gates externos para o projeto Mercury. A dispensa cobre
SonarQube (Quality Gate/hotspots), Veracode, Wiz e cobertura quantitativa; vale
para o projeto até nova orientação e está registrada na constituição e em
CLAUDE.md. Referência: decisão explícita nesta conversa, sem ticket externo.

Risco aceito: análises automatizadas de segurança/dependências e a medição de
cobertura podem deixar problemas sem detecção. Os checks externos não foram
executados e não são declarados PASS. Testes, revisão local, build, Clippy,
formatação e manutenção continuam obrigatórios; achados conhecidos não são
ignorados. A implementação 008 fica encerrada conforme a política autorizada,
com K023 concluído por dispensa. Limites técnicos/manuais acima permanecem.
