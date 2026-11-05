# Ativação da memória local — 2026-10-29

Solicitação explícita do usuário: “Vamos ativar a memoria local também”.
Operação sobre a feature008 já aprovada e implementada, sem alterar produção.

- Instância debug ativa, protocolo22, versão0.9.3.
- Config: `~/.config/mercury-dev/config.toml`.
- Backup privado: `config.toml.backup-local-memory-20261029-135718`, no mesmo
  diretório. Gravação atômica, validada antes e após a substituição.
- `[knowledge]`: enabled=true, vault configurado como solicitado,
  allow_external=false, allowed_providers=[], allow_route_external=false,
  route_destinations=[]. Nenhuma outra seção modificada. Learn não foi habilitado.
- `mercury config check`: config ok.
- `mercury server reload-config`: applied, diagnostics=[]. Sem encerrar sessões.
- `mercury knowledge status --json`: enabled=true, schema_version=3.
- Index job `3605d65647305aefd2029fcc276f1e573f2af927980ea1d55d9b4f08e9130dc8`:
  completed, error=null, diagnostics=[].
- Índice: 0 notas, 0 chunks, 0 relações; vault já estava vazio antes da operação.
- Busca textual por mercury: hits=[], revisão igual à do índice concluído.
- Nenhuma nota criada/alterada; nenhuma chamada LLM ou envio Route realizado.
  O arquivo temporário de configuração usado na validação foi removido.

GraphRAG/modelos continua dependendo de concessão explícita de saída na config,
conforme política da spec008. Ativar índice local não transforma inferências
em decisões aprovadas, nem habilita edição automática de regras Learn.
