# 009 — Usage por conta, Crons guiados e consistência visual

**Status:** aprovada, implementada e validada localmente em 2026-10-29.
**Data:** 2026-10-29. **Origem:** sete ajustes solicitados pelo usuário nesta conversa.

## Problema

Usage exibe `n/a` sem um diagnóstico acionável e mistura dados de sessões com
limites da conta. O usuário quer acompanhar os logins atuais Claude/Codex, como
no Vorssaint. Criar cron exige expressão manual, tipos técnicos e passos que
mudam conforme o alvo. Exclusão existe no CLI, mas não aparece na TUI. Cores,
ações e estados precisam de uma linguagem visual consistente.

## Solução

### U — Usage das contas conectadas

- Mostrar um cartão por perfil autenticado configurado, inicialmente os perfis
  atuais dos CLIs Codex e Claude, mesmo sem painel/sessão Mercury aberto.
- Exibir provedor, rótulo do perfil, plano quando disponível, barras de percentual
  **utilizado**, janela, horário/contagem para renovação e atualização da fonte.
- Tratar também `individualLimit.remainingPercent` e `resetsAt` do Codex:
  sondagem real encontrou Business com 23% restantes, primary/secondary nulos.
  Rotular restante explicitamente; se convertido em utilizado, usar complemento
  e indicar o sentido da barra. Não interpretar limit/used como moeda.
- Consultar metadados pelos CLIs autenticados, sem prompts/modelos, ferramentas,
  hooks do usuário, login automático ou cópia/leitura de tokens pelo Mercury.
- Codex: reutilizar app-server; separar leitura de identidade/autenticação,
  limites e consumo de sessões. Claude: adicionar consulta estruturada de uso
  pelo CLI, negociada conforme a versão; statusline segue como fonte de sessão.
- Estado explícito: consultando, atualizado, desatualizado, sem login, CLI ausente,
  versão incompatível, consulta falhou ou limites não informados pelo plano.
- Preservar último resultado em falha temporária, marcado como desatualizado;
  nunca reutilizar os limites de uma conta após mudança de identidade/perfil.
- Atualização manual e automática com limite de frequência, timeout e backoff;
  padrão 60s enquanto Usage estiver visível, 5min em segundo plano se necessário
  ao roteamento; falhas recuam até 30min. Fonte mais velha mantém seu timestamp.
- Consumo local, cache e valor equivalente de API aparecem em bloco separado,
  com período e cobertura; valor estimado não se apresenta como fatura do plano.
- Se o provedor não expuser uma janela, não calcular porcentagem por tokens,
  gasto, quantidade de sessões ou limites configurados manualmente.

### C — Criação, edição e exclusão de crons

- Substituir sequência de caixas Rename por formulário guiado, com seleção via
  setas/Enter e mouse, foco visível, ajuda contextual, Voltar/Cancelar e rascunho.
- Agenda: uma vez em N minutos/horas, data/hora específica, a cada N minutos/horas,
  diariamente, dias úteis, semanalmente com seleção de dias e mensalmente por dia.
  Expressão cron permanece em opção Avançado. Validar valores antes de avançar.
- Mostrar frase em português, fuso explícito e até três próximas execuções.
  Para ocorrência única, mostrar somente uma. Avisar agenda sem próxima data.
- Usar o offset local do servidor como sugestão, editável, nunca UTC implícito.
  O motor atual usa offset fixo: informar isso, sem prometer regras de horário
  de verão ou nomes IANA que o backend não suporta.
- Tipo de execução: “Claude — nova execução em segundo plano”, “Codex — nova
  execução em segundo plano”, “Painel existente — envia a instrução ao painel”
  e “Programa local — executa um arquivo”. Mostrar somente campos pertinentes.
- Escolher painel por lista com identificação legível e ID; validar existência
  novamente no envio. Programa local exige caminho e permite argumentos
  separados, sem criar shell implícito. Diretório tem seletor entre workspace
  atual e caminho absoluto; prompt permite múltiplas linhas.
- Resumo final mostra nome, agenda/fuso/próximas datas, destino, instrução ou
  programa/argumentos, diretório e estado inicial antes de “Salvar”. Exibir
  confirmação somente após resposta do servidor e nova listagem coerente.
- Editar reaproveita o formulário preenchido e o mesmo ID; persistir via cron.edit.
- Excluir aparece no detalhe e na lista, por botão e atalho com confirmação
  nome/ID. Cancelar não altera nada. Usar cron.rm; remover agenda futura,
  preservar histórico/logs existentes e informar que execução já iniciada
  continua, conforme contrato atual. Não disparar execução ao salvar/excluir.
- Manter pausa/retomada, executar agora, histórico e logs acessíveis. Executar
  agora é ação distinta de salvar e explicitamente identificada.

### V — Revisão visual transversal

- Auditar barra de abas, sidebar, overlays, Usage, Crons, Route, History, Learn,
  Knowledge e configurações. Registrar achados e aplicar correções coerentes
  de hierarquia, espaçamento, legenda, foco, mensagens vazias/erros e atalhos.
- Usar palette existente: accent para foco/ações, green para sucesso/ativo,
  yellow para aviso/execução, red para erro/destruição, subtext/overlay para
  informações secundárias. Barras usam gauge_warn/gauge_crit existentes.
- Cor sempre acompanha texto/ícone; respeitar temas claro/escuro e ausência de
  cores, sem introduzir novo framework ou recolorir saída dos agentes.
- Layout utilizável a partir de 80x24, detalhes roláveis e conteúdo integral
  acessível por seleção; nada importante depende só de hover ou cor.

### M — Memória local (operação já autorizada na spec 008)

Ativar `[knowledge]` no config ativo com backup e recarga, vault informado pelo
usuário, índice privado e allow_external=false. Não habilitar Learn automático
nem envio de contexto Route. Confirmar índice e busca local sem modelo.
GraphRAG com Codex/Claude continua disponível mediante configuração externa
explícita conforme spec 008; ativar armazenamento não concede essa permissão.

## Critérios de aceite

1. Sem panes de agente, contas autenticadas continuam aparecendo; fixture com
   limites reportados mostra percentual, plano, renovação e atualização corretos.
   Inclui Business com primary/secondary nulos e individualLimit: não mostrar
   n/a nem interpretar 23% restantes como 23% utilizados.
2. CLI ausente, logout, timeout, resposta incompleta e plano sem limites têm
   estados diferentes. Falha transitória mantém amostra antiga marcada; trocar
   conta invalida cache. Nenhum segredo entra no snapshot/log/argv.
3. Atualizar várias vezes não cria sondagens simultâneas. Sondagens encerram
   seus filhos, respeitam limites de saída/tempo e não enviam prompt/modelo.
4. Usuário agenda dias úteis às 09:00 no offset -03:00 sem digitar cron;
   prévia e definição persistida concordam, inclusive na virada de dia UTC.
5. Todos os presets geram agenda válida; erros de data, dia e intervalo são
   explicados no campo. Voltar preserva rascunho e Cancelar não cria job.
6. Os quatro destinos têm descrição, campos pertinentes e resumo verificável;
   o contador reflete passos reais, sem lacunas como pular de 3/6 para 5/6.
7. Criar/editar persiste após reinício em fixture. Respostas antigas de listagem
   não substituem resultados novos. Erro de servidor mantém rascunho recuperável.
8. Excluir exige confirmação; cancelar conserva job; confirmar impede futuras
   execuções e preserva histórico. Ações falhas mostram erro sem sucesso falso.
9. Navegação por teclado e mouse funciona em 80x24 e janela maior; temas claro,
   escuro e monocromático preservam entendimento/foco, sem sobreposição.
10. Memória real: status enabled, config sem diagnóstico, índice e busca local
    funcionais, vault sem alteração de notas e sem chamada a provedores.

## Trade-offs e fora de escopo

Identidade e cotas são o que a fonte efetivamente fornece; não se garante
percentual numérico para plano que não o publica. Não haverá scraping de cookies,
leitura de Keychain/tokens, compra de créditos, novo login, captura global de
contas, sincronização cloud, reescrita do motor cron nem migração IANA neste ciclo.
Perfis adicionais só por configuração explícita, sem varrer credenciais do disco.
Aparência segue os componentes de terminal, inspirada na organização do print.

## Tarefas e dependências

Grupos: U coleta/contas/Usage; C agenda/formulário/CRUD; V auditoria e ajustes;
M ativação local; R revisão/testes/build/Clippy/manutenção consolidados.
Depende das versões e respostas reais dos CLIs; adapters não suportados devem
falhar de forma explicada. Gates externos permanecem dispensados por decisão
do usuário; revisão e verificações locais continuam obrigatórias.
