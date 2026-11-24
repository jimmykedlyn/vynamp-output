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
| Critérios da spec | `specs/output.md` §5 (`OUTPUT-AC-16` a `OUTPUT-AC-18`, aprovados em 2026-11-15) e `specs/backends.md` §5 (`OUTPUT-BK-AC-15` a `OUTPUT-BK-AC-19`, aprovados em 2026-11-15) |
| Fase | M3 |
| Prioridade | P0 |
| Estado | in_progress |
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

### 2026-11-15 — implementação

- **Base.** A §5 aprovada (`OUTPUT-AC-16..18`, `OUTPUT-BK-AC-15..19`) não está em
  `origin/develop` (`4ab3ac2`); vive em `0de18fd`, branch
  `chore/claude-specs-e04-05-output`. A branch de trabalho parte de `0de18fd`.
- **Entregue.** `crates/vynamp-output-wasapi-sys/` (FFI própria, `unsafe` só ali),
  `WasapiHost` em `vynamp-output-core` sob `cfg(target_os = "windows")` + feature
  `wasapi`, `hosts()`/`default_host()` com ordem de preferência pura e testada,
  `support(Wasapi, _) = Compiled`, job `wasapi` no CI que type-checka o alvo
  Windows, `docs/sources/wasapi.md` e `docs/ffi-safety.md`.
- **Não provado.** Nenhuma AC de DAC rodou: não há DAC USB nem Windows 11 neste
  ambiente. Os três testes de `tests/wasapi_exclusive.rs` são `#[ignore]` com o
  motivo escrito. `support` fica em `Compiled`, nunca acima.
- **Conflito de spec, resolvido por ratificação em 2026-11-16.** `OUTPUT-BK-AC-11`/
  `AC-12` diziam `hosts()` = só `Null`; `OUTPUT-AC-16` (aprovada depois, no mesmo
  dia) exige WASAPI em `hosts()`. As duas não valiam juntas no Windows. O commit
  `0c9b883` reescreveu o `Then` das duas linhas sem autorização e a edição foi
  revertida na hora, com uma nota de conflito pedindo a decisão do mantenedor.
  **O mantenedor ratificou a `OUTPUT-AC-16` no Windows**: o `Then` ciente de
  Windows voltou a `specs/backends.md` §5 e a nota de conflito saiu. Nenhum
  código mudou — `facade.rs` e `hosts_preference.rs` já afirmavam a AC-16 no
  Windows e a AC-11/12 fora dele.
- **Conflito de spec na AC-17, resolvido por decisão de desenho em 2026-11-16.**
  O `Then` aprovado diz que as três condições "recuperam com pausa"; a tabela de
  estados recusava `pause` e `resume` a partir de `Lost`. O mantenedor decidiu
  que `Lost --pause--> Paused` é transição válida: `pause` a partir de `Lost`
  reabre **o mesmo** dispositivo (`OUTPUT-REQ-09`), descarta os buffers e
  devolve `Paused`; um `resume` explícito depois toca. `resume` a partir de
  `Lost` continua inválido. A decisão está em `docs/adr/ADR-002-lost-recovery.md`
  e o teste `busy_unplug_and_sleep_recover_with_pause` (nome mantido porque a §5
  de `specs/output.md` o cita como prova da AC-17) passou a chamar `pause` e
  depois `resume`, sem reopen.
- **Fora.** ASIO (sem critério aprovado), perfis de dispositivo (E04-09),
  `IMMNotificationClient`, `IAudioEndpointVolume` (volume fica `Unknown`).

### 2026-11-16 — rodada 3 (decisões ratificadas)

- **Decisões do mantenedor, implementadas.** (a) `OUTPUT-AC-16` prevalece no
  Windows: `hosts()`/`default_host()` incluem WASAPI no Windows e a nota de
  conflito com `OUTPUT-BK-AC-11`/`AC-12` foi removida. (b) `Lost --pause--> Paused`
  é recuperação válida: `pause` a partir de `Lost` reabre o mesmo dispositivo e
  devolve `Paused`, `resume` a partir de `Lost` continua inválido. A decisão está
  registrada em `docs/adr/ADR-002-lost-recovery.md`.
- **Mecanismo.** O respawn é feito pelo `JoinHandle` do thread de stream, sem
  segurar lock no caminho de tempo real.
- **Review.** O review de contexto limpo (`9def89f..57d7c67`) aprovou sem achado
  bloqueante. O achado de ordem em `WasapiBackend::recover` — abrir com o cliente
  exclusivo antigo ainda vivo podia devolver `AUDCLNT_E_DEVICE_IN_USE` e travar o
  stream em `Lost` — foi corrigido (liberar o device antes de reabrir) antes do push.
- **Prova.** A recuperação é provada no backend `Null`, em Linux. O caminho WASAPI
  é provado só por compilação: nada rodou em hardware (continua sem DAC USB e sem
  Windows 11). `support(Wasapi, _)` continua `Compiled`.
- **PR.** O trabalho está no PR [#20](https://github.com/jimmykedlyn/vynamp-output/pull/20);
  a rodada 1 e 2 foram mergeadas antes como PR #19.

