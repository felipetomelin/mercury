# Isolamento do indicador de contexto por chat

Status: correção de contexto aprovada pelo usuário em 2026-11-09.
Ampliação de títulos aprovada para execução pelo usuário em 2026-11-09.
Data: 2026-11-09

## Problema

O usuário relata que trabalhar em outro chat aumenta o indicador do chat
“CI/CD”, que está ocioso. O contexto deve representar a sessão daquele chat.

No histórico, a seleção de conversas também apresenta IDs em vez de títulos,
dificultando reconhecer a conversa. Incluir essa correção no planejamento.

## Solução

Garantir a correspondência entre chat, terminal, provedor e sessão em toda
atualização de contexto. Descartar resultados de sessões anteriores e investigar
a atribuição inicial da sessão quando há chats simultâneos no mesmo projeto.
Uma sessão em execução em segundo plano pode atualizar seu próprio indicador.
Cotas globais da conta continuam sendo globais.

## Critérios de aceite

1. Dados dois chats com sessões distintas no mesmo diretório, quando apenas B
   recebe novos dados de contexto, o indicador de A permanece inalterado.
2. Ao trocar a sessão de um terminal, uma atualização atrasada da sessão anterior
   não modifica nem remove os dados da sessão atual.
3. Eventos de hooks/statusline são atribuídos somente ao chat correspondente.
4. O isolamento funciona tanto para Claude quanto para Codex.
5. Trocar o foco entre chats não muda a atribuição das medições.
6. No histórico, priorizar o título da conversa; sem título disponível, usar
   uma prévia legível da primeira mensagem e, em último caso, “Conversa sem título”.
   Manter provedor/data/projeto para distinguir conversas e usar o ID internamente
   para reabrir a sessão correta.

## Esclarecimentos

Sem dúvidas de produto impeditivas. O provedor do caso real não foi informado;
a investigação contempla ambos. Não presumir que chat sem foco está ocioso.

## Trade-offs

Priorizar a identidade da sessão; descartar medições sem vínculo confiável.
Não alterar a fórmula do contexto nem o comportamento das cotas globais.

## Tarefas

1. Rastrear atribuição de sessões e projeção do indicador até a UI.
2. Corrigir validação e invalidação de resultados/cache conforme o diagnóstico.
3. Fazer revisão consolidada e testes de regressão de isolamento e troca de sessão.
4. Executar verificações locais aplicáveis e registrar limitações de smoke real.
5. Planejar coleta, persistência e apresentação de títulos no histórico, incluindo
   compatibilidade com índice existente e fallback para conversas antigas.

## Dependências

Estado persistido dos terminais, hooks dos agentes, leitores de transcripts,
projeção de uso no protocolo e sidebar. Nenhuma dependência nova prevista.
