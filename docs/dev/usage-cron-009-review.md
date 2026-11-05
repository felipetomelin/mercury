# Revisão e validação consolidada — entrega 009

Data: 2026-10-29. **Veredito local: PASS.** Spec009 aprovada explicitamente pelo
usuário antes da implementação. Sem commit, merge ou publicação.

## Resultado

- Usage consulta os CLIs autenticados, acompanha identidade internamente e mostra
  plano, percentual utilizado/restante, renovação, fonte/idade e erros distintos.
  `individualLimit.remainingPercent` do Codex Business agora é suportado, sem
  tratar valores de unidade não declarada como USD. Claude usa apenas os comandos
  de metadados `auth status` e control requests initialize/get_usage.
- Cache antigo fica sinalizado; logout/mudança de identidade invalida limites.
  Dados de sessões não substituem leitura autenticada da conta. Timeout é falha
  transitória, sem ser confundido com logout. Cotas vencidas/antigas ficam stale.
- Leitura e processos são limitados, fora do render; stdout tem teto e canais
  limitados. Filhos são encerrados/reaped e, em Unix, o grupo é finalizado.
  IPC Mercury herdado é removido dos subprocessos; não há prompt ou chamada LLM.
- Formulário Cron próprio, presets, dias selecionáveis, diretório, argumentos por
  linha/instruções multilinha, prévia pelo motor do servidor, revisão e edição.
  Preserva parâmetros avançados existentes. Painel é revalidado ao salvar.
- Exclusão confirmada com nome/ID, preservação de histórico/log, sem interromper
  processo já iniciado. Botão Excluir permanece visível em 80 colunas.
- Contratos aditivos cron.preview e usage.refresh; cron.rm já existente passou a
  ser permitido na interface. Schema e hashes dos métodos novos foram congelados;
  digests antigos preservados. Campos privados novos têm defaults de leitura.
- Ajustes visuais e limites de inspeção em [auditoria visual](visual-audit-009.md).
  Operação em [guia de Usage e Crons](usage-cron-guide.md).

## Achados resolvidos na revisão

1. Business sem primary/secondary aparecia n/a: corrigido pelo campo individual.
2. Cota restante podia ser lida como utilizada: direção explícita, complemento
   apenas a partir do percentual informado, sem inferência de moeda.
3. Resposta antiga de agenda podia atingir outro rascunho: geração por formulário
   e chave de agenda/offset; respostas obsoletas são descartadas. Fuso inicial
   ainda pode ser usado ao trocar preset, mas suas datas antigas não são reusadas.
4. Editar podia perder metadados não expostos: definição original preserva
   modelos, skills, effort, prompt files, entregas, repeat e catch-up.
5. Usage podia abrir nas sessões e esconder contas: abre no topo; rolagem completa
   por teclado e mouse, com navegação de agente separada.
6. Botões não cabiam na largura mínima: duas linhas de ações em Crons e ações
   permanentes no rodapé do formulário; sem texto antigo sobreposto.
7. Sondagem poderia herdar conexão de pane: variáveis Mercury removidas, hooks
   do usuário/MCP/tools desativados no protocolo Claude e nenhum user message.
8. Fixture PTY precisava se adaptar à tela rolável e isolar históricos: homes
   temporários de providers e verificações separadas de conta/sessão/orçamento.

## Evidências de testes e checks

| Check | Resultado | Log local |
|---|---|---|
| Workspace completo, sem filtros, serial | 4.131 PASS, 0 FAIL, 13 ignorados; exit 0 | /private/tmp/mercury-009-workspace-final.log |
| Cron final, após ampliar assert de histórico retido | 50/50 PASS | /private/tmp/mercury-009-cron-final.log |
| Usage PTY focado após correção de rolagem | 1/1 PASS | /private/tmp/mercury-009-usage-e2e.log |
| Cron PTY final, prévia sem pressupor timezone | 1/1 PASS | /private/tmp/mercury-009-cron-pty-final.log |
| Shell focado intermediário | 387 PASS, 2 ignorados | /private/tmp/mercury-009-shell-tests.log |
| Manutenção + arquitetura hot path | 80/80 PASS | /private/tmp/mercury-009-maintenance.log |
| Clippy workspace/all-targets -D warnings | PASS | /private/tmp/mercury-009-clippy-final.log |
| Build debug Mercury | PASS | /private/tmp/mercury-009-build-final.log |
| cargo fmt --all -- --check | PASS | /private/tmp/mercury-009-fmt-final.log |
| git diff --check | PASS | /private/tmp/mercury-009-diff-final.log |

Asserções finais de retenção após excluir e de prévia independente de timezone
foram verificadas no escopo afetado após a suíte completa; código de produção não
foi alterado nessa extensão. Schema/capacidades fazem parte da suíte completa.
Testes incluem provider simulado com JSON de identidade em várias linhas, controle
sem prompt, Enterprise sem janela, Business restante23→utilizado77, números
inválidos, backoff, logout/identidade, calendário/offset e render em três palettes.
As primeiras execuções revelaram restrições de socket no sandbox e as falhas
reais de navegação/expectativa descritas acima; foram corrigidas e reexecutadas.

## Instância real

Após os checks, foi realizado live handoff para o binário debug validado, com
protocolo22 e versão0.9.3 esperados. Antes: 1 painel, 1 workspace, 1 cron, nenhuma
execução ativa ou próxima agenda habilitada. Após: mesmos IDs e PIDs de shell,
mesmos IDs de cron; nova API cron.preview retornou três ocorrências, usage.refresh
foi aceita, config check passou e Knowledge permaneceu enabled com o mesmo índice.

Um cliente temporário abriu a aba Usage real e confirmou Business/Enterprise e
**22% restante** do Codex na leitura atual. A consulta inicial de pesquisa havia
informado23%; não é valor fixado no código. Enterprise foi identificado sem
inventar cota quando get_usage não fornece janela. Nenhum prompt/modelo foi usado
por essas sondagens, nem cron real foi executado manualmente.

O script do cliente temporário confirmou os indicadores, mas expirou ao aguardar
o encerramento sem continuar drenando o PTY. O fechamento do PTY encerrou o
cliente; verificação posterior encontrou zero clientes temporários órfãos e o
servidor continuou ativo. Isso é registrado separadamente dos smokes automatizados
de saída/PTY que passaram; não se transforma a saída1 do script em PASS integral.

Para carregar o código novo da interface, reabrir o cliente com o binário local.
O handoff atualiza o servidor e preserva panes; não reescreve um cliente antigo
que já estava em execução. O vault permanece vazio; ativação local foi verificada
em [registro da memória](local-memory-activation-20261029.md).

## Limites e política

Validação executada em macOS/Unix. A qualificação real de Windows/ACLs, terminal
Windows e regras IANA/horário de verão não faz parte deste aceite. Perfil histórico
não identifica retroativamente cada login: o bloco local explicita esse limite.
API/protocolos de provider podem mudar; falhas ficam visíveis, sem números
fabricados. Métodos extras de troca/login/compra de créditos não são executados.

Scanners externos SonarQube/Veracode/Wiz e cobertura quantitativa: **dispensados,
não executados**, conforme política vigente do usuário. Não houve achado crítico
ou alto não mitigado na revisão local; isso não equivale a resultado de scanners.
Nenhuma alteração de configuração pessoal além da memória já autorizada; Learn
automático e envio de conteúdo Knowledge/Route continuam desligados.

Sem subagentes. Leituras e manutenção independentes foram agrupadas; mudanças,
checks de código, build e atualização da instância seguiram dependências.
