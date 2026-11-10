# TASK-E04-09 — Entregar perfis e headphone preset import

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E04-09` |
| Módulo dono | `vynamp-output` |
| Repo | `jimmykedlyn/vynamp-output` |
| REQ coberto | `REQ-E04-09` |
| Critérios da spec | pendente — `specs/output.md` §5 |
| Fase | M3 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E04-04`, `TASK-E04-05` |
| Bloqueia | `TASK-E04-10` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 5–15 dias úteis |
| Risco | alto |

## Entrega

Per-device profiles e import manual com origem/licença.

## Boundary

- **Não** implementa a curva nem o schema do preset de EQ. Quem define os filtros, a validação por Nyquist e o `audio-preset.schema.json` v2 é `TASK-E04-04`; aqui se entrega o registro de perfil por dispositivo, o import com origem e licença e a resolução da camada que vence. A confusão é plausível porque o preset importado parece pertencer ao EQ.
- **Não** processa o ganho escolhido no áudio. Quem transforma a camada vencedora em amostras é o grafo de `TASK-E04-03` e o EQ10 com ganho de `TASK-E04-01`; esta TASK entrega qual perfil foi aplicado e com que valores, e o oráculo dela é a resolução da camada, não a resposta em frequência.
- **Não** implementa o backend exclusivo nem produz o identificador estável do dispositivo. Quem produz o stable ID e o perfil de saída é `TASK-E04-05`; esta TASK consome o ID e guarda o perfil por dispositivo. A confusão é plausível porque as duas TASKs tocam em 'perfil de dispositivo'.
- **Não** desenha a UI de device profiles. A tela e o audio-chain inspector são `TASK-E02-07`; aqui fica a API e os dados que a tela mostra.
- **Não** baixa preset de lugar nenhum. O import é manual e local, com origem e licença declaradas; busca em catálogo remoto e direitos de conteúdo são `TASK-E05-10`, e a spec de DSP diz explicitamente que transferir preset não acessa a rede. A confusão é plausível porque 'headphone correction' costuma vir de um banco online.
- **Não** valida DSP nem faz soak. O gate numérico é `TASK-E04-10` e o soak é `TASK-E01-10`; aqui o oráculo é a ordem de precedência e a identificação da origem.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e04-09-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E04-09-AC-1` | um DAC USB com stable ID 'wasapi/0-spdif-bitexact' e dois fones Bluetooth com IDs distintos, cada um com um preset de headphone correction importado de origem conhecida, e uma faixa com tag de gênero 'Jazz' e um preset global de EQ10 em +1,5 dB nas baixas | as cinco camadas são preenchidas com valores distinguíveis (manual +0,5 dB, faixa +1 dB, gênero +1,5 dB, dispositivo +2 dB e global +2,5 dB), e o cliente resolve o perfil da faixa corrente cinco vezes, removendo uma camada por vez da mais específica para a mais geral, lendo em cada passo qual camada venceu e qual valor ficou aplicado | Manual→track→genre→device→global aplicado |
| `TASK-E04-09-AC-2` | o DAC USB tocando em silêncio com o volume da aplicação em 30 por cento e um perfil de dispositivo que declara ganho de saída de +6 dB para o fone A | o usuário troca a saída para o fone A com o stream em `Running`, e o cliente relê o volume da aplicação antes e depois da troca e lê o volume efetivo e o ganho devolvidos pelo módulo | device change não aumenta volume automaticamente |
| `TASK-E04-09-AC-3` | um preset de headphone correction em JSON com campos de origem e licença preenchidos, e um segundo arquivo sem o campo de licença | os dois arquivos são submetidos ao import e o cliente lê, para cada submissão, o registro devolvido pelo módulo com origem, licença e nome do preset | fonte identificada. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-output` | verde no CI Gate |
| Oráculo independente | `cargo test --workspace --locked` em `libs/output` com um teste de precedência que monta as cinco camadas e afirma a camada vencedora, mais a listagem do módulo com a origem e a licença de cada preset importado | **medido em 2026-11-10**: isso não existe no repo hoje — a contagem de `profile`, `preset` e `transparent` em `crates/` é zero, e o único campo de origem existente é `CapabilityOrigin` em `crates/vynamp-output-core/src/negotiate.rs:25`, que diz de onde veio a capacidade do dispositivo, não a origem nem a licença de um preset. Evidência esperada ao fim da TASK: um teste que fixa a ordem manual, faixa, gênero, dispositivo e global, imprimindo a camada vencedora e o valor final para cada combinação, e um import que, diante de um JSON sem licença, recusa o preset e mantém o anterior, como a spec de EQ exige. O oráculo numérico do ganho em dB depende do DSP de `TASK-E04-01` |
| Hardware | nenhum: a precedência de camadas, a identificação da origem do preset e a regra de volume são lógica pura sobre IDs e números, e os IDs de dispositivo são sintéticos nos testes; nenhum par de fone ou DAC real é necessário para provar a ordem | ausência é limitação registrada, não aprovação |

## DoD — pronto para review

- [ ] Todos os critérios de aceite com evidência
- [ ] `cargo fmt --check` e `cargo clippy --all-targets -- -D warnings` limpos
- [ ] `cargo test` verde; testes ignorados listados com motivo
- [ ] Spec do módulo atualizada se a implementação revelou algo novo
- [ ] `backlog.json` com o estado correto
- [ ] Revisão feita por contexto limpo, não pelo implementador
- [ ] PR aberto com template preenchido; CI Gate verde no SHA final
## Notas de execução

Preenchido durante a execução, não antes.
