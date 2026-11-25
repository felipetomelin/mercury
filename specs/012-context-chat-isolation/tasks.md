# Tarefas

## Contexto (aprovado)

- [x] Rastrear o uso de sessão no polling, na statusline e na projeção da sidebar.
- [x] Identificar atualizações de background por provedor e sessão, mesmo em falhas.
- [x] Recusar atualizações de sessão diferente para Claude e Codex.
- [x] Impedir projeção de cache antigo e invalidá-lo na próxima coleta.
- [x] Acrescentar regressões de dois chats e de troca de sessão com resposta atrasada.
- [x] Concluir validação Rust e revisão consolidada (evidências em `validation.md`).
- [ ] Validar o comportamento nas conversas reais após ativação da versão corrigida.

## Títulos do histórico (execução aprovada pelo usuário)

- [x] Investigar títulos do Mercury e dos provedores e definir a fonte de verdade.
- [x] Persistir títulos e migrar o índice existente sem perder conversas.
- [x] Propagar título opcional pela API e atualizar schema.
- [x] Exibir título prioritariamente, com fallback legível; reabrir por ID.
- [x] Testar migração, conversas antigas, títulos repetidos e reabertura correta.
