# Descoberta

src/config/learn.rs exige targets quando enabled; src/app/learn.rs resolve Claude,
revisa marcos/fim/semana, registra sessão concluída e aplica Changes reversíveis.
src/app/api/learn.rs valida sessão Claude corrente e exclui automação;
src/app/api.rs valida saídas Codex e probes em idle. Esses produtores serão
reutilizados, mantendo transcrição estável/identidade e exclusão de origens.

Knowledge Config é default-off e vault vazio; ingest scan preserva revisão de
fontes. src/server/headless/knowledge.rs serializa modelo/API em SERIAL, captura
apenas sessão concluída e usa roteamento NativeModel. Index/SQLite persistem
propostas/jobs/cache. capture.review é writer journalizado e conflita com edição.

Extensões: config de automação; vault/memória gerenciados; journal de capturas e
status; worker do servidor compartilhando SERIAL; aceitar publicação proposed
sem falsificar decisão validada; status JSON já genérico e TUI Knowledge.
Riscos: snapshots de config obsoletos, handoff com worker antigo ativo, callback
perdido antes de journal, sessão ainda aberta, custo por reindexação repetida,
notas humanas alteradas durante retry. Mitigar com cancelamento, hashes/estados
persistidos, debounce, teto diário conservador e limites existentes.
Não há skill Rust instalada; seguir arquitetura/padrões Rust deste repositório.
