# TASK-E04-05 — Entregar saída exclusiva e device profiles

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E04-05` |
| Módulo dono | `vynamp-output` |
| Repo | `jimmykedlyn/vynamp-output` |
| REQ coberto | `REQ-E04-05` |
| Critérios da spec | pendente — `specs/output.md` §5 |
| Fase | M3 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E01-02`, `TASK-E04-02` |
| Bloqueia | `TASK-E04-06`, `TASK-E04-09` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 5–15 dias úteis |
| Risco | alto |

## Entrega

WASAPI event-exclusive, stable IDs, capabilities e ASIO adapter opcional.

## Boundary

- **Não** decide o selo OUTPUT nem o modo transparente. Selo, condições ativas e rótulos de diagnóstico são de `TASK-E04-06`, que depende desta; aqui sai a `StreamConfig` real com `PathProperties`, que é a matéria-prima daquele selo.
- **Não** aplica a precedência de perfis (manual, track, genre, device, global) nem importa AutoEQ. O import com origem e licença e a seleção de camada são de `TASK-E04-09`; a confusão é plausível porque o perfil por dispositivo nasce nesta TASK e a spec de EQ cita o stable ID dela.
- **Não** implementa ALSA, PipeWire, PulseAudio, CoreAudio nem AAudio. `HostId` enumera esses backends em `crates/vynamp-output-core/src/format.rs`, mas `support()` devolve `None` para todos; a execução por alvo é de `TASK-E14-04`. A confusão é plausível porque a política de saída exclusiva da spec vale para as cinco plataformas.
- **Não** promove o ASIO a caminho padrão nem o entrega como requisito. `reference/audio/05-platform-backends.md` põe o ASIO fora do núcleo, como adapter privado opcional, e quem valida um backend como caminho de referência é `TASK-E04-10`, dona da matriz de hardware e da qualidade numérica. A entrega obrigatória desta TASK é o WASAPI event-exclusive; alegar ASIO como padrão aqui afirmaria transparência que nenhum AC desta TASK mede.
- **Não** mede o resampler nem o packing PCM24/32. Isso foi provado por `TASK-E04-02` (ADR-011); aqui o formato chega pronto ou a conversão é recusada com `Conversion::Forbid`.
- **Não** faz o soak de 8 h nem mede o budget de callback p99.9, e não constrói a tela de perfis. Os gates são de `TASK-E01-10` e `TASK-E04-10`, e o audio-chain inspector é `TASK-E02-07`; esta TASK prova recuperação de evento e entrega a API que essas telas leem.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e04-05-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E04-05-AC-1` | um DAC USB que anuncia 24 bits válidos em 96 kHz e em 192 kHz, ligado a uma máquina Windows 11, e um host WASAPI event-exclusive já presente na lista de `hosts()` do módulo | o cliente abre `ShareMode::Exclusive` com `DeviceSelector::Id` do DAC, `SampleFormat::I24Packed`, `Conversion::Forbid` e `OnUnavailable::Error`, uma vez a 96 000 Hz e outra a 192 000 Hz, e lê `stream.config()`, `config.valid_bits` e `config.path.os_mixer_bypassed` nos dois streams | 24/96 e24/192 negociados emDAC |
| `TASK-E04-05-AC-2` | um stream exclusivo I24Packed a 96 000 Hz tocando num DAC USB em Windows 11, com outro processo pronto para ocupar o mesmo dispositivo em exclusivo, o cabo USB ao alcance e a suspensão da máquina habilitada | as três condições são induzidas em sequência (busy do dispositivo, unplug do cabo e sleep seguido de resume), e depois de cada uma o cliente lê a fila de `Event` do `Stream`, o `StreamState` corrente e a posição apresentada, e chama `resume` depois que o dispositivo volta | busy/unplug/sleep recuperam com pausa |
| `TASK-E04-05-AC-3` | um DAC USB aberto por `DeviceSelector::Id` em `ShareMode::Exclusive` e, ao lado, o alto-falante interno que o Windows expõe como default do role `Multimedia`, sem nenhuma preferência de fallback autorizada no perfil do dispositivo | o cabo do DAC é removido com o stream em `Running` e o cliente lê o `StreamState` final e a lista de dispositivos que o host tentou abrir ou abriu | sem fallback speaker automático. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-output` | verde no CI Gate |
| Oráculo independente | `cargo test --workspace --locked` em `libs/output` (inclui `--test negotiate`, `--test stream`, `--test null_host` e `--test zero_alloc_device`) mais `cargo run -p output-probe -- --host wasapi --device <id-do-DAC> --rate 192000 --clock real` no DAC físico | **medido em 2026-11-10**: `cargo test --workspace --locked` dá 133 verdes, 1 ignorado, 0 falhas; `cargo run -p vynamp-output --example list_devices` imprime só `host null`, com I24Packed de 44 100 a 768 000 Hz e 1–8 canais; `support(HostId::Wasapi, ...)` em `crates/vynamp-output-core/src/support.rs:88` devolve `None` e `docs/support-matrix.md` mostra wasapi com `-` nas seis colunas. Não existe hoje código WASAPI nem enumeração de device real, então a negociação de 96/192 kHz só pode ser provada contra entradas anunciadas do `NullHost`, não contra um DAC. A evidência esperada ao fim da TASK é: `config()` com `mode: Exclusive`, `format: I24Packed`, `valid_bits: 24`, `rate: 96000` e depois `rate: 192000`, com `path.os_mixer_bypassed: Yes`; a linha wasapi da matriz gerada saindo de `-` para `exclusive: integration`; e a captura do DAC físico anexada como evidência |
| Hardware | precisa de um DAC USB que aceite 24 bits em 96 e 192 kHz; este ambiente só tem 'Conexant ISST Audio' e 'Audio Intel(R) para telas', que são codecs internos e não um DAC USB hi-res, e o repo ainda não tem backend WASAPI para abrir nenhum deles — o critério de negociação exclusiva não roda aqui | ausência é limitação registrada, não aprovação |

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
