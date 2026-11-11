# Spec — vynamp-output · output

| Campo | Valor |
|---|---|
| Módulo | `vynamp-output` |
| Crate(s) | `vynamp-output-core` (núcleo, std-only), `vynamp-output` (fachada), `vynamp-output-testkit` (dev) |
| Tipo | lib |
| Versão da spec | 0.2.0 |
| Data | 2026-11-11 |
| Estado | **rascunho autorado, não aprovado** — §3 a §10 escritos a partir do código e dos testes existentes; a aprovação é do mantenedor, por `GOVERNANCE.md` |
| Dono | mantenedor do `vynamp-output` |
| Depende de | `vynamp-dsp` (opcional, feature `resample`) |
| É dependido por | engine |

> Esqueleto gerado pelo passo 37, preenchido em 2026-11-11. §1, §2, §7 e §8 vêm de
> `01-arvore-final.md` §2 e valem como norma; não foram alterados. §3 a §6 e §9 a §10
> descrevem o contrato que o código em `crates/` já cumpre, e marcam como **planned**
> o que o design (`docs/superpowers/specs/2026-10-25-vynamp-output-design.md`) decide
> mas ainda não existe. Esta spec cobre o contrato comum a todo host: pedido,
> negociação, stream, callback, DoP e verificação. O que é de cada host está em
> `specs/backends.md`.

## 1. Propósito

Entregar amostras ao dispositivo de audio com taxa, layout, latencia e exclusividade corretos.

**Não faz:** Nao decodifica, nao processa DSP musical, nao conhece arquivo.

## 2. Fronteira

| Permitido | Proibido |
|---|---|
| std; `vynamp-dsp` opcional atras de feature `resample` | qualquer crate de dominio `vynamp-*`; qualquer tipo de dominio (exceto `PathProperties`, que e deste modulo e so de primitivos) |

## 3. API pública

Só o que está aqui é contrato. A fachada `vynamp-output` reexporta todo o núcleo e
acrescenta a seleção de host (`specs/backends.md` §3).

### 3.1 Pedido

| Tipo | Contrato |
|---|---|
| `StreamRequest` | `device: DeviceSelector`, `mode: ShareMode`, `format: SampleFormat`, `rate: u32`, `channels: u16`, `layout: ChannelLayout`, `buffer: BufferRequest`, `on_unavailable: OnUnavailable`, `conversion: Conversion`, `allow_rate_change_of_device: bool` |
| `DeviceSelector` | `Id(DeviceId)` ou `Default(Role)`. Índice de enumeração nunca é ID |
| `ShareMode` | `Shared` ou `Exclusive`. Nunca há troca implícita de um pelo outro |
| `SampleFormat` | `F32`, `F64`, `I16`, `I24Packed` (3 bytes), `I24In32 { justify }`, `I32 { valid_bits }`, `U8`, `Dop { dsd_rate }`. Contêiner e bits válidos são sempre separados |
| `OnUnavailable` | `Error` (padrão) ou `Downgrade` |
| `Conversion` | `Forbid` (padrão) ou `Allow(ConversionSpec)`: conversão de tipo de amostra determinística, sem dither, sem resampling |
| `BufferRequest` | quadros, duração ou padrão do backend |

### 3.2 Negociação e configuração real

- `negotiate(&StreamRequest, &Capabilities) -> Result<Negotiated, Error>` é pura e
  determinística. Ela tenta exatamente o pedido. Se não houver, aplica a conversão
  pedida. Só com `OnUnavailable::Downgrade` ela escolhe a entrada mais próxima, e
  devolve o motivo.
- A regra de "mais próximo" é fixa:
  1. mesmo modo, nunca cruzando PCM e DoP;
  2. taxa igual ou acima antes da taxa abaixo;
  3. a taxa domina a largura de contêiner;
  4. na mesma taxa, a mesma largura de contêiner, e no empate a mais larga;
  5. por fim, os canais.
- `Capabilities` é uma lista de `CapabilityEntry` (modo, formato, taxas, canais),
  marcada `CapabilityOrigin::Advertised` ou `Probed`.
- `StreamConfig` é o que de fato abriu: host, device, modo, formato, taxa, canais,
  layout, buffer e `PathProperties`. `Stream::config()` sempre devolve o real.
- `PathProperties` só tem primitivos:
  - `os_mixer_bypassed: Tri`
  - `os_resampling: bool`
  - `library_conversion: Option<ConversionSpec>`
  - `endpoint_volume: EndpointVolume` (`Unity`, `NotUnity(f32)` ou `Unknown`)
  - `transport: Transport` (`Usb`, `Bluetooth`, `Hdmi`, `BuiltIn`, `Virtual` ou `Unknown`)
  - `rt_priority: RtPriority`

### 3.3 Stream

| Item | Contrato |
|---|---|
| `Stream::state()` | `StreamState`: `Opened`, `Running`, `Paused`, `Draining`, `Stopped`, `Lost` |
| `start` / `pause` / `resume` / `stop` | transições da tabela de estados; uma transição inválida é `Error::InvalidRequest`. `Drop` equivale a `stop` |
| `drain(timeout) -> DrainOutcome` | `Drained` só depois que o último quadro de conteúdo foi entregue; `TimedOut` se a fonte nunca terminar |
| `timing()` | `Option<PresentationTiming>`, fora da callback |
| `counters()` | `Counters { source_underruns, os_xruns, frames_delivered, callbacks }` |
| `events()` | `&Receiver<Event>` (`std::sync::mpsc`). Os eventos são produzidos fora da thread de tempo real |

`Event` (non_exhaustive):

- dispositivos: `DeviceAdded`, `DeviceRemoved`, `DefaultChanged { role }`, `DeviceLost`, `SharedFormatChanged`
- negociação: `Downgraded { requested, actual, reason }`
- falhas de entrega: `OsXrun { count }`, `SourceUnderrun { count }`, `SourcePanicked`
- avisos: `RtPriorityDenied { reason }`, `DopVolumeUnverified`

`Downgraded` e `DopVolumeUnverified` chegam antes da primeira callback.

### 3.4 Contrato de tempo real

```rust
pub trait RenderSource: Send + 'static {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext) -> Rendered;
}
```

- `DeviceBuffer` traz slices tipadas, intercaladas, no formato **real** do stream.
- `RenderContext` traz:
  - `frames`;
  - `device_frame_index`, um contador monotônico de **todo** quadro entregue, conteúdo
    e silêncio;
  - `timing`;
  - `config`.
- `Rendered { frames }` diz quanto a fonte escreveu. A biblioteca completa o resto com
  o silêncio correto do formato, conta um underrun por período curto e nunca repete
  amostras.
- Um panic em `render` é capturado com `catch_unwind`. A partir dele o stream entrega
  silêncio, mantém a cadência e emite `SourcePanicked` uma vez.
- Nada aloca na seção de tempo real: nem a callback da biblioteca, nem a conversão, nem
  o empacotamento DoP, nem o silêncio depois de um panic.

### 3.5 DoP

- `dop_pcm_rate(dsd_rate)` dá a taxa da portadora PCM, que é `dsd_rate / 16`.
- `check_dop_path(&PathProperties)` recusa o caminho se o mixer não for comprovadamente
  contornado, se houver conversão da biblioteca, ou se o volume do endpoint não for
  unity. Com volume `Unknown`, o stream abre e emite `DopVolumeUnverified`.
- O empacotamento põe 16 bits DSD por canal em amostras de 24 bits. O marcador
  `0x05`/`0xFA` alterna sem quebra, inclusive no silêncio, e o silêncio usa o padrão
  idle `0x69`.

### 3.6 Verificação

- `support(host, feature) -> Option<VerificationLevel>`, para cada `Feature` de
  `Feature::ALL`: `SharedMode`, `ExclusiveMode`, `IntegerPcm`, `Dop`, `Timing`,
  `Events`.
- Os níveis vão do mais forte ao mais fraco: `CaptureVerified`, `HardwareVerified`,
  `Integration`, `LibraryVerified`, `Compiled`.
- `support_matrix_markdown()` gera `docs/support-matrix.md`.

## 4. Requisitos

| ID | Requisito | REQ de produto | TASK | Onde é provado |
|---|---|---|---|---|
| `OUTPUT-REQ-01` | A callback não aloca, não trava e não faz I/O em nenhum estado de entrega: tocando, em underrun, convertendo, após panic, empacotando DoP ou gravando captura — implemented | `REQ-E01-02` | `TASK-E01-02` | `tests/zero_alloc_device.rs`; `src/zero_alloc_tests.rs`; `src/rt_guard.rs` |
| `OUTPUT-REQ-02` | Underrun completa o período com o silêncio do formato, é contado uma vez por período curto e nunca repete amostras — implemented | `REQ-E01-02` | `TASK-E01-02` | `tests/stream.rs`; `tests/capture.rs` |
| `OUTPUT-REQ-03` | `device_frame_index` conta conteúdo e silêncio, e `timing()` expõe `PresentationTiming` fora da callback — implemented | `REQ-E01-02` | `TASK-E01-02` | `tests/stream.rs` |
| `OUTPUT-REQ-04` | Pedido indisponível é erro com `nearest`, salvo `OnUnavailable::Downgrade` explícito, que abre o mais próximo e emite `Downgraded` antes da primeira callback — implemented | `REQ-E04-05` | `TASK-E04-05` | `tests/negotiate.rs`; `tests/null_host.rs` |
| `OUTPUT-REQ-05` | Conversão de tipo só com `Conversion::Allow`, nunca entre modos e nunca de PCM para DoP; quando acontece, é registrada em `library_conversion` — implemented | `REQ-E04-06` | `TASK-E04-06` | `tests/negotiate.rs`; `tests/capture.rs` |
| `OUTPUT-REQ-06` | PCM inteiro (I24Packed, I32 com bits válidos, F32) chega bit a bit ao ponto de captura, sem troca de canais — implemented no host `Capture`; planned (M3) em hardware | `REQ-E04-06` | `TASK-E04-06` | `tests/capture.rs` |
| `OUTPUT-REQ-07` | DoP DSD64/128/256 em 176,4/352,8/705,6 kHz, com marcador contínuo através de períodos e underrun, e recusado em caminho misto, desconhecido, shared, com volume não unity ou com conversão — implemented | `REQ-E12-04` | `TASK-E12-04` | `tests/dop.rs` |
| `OUTPUT-REQ-08` | Máquina de estados fixa; `drain` termina exatamente após o último quadro de conteúdo ou devolve `TimedOut`; `Drop` para o stream — implemented | `REQ-E01-02` | `TASK-E01-02` | `tests/stream.rs` |
| `OUTPUT-REQ-09` | `DeviceLost` vem só do backend; a biblioteca nunca migra para outro dispositivo sozinha — implemented no contrato; planned (M3) nos hosts reais | `REQ-E04-05` | `TASK-E04-05` | `tests/stream.rs` |
| `OUTPUT-REQ-10` | A matriz de suporte publicada é igual à gerada pelo código — implemented | `REQ-E01-02` | `TASK-E01-02` | `crates/vynamp-output/tests/support_matrix.rs` |
| `OUTPUT-REQ-11` | Formato DSD nativo (`DsdNative`) distinto de `Dop` em `SampleFormat` e `StreamConfig` — planned (M5) | `REQ-E12-04` | `TASK-E12-04` | — |
| `OUTPUT-REQ-12` | PCM 768 kHz anunciado e aberto quando o dispositivo suporta — planned (M5) | `REQ-E12-04` | `TASK-E12-04` | — |
| `OUTPUT-REQ-13` | Diagnóstico de rota expõe transporte Bluetooth e volume de endpoint; o que o OS não expõe aparece como `Unknown`, nunca presumido — planned (M3); hoje só existem os tipos (`Transport`, `EndpointVolume`) | `REQ-E17-19` | `TASK-E17-19` | — |
| `OUTPUT-REQ-14` | ID estável e capacidades por dispositivo, base dos perfis de dispositivo do player — planned (M3); hoje só `DeviceId` e `IdStability` | `REQ-E04-09` | `TASK-E04-09` | — |

## 5. Critérios de aceite

| ID | Given | When | Then | Teste |
|---|---|---|---|---|
| `OUTPUT-AC-01` | um stream no host `Null`, tocando, em underrun, convertendo, após panic, com DoP e no ring de captura | os períodos rodam sob o alocador de contagem por thread | zero alocações em cada caso | `zero_alloc_device.rs::{running_and_underrun_periods_do_not_allocate, library_conversion_does_not_allocate, silence_after_a_panic_does_not_allocate, capture_ring_does_not_allocate, dop_packing_and_idle_do_not_allocate}` |
| `OUTPUT-AC-02` | uma fonte que devolve menos quadros que o período | o período é entregue | o restante é silêncio, `source_underruns` sobe 1 por período curto e nenhuma amostra se repete | `stream.rs::underrun_is_counted_once_per_short_period`; `capture.rs::f32_bytes_are_bit_exact_with_underrun_silence` |
| `OUTPUT-AC-03` | o host `Null` com relógio virtual | o stream avança períodos e pausa | os períodos são exatos em quadros e um tick pausado não entrega nada | `stream.rs::periods_are_frame_exact_and_paused_ticks_deliver_nothing` |
| `OUTPUT-AC-04` | uma fonte que entra em panic | a callback seguinte roda | sai silêncio, a cadência se mantém e `SourcePanicked` chega uma vez | `stream.rs::panic_gives_silence_one_event_and_keeps_cadence` |
| `OUTPUT-AC-05` | um stream em cada estado | cada operação é chamada | a transição segue a tabela; as inválidas são `InvalidRequest` | `stream.rs::{state_machine_table, stop_from_opened_and_paused, drop_stops_the_stream}` |
| `OUTPUT-AC-06` | uma fonte que sinaliza fim, e outra que nunca termina | `drain(timeout)` é chamado | `Drained` exatamente após o último quadro de conteúdo; `TimedOut` para a fonte sem fim | `stream.rs::{drain_completes_exactly_after_the_last_content_frame, drain_times_out_when_the_source_never_ends}` |
| `OUTPUT-AC-07` | capacidades sem o formato pedido | `negotiate` roda com `Error` e com `Downgrade` | com `Error`: `FormatUnsupported { nearest }`; com `Downgrade`: abre o mais próximo pela regra de §3.2, com motivo | `negotiate.rs::{downgrade_returns_nearest_with_reason, downgrade_without_candidates_is_an_error, nearest_prefers_rate_at_or_above, nearest_rate_dominates_container_width, nearest_only_considers_same_mode}` |
| `OUTPUT-AC-08` | um downgrade aceito no host `Null` | o stream é aberto e iniciado | `Downgraded` chega antes da primeira callback | `null_host.rs::downgrade_event_comes_before_the_first_callback` |
| `OUTPUT-AC-09` | um pedido F32 e um dispositivo só I24 | `conversion` é `Forbid` e depois `Allow` | `Forbid`: `FormatUnsupported`; `Allow`: abre em I24, com `library_conversion` preenchido; nunca cruza modos | `negotiate.rs::{conversion_forbidden_is_unsupported, conversion_allowed_opens_target_format, conversion_does_not_cross_modes, pcm_to_dop_always_rejected_and_dop_to_pcm_never_converted}`; `capture.rs::library_conversion_f32_to_i24_is_recorded` |
| `OUTPUT-AC-10` | uma rampa conhecida em I24Packed, I32 e F32 | o host `Capture` grava o que a callback entregou | os bytes são iguais ao oráculo e os canais não estão trocados | `capture.rs::{i24_packed_bytes_are_bit_exact_with_underrun_silence, i32_bytes_are_bit_exact_with_underrun_silence, channels_are_not_swapped}` |
| `OUTPUT-AC-11` | DSD64/128/256 em DoP no `Null` e no `Capture` | o stream toca, sofre underrun e fica em silêncio | taxa da portadora = DSD/16; os marcadores alternam sem quebra; o silêncio é o idle pattern | `dop.rs::{pcm_rate_is_dsd_rate_over_16, null_opens_dsd64_128_256_at_their_carrier_rates, markers_continue_across_periods_and_an_underrun, silence_is_idle_pattern_with_alternating_markers}` |
| `OUTPUT-AC-12` | um caminho DoP misto ou desconhecido, shared, com volume não unity ou com conversão | o stream DoP é aberto | erro, nunca DoP pelo mixer; volume `Unknown` abre com `DopVolumeUnverified` | `dop.rs::{refused_on_a_mixed_or_unknown_path, refused_on_a_shared_path, refused_when_endpoint_volume_is_not_unity, refused_with_library_conversion, unknown_endpoint_volume_opens_with_a_warning}` |
| `OUTPUT-AC-13` | um dispositivo só DSD64 | DSD128 é pedido com `Downgrade` | abre DSD64 em DoP, nunca PCM | `dop.rs::dsd128_downgrades_to_dsd64_on_a_dsd64_only_device`; `negotiate.rs::dop_request_never_gets_pcm_nearest` |
| `OUTPUT-AC-14` | um stream no `Null` | o host não sinaliza perda | `DeviceLost` nunca aparece; só o backend o produz | `stream.rs::device_lost_comes_only_from_the_backend` |
| `OUTPUT-AC-15` | o código atual | `support_matrix_markdown()` roda | o resultado é igual a `docs/support-matrix.md` | `support_matrix.rs::committed_matrix_matches_generator` |

### Critérios que esta spec **não** fixa

- Os critérios de hardware de `TASK-E04-05` e `TASK-E04-06`: DAC USB, captura S/PDIF,
  busy, unplug e sleep. Eles dependem dos hosts reais (`specs/backends.md`) e
  continuam `planned`.
- Os critérios de `TASK-E04-06` AC-2/AC-3 (EQ, ReplayGain, selo de origem) e de
  `TASK-E04-09` (camadas de perfil, import de preset de fone). Esses são do player e do
  engine. Este módulo só fornece `PathProperties`, `DeviceId` e capacidades.
- `TASK-E12-04` AC-1/AC-2 (`DsdNative`) e `TASK-E17-19` (diagnóstico de rota). As APIs
  ainda não existem; ficam em `OUTPUT-REQ-11` a `OUTPUT-REQ-13`.

## 6. Erros e cancelamento

Erro é dado, nunca `panic!`. `Error` é `non_exhaustive` e não leva `String` como único
conteúdo.

| Situação | Erro ou efeito |
|---|---|
| Biblioteca do OS ausente | `LibraryUnavailable { lib }`; o host é pulado com motivo |
| Nenhum host utilizável | `NoBackend` |
| `DeviceId` desconhecido | `DeviceNotFound` |
| Dispositivo ocupado em exclusivo | `DeviceBusy`. Nunca vira shared em silêncio |
| Exclusivo ou DoP impossível no caminho | `ExclusiveUnavailable { reason }` |
| Formato fora das capacidades, sem `Downgrade` | `FormatUnsupported { nearest }` |
| Permissão negada pelo OS | `PermissionDenied` |
| Dispositivo removido durante o stream | estado `Lost` + `Event::DeviceLost`; operações seguintes são `DeviceLost` |
| Enumeração travada | `Timeout` (`specs/backends.md`) |
| Pedido inválido (canais 0, taxa 0, conversão impossível, transição de estado inválida) | `InvalidRequest { what }` |
| Falha do OS sem mapeamento | `Os { backend, code, call }` |
| Panic da fonte | não é erro de API: silêncio + `SourcePanicked` |

Cancelamento é explícito:

- `stop()` encerra o stream e descarta os buffers do dispositivo.
- `drain(timeout)` limita a espera e devolve `TimedOut`, sem cortar o que já foi
  entregue.

Nenhuma operação desiste por conta própria fora de `drain` e de `enumerate_with_timeout`.

## 7. Fora de escopo

Selecao de faixa, volume de aplicacao, EQ; casting e AirPlay sao do player (E12-09)

## 8. Dependências

Depende de: `vynamp-dsp` (opcional, feature `resample`)

É dependido por: engine

## 9. Testes e fixtures

Comando:

```bash
cargo test --workspace
```

| Arquivo | Prova |
|---|---|
| `crates/vynamp-output-core/tests/stream.rs` | estados, períodos, underrun, panic, drain, perda, drop, relógio virtual |
| `crates/vynamp-output-core/tests/negotiate.rs` | negociação exata, mais próximo, downgrade, conversão, pedidos inválidos, Display de erros e eventos |
| `crates/vynamp-output-core/tests/capture.rs` | oráculo bit a bit: rampa conhecida comparada com os bytes gravados |
| `crates/vynamp-output-core/tests/dop.rs` | empacotamento DoP, marcadores, idle, recusas de caminho |
| `crates/vynamp-output-core/tests/null_host.rs` | dispositivo virtual, capacidades por modo, período, `DeviceNotFound` |
| `crates/vynamp-output-core/tests/zero_alloc_device.rs`, `src/zero_alloc_tests.rs` | zero alocação na seção de tempo real |
| `crates/vynamp-output-core/src/rt_guard.rs` | guarda estática de tokens proibidos nos módulos de tempo real |
| `crates/vynamp-output/tests/support_matrix.rs` | matriz publicada igual à gerada |
| `crates/vynamp-output-testkit/tests/counting_alloc.rs` | o próprio alocador de contagem |
| `tools/output-probe/tests/probe.rs` | o probe de linha de comando no `Null` e no `Capture` |

- O oráculo é numérico: as fontes geram rampas determinísticas, e `capture.rs` e
  `dop.rs` calculam os bytes esperados.
- Não há fixture de arquivo grande nem teste `#[ignore]`.
- `null_host.rs::real_clock_smoke` usa o relógio real, com tolerância.

## 10. Limitações conhecidas

| Limitação | Consequência | Quem cobre |
|---|---|---|
| Só os hosts `Null` e `Capture` existem | nenhum som sai num dispositivo real; `support()` devolve `None` para os hosts do OS | `specs/backends.md`, `TASK-E04-05` (M3) |
| Todo recurso está em `LibraryVerified` | não há prova em hardware nem de integração | `docs/hardware-matrix.md`, `TASK-E04-06` |
| `DsdNative` e PCM 768 kHz não existem | DSD só por DoP | `OUTPUT-REQ-11` e `OUTPUT-REQ-12`, `TASK-E12-04` (M5) |
| `Stream::reconfigure(rate)` do design não existe | troca de taxa em exclusivo exige fechar e reabrir o stream | futura TASK de hosts reais |
| A feature `resample` não está ligada a código | `vynamp-dsp` ainda não é usado; a biblioteca nunca faz resampling | `specs/backends.md` |
| Com `panic = "abort"` não há captura de panic | `SourcePanicked` não chega; o processo aborta | documentado; o hook de panic é do aplicativo |
| Diagnóstico Bluetooth e perfis de dispositivo não existem | E17-19 e E04-09 dependem de API nova | `OUTPUT-REQ-13` e `OUTPUT-REQ-14` |
