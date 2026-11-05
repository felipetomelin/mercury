# Restauração de arquivos de terceiros — 2026-10-29

Os testes de manutenção falhavam por arquivos ausentes. Foram restaurados bytes
autênticos; nenhum hash de aceitação foi alterado.

## portable-pty 0.9.0

Fonte: https://static.crates.io/crates/portable-pty/portable-pty-0.9.0.crate
Entrada: `portable-pty-0.9.0/LICENSE.md`.
Destino: `vendor/portable-pty/LICENSE.md`.
SHA-256 do arquivo: `191c46fcf52061382b1c51a70311eb9081381cc158e5899f3739473a9432185b`.

## Microsoft ConPTY

Pacote oficial conforme `packaging/windows/conpty.json`:
https://github.com/microsoft/terminal/releases/download/v1.24.11911.0/Microsoft.Windows.Console.ConPTY.1.24.260710001.nupkg

SHA-256 confirmado: `9382ad7becb7e4d84e300578d8e4f4df28f43d979d9055d978c42913c47e0e9d`.
O pacote não contém LICENSE/NOTICE; os textos foram obtidos do tag correspondente:

- https://raw.githubusercontent.com/microsoft/terminal/v1.24.11911.0/LICENSE
  → `packaging/windows/licenses/Microsoft.Windows.Console.ConPTY-LICENSE.txt`.
  SHA-256: `5d177f23ecfeb0ea8e050b6a5a16355e1ae9a0b286436ca8f83ed08b3795be6b`.
- https://raw.githubusercontent.com/microsoft/terminal/v1.24.11911.0/NOTICE.md
  → `packaging/windows/licenses/Microsoft.Windows.Console.ConPTY-NOTICE.md`.
  SHA-256: `e7fbaadee6ab20c28b87730a510ee5f5815d8fb4bd88d1d54d282dc2a74c0726`.

Ambos correspondem exatamente aos hashes preexistentes. Downloads via curl com
TLS validado pelo sistema; Python falhou na cadeia corporativa e não foi usado
com verificação desabilitada. Nenhum binário baixado foi executado.
Isso resolve os contratos de arquivos ausentes; não valida assinatura NuGet,
empacotamento Windows completo, execução Windows ou todos os avisos do produto.
