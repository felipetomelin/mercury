# Pesquisa brownfield — 2026-10-29

## Áreas relevantes

- `Cargo.toml`: workspace Rust, seis crates auxiliares; reqwest/tokio/serde/sha2
  já disponíveis. Não há regra Rust global entre as regras inspecionadas;
  seguir padrões locais e Clippy, sem tratar C como Rust.
- `crates/mercury-history/src/lib.rs`: SQLite/FTS5, sincronização por arquivos,
  proteção de diretório e conexão privada. Aproveitar padrões, não misturar
  transcrições com o índice de conhecimento validado.
- `src/server/headless/history.rs`: dispatch limitado (`MAX_PENDING=8`),
  `spawn_blocking` e respostas tipadas. Base para serviço de knowledge.
- `src/api/schema/{history,learn,route}.rs`, `src/api/schema/response.rs`,
  `src/cli/spec.rs`, `src/server/client_commands.rs`: contratos e capacidades
  devem mudar juntos. Schema gerado em `docs/next/api/mercury-api.schema.json`.
- `crates/mercury-learn/src/{policy,changes,storage,transcript}.rs`: escrita
  auditável, identidade, undo e leitura limitada. Não reutilizar o flag Learn
  como consentimento para publicar conhecimento ou transmitir contexto.
- `src/app/learn.rs`, `src/app/api/route.rs`, `src/cli/route/`: integrar contexto
  por ação explícita; não alterar caminho normal do Route por default.

## Padrões e contratos

Servidor detém estado, clientes consomem APIs. Workers possuem limites, filas e
respostas tipadas. Config usa TOML; novos campos devem ter defaults compatíveis.
Testes usam diretórios temporários para configuração, runtime e dados. Já houve
vazamento de fixtures para dados pessoais, corrigido no ciclo anterior: manter
esse isolamento inclusive para vault, modelos mock e índices.

## Riscos e restrições

- 1.313 entradas no status inicial (908 removidas, 303 modificadas, 102 novas).
  Não restaurar, limpar, trocar de branch ou atribuir essas mudanças a esta tarefa.
- Vault informado existe e estava vazio; não é evidência de permissões futuras
  de escrita. Nesta fase ele foi apenas inspecionado.
- Um agente externo recebendo contexto também é egress, mesmo com índice local.
- Modelo pode inventar relação ou citar trecho que não a comprova. Validação
  estrutural de citações e avaliação humana de suporte são controles distintos.
- Busca semântica, grafo de links e Microsoft GraphRAG são conceitos distintos.
- Licenças/notices ausentes e smokes reais são pendências históricas; revalidar.
- Dispensa histórica de scanners aplica-se à entrega anterior, não se estende
  automaticamente à nova feature.

## Referências primárias consultadas

- https://obsidian.md/help/bases — visualizações sobre Markdown/propriedades.
- https://obsidian.md/help/plugins/graph — grafo de links e navegação local.
- https://microsoft.github.io/graphrag/index/methods/ — extração de entidades,
  relações, sumarização e relatórios de comunidades no pipeline padrão.
- https://microsoft.github.io/graphrag/config/models/ — modelos customizados e
  exigência de saídas estruturadas; compatibilidade local precisa ser validada.
- https://docs.ollama.com/api/embed — API local de embeddings e limites de entrada.

## Decisões propostas

Pipeline Rust local com backend Ollama opcional configurável, SQLite e GraphRAG
local/global. Não depender do pacote Python Microsoft GraphRAG para o núcleo:
isso adicionaria runtime e pipeline separados ao produto Rust. O nome GraphRAG
designa a abordagem implementada; não anunciar compatibilidade Microsoft sem
adaptador e testes. Validar extração, agrupamento, resumos e respostas reais,
sem substituir GraphRAG por BFS com uma resposta de LLM.

## Alteração autorizada durante implementação

Usuário rejeitou Ollama e solicitou roteamento entre Codex e Claude. Backend
HTTP foi removido, não mantido como requisito oculto. CLIs presentes em
`~/.local/bin`; flags verificadas por help/features locais. Fontes oficiais:
https://learn.chatgpt.com/docs/non-interactive-mode e
https://learn.chatgpt.com/docs/config-file/config-reference .
`--ignore-user-config` preserva autenticação sem carregar MCPs do usuário.
Features locais + expansão semântica substituem a dependência de embeddings
externos; essa alteração técnica é explícita e será avaliada, sem alegação
de equivalência a embeddings neurais.
