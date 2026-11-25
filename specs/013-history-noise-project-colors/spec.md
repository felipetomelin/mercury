# Histórico de agentes principais e cores por diretório

Status: aprovado para execução pelo usuário em 2026-11-18, com ampliação
para excluir todas as sessões de subagentes e exibir apenas agentes principais.
Data: 2026-11-18

## Problema

O histórico inclui centenas de sessões “Guardian review”, dificultando localizar
as conversas do usuário. Os metadados locais do Codex identificam essas sessões
como subagente `guardian`, sem evento de usuário; o Mercury não as diferencia.
Além disso, falta uma referência visual consistente para os chats do mesmo path.

## Solução

1. Exibir somente sessões de agentes principais, classificando os subagentes
   pelos metadados de origem do Codex e de sidechain do Claude. Preservar os
   transcripts e os dados originais. Não oferecer opção para incluir subagentes.
2. Usar um marcador colorido consistente por diretório no histórico e na lista
   lateral de chats/projetos, conforme escolha explícita do usuário nesta conversa.
3. Manter título, path e indicadores de estado legíveis. A cor complementa esses
   dados; as cores de erro, execução, seleção e uso de contexto mantêm sua função.

## Critérios de aceite

1. Todas as sessões Codex com origem de subagente, Guardian ou qualquer outro
   papel, e sessões Claude de sidechain ficam fora da listagem e da busca,
   mesmo com título alterado.
2. Uma conversa comum chamada “Guardian review” permanece visível. Título, idade,
   repetição do nome ou ausência de mensagem de usuário isoladamente não ocultam
   uma conversa. Metadados desconhecidos permanecem visíveis.
3. A listagem oferece apenas agentes principais, sem opção de mostrar subagentes.
   Nenhum transcript nem registro original dos provedores é apagado.
4. A classificação vale para registros já indexados e novos, sem exigir que os
   arquivos antigos sejam modificados. O filtro ocorre antes do limite/paginação.
5. Conversas comuns distintas com o mesmo título não são deduplicadas por nome.
6. O mesmo diretório, no mesmo endpoint, recebe o mesmo marcador no histórico,
   nos projetos da lateral e nos chats associados, inclusive após reinício.
7. A cor independe da ordem de abertura, do foco e do título. Mudar o título ou
   executar `cd` dentro de uma conversa não muda sua identidade de projeto.
8. Path ausente tem marcador neutro. O path legível continua disponível e os temas
   claros/escuros e o destaque de seleção permanecem utilizáveis.
9. Toda leitura de arquivo e classificação persistente ocorre no servidor em
   background; a renderização usa apenas dados já presentes no snapshot.

## Esclarecimentos

- Área confirmada: **histórico e lista lateral de chats/projetos**.
- A solicitação de cores é interpretada como identidade visual por diretório,
  sem exigir cópia exata da paleta do Codex.
- Escopo ampliado explicitamente: todos os subagentes. Conversas de origem
  desconhecida/legada permanecem visíveis quando não há metadado que comprove
  origem de subagente; não inferir pela ausência de mensagens ou pelo título.
- A identidade é o diretório de origem/projeto da conversa, não o diretório
  temporário do processo em primeiro plano. Endpoints distintos têm identidades
  separadas mesmo que usem a mesma string de path.

## Trade-offs

- Paleta finita: diretórios diferentes podem compartilhar uma cor. O path e o
  título continuam sendo a identificação definitiva.
- Normalização lexical evita I/O na UI; aliases por symlink podem manter cores
  distintas. Não haverá seleção manual de cor nesta entrega.
- Sessões de subagentes ficam fora do histórico Mercury, preservadas no provedor.
  A execução automática dos subagentes não será desativada.

## Tarefas

1. Persistir classificação de origem e migrar os registros existentes.
2. Aplicar o filtro obrigatório na busca/listagem antes de LIMIT.
3. Propagar a identidade estável de projeto até a UI com compatibilidade explícita.
4. Compartilhar cálculo de marcador por path entre histórico e lateral.
5. Revisão consolidada, regressões, verificação visual em buffer, manutenção,
   build, Clippy, formatação e registro de limitações.

## Dependências

Índice privado mercury-history, session_meta Codex, API de histórico, snapshot
client-shell, identidade dos workspaces e paletas existentes. Sem nova
dependência externa prevista. A instalação da entrega será tratada separadamente.
