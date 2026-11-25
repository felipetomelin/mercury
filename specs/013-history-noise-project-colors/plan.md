# Plano técnico

1. Classificar todos os subagentes ao ler session_meta Codex pelo source
   estruturado e sidechain Claude, preservando registros principais mistos. Manter
   visibilidade em caso desconhecido. Persistir indicador no índice, com migração
   transacional e backfill de registros antigos sem alteração de mtime.
2. Excluir subagentes em ambas as consultas SQL antes de LIMIT, sem opção de
   inclusão na API/CLI/UI. Reindexar registros antigos na migração. Não filtrar
   por título nem deduplicar conversas distintas com nomes iguais.
3. Definir identidade de path estável: diretório de origem do transcript para
   histórico e identity_cwd do workspace para projetos/chats ativos. Expor este
   último como dado tipado no snapshot, sem reutilizar cwd de criação dinâmico.
   Atualizar protocolo/fixtures se a serialização binária exigir, preservando a
   negociação de incompatibilidade e a atualização conjunta de servidor/cliente.
4. Criar helper puro de normalização lexical e hash determinístico, selecionando
   cor de identidade na paleta do tema. Compor com endpoint quando aplicável;
   evitar RandomState, título ou índice de linha como entrada.
5. Renderizar marcador colorido comum no histórico e nas linhas de projetos e
   chats da lateral; manter cores semânticas e destaque de seleção existentes.
6. Executar uma revisão consolidada e um ciclo agrupado de testes: migração,
   classificação por metadados, mesmo título em conversa comum, filtro antes do
   limite, cores determinísticas, paridade histórico/lateral, path ausente,
   buffers em temas claros/escuros e regressões de protocolo/reabertura.

Verificações locais: testes afetados e de contrato, suíte principal quando o wire
for alterado, integração do histórico, manutenção, fmt, Clippy e build.
Gates externos e cobertura quantitativa dispensados conforme constituição.
Não haverá exclusão de conversas, alteração de configuração do Codex ou deploy
como efeito da pesquisa/especificação.
