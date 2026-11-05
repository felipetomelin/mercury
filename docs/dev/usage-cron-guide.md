# Usage, Crons e memória local

## Contas conectadas

A aba **usage** consulta os perfis atuais dos CLIs Codex e Claude, mesmo sem um
agente aberto no Mercury. Cada conta mostra plano, limites informados, barra de
uso, renovação e idade da leitura. Na conta Codex Business, `23% restante`
corresponde a `77% utilizado`; a barra representa a parte utilizada.

O Claude Enterprise pode informar o plano e nenhuma janela numérica. Nesse caso
a tela explica a ausência de limites, sem calcular uma porcentagem por tokens.
Falha temporária preserva a última leitura como **desatualizada**. Sem login,
CLI ausente e versão incompatível têm diagnósticos próprios. A identidade não é
exibida como email nem copiada para os logs.

`r` pede atualização; há sondagem a cada minuto enquanto a aba está visível.
Sondagens normais em segundo plano usam cinco minutos; erro aplica recuo até
30 minutos. Atualização não envia prompt nem usa modelo. O CLI mantém sua própria
autenticação; Mercury não lê tokens, cookies ou Keychain. Claude é sondado após
abrir Usage/pedir atualização, e continua no intervalo de fundo quando aplicável.

O bloco **Histórico local · Hoje (UTC)** é separado das cotas: mostra tokens,
cache da entrada, custo efetivamente informado e atividade dos logs disponíveis.
O histórico pode incluir logins anteriores do mesmo perfil e tem cobertura
parcial; não é fatura nem estimativa da porcentagem restante da assinatura.
Os sete pontos da tendência representam dias UTC, do mais antigo até hoje.

Use **Page Up/Down, Home e End** ou a roda do mouse para rolar. Setas selecionam
agentes; Enter abre o painel selecionado. A aba começa no topo das contas.

## Agendamentos

Na aba **crons**, use os botões ou `a` para criar, `e` para editar, `d` para
excluir, `p` para pausar/retomar, `n` para executar agora e `r` para atualizar.
Enter abre detalhes e histórico; Enter numa execução abre seu log.

O formulário oferece agendas de dias úteis, diária, semanal com dias marcados,
mensal, intervalo, duração até uma única execução, data/hora única e expressão
avançada. Use setas para escolher ou clique nas opções. O servidor informa seu
offset UTC; é possível editá-lo. A próxima data fica visível no topo; a revisão
mostra até três. Offset é fixo e não acompanha horário de verão. Meses que não
contêm o dia mensal escolhido são pulados.

Escolha Claude/Codex para uma execução em segundo plano, painel existente para
enviar uma instrução ao agente aberto ou programa local para executar um arquivo.
O programa usa caminho absoluto e um argumento por linha, sem shell implícito.
Instruções e argumentos aceitam várias linhas: **Alt+Enter** insere quebra e
colagem preserva as linhas. **Tab/Shift+Tab** ou setas verticais mudam o campo.

**Ctrl+S** ou o botão **Revisar** abre o resumo; **Ctrl+S/Salvar** confirma após
uma prévia válida. **Esc** volta ao formulário ou cancela. Uma falha mantém o
rascunho. Editar conserva ID, histórico, entregas e opções avançadas do agente.
Salvar não executa imediatamente; **Executar agora** é outra ação.

Excluir pede confirmação com nome/ID. Cancela agendamentos futuros, preserva
logs/histórico e não interrompe uma execução já iniciada. Cancelar a confirmação
mantém o job. No CLI, a mesma operação é `mercury cron rm JOB_ID`.

## Memória

A memória local foi ativada no config debug com o vault solicitado e sem envio
externo. O vault estava vazio: o índice inicia com zero notas. Crie notas Markdown
no Obsidian e execute `mercury knowledge index --json` para atualizar a busca.
Veja [Knowledge e GraphRAG](../knowledge/README.md) para captura revisável e modelos.
Knowledge não habilita edição automática de regras Learn.
