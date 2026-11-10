# TASK-E01-02 — Provar callback e output clock

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E01-02` |
| Módulo dono | `vynamp-output` |
| Repo | `jimmykedlyn/vynamp-output` |
| REQ coberto | `REQ-E01-02` |
| Critérios da spec | pendente — `specs/output.md` §5 |
| Fase | M0 |
| Prioridade | P0 |
| Estado | implemented |
| Depende de | `TASK-E11-01` |
| Bloqueia | `TASK-E01-03`, `TASK-E04-02`, `TASK-E04-05`, `TASK-E13-01` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | alto |

## Entrega

Spike shared output, ring SPSC e harness de callback com instrumentation.

## Boundary

- **Não** conserta o viés do relógio apresentado. `presented = conteúdo escrito antes da
  callback − latência` subestima a posição em até `latency_frames` quando há silêncio
  de underrun ou de drenagem enfileirado — 12 393–12 800 frames no hardware medido. A
  correção depende do dreno completo, que é `TASK-E01-05` (engine). Esta TASK publica o
  relógio e caracteriza o limite; o que drenar é decisão do engine.
- **Não** escreve o backend de OS. O repo entrega `NullHost` e `CaptureHost`; a
  `crates/vynamp-output/src/lib.rs` declara que "release only the pure-Rust
  [`NullHost`] exists; OS backends (WASAPI, ...)" vêm depois. WASAPI exclusive, ALSA
  `hw:` e CoreAudio são `TASK-E04-05`, e DoP/384 kHz é `TASK-E04-06`.
- **Não** faz resampling, dither nem ganho. A ADR-011 fixa que "a saída nunca faz
  resampling, dither nem ganho" — essas etapas são do grafo DSP. `specs/output.md` §2
  permite `vynamp-dsp` atrás da feature `resample`, mas nenhum `Cargo.toml` do workspace
  declara essa dependência: o caminho não existe, não está só desligado.
- **Não** mixa, não drena e não faz transição de faixa. O `drain` desta lib só espera a
  posição alcançar `content_end`; quem enfileira conteúdo, faz fade e troca de fonte é
  o engine (`TASK-E01-03`, `TASK-E01-05`).
- **Não** prova áudio audível nem bit-perfect de hardware. A validação acústica segue
  ausente, e `docs/hardware-matrix.md` está com toda linha em "not run" — o spike
  mediu contadores e alocações, não o que sai da tomada do DAC.
- **Não** fecha AC-3 como comportamento de player. Underrun gerando zeros sem replay é
  provado na `driver.rs` e em `tests/stream.rs`; o que o engine deve fazer ao detectar
  o underrun não é decidido aqui.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e01-02-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E01-02-AC-1` | a callback de áudio rodando com o ring SPSC pré-alocado | o harness instrumenta uma reprodução completa | Nenhum lock/alloc/I/O na callback |
| `TASK-E01-02-AC-2` | a callback publicando `presented` por seqlock de escritor único | a latência reportada do primeiro frame do buffer é comparada com o conteúdo consumido | clock deriva consumo e device latency |
| `TASK-E01-02-AC-3` | o ring sem amostras disponíveis e a marca de fim de stream ainda não lida | a callback preenche a região restante | underrun gera zeros sem replay. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test --workspace --locked` no repo `vynamp-output` — cobre `--test zero_alloc_device` (AC-1), `--test stream` e `--test null_host` (AC-2), e `driver::tests::underrun_tail_is_silence_and_samples_are_never_replayed`, unit test da lib de `vynamp-output-core`, junto com `--test stream` (AC-3) | `133 passed, 1 ignored, 18 suites`, exit 0; o ignorado é o doctest `alloc (line 8)` do testkit. Suítes por AC: 5 verdes em `zero_alloc_device`, 10 em `stream`, 7 em `null_host`, e o `underrun_tail_…` no binário de unittests da lib do core. Verde no `CI Gate` de `.github/workflows/ci.yml` (`fmt` → `clippy` → `test` → `machete` → agregado) |
| Oráculo independente | `ChannelRamp` do testkit — frame `f` carrega `start + step * f`, então uma amostra repetida ou fora de ordem aparece como valor que a rampa não prevê —, mais `expected_values()` em `tests/capture.rs`, que reconstrói a sequência esperada com `None` no rabo de underrun; no hardware, os JSON de `evidence/PR/task-e01-02/` (SHA-256 `a3621cc7…` e `4844d2fd…`, reconferidos) | comparação amostra a amostra contra a sequência prevista, não "parece certo": no spike, `presented_frames_final` 11 507 474 contra 11 520 000 de conteúdo, diferença dentro da faixa de latência medida (12 393–12 800), `presented_regressions: 0` e `clock_rate_ratio` 1,00017 |
| Hardware | spike em WASAPI shared real: dispositivo padrão `Headset (USB 384KZ HIFI Audio)`, 384 000 Hz, 3 840 frames de buffer, 30 s, com e sem stall de 300 ms (`probe-steady` e `probe-stall`) | `rt_alloc_events: 0` nos dois, `underruns` 0 e 25, `stream_errors: 0` e `presented_regressions: 0`. **Limitação declarada, não aprovação:** o repo hoje só tem `NullHost` e `CaptureHost` — a `crates/vynamp-output/src/lib.rs` diz que backends de OS chegam em releases posteriores —, então o probe atual não reproduz esse run sem o backend WASAPI, que é `TASK-E04-05`. Linux, macOS e Android não foram medidos |

## DoD — pronto para review

- [ ] Todos os critérios de aceite com evidência
- [ ] `cargo fmt --check` e `cargo clippy --all-targets -- -D warnings` limpos
- [ ] `cargo test` verde; testes ignorados listados com motivo
- [ ] Spec do módulo atualizada se a implementação revelou algo novo
- [ ] `backlog.json` com o estado correto
- [ ] Revisão feita por contexto limpo, não pelo implementador
- [ ] PR aberto com template preenchido; CI Gate verde no SHA final
## Notas de execução

Fechada em 2026-11-09 por verificação cláusula a cláusula. A medição em hardware
WASAPI real é a evidência original (2026-10-18, `evidence/PR/task-e01-02/`);
o que segue confirma cada AC contra o estado atual do código e contra essas
medições, e registra a limitação de clock que o mantenedor aceitou para a entrega.

| AC | Verificação executada | Resultado |
|---|---|---|
| AC-1 | `cargo test --workspace --locked --test zero_alloc_device` | 5/5 verdes, incluindo `running_and_underrun_periods_do_not_allocate`; `zero_alloc_tests::fill_and_convert_do_not_allocate` (lib) também verde; no spike em hardware `rt_alloc_events: 0` em `probe-steady` e `probe-stall` |
| AC-2 | `tests/stream.rs` `periods_are_frame_exact_and_paused_ticks_deliver_nothing` + `drain_completes_exactly_after_the_last_content_frame`; `tests/null_host.rs` `real_clock_smoke`; spike `probe-steady` | `device_frame` é múltiplo exato do período, fonte `Estimated`, `latency_frames` vai junto no snapshot do seqlock (`rtflags.rs`), `frames_delivered` = callbacks × período, e `drain` espera `device_frame >= content_end`. No spike: `latency_frames_min` 12 393 / `max` 12 800, `clock_rate_ratio` 1,00017, `presented_regressions` 0 |
| AC-3 | `driver.rs` `underrun_tail_is_silence_and_samples_are_never_replayed` + `tests/stream.rs` `underrun_is_counted_once_per_short_period` | a sequência com período curto devolve `[1,2,3,4]`, `[5,0,0,0]`, `[0,0,0,0]`, `[13,14,15,16]` — silêncio no rabo, zero replay, 2 underruns contados. No spike de stall: `underruns: 25`, `underrun_frames: 94 976` |

`cargo test --workspace --locked` → **133 passed, 1 ignored, 18 suites**. O
ignorado é o doctest `alloc (line 8)` do testkit, documentado como exemplo.

**AC-3 — o texto do Then tem um travessão órfão.** O critério no backlog
termina em "underrun gera zeros sem replay." (ponto final). A tabela do item
ficou com "underrun gera zeros sem replay. |" porque a coluna foi truncada
quando o backlog foi gerado. O teste cobre a semântica inteira, então a TASK
fecha; a grafia é do backlog e não muda aqui.

**Limitação de clock declarada, não escondida.** O campo `presented`
(`PresentationTiming.device_frame`) subestima o consumo real em até
`latency_frames` quando há silêncio enfileirado — no spike, 12 393–12 800
frames. Medido: `probe-steady` apresentou 11 507 474 contra 11 520 000 de
conteúdo (12 526 frames de diferença) e `probe-stall` 11 507 560, ambos com
`presented_regressions: 0`. O mantenedor aceitou esta limitação como entrega
de M0 em 2026-11-09, com a correção atribuída à **TASK-E01-05**
(`vynamp-engine`, M1, `planned`) — que é quem implementa mixer, drain e
transições. A dona desta limitação é o engine, não o output: este repo publica
o relógio, o engine decide o que drenar.

**O que esta TASK não provou:** nada aqui é áudio ouvido por um humano. A
validação acústica continua ausente. Nenhuma flag de prontidão
(`application_ready`, `release_enabled`, `branding_exports_ready`) mudou.

