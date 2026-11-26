# Análise de consistência e gates

Pesquisa, esclarecimentos, plano e análise concluídos. O usuário aprovou
planejamento, execução, testes e publicação em 2026-11-26, com escopo portátil
para o repositório central. Implementação autorizada.

## Rastreabilidade

| Evidência / necessidade | Grupo | Aceite |
| --- | --- | --- |
| Worker/status não explicam descartes e curadoria no-op | G1 | D1, D2 |
| Binding volátil, hooks insuficientes e formato id-only | G2 | A1–A4 |
| Chats longos e contagem anulada pelo truncamento | G2, G3 | A4, B1 |
| Captura terminal source changed, sem proposta | G3, G4 | B1, B4, B5 |
| Notas ao concluir tarefa sem fechar o chat | G2–G4 | A1, B2 |
| Apenas memória e notas gerenciadas | G4 | B2–B4 |
| Recuperação por grafo com pouco contexto | G5 | C1–C7 |
| Comprovar uso real, não só mocks/flags enabled | G6 | D4, D5 |

## Decisões e limites

- A nova captura por checkpoint amplia deliberadamente o contrato anterior da
  011, que limitava Knowledge a encerramentos. Foi confirmada pelo usuário.
- Sem sinal confiável, não deduzir conclusão de texto/idle: mostrar limitação e
  oferecer checkpoint explícito. Não redefinir fingerprint de fonte viva sem
  delimitar e validar o trecho concluído.
- O defeito id-only foi reproduzido, mas os headers das três sessões ativas têm
  ambas as chaves. Não é apresentado como causa direta dessas três sessões.
- Handoff sem rebind e contagem de turnos truncada são caminhos independentes
  de perda de progresso. Corrigir só o erro source changed não fecha a entrega.
- GraphRAG presente sobre uma nota não prova aprendizagem de novas sessões.
  Captura/publicação/recuperação devem ser aceitas separadamente.
- Grafo otimiza seleção de evidências; não autoriza inferências como decisões
  validadas nem dispensa orçamento/citações. Meta de ~2 mil tokens é aproximada
  sem tokenizer; teto em bytes é verificável.
- Não adicionar edição de código, skills, instruções humanas nem ampliar egress
  por conveniência. O consentimento de inferência existente não habilita Route.
- Smoke real pessoal exige checkpoints selecionados e ativação documentada,
  sem backfill global. Nenhuma dessas ações foi executada nesta investigação.

## Gates

Investigação, spec, esclarecimentos, plano e análise: preparados e consistentes.
Implementação, revisão e validação locais concluídas. Publicação autorizada após gates locais PASS.
Relatório de execução e limitações em validation.md.
Externos/coverage: dispensa vigente do Mercury, sem resultado fabricado.
Nenhum agente delegado; leituras independentes foram agrupadas em ferramentas.
