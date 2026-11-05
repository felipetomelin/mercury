# Knowledge, Obsidian e GraphRAG

O Mercury indexa um vault Markdown local e usa os CLIs autenticados Codex/Claude
para extração, resumos, expansão de consultas e respostas. Não usa Ollama nem
instala modelos. Índices, propostas e jobs ficam no diretório privado de dados.
O Obsidian pode permanecer fechado. A funcionalidade começa desabilitada.
Neste ciclo, o armazenamento privado exige permissões Unix (macOS/Linux); Windows
recusa a abertura até existir validação equivalente de ACLs.

## Configuração

Acrescente à configuração ativa do Mercury e recarregue pelo fluxo de config.
Use caminhos absolutos canônicos (sem symlinks) e executáveis já autenticados.
O exemplo habilita o envio dos trechos selecionados aos provedores listados.

```toml
[knowledge]
enabled = true
vault = "/absolute/path/to/obsidian-vault"
allow_external = true
allowed_providers = ["claude", "codex"]
max_model_calls = 128
max_call_budget_usd = 0.10 # limite por chamada Claude; Codex não reporta custo
# Não habilita envio de contexto a tarefas Route:
allow_route_external = false
route_destinations = []
timeout_secs = 60
max_context_chars = 32000
exclusions = ["Pessoal"]

# Opcionais: normalmente resolve executáveis pelo PATH do servidor.
# [knowledge.executables]
# claude = "/caminho/absoluto/claude"
# codex = "/caminho/absoluto/codex"

# Opcionais: escolha nomes disponíveis na sua conta.
# Sem modelo explícito, usa o default do CLI autenticado.
# [knowledge.models]
# claude = "alias-disponivel"
# codex = "modelo-disponivel"
```

As regras `[route.rule]` existentes têm precedência e podem usar tags
`["knowledge", "extract"]`, `["knowledge", "summarize"]` ou
`["knowledge", "answer"]`, inclusive por projeto. Sem regra específica, a
estratégia prioriza Claude para extração/resumos e Codex para respostas. Respeita
limites observados, permite fallback somente entre os provedores habilitados e
não promete que uma escolha é superior sem avaliação. Autenticação permanece
nos CLIs; não copie tokens para o vault ou configuração.

## Fluxo básico

Com o servidor Mercury atualizado em execução:

```sh
mercury knowledge status --json
mercury knowledge doctor --json
mercury knowledge index --graph --json
mercury knowledge index --graph --rebuild --json
mercury knowledge jobs show --id JOB_ID --json
mercury knowledge jobs cancel --id JOB_ID --json
mercury knowledge search --query "traces ausentes" --project PV --json
mercury knowledge search --query "fila parada" --mode hybrid --json
mercury knowledge related --id "Meu Servico.md" --json
mercury knowledge ask --query "Quais decisões afetam este serviço?" --mode local --json
mercury knowledge ask --query "Quais causas se repetem?" --mode global --json
mercury context build --query "migração" --project PV --json
```

`index` retorna um job; `ask`, busca híbrida, `doctor` e `capture` aguardam o job
pelo CLI. O ID permite inspeção e cancelamento por outro terminal. Timeout do
cliente não implica cancelamento; use o comando explícito. Jobs interrompidos
por restart são identificados como interrompidos, sem reenvio silencioso.
`doctor` verifica executáveis/versões e configuração, sem consumir modelo nem
alegar autenticação confirmada. A autenticação só é verificada numa chamada real.

Na TUI, clique em **knowledge** (ou **kg** em largura reduzida): `/` busca,
setas selecionam, Enter mostra a fonte, `r` consulta status e `q` retorna.
A interface lê dados do servidor; não inicia agentes ao buscar ou abrir fonte.

## Notas e grafo

```markdown
---
id: servico-alpha
project: PV
type: service
status: validated
depends_on:
  - '[[Servico Beta]]'
---
# Serviço Alpha
Usa [[Servico Beta]] porque precisa de replay.
```

Propriedades suportadas são escalares e listas de strings no YAML de topo; campos
aninhados/tags executáveis não são interpretados. O texto original é preservado.
Tipos sugeridos: project, service, decision, incident, runbook, learning.
Estados: proposed, validated e deprecated; ausência/outro valor é unreviewed.
Links ambíguos são diagnósticos; não se escolhe arbitrariamente uma nota.

Wikilinks e relações tipadas explícitas alimentam a navegação. GraphRAG extrai
entidades/relações, agrupa comunidades, produz resumos citados e consulta modos
local/global. Relações extraídas continuam **inferidas**. Citação existente e
quote literal não provam correção semântica: revise decisões antes de adotá-las.
Vetores são features locais de tokens/bigramas, complementadas por descritores
e expansão semântica por LLM; não são embeddings neurais de uma API.

## Captura e revisão

```sh
mercury knowledge capture --provider codex --id SESSION_ID --completed \
  --title "Aprendizado de migração" --project PV --json
mercury knowledge proposals --json
mercury knowledge review --id PROPOSAL_ID --mode accept --json
mercury knowledge review --id PROPOSAL_ID --mode reject --json
mercury knowledge review --id PROPOSAL_ID --mode undo --json
```

Use `--completed` somente após encerrar a sessão; essa declaração do usuário fica
registrada na origem. Sem ela, a captura exige conclusão previamente registrada
pelo Mercury. O leitor verifica identidade e estabilidade da transcrição antes
e depois do modelo. Isso funciona sem ativar o Learn.

Captura cria proposta privada. Aceite explícito publica em `Mercury/` no vault;
repetição é idempotente. Relações explícitas de notas validadas do mesmo projeto, com os dois nomes presentes no aprendizado, podem acrescentar wikilinks na proposta (até oito); relações inferidas não são exportadas como aprovadas. Undo remove somente a nota gerada ainda intacta; edição
humana produz conflito. O ledger conserva os bytes para auditoria/recuperação.
Nenhuma nota humana fora do diretório gerenciado é substituída. Há uma corrida
residual entre verificação e unlink contra outro editor do mesmo usuário; não
se promete compare-and-swap universal do filesystem.

## Contexto no Route

Gerar contexto não envia tarefa. O bundle agrupa IDs de decisões validadas, fontes pendentes e fontes obsoletas, sem duplicar texto. Inferências automáticas não são promovidas ao contexto de execução; a lista de inferências permanece separada e vazia neste fluxo conservador. Para anexar um bundle com o mesmo projeto da
workspace escolhida, habilite explicitamente `allow_route_external=true` e
`route_destinations=["codex", "claude"]` (ou somente o destino desejado).

```sh
mercury route submit --stdin --project PV --context BUNDLE_ID
```

O Mercury verifica projeto, revisão, fontes atuais e destino permitido antes de
criar a execução e novamente antes de enviar o prompt. Mudanças nas fontes exigem
reindexar e gerar outro bundle. Uma falha tardia preserva o pane para inspeção.

## Limites e recuperação

Defaults: 10.000 notas, 1 MiB/nota, 256 MiB de texto; GraphRAG limita 4.096 chunks,
64 MiB de vetores e chamadas conforme config. Uma escrita/modelo por vez, fila de
até oito operações, deadline de uma hora por job e timeout por subprocesso.
Consulta local: dois saltos, 50 entidades, 100 relações, até 20 chunks. Global:
até 16 comunidades; seus resumos têm cobertura limitada às fontes citadas.
Envelopes grandes falham explicitamente. Conteúdo nunca vira comando/shell.

Alterar/remover/renomear fonte invalida derivados e bundles na próxima indexação;
a consulta detecta fontes divergentes e exige reindexação. Falha/cancelamento não
publica um grafo parcial. Extrações de chunks intactos são reutilizadas, com hash e fingerprint; fontes alteradas/removidas eliminam seu cache. Índice e grafo são reconstruíveis com `index --graph --rebuild`.
Propostas e auditoria de captura são dados duráveis: faça backup do diretório
privado antes de removê-lo. Nenhum comando apaga o vault.

## Verificação

`cargo test -p mercury-knowledge --offline` usa apenas fixtures/mocks.
`cargo run -p mercury-knowledge --example native_smoke --offline` faz chamadas
reais em nota sintética temporária. `--example evaluate` usa o corpus versionado
em `tests/fixtures/evaluation`, 20 perguntas + 5 sem resposta e grava relatório
em `/private/tmp` (ou temp canônico do sistema). Essa avaliação consome modelos;
não integra a suíte automática. A rubrica de palavras-chave/fontes complementa
a revisão humana de suporte semântico, não a substitui.

Defaults de modelos podem mudar sem alterar o binário do CLI. Para uma avaliação
reproduzível, configure modelos explicitamente e registre os modelos reportados;
use `index --graph --rebuild` após trocar defaults. Não há alegação de latência
ou ganho de qualidade universal a partir do corpus sintético.
