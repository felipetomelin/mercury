# Auditoria visual 009

Escopo: interface de terminal Mercury, mantendo a palette dos temas existentes.
Data: 2026-10-29. Cores nunca substituem o rótulo textual do estado/ação.

| Área | Achado | Resultado |
|---|---|---|
| Usage | n/a ocultava causa e formato Business não suportado | Cards com plano, fonte/idade, diagnóstico, barra e restante/utilizado explícitos |
| Usage | Abertura podia saltar às sessões e esconder contas | Topo nas contas; rolagem completa por mouse/PgUp/PgDn/Home/End |
| Usage | Custos/tokens locais não significam quota da conta | Bloco local separado, janela UTC e cobertura parcial explícitas |
| Crons | Campos Rename, opções técnicas, etapas puladas | Formulário com seletores, ajuda, campos condicionais e revisão |
| Crons | Sem agenda guiada nem prévia | Presets e dias marcados; fuso/primeira data visíveis; servidor calcula ocorrências |
| Crons | Excluir ausente na TUI e botões longos cortavam em 80 colunas | Botões em duas linhas; Criar/Editar/Excluir sempre prioritários; confirmação |
| Crons | Rascunho e ação final pouco visíveis | Caret/foco, mensagens persistentes e botões Revisar/Salvar/Voltar/Cancelar no rodapé |
| Crons/Learn | Status e sucesso/erro tinham pouca distinção | Palette semântica compartilhada; mantém palavras success/failed/applied/conflict etc. |
| History | Cabeçalho e campo ativo pouco distintos | Accent no título/busca, amarelo em carregamento, foco e texto preservados |
| Knowledge | Status era JSON bruto e vazio pouco explicado | Contagens legíveis, instrução para vault vazio, seleção e erro com destaque |
| Sidebar/abas | Já utilizam estados, foco, seleção e palette por agente | Reutilizados; sem recolorir saída dos agentes |
| Overlays/menus/configurações | Já possuem foco, navegação, opções e confirmação próprias | Mantidos; formulário Cron deixa de sobrecarregar Rename |
| Route | Não há uma página Route independente | Preview e tarefas continuam no CLI/painel existente; sem nova tela especulativa |

Sem cores fixas de marca: accent=foco/ações; green=sucesso; yellow=aviso/pendente;
red=erro/destrutivo; subtext=informação auxiliar; barras usam gauge_warn/gauge_crit.

Validação de renderização inclui formulário em 80x24 e 120x40, palettes clara,
escura e cores Reset. Evidências de texto opcionais saem pelo teste com
`MERCURY_009_RENDER_DIR`; seleção e conteúdo são verificáveis sem cor.
Testes de interface verificam Excluir visível em 80 colunas, colagem multilinha,
cancelamento, confirmação, erro persistente e proteção contra resposta antiga.
O fluxo PTY real usa agentes/programas de fixture; não substitui a preferência
visual do usuário nem qualifica terminais Windows não executados nesta máquina.
