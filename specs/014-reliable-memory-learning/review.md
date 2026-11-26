# Revisão consolidada — 014

Revisão local com pv-post-implement-review, pv-test-and-review e
pv-security-quality-gate; escopo: produtores/hook APIs, diário SQLite, snapshots,
consumidores Learn/Knowledge, recuperação, contexto/cache, diagnósticos e CI.

## Achados corrigidos antes da publicação

- Cache concatenado por delimitador poderia colidir entre projeto e consulta:
  chave agora usa JSON estruturado; regressão verifica isolamento entre projetos.
- Um Stop lido após a próxima tarefa poderia incluí-la no snapshot: o evento
  agora persiste offsets exatos, com fila limitada e reconhecimento durável.
- Origem automatizada anterior ao binding não sobrevivia ao restart: tombstones
  de sessão e marcadores pelo UUID estável do terminal são persistidos antes da
  submissão; falha ao gravar impede o envio automático.
- Excluir somente checkpoints com ambos os estágios pendentes deixava outro
  consumidor elegível: cada estágio pendente é excluído separadamente.
- Amostragem de tarefa com muitas atualizações podia perder a única mensagem do
  usuário: retenção preserva essa evidência e transporta o sinal de truncamento.
- O teto de texto não abrangia o escape JSON do snapshot: leitura/verificação
  permitem 256 KiB serializados, mantendo 64 KiB de texto e 64 mensagens.
- Deduplicação não podia contornar validação da proposta nem mascarar erro do
  ledger: entradas são validadas e mapeamentos persistidos em transação.
- No-op da memória e repetição de lição são no_change, com auditoria e sem nova
  escrita. Cache opcional de respostas preserva evidências já validadas.

## Segurança e limites

Sem segredos, caminhos pessoais ou conversas reais adicionados ao repositório.
Snapshots/ledger privados usam os controles existentes de proprietário/permissão
Unix, rejeição de symlinks e validação de identidade. Janela consumida é relida e
verificada antes do commit do cursor. Escrita em notas/memória mantém os writers
reversíveis e suas verificações de hash. Modelos continuam sem ferramentas.

Armazenamento, filas, fontes, amostras, grafo local, contexto e caches são
limitados. Novos checkpoints oferecem somente a memória gerenciada ao Learn;
o alvo reservado deriva da instalação. Nenhuma permissão de Route é ampliada.

A inspeção de hooks cobre os registros globais gerenciados; overlays de projeto
ou plugins podem desabilitá-los e são reportados como limite do diagnóstico.
Tokens são uma estimativa por bytes/4. CI Linux/macOS executou com sucesso para 518008c
(run 45835386533, ambos os jobs success). Windows não é qualificado para as
escritas privadas Unix. Smokes de inferência são separados dos testes sintéticos.

Gates SonarQube/Veracode/Wiz e cobertura quantitativa: dispensados pela decisão
vigente de 2026-10-29 na constituição do Mercury, sem alegação de PASS externo.
O veredito local final e os comandos/resultados estão em validation.md.
Nenhum subagente foi criado; revisão e testes foram consolidados por grupo.
