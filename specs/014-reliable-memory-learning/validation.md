# Validação da implementação portátil — 014

Data: 2026-11-26. Autorização: planejar, implementar, testar e publicar no
repositório central. Sem alteração da configuração/vault pessoal.

## Resultados locais

- Workspace completo, serial: **4.186 PASS / 0 FAIL / 16 ignorados** em 44
  execuções de testes/doc-tests. Inclui IPC/PTY, sessões, detach/reattach,
  handoff (23 testes), retomada, roteamento e os demais crates.
- Suíte principal repetida após ajustes de origem/retry/cache: **3.724 PASS /
  0 FAIL / 14 ignorados**. Alterações finais receberam testes focados adicionais: Learn **57 PASS /
  1 smoke ignorado**, Knowledge **10 PASS / 1 smoke ignorado**.
- Learn + Knowledge finais: **135 PASS / 0 FAIL**; incluem 11 testes novos de
  checkpoints com SQLite/snapshots reais e instalações temporárias independentes.
- Grupo Knowledge no binário: **10 PASS / 0 FAIL / 1 smoke ignorado**. Fluxo
  integrado dos dois workers validou memória, nota, restart sem novo binding,
  dedupe, no_change, 100 contextos sem novo gasto e undo.
- Manutenção/arquitetura: **80 PASS / 0 FAIL**.
- Clippy all-targets dos três pacotes afetados com `-D warnings`, build do
  Mercury, fmt e diff checks: PASS. Schema API regenerado e conferido.

A execução inicial de cargo test com threads paralelas encontrou colisões de
variáveis globais/sockets e locks envenenados em testes compartilhados. A execução
serial do workspace resolveu esses resultados sem esconder testes. A manutenção
exigiu PATH do Rust e CARGO_HOME do cache local; após configurar ambos, 80/80.
Os testes de CLI com cfg específico não executados no macOS não são qualificados.

## Contexto e cache

Corpus versionado no teste `local_context_budget_cache_graph_hop_and_hundred_queries_need_no_model`:
32 notas, dois projetos, decisão de deploy e relação explícita de rollback.
Baseline FTS recupera a decisão; contexto compacto mantém a decisão e inclui a
regra de rollback (2/2 evidências verificadas), com oito fontes e **7.913 bytes**
no resultado observado. Primeira recuperação: ~482 ms; 100 recuperações em cache:
~317 ms no total; **zero chamadas de modelo**. São medidas desta execução, sem
alegação de ganho percentual geral ou equivalência com busca semântica LLM.

Regressões verificam invalidação por revisão, filtro/projeto, orçamento,
modelo/grafo para respostas opcionais, caracteres delimitadores, bytes escapados,
Unicode, separação de status e falta de evidência. Respostas GraphRAG repetidas
reutilizam o cache; alterações na revisão impedem reutilizar o grafo antigo.

## Provedor real, somente fixtures sintéticas

- Learn: **PASS**, uma aplicação em LearnMemory temporário, frontmatter proposed
  preservado e undo restaurando exatamente os bytes originais. Custo monetário
  reportado pelo Codex: desconhecido (`None`), sem estimativa fabricada.
- Knowledge: **PASS**, fixture de decisão explícita ADR-17; três notas indexadas
  (duas fontes + uma proposta publicada), nove entidades, cinco relações
  inferidas, três resumos, grafo current e nenhuma falha. Dezoito tentativas
  reservadas conservadoramente no orçamento; não são dezoito chamadas medidas.
- A primeira fixture genérica retornou legitimamente no_change, sem proposta.
  O harness foi interrompido e refinado para uma decisão sintética inequívoca;
  isso não foi contado como comprovação de publicação. O segundo smoke passou
  em ~118 segundos. Não houve leitura/envio de conversas ou notas pessoais.

## Gate local e publicação

Revisão consolidada: **PASS**, sem achado local bloqueante conhecido.
Achados corrigidos e limites em review.md. SonarQube, Veracode, Wiz e cobertura
quantitativa continuam dispensados pela exceção vigente de 2026-10-29 do Mercury;
não foram executados nem reportados como PASS externo.

CI Linux/macOS executado com sucesso para o commit de implementação `518008c`,
com testes de memória/indexação/retrieval isolados, sem credenciais.
Execução: [https://github.com/felipetomelin/mercury/actions/runs/45835386533](https://github.com/felipetomelin/mercury/actions/runs/45835386533). Ambos os jobs concluíram success.
Implementação publicada em origin/main; diretório de trabalho sincronizado.
Windows não foi qualificado para gravações privadas Unix. Nenhuma ativação em
configuração pessoal, deploy ou aceite manual do vault real faz parte desta entrega.

---

# Evidências da investigação original (histórico)

Data: 2026-11-26.

## Executado

- Inspeção do código e das specs 007/008/011 e decisões posteriores.
- Leitura seletiva da configuração ativa, sem imprimir credenciais.
- CLI instalada: status, learn status e knowledge status, somente consulta.
- SQLite em mode=ro: contagens/estados de Learn/Knowledge e metadados de sessão.
- Vault: existência, symlink, contagem, tamanho/data de notas; não leu conteúdo.
- Hooks globais: somente tipos/eventos e presença da integração Mercury.
- CLIs: --version/--help; nenhuma chamada de modelo nem alegação de autenticação.
- Probe Rust sintético temporário, vinculado à biblioteca Learn compilada:
  - session_id: leitura OK;
  - somente id: MissingIdentity;
  - 40 turnos: 64 mensagens retidas, truncated=true, user_turns=None.
- Inspeção de metadados e tipos de eventos das três sessões Codex ativas, sem
  imprimir mensagens/base_instructions; todas excedem 4 MiB.

## Não executado

Não houve geração/publicação de notas, atualização de memória, retry/rebuild,
mudança de consentimento, configuração, hooks ou código de produção, inferência
real, instalação/restart, commit/push nem processamento de histórico em massa.
Nenhuma suíte de implementação foi repetida: esta etapa só alterou documentação.
O probe demonstra os comportamentos do leitor e não substitui testes de regressão
versionados nem um smoke ponta a ponta com provedores reais.

## Localização e privacidade

Respostas brutas e fixtures/probe ficaram em diretórios temporários privados,
fora do repositório. Os arquivos deste plano contêm apenas observações agregadas,
erros sanitizados e exemplos sintéticos. Caminho pessoal do vault, IDs de sessão,
mensagens, instruções e conteúdo de notas não foram incluídos no plano público.

## Resultado

Diagnóstico reproduzível e plano preparados. Não se declara o fluxo corrigido
ou funcional em produção. Os gates de implementação e aceite operacional ainda
estão pendentes; a prova final está definida nos critérios D4/D5 da spec.

## Comandos reproduzíveis

```sh
cargo test --workspace --locked -- --test-threads=1
cargo test -p mercury-learn -p mercury-knowledge --locked
cargo test -p mercury --bin mercury learn --locked -- --test-threads=1
cargo test -p mercury --bin mercury knowledge:: --locked -- --test-threads=1
cargo test -p mercury --bin mercury generated_protocol_schema_artifact_is_current --locked
cargo clippy -p mercury -p mercury-learn -p mercury-knowledge --all-targets --locked -- -D warnings
cargo build -p mercury --locked
cargo fmt --check
git diff --check
```

Smokes reais são opt-in: filtros exatos
`live_routed_learn_smoke_with_synthetic_memory_only` (MERCURY_LEARN_LIVE_SMOKE=1)
e `live_automation_smoke_with_synthetic_session_only`
(MERCURY_KNOWLEDGE_LIVE_SMOKE=1), com `--ignored --nocapture`. Não ativar os demais
testes ignorados sem ler seus contratos. Seguir o toolchain Rust/Zig do projeto;
cache offline e PATH usados aqui são opções locais, não caminhos incorporados.
Varredura local de padrões de segredos em diff e arquivos novos: zero achados.
