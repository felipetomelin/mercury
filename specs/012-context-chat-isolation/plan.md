# Plano técnico

1. Rastrear o vínculo de hooks/sessões até o terminal e a projeção da sidebar.
2. Exigir identidade da sessão para ambos os provedores na aplicação dos
   resultados; preservar identidade do alvo também para respostas vazias se
   necessário. Invalidar caches ao trocar a sessão conforme padrões existentes.
3. Adicionar regressões com dois chats/sessões e resposta atrasada após troca.
4. Revisar o grupo de mudanças, rodar testes relevantes, fmt, build, Clippy e
   verificações de manutenção aplicáveis.

Gates externos e cobertura quantitativa dispensados pela constituição do projeto;
registrar como não executados, jamais como aprovados.

## Histórico: título da conversa (ampliação solicitada)

- `crates/mercury-history/src/parse.rs` hoje extrai sessão, diretório, data e
  mensagens, sem título. `HistoryIndex` persiste esses campos no SQLite.
- `SearchHit` e `HistoryHitInfo` precisam transportar um título opcional.
- Priorizar o título atribuído pelo usuário no Mercury, vinculando-o ao provedor
  e ID da sessão; depois usar o título explícito do provedor quando disponível.
- Coletar título explícito dos metadados disponíveis de Claude/Codex e investigar
  os arquivos auxiliares que os provedores usam para renomear conversas.
- Migrar o índice existente sem perder histórico e atualizar registros antigos.
- Fallback: prévia da primeira mensagem; sem conteúdo, “Conversa sem título”.
- `src/client/shell/history_view.rs` deve priorizar o título na linha, mantendo
  provedor/data/projeto e reabertura por ID. Não usar trecho de busca como título.
- Testar título explícito, ausência de título, Unicode, títulos repetidos,
  reabertura da sessão correta e migração do índice. Atualizar schema da API.
- Execução aprovada pelo usuário em 2026-11-09; implementação e validação
  registradas em `validation.md`.
- Persistir nomes manuais no mesmo worker do checkpoint de sessão e capturar
  nomes ativos nas consultas ao histórico, preservando-os após fechar o painel.
- Limitar e normalizar títulos; ler metadados em background, com limites de
  tamanho e sem seguir links de arquivos de índice dos provedores.
