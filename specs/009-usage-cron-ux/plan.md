# Plano técnico

Status: aprovado, implementado e validado localmente; evidências em
`docs/dev/usage-cron-009-review.md`.

1. **U — coleta e contrato:** confirmar limites dos CLIs atuais sem prompt;
   adaptar transporte read-only; acompanhar identidade de perfil, fonte,
   observed_at, último erro sanitizado e stale. Cache por perfil/conta, cancelamento,
   limites de concorrência/tempo/saída, backoff e refresh manual. Reusar resolução
   de executáveis sem depender só do PATH herdado por aplicativo gráfico.
   Corrigir parser Codex para individualLimit.remainingPercent/resetsAt mesmo
   sem primary/secondary; a sondagem real confirmou esse caso (Business, 23%
   restantes). Testar nulos, faixas inválidas e direção restante/utilizado.
2. **U — apresentação:** estender projeção privada Usage com cards, barras,
   plano, reset e diagnósticos; separar histórico/custos estimados. Qualquer
   mudança de contrato deve atualizar capacidades/schema e testes pertinentes;
   não modificar hashes de métodos antigos sem alteração real do contrato.
3. **C — modelo de formulário:** draft tipado de agenda/destino, presets e
   conversão para Schedule existente. Testar prévias com relógio fixo, offset,
   meses curtos, datas passadas e ocorrência única. Mensal dia 31 pula meses
   sem esse dia; explicar na prévia. Agenda avançada mantém comportamento atual.
4. **C — interação:** formulário próprio renderizado na superfície Crons com foco, seleção/checkboxes,
   campos e resumo; integrar input/mouse/rolagem. Reaproveitar mesmo formulário
   para edit. Confirmação de exclusão envia CronRemove e atualiza lista, sem
   novo mecanismo de armazenamento. Proteger contra respostas atrasadas.
5. **V — auditoria visual:** inventariar todas as telas listadas na spec e
   registrar problemas/prioridade/correção; aplicar palette semântica e textos
   de estado/ações consistentes. Corrigir truncamento, contraste/foco e atalhos
   que impeçam uso. Capturar evidências renderizadas em 80x24 e 120x40.
6. **M — operação:** backup privado, mudança atômica de knowledge, config check,
   server reload-config, status/index/search. Sem restart ou mudança em Learn.
   Esta operação utiliza implementação e aprovação já existentes da spec 008.
7. **R — fechamento:** revisão local de código/segurança; regressões focadas,
   integração PTY/API com provedores simulados e ambiente isolado. Como há
   mudanças em contratos e navegação compartilhada, executar workspace completo,
   build, Clippy -D warnings, fmt, diff e manutenção. Registrar resultados reais
   e limitações de contas/plataformas; scanners externos dispensados, sem PASS.

## Fluxo proposto

```text
Novo agendamento
Nome: [Revisão diária                     ]
Agenda: [Dias úteis v]   Horário: [09:00]   UTC: [-03:00]
Próximas: ... calculadas pelo motor do servidor ...
Executar com: [Codex v]
  Nova execução em segundo plano usando sua conta conectada.
Instrução: [campo de várias linhas                     ]
Diretório: [Workspace atual v]
                 [Cancelar] [Voltar] [Revisar]

Revisar agendamento → [Voltar] [Salvar]
Detalhe → [Editar] [Pausar] [Executar agora] [Excluir…]
```

Todos os controles têm operação por teclado e mouse. O resumo é obrigatório;
seleção não executa tarefa nem salva automaticamente.
