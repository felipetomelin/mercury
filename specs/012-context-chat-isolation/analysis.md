# Análise de consistência

Pesquisa, esclarecimentos e plano cobrem os critérios de isolamento, ambos os
provedores, troca de sessão, respostas atrasadas e preservação de cotas globais.
A falha concreta na validação Claude justifica regressão específica; não comprova
sozinha a causa do incidente reportado. Investigar vínculo inicial antes de
declarar resolução do incidente ao vivo. A validação da identidade, a proteção
contra cache antigo e as regressões foram implementadas; evidências e limitações
em `validation.md`.

Gate de aprovação do contexto satisfeito pelo usuário em 2026-11-09.
Ampliação de títulos aprovada explicitamente pelo usuário em 2026-11-09.
Pesquisa complementar concluída: nomes manuais vêm do label associado à sessão
no Mercury; Codex fornece `id`/`thread_name` em `session_index.jsonl`; Claude usa
`custom-title` no transcript e títulos/resumos em `sessions-index.json`.
A migração adiciona título e primeira mensagem, preserva o FTS e força uma única
releitura dos transcripts sem depender de mudança de tamanho/data. O contrato
opcional é compatível com respostas antigas. Nomes do Mercury persistem além do
fechamento do painel e têm prioridade; no Claude, o título explícito do transcript
precede resumos do índice. Fallback usa a primeira mensagem do usuário, depois
“Conversa sem título”. Não há questões de produto em aberto nem execução por
agentes paralelos. Testes e limitações estão em `validation.md`.
