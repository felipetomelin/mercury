# Plano técnico — instalação local

Status: análise de consistência PASS; implementação, testes e ativação pessoal
concluídos. Evidências em docs/dev/local-install-010-review.md.

1. Implementar scripts/install-local.sh em Bash compatível com macOS 3.2/Linux.
   Validar flags, SO, prefixo absoluto e destino antes de compilar. Recusar
   links/diretórios inesperados no diretório privado lib/mercury e conflitos
   bin/mercury; aceitar somente link que aponta ao binário gerenciado esperado.
2. Compilar host com cargo build --locked --bin mercury --manifest-path ...
   --target-dir <checkout>/target --target <rustc host-tuple>, com --release salvo --profile dev.
   Rejeitar CARGO_BUILD_TARGET configurado para evitar selecionar artefato errado.
   --no-build usa target/<perfil>/mercury existente, ou target/<host>/<perfil>/mercury
   quando o artefato convencional não existe. O host é obtido com rustc --print
   host-tuple; a seleção explícita também ignora cross-target em Cargo config. Sem download próprio,
   alteração de rustup, sudo ou instalações implícitas de dependências.
3. Copiar o binário para staging privado no diretório de destino, chmod 755,
   validar --version, substituir por rename e expor link absoluto. Recusar
   binário/diretório interno inesperado. Atualização não trunca executável ativo.
   Trap limpa staging. Nenhum eval e todos os caminhos tratados como argumentos.
4. Desinstalação só remove o link esperado e executável gerenciado regular;
   rmdir apenas se vazio. Se não existe instalação, retornar sucesso. Config,
   estado, vaults e shell ficam intactos.
5. Exibir instrução mercury, perfil/config namespace, diagnóstico PATH e
   exemplo export com quoting Bash seguro. Não editar rc. Se outro comando
   ganha precedência, informar command -v mercury e correção do PATH.
6. Testes unittest com subprocess em prefixos temporários e falso Cargo/binários:
   instalação, update, uninstall, conflitos, no-build, falhas, espaços, perfis,
   PATH ausente/shadow, symlinks internos e binário independente do checkout.
   Smoke real de debug instalado com --version/--help/config check/status,
   comparação do config path e inventário read-only do servidor antes/depois.
7. Atualizar README EN/PT e docs/dev/local-install.md. Registrar evidências,
   rodar revisão consolidada, suite focada, manutenção pertinente, build, Clippy,
   fmt e diff. Sem repetir toda a suite Rust pois API/contratos não mudam.
8. Instalar pessoalmente --profile dev --no-build, verificar em shell de login
   e diretório /private/tmp, commitar e dar push em main após fast-forward local.

## Constituição / gates

PASS planejado: escopo aprovado, sem decisões pendentes, sem modelo remoto ou
mudança de configuração/dados; gates externos dispensados conforme constituição.
Sem skill de implementação específica de Bash/Rust instalada; seguir padrões
existentes do repositório. Skills de revisão/testes/segurança locais se aplicam.

## Contrato de arquivos

Somente <prefix>/lib/mercury/mercury (cópia) e <prefix>/bin/mercury (symlink).
Nenhum novo schema ou arquivo de configuração. Testes Python são desenvolvimento.
