# 010 — Instalação local do comando Mercury

Status: implementada e validada localmente em 2026-11-05; publicação autorizada
pelo pedido do usuário. Evidências em docs/dev/local-install-010-review.md.

## Problema

O usuário precisa chamar um binário dentro de target/debug ou target/release.
Não existe instalação Unix documentada que permita executar mercury de qualquer
diretório. Trocar inadvertidamente debug por release abre outra configuração.

## Solução

Instalador Bash 3.2+ para macOS/Linux, executado a partir do checkout, sem sudo.
Por padrão compila release com Cargo.lock e instala sob ~/.local; --profile dev
usa o binário debug e mantém o namespace mercury-dev. O comando é um link para
uma cópia gerenciada do binário, independente da pasta target e do checkout.
--prefix permite um destino absoluto alternativo; --no-build instala um build
existente; --uninstall remove somente a instalação que este script gerencia.
O instalador nunca abre TUI, inicia/para servidor ou altera configurações,
memória, agendas, shell rc, aliases ou integrações dos agentes.

## Critérios de aceite

- AC1: com prefix/bin no PATH, instalação válida permite mercury --version,
  mercury --help e mercury config check de qualquer diretório.
- AC2: instalação usa build release por padrão; --profile dev seleciona debug
  explicitamente. A saída informa o perfil e a separação dos dados.
- AC3: reinstalação substitui atomicamente a cópia gerenciada; não depende do
  checkout depois de instalada e não interrompe processos já executando.
- AC4: arquivo, diretório ou link mercury de outra origem é recusado sem
  alteração; caminhos com espaços são suportados e prefix relativo é rejeitado.
- AC5: build com erro/binário ausente não substitui a instalação anterior;
  diretórios internos redirecionados por symlink são recusados.
- AC6: se prefix/bin não está no PATH, mostrar instrução segura para adicioná-lo
  e verificar qual mercury é resolvido; não editar shell automaticamente.
- AC7: uninstall é idempotente, remove somente link/binário gerenciados e nunca
  toca config, estado, vault ou binário externo.
- AC8: nesta máquina instalar perfil dev e verificar o comando contra o mesmo
  caminho de config usado antes; não reiniciar servidor nem migrar dados.
- AC9: README EN/PT e guia explicam instalar, abrir, atualizar, desinstalar,
  pré-requisitos e separação de perfis; commit/push da entrega autorizado.

## Clarificações resolvidas

O usuário autorizou todas as fases nesta solicitação; não há decisão de produto
pendente. Preservar a instalação atual implica usar --profile dev nesta máquina.
Novos usuários usam release por padrão. Sem binários publicados, Homebrew,
curl-pipe-shell, auto-download, mudança de versão ou novo feed nesta entrega.
Windows mantém seu caminho experimental separado. PATH ausente exige um passo
explícito no shell e instrução para torná-lo persistente; nenhuma edição oculta.

## Trade-offs

A instalação ainda exige Rust/rustup, Zig 0.16 e ferramentas nativas de build.
Uma cópia evita quebra por cargo clean e atualizações in-place do executável
ativo. Atualização do servidor continua sendo ação explícita via live-handoff.
Verificação de conflitos não oferece proteção contra concorrência hostil entre
processos do mesmo usuário. Testes automatizados simulados não qualificam Linux.

## Tarefas

T1 descoberta/clarificação; T2 instalador; T3 testes de ciclo de vida;
T4 documentação; T5 revisão/checks; T6 instalação pessoal, commit e push.

## Dependências

Cargo/Rust fixados no projeto, Zig 0.16, Bash 3.2+, utilitários Unix padrão.
Python stdlib apenas para testes. Nenhum contrato de API/servidor é alterado.
