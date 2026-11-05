# Fase 3 — Temas Mercury

Status: especificação, esclarecimentos, plano e análise concluídos para a Fase 3 (2026-10-06), no objetivo aprovado de concluir `PLAN.md`. Implementação local concluída; revisão/gates em andamento.

## Problema e estado atual

O Mercury mantém os temas built-in herdados, auto-switch light/dark e overrides `[theme.custom]`. O overlay Settings já lista temas, faz preview ao mover a seleção e restaura o original em Cancel; ao confirmar, persiste o nome e recarrega a configuração. O gauge de contexto usa `palette.green/yellow/red` genéricos. Falta identidade visual própria, cores semânticas de gauge e import de formatos de tema externos.

## Solução proposta

1. Adicionar o tema built-in `mercury` ao catálogo, com paleta própria dark e variante light para auto-switch. Não alterar as cores dos temas existentes nem o default sem decisão explícita do usuário.
2. Adicionar `gauge_ok`, `gauge_warn` e `gauge_crit` à `Palette`, com fallback derivado das cores semânticas atuais para todos os temas legados. Permitir override via `[theme.custom]` e blocos light/dark. Usar os tokens no gauge de contexto e nos próximos indicadores de orçamento.
3. Reaproveitar o picker de Settings e seus estados de preview/Cancel/Confirm. Adicionar teste de integração que seleciona `mercury`, verifica mudança imediata de palette, Cancel e persistência no Confirm. Evitar criar um segundo picker.
4. Permitir import local de Base16 YAML e temas Ghostty em um formato limitado e documentado. A interface será `[[theme.import]]` com `name`, `path` absoluto e `format = "base16" | "ghostty"`; `theme.name`/`dark_name`/`light_name` podem selecionar o nome. O import deve ser explícito, local, sem rede, com validação rigorosa de cores e mensagens diagnósticas. A paleta importada deve funcionar no preview e com reload ao vivo; arquivo inválido preserva o tema anterior.

## Critérios de aceite

- `mercury` aparece no picker e troca de tema sem reiniciar.
- Gauge usa tokens semânticos em todos os temas; overrides custom funcionam.
- Preview muda ao mover a seleção, Cancel reverte, Confirm persiste.
- Dado um tema Base16/Ghostty válido configurado, quando a configuração carrega, então ele aparece no picker e suas cores são usadas no preview e após Confirm.
- Dado arquivo ausente, sintaxe inválida, cores inválidas ou nome duplicado, quando a configuração carrega, então um diagnóstico é emitido e a paleta ativa anterior permanece; nenhuma entrada de tema quebrada é aplicada parcialmente.
- Build, Clippy, testes de config/palette/picker e gate de arquitetura passam.

## Trade-offs

- O tema `mercury` não vira o default automaticamente: mudar o visual de usuários existentes seria uma migração separada.
- Base16 aceita somente o subconjunto plano `base00`–`base0F` com hex RGB; Ghostty aceita `background`, `foreground`, `cursor-color`, `cursor-text`, seleção e `palette` 0–15. Outras opções Ghostty, YAML aninhado/âncoras e comandos de tema ficam fora de escopo. Isso evita dependência nova e execução de conteúdo do arquivo. Até 32 imports de 128 KiB cada.
- Import é leitura no load/reload da configuração, nunca durante renderização. O arquivo permanece como dependência local declarada; Mercury não o copia nem modifica.

## Dependências

- `ThemeConfig`, `Palette`, `ThemeRuntimeConfig`, picker Settings e reload ao vivo existentes. Sem biblioteca YAML nem rede.

## Tarefas e plano técnico

Etapa A: catálogo + paletas + tokens de gauge e testes. Etapa B: importadores puros e fixtures. Etapa C: integração com config/picker/reload e testes. Etapa D: revisão consolidada, ciclo de testes e gate de segurança. `src/config/theme.rs` define catálogo e overrides; `src/app/state.rs::Palette` resolve cores; `src/client/shell/settings.rs` já implementa preview.

## Análise de consistência

- Picker existente já tem preview, cancelamento e persistência de nome; a lista estática `THEME_NAMES` precisa incluir imports válidos sem afetar índices das outras seções.
- `ThemeRuntimeConfig` já é reconstruído no load/reload; o conteúdo importado deve ser resolvido ali e mantido em memória. Não ler arquivo no render.
- `parse_color` hoje retorna cyan para entrada inválida, portanto o importador precisa de parser estrito próprio para impedir aceitação silenciosa de cores malformadas.
- Não há parser YAML em `Cargo.lock`; o subconjunto Base16 plano será analisado por linhas, explicitamente documentado.
- Se um import inválido tiver o mesmo nome do tema selecionado, manter a palette anterior no reload; no startup, usar fallback built-in com diagnóstico. A leitura de imports não altera o wire público.

## Configuração de import

```toml
[theme]
name = "ocean"

[[theme.import]]
name = "ocean"
path = "/caminho/absoluto/tema.conf"
format = "ghostty" # ou "base16"
```

O nome importado aparece no picker Settings e pode ser usado em `dark_name` ou `light_name`. O arquivo é lido somente no startup/reload. A importação Ghostty segue o subconjunto de cores do [exemplo oficial](https://ghostty.org/docs/features/theme), sem aplicar outras opções da configuração Ghostty.
