# Análise de consistência

Pesquisa concluída e esclarecimento de área respondido pelo usuário. O plano
atende aos critérios de ocultação por origem, preservação dos dados, somente
agentes principais e identidade visual por path em ambas as áreas confirmadas.

Riscos explicitamente tratados: falso positivo por título; índice antigo;
filtro aplicado após LIMIT; path dinâmico mudando cores; conflito com status;
incompatibilidade do protocolo binário. Testes previstos cobrem esses riscos.

A origem foi comprovada nos metadados locais. A finalidade mais específica do
Guardian não foi inferida de conteúdo privado; esta entrega apenas diferencia
revisões internas e conversas comuns. O usuário aprovou executar e ampliou a classificação para todos os subagentes.
O plano e os critérios foram atualizados antes da implementação.

Gates de especificação, esclarecimentos, plano, análise e aprovação concluídos.
Sem perguntas impeditivas. A opção de incluir internos foi removida por instrução
explícita do usuário: apenas sessões de agentes principais.

Implementação: campo is_subagent no índice com migração transacional e backfill;
fontes Codex de subagente e sidechains Claude excluídas antes do LIMIT e da
resolução para reabertura. Não foi adicionado filtro de inclusão. A leitura Codex
termina no metadado quando a sessão é de subagente, evitando indexar suas mensagens.

Identidade visual: project_path opcional no snapshot de workspaces, baseado em
identity_cwd; protocolo binário privado 22 -> 23. Endpoint JSON continua geração
1, com default None para servidores antigos. Marcadores usam hash estável de
endpoint + path lexical e não substituem cores de estado ou de seleção.
Não houve despacho de agentes paralelos nem instalação nesta fase.

Revisão consolidada e gates locais concluídos, com evidências em validation.md.
Build, Clippy, formatação e manutenção passaram. Testes cobrem o índice antigo,
todos os papéis de subagentes, falso positivo por título, LIMIT, reabertura,
compatibilidade endpoint geração 1 e cores nos três locais em ambos os temas.
A suíte de handoff teve um timeout de startup; o reteste focal passou.
Instalação/ativação e smoke humano permanecem fora desta etapa.
