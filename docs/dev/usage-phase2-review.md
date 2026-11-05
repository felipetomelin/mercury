# Fase 2 — verificação consolidada de Usage

Estado em 2026-10-06: **implementação local verificada; gate externo
FAIL/não verificável**. Sem commit.

## Evidência local

- `cargo test --offline -p mercury-usage`: 46/46 PASS, cobrindo parsers,
  histórico, janelas, orçamento, statusline, app-server e ledger.
- `cargo test --offline --bin mercury usage -- --test-threads=1`: 35/35 PASS,
  incluindo avaliação de orçamento, atualização de configuração, snapshot,
  sidebar e aba Usage.
- `mercury config check` no fixture temporário: `config: ok`.
- Smoke em PTY isolado: um rollout Codex sintético recente reportou 1.200
  tokens de entrada e 300 de saída. O scan histórico alimentou o orçamento
  `tui-codex` de 2.000 tokens em 24 h. A aba Usage mostrou `1.5k / 2k`,
  `75%` e aviso `⚠`; a lista de agentes permaneceu vazia porque nenhuma sessão
  estava vinculada a pane. O servidor foi parado. Nenhuma conta real foi lida.

## Limites e gate

O smoke não valida limites/custo reportados por conta real, statusline em pane
real nem alerta desktop; estes caminhos têm testes locais, mas requerem
verificação operacional própria. SonarQube, Veracode, Wiz e cobertura de
projeto não estão disponíveis. O gate PagueVeloz permanece **FAIL/não
verificável**; não fazer commit nem declarar a fase concluída. O diretório
temporário `/private/tmp/mercury-tui-smoke-j7i01L` permanece porque a política
do ambiente impede sua exclusão.

Revisão visual adicional em 2026-10-07: a área da aba Usage agora é limpa antes de desenhar, para não herdar símbolos do pane subjacente. A correção foi revisada junto às abas Crons, History e Learn; testes da shell 331 PASS/2 ignorados, client_mode 28/28 na rerodada, Clippy global e formatação PASS. O gate externo permanece FAIL/não verificável.

## Statusline em pane PTY e alerta integrado (2026-10-07)

Um novo teste de integração inicia servidor/cliente reais em PTY isolado, um
executável Claude descartável que reporta uma sessão com a origem oficial, e
um transcript temporário com 510 tokens. O comando real `mercury statusline`
recebe JSON por stdin com contexto de 85%, limite `five_hour` de 61% e custo
reportado. A aba Usage mostra o limite, gauge exato do agente e orçamento de
510/1000 tokens com alerta de 51%; nenhum modelo ou conta real é acionado.

O smoke encontrou uma falha: se o statusline chegava dentro do throttle de
1 segundo após o poll do transcript, o servidor guardava o snapshot mas
retornava antes de aplicar os limites de conta. O próximo poll atualizava o
contexto do pane, porém não esses limites. `apply_statusline_report` agora
aplica limites do provedor antes do retorno do throttle; o overlay da sessão
continua diferido. Um statusline idêntico apenas renova o horário de
observação, sem solicitar redesenho por mudança artificial do timestamp.
Teste unitário fixa os dois casos e o teste PTY cobre a cadeia
CLI → socket → sessão vinculada → snapshot → TUI. Uma origem não oficial não
vincula sessão e não é usada como prova do bridge; o fixture foi corrigido para
imitar o hook oficial e aguardar a sessão aparecer em `pane.list`. O teste de
sessão divergente também fixa que limites do provedor não são aceitos de um
statusline não vinculado ao pane.

Revisão consolidada: **PASS local** para a correção, sem mudança de protocolo
ou escrita em config real. `cargo test --offline --bin mercury usage` **38/38**,
`cargo test --offline --test client_mode` **31/31**, Clippy global,
`cargo fmt --all --check` e `git diff --check` passaram. A primeira versão do
teste exigia `ctx 85%` contíguo, mas a barra visual fica entre esses textos;
a asserção agora verifica a linha do agente e cada indicador, sem mudar o
comportamento. Continua faltando smoke com Claude autenticado/statusline real,
alerta desktop e gate SonarQube/Veracode/Wiz/cobertura; a Fase 2 não é
declarada concluída.

## Statusline sem limites após limite anterior (2026-10-07)

Uma statusline Claude válida e vinculada ao pane é a fonte atual dos limites
da conta. Se uma atualização posterior não trouxer `rate_limits`, o servidor
substitui o valor anterior por uma entrada Claude sem janelas: a aba Usage
mostra `n/a` e não conserva uma porcentagem obsoleta. A atualização de conta
continua independente do throttle do overlay do pane; sessão divergente é
rejeitada antes de tocar no cache. O cache ainda é global por provedor, então
o último relatório Claude válido prevalece quando há múltiplas contas Claude;
atribuição por conta exigiria contrato de identidade próprio.

O teste unitário cobre a transição 61% → `n/a`; o teste PTY percorre
`mercury statusline` → socket → Usage com um Claude descartável, sem chamada
de modelo. Verificação local: filtro Usage do binário **39/39** e
`client_mode` **31/31**. Não houve envio externo nem mudança de protocolo.
O filtro Route **25/25**, Clippy do workspace com `-D warnings`,
`cargo fmt --all --check` e `git diff --check` também passaram após o ajuste.
Os smokes com conta autenticada e alerta desktop, além do gate externo,
continuam pendentes; esta evidência não conclui a Fase 2.
