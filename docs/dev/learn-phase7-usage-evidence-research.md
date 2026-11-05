# Fase 7 — pesquisa de evidência de uso (2026-10-07)

## 1. Áreas relevantes

- `crates/mercury-learn/src/curator.rs`: `plan_unused_archive` aceita
  `UsageEvidence` com janela, contagem e `coverage_complete`; recusa janela
  inferior a 30 dias, uso observado, cobertura incompleta ou data antiga.
  Nenhum produtor de cobertura completa está ligado ao scheduler semanal.
- `src/app/learn.rs` e `src/app/api/learn.rs`: gatilhos de fim/marco e revisão
  semanal validam sessão e transcript, mas não observam invocação de skill.
- `src/integration/claude_settings.rs` instala hooks `SessionStart`, `Stop` e
  `SessionEnd`; o script só reporta identidade/caminho da sessão. O campo
  `skills` de Crons é acrescentado ao prompt, sem confirmar carregamento.

## 2. Padrões e contratos existentes

O ledger Learn mantém metadados privados e limitados; o transcript entregue ao
revisor exclui tool output e não é um registro completo de todos os eventos de
ferramenta. Um evento positivo de uso precisaria associar um ID exato da
allowlist a uma sessão autenticada, sem guardar texto de prompt, argumentos ou
caminho arbitrário de ferramenta.

Na documentação atual do Claude Code, `PreToolUse` com matcher `Skill` observa
invocações do modelo, mas uma invocação direta `/skill` passa por
`UserPromptExpansion`; os dois caminhos são necessários. O evento
`InstructionsLoaded` cobre `CLAUDE.md` e `.claude/rules/*.md`, não é prova de
carregamento de qualquer `SKILL.md` ou memória arbitrária. Fontes:
[hooks Claude](https://code.claude.com/docs/en/hooks) e
[skills Claude](https://code.claude.com/docs/en/skills).

## 3. Riscos e restrições

- Ausência de evento não prova ausência de uso se o hook foi removido,
  desabilitado, falhou, ou se houve sessão fora da instrumentação. Instalação
  pontual e um heartbeat do Mercury não comprovam cobertura contínua dos
  processos Claude/Codex externos.
- O Codex pode carregar um skill por leitura de `SKILL.md`; metadados de skill
  no contexto indicam disponibilidade, não invocação. Não há, neste projeto,
  um hook Codex verificado que ateste todas essas leituras. A documentação de
  [skills da OpenAI](https://developers.openai.com/api/docs/guides/tools-skills)
  descreve descoberta e leitura, mas não fornece aqui uma trilha local de
  cobertura para todos os processos.
- Memórias podem ser lidas como arquivos comuns ou inseridas por mecanismos de
  contexto do provedor. Registrar apenas `/skill` perderia esses usos.
- Mesmo após implementar captura positiva, a primeira janela elegível começa
  na ativação comprovada de cobertura e precisa transcorrer por 30 dias reais.
  Dados anteriores não podem ser retroativamente rotulados como completos.

## 4. Decisão de análise

Manter arquivamento automático por desuso desligado enquanto não existir um
ledger de cobertura contínua verificável para **todos** os mecanismos de
carregamento em escopo. É seguro adicionar futuramente contagem positiva de
uso, mas isso não autoriza `coverage_complete=true`. Duplicatas byte-idênticas
continuam sendo curadas reversivelmente pelo fluxo já testado.

## 5. Questões em aberto

1. O escopo de “sem uso” pode ser restrito explicitamente a sessões geridas
   pelo Mercury, ou deve incluir Claude/Codex iniciados fora dele?
2. Qual evento oficial do Codex comprova carregamento de `SKILL.md` e de
   memórias para versões suportadas?
3. Como detectar lacunas de hooks/versão, falha de entrega e períodos sem
   processo sem transformar silêncio em cobertura completa?
