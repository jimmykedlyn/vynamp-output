# Spec — vynamp-output · backends

| Campo | Valor |
|---|---|
| Módulo | `vynamp-output` |
| Crate(s) | `vynamp-output-core` (trait `Host`, `NullHost`, `CaptureHost`, `support`), `vynamp-output` (fachada: `hosts`, `default_host`, `support_matrix_markdown`) |
| Tipo | lib |
| Versão da spec | 0.2.0 |
| Data | 2026-11-11 |
| Estado | **§5 aprovada em 2026-11-15** pelo mantenedor, na abertura da `TASK-E04-05`, com escopo estendido a todas as plataformas. As demais seções seguem rascunho autorado a partir do código e dos testes existentes: §3, §4, §6, §9 e §10 não foram aprovadas e não devem ser lidas como contrato. A aprovação é do mantenedor, por `GOVERNANCE.md` |
| Dono | mantenedor do `vynamp-output` |
| Depende de | `vynamp-dsp` (opcional, feature `resample`) |
| É dependido por | engine |

> **Esqueleto gerado pelo passo 37, preenchido em 2026-11-11.** §1, §2, §7 e §8
> vêm de `01-arvore-final.md` §2 e não mudaram. §3 a §6, §9 e §10 descrevem o que
> o código faz hoje e o que está planejado, separando os dois em cada linha. O
> contrato de stream, negociação e DoP está em `specs/output.md`; esta spec cobre
> só quem implementa esse contrato (os hosts) e como a fachada os escolhe.

## 1. Propósito

Entregar amostras ao dispositivo de audio com taxa, layout, latencia e exclusividade corretos.

**Não faz:** Nao decodifica, nao processa DSP musical, nao conhece arquivo.

## 2. Fronteira

| Permitido | Proibido |
|---|---|
| herda a fronteira de `specs/output.md` | herda as proibicoes de `specs/output.md` |

## 3. API pública

### 3.1 O trait `Host` (`vynamp-output-core::host`)

```rust
pub trait Host: Send + Sync {
    fn id(&self) -> HostId;
    fn enumerate(&self) -> Result<Vec<DeviceInfo>, Error>;
    fn default_device(&self, role: Role) -> Result<Option<DeviceInfo>, Error>;
    fn capabilities(&self, device: &DeviceId, mode: ShareMode) -> Result<Capabilities, Error>;
    fn open(&self, req: &StreamRequest, source: Box<dyn RenderSource>) -> Result<Stream, Error>;
}

pub struct DeviceInfo {
    pub id: DeviceId,
    pub id_stability: IdStability,
    pub name: String,
    pub host: HostId,
    pub default_for: Vec<Role>,   // papéis em que é o padrão do sistema
    pub transport: Transport,
}

pub fn enumerate_with_timeout(host: Arc<dyn Host>, timeout: Duration)
    -> Result<Vec<DeviceInfo>, Error>;
pub fn abandoned_enumeration_threads() -> u64;
```

- `open` negocia o pedido com as regras de `specs/output.md` §3.2 e devolve o
  stream no estado `Opened`. A `RenderSource` só é chamada na thread do
  dispositivo, depois de `start`.
- `enumerate` pode bloquear num driver defeituoso. `enumerate_with_timeout` roda
  a enumeração numa thread auxiliar e espera no máximo `timeout`. Se estourar,
  devolve `Error::Timeout`, **abandona** a thread (nunca a mata) e a conta em
  `abandoned_enumeration_threads()`. O host vai num `Arc` porque a thread
  abandonada pode viver mais que o chamador.

### 3.2 `HostId`

`Null`, `Capture`, `Wasapi`, `Alsa`, `PipeWire`, `Pulse`, `CoreAudio`, `AAudio`.
Só `Null` e `Capture` têm implementação nesta versão (§10).

### 3.3 `NullHost` e os relógios

```rust
pub enum NullClock {
    Real,                          // um período a cada period_frames / rate de tempo real
    Virtual(VirtualClockHandle),   // períodos só quando o teste chama advance
}

impl VirtualClockHandle {
    pub fn new() -> Self;
    pub fn advance(&self, periods: u64);
    pub fn inject_device_lost(&self);
    pub fn set_tick_probe(&self, probe: fn() -> u64) -> bool;
    pub fn tick_probe(&self) -> u64;
}

impl NullHost {
    pub fn new(clock: NullClock) -> Self;
    pub fn with_capabilities(clock: NullClock, entries: Vec<CapabilityEntry>) -> Self;
    pub fn with_os_mixer_bypassed(self, bypassed: Tri) -> Self;
    pub fn with_endpoint_volume(self, volume: EndpointVolume) -> Self;
}

pub fn default_capability_entries() -> Vec<CapabilityEntry>;
```

- O `NullHost` expõe um dispositivo virtual que é o padrão de todos os papéis e
  descarta as amostras. Serve de host de referência: tudo que o contrato de
  `specs/output.md` exige é provado nele.
- Um `VirtualClockHandle` dirige no máximo um stream aberto por vez. Depois que
  esse stream para, outro pode ser aberto no mesmo handle. Os ticks contam do
  zero por stream. O `Mutex` do handle só é usado do lado de controle; a thread
  do dispositivo nunca o trava.
- `inject_device_lost` é a única forma de o Null produzir `DeviceLost`. Isso
  vale para o contrato inteiro: perda de dispositivo só vem do backend.
- `with_os_mixer_bypassed` e `with_endpoint_volume` simulam o caminho do OS para
  provar as recusas de DoP (`specs/output.md` §3.5).

### 3.4 `CaptureHost` (feature `capture`)

```rust
impl CaptureHost {
    pub fn new(path: PathBuf) -> Self;
    pub fn with_capabilities(path: PathBuf, entries: Vec<CapabilityEntry>) -> Self;
    pub fn with_clock(self, clock: NullClock) -> Self;
    pub fn with_os_mixer_bypassed(self, bypassed: Tri) -> Self;
    pub fn with_endpoint_volume(self, volume: EndpointVolume) -> Self;
}
```

- Um dispositivo virtual (`"capture"`) que grava os bytes entregues em `path`,
  no formato do container negociado, e escreve `<path>.manifest` com `format`,
  `container_bytes`, `valid_bits`, `byte_order=little`, `rate`, `channels`,
  `period_frames`, `frames` e `overruns`.
- A feature `capture` do core é desligada por padrão. A fachada a expõe como
  `test-capture`; o testkit a liga.

### 3.5 Declaração de suporte (`vynamp-output-core::support`)

```rust
pub enum VerificationLevel {    // do mais forte ao mais fraco
    CaptureVerified, HardwareVerified, Integration, LibraryVerified, Compiled,
}
pub enum Feature { SharedMode, ExclusiveMode, IntegerPcm, Dop, Timing, Events }
pub fn support(host: HostId, feature: Feature) -> Option<VerificationLevel>;
```

- `support` diz o nível mais forte que cada par (host, feature) já alcançou, ou
  `None` se o host não implementa a feature nesta versão.
- `docs/support-matrix.md` é **gerado** a partir de `support`. A matriz nunca
  pode prometer mais do que o código declara.

### 3.6 Fachada (`vynamp-output`)

```rust
pub struct HostSelection {
    pub chosen: HostId,
    pub skipped: Vec<(HostId, &'static str)>,   // hosts tentados antes, com o motivo
}
pub fn hosts() -> Vec<Box<dyn Host>>;               // em ordem de preferência
pub fn default_host() -> (Box<dyn Host>, HostSelection);
pub fn support_matrix_markdown() -> String;
```

- Hoje `hosts()` devolve só o `NullHost` no relógio real, e `default_host()`
  devolve esse mesmo host com `skipped` vazio. A `HostSelection` existe para que,
  quando houver backends do OS, o chamador saiba qual host foi escolhido e por
  que os outros foram pulados, sem precisar adivinhar.

### 3.7 Backends do OS (desenho, não implementado)

WASAPI, ALSA, PipeWire, PulseAudio, CoreAudio e AAudio entram como
implementações de `Host`, cada uma atrás da sua feature e do seu alvo. Regras
que valem desde já:

- `unsafe_code = "forbid"` vale no workspace inteiro. A única exceção hoje é o
  `vynamp-output-testkit` (só `src/alloc.rs`, para o alocador contador), que é de
  teste e nunca é publicado. FFI de backend fica em crates `-sys` próprias,
  seguindo `docs/ffi-safety.md`.
- Um backend novo entra em `support` com o nível que realmente alcançou e só
  passa para `hosts()` quando tiver pelo menos `Integration`.

## 4. Requisitos

| ID | Requisito | REQ de produto | TASK | Onde é provado |
|---|---|---|---|---|
| OUTPUT-BK-REQ-01 | Todo host implementa `Host` e é intercambiável: o mesmo `StreamRequest` e a mesma `RenderSource` funcionam em qualquer um. — implemented | REQ de saída bit-perfect | E04-05 | `null_host.rs`, `capture.rs` |
| OUTPUT-BK-REQ-02 | Enumeração com prazo: estourou, devolve `Timeout`, abandona e conta a thread. — implemented | REQ de robustez | E04-06 | `enumerate_timeout.rs` |
| OUTPUT-BK-REQ-03 | O Null expõe um dispositivo virtual padrão de todos os papéis, com capacidades por modo e configuráveis. — implemented | REQ de testabilidade | E04-05 | `null_host.rs` |
| OUTPUT-BK-REQ-04 | O relógio virtual dirige um stream por vez, de forma determinística. — implemented | REQ de testabilidade | E04-05 | `stream.rs` |
| OUTPUT-BK-REQ-05 | O Capture grava os bytes entregues bit a bit, com manifesto. — implemented | REQ de saída bit-perfect | E04-05 | `capture.rs` |
| OUTPUT-BK-REQ-06 | A fachada lista hosts em ordem de preferência e diz qual escolheu e por quê. — implemented (só Null) | REQ de saída | E04-05 | `facade.rs` |
| OUTPUT-BK-REQ-07 | A matriz de suporte é gerada do código e não pode divergir dele. — implemented | REQ de transparência | E04-05 | `support_matrix.rs` |
| OUTPUT-BK-REQ-08 | Sem `unsafe` fora das crates `-sys` e do testkit. — implemented | REQ de segurança | E04-05 | lint do workspace (`unsafe_code = "forbid"`) |
| OUTPUT-BK-REQ-09 | Backend WASAPI (compartilhado e exclusivo). — implemented (exclusivo); compila para `x86_64-pc-windows-msvc`, nunca rodou em hardware | REQ de saída bit-perfect | E04-05 | `crates/vynamp-output-wasapi-sys/tests/format.rs`; `crates/vynamp-output-core/tests/wasapi_format.rs`; job `wasapi` do CI |
| OUTPUT-BK-REQ-10 | Backends ALSA, PipeWire, Pulse, CoreAudio e AAudio. — planned (M3) | REQ de saída por plataforma | E04-06 | — |
| OUTPUT-BK-REQ-11 | Perfis por dispositivo. — planned (M3) | REQ de perfis | E04-09 | — |
| OUTPUT-BK-REQ-12 | Saída Bluetooth. — planned (M3) | REQ de Bluetooth | E17-19 | — |

## 5. Critérios de aceite

| ID | Given | When | Then | Teste |
|---|---|---|---|---|
| OUTPUT-BK-AC-01 | um host cuja `enumerate` bloqueia | `enumerate_with_timeout` com prazo curto | devolve `Timeout` e `abandoned_enumeration_threads()` sobe 1 | `enumerate_timeout.rs::blocking_enumeration_times_out_and_the_thread_is_counted` |
| OUTPUT-BK-AC-02 | um host rápido | `enumerate_with_timeout` | devolve os dispositivos | `enumerate_timeout.rs::fast_enumeration_returns_the_devices` |
| OUTPUT-BK-AC-03 | um `NullHost` | `default_device` para cada papel | o mesmo dispositivo virtual em todos | `null_host.rs::one_virtual_device_for_every_role` |
| OUTPUT-BK-AC-04 | um `NullHost` com entradas próprias | `capabilities` por modo | cada modo devolve só as suas entradas | `null_host.rs::capabilities_are_per_mode_and_configurable` |
| OUTPUT-BK-AC-05 | um pedido de buffer | `open` | o período vem do pedido | `null_host.rs::period_comes_from_the_buffer_request` |
| OUTPUT-BK-AC-06 | um `DeviceId` desconhecido | `open` | `DeviceNotFound` | `null_host.rs::unknown_device_id_is_not_found` |
| OUTPUT-BK-AC-07 | uma entrada DoP anunciada | `open` com `Dop` | abre | `null_host.rs::dop_opens_on_an_advertised_dop_entry` |
| OUTPUT-BK-AC-08 | o relógio real | abrir, tocar e parar | entrega períodos sem erro | `null_host.rs::real_clock_smoke` |
| OUTPUT-BK-AC-09 | um `VirtualClockHandle` | abrir um segundo stream com o primeiro ainda ativo | `InvalidRequest { what: "virtual clock already drives a stream" }`; depois de parar o primeiro, o novo abre e recebe os ticks | `stream.rs::a_virtual_clock_drives_one_stream_at_a_time` |
| OUTPUT-BK-AC-10 | um `CaptureHost` cujo diretório não existe | `open` | `Error::Os { backend: Capture, .. }` | `capture.rs::unwritable_path_is_an_os_error` |
| OUTPUT-BK-AC-11 | a fachada | `hosts()` | só `Null` | `facade.rs::hosts_lists_null_only_where_no_os_backend_exists` |
| OUTPUT-BK-AC-12 | a fachada | `default_host()` | `Null`, com `HostSelection { chosen: Null, skipped: [] }` | `facade.rs::default_host_is_the_first_listed_and_says_so` |
| OUTPUT-BK-AC-13 | `docs/support-matrix.md` commitado | gerar com `support_matrix_markdown()` | o texto é idêntico | `support_matrix.rs::committed_matrix_matches_generator` |
| OUTPUT-BK-AC-14 | o alocador contador do testkit | alocar nesta thread / não alocar / alocar em outra thread | conta 1 / conta 0 / conta 0 aqui | `counting_alloc.rs::{counts_an_allocation_on_this_thread, no_allocation_is_counted_without_allocating, allocations_on_another_thread_are_not_counted_here}` |
| OUTPUT-BK-AC-15 | um DAC USB ligado a um Linux x86_64 nativo com ALSA hw disponível (Camada 2) e PipeWire ativo na sessão, anunciando 24 bits válidos em 96 kHz e em 192 kHz | o cliente abre `ShareMode::Exclusive` pelo host `alsa`, uma vez a 96 000 Hz e outra a 192 000 Hz, e lê `stream.config()` e `config.path.os_mixer_bypassed`; em seguida a mesma abertura é tentada pelo host `pipewire`, e o modo efetivo devolvido é lido no `StreamConfig` | pelo ALSA o DAC negocia 24/96 e 24/192 sem mixer; pelo PipeWire o stream abre no modo que o servidor de fato conceder, e `config.mode` relata esse modo em vez de repetir o pedido (TASK-E12-01, TASK-E14-04-AC-2) | `alsa_exclusive.rs::dac_negotiates_24_96_and_24_192`, `pipewire.rs::negotiated_mode_is_reported_not_assumed` (`#[ignore]`, hardware) |
| OUTPUT-BK-AC-16 | um DAC USB ligado a um Mac real — uma máquina Intel x86_64 e uma Apple Silicon arm64, cada uma com a slice nativa — com o host `coreaudio` presente | o cliente abre `ShareMode::Exclusive` com o formato físico do DAC pedido em `SampleFormat::I24Packed`, uma vez a 96 000 Hz e outra a 192 000 Hz, com hog mode e taxa de dispositivo solicitados quando o hardware permitir, e lê `stream.config()`, `config.valid_bits` e `config.path.os_mixer_bypassed` nas duas arquiteturas | nas duas arquiteturas o CoreAudio abre o DAC em 24/96 e 24/192 e relata se o mixer do OS ficou fora do caminho, sem presumir bypass onde o agregado ou o default do sistema interpõe processamento (TASK-E12-02-AC-1, TASK-E14-04-AC-1) | `coreaudio_exclusive.rs::dac_negotiates_24_96_and_24_192_on_both_arches` (`#[ignore]`, hardware, `sysctl.proc_translated = 0`) |
| OUTPUT-BK-AC-17 | um DAC USB ligado por adaptador a um aparelho Android arm64 real (Android 10 ou superior, sem emulador), com o host `aaudio` roteando o stream por USB pela rota do sistema | o cliente abre o stream pedindo `ShareMode::Exclusive` com `SampleFormat::I24Packed`, uma vez a 96 000 Hz e outra a 192 000 Hz, e lê `stream.config()`, o sharing efetivo devolvido pelo AAudio e `config.valid_bits`; a rota de saída é lida no `AudioManager` do sistema | o stream toca pela rota USB nos formatos que OS, ROM e DAC concederem, e `config.mode` relata o sharing efetivo: `Exclusive` só aparece quando o aparelho realmente concedeu, nunca porque foi pedido (TASK-E13-01-AC-1, TASK-E13-10, TASK-E14-04-AC-2) | `aaudio_exclusive.rs::usb_dac_negotiates_24_96_and_24_192_or_reports_shared` (`#[ignore]`, hardware) |
| OUTPUT-BK-AC-18 | um DAC USB ligado por adaptador a um aparelho iOS arm64 real (não simulador), com o host CoreAudio do alvo `aarch64-apple-ios` e a `AVAudioSession` configurada pela categoria de reprodução | o cliente abre o stream com `SampleFormat::I24Packed`, uma vez a 96 000 Hz e outra a 192 000 Hz, e lê `stream.config()`, a `sampleRate` que a sessão de áudio de fato concedeu e `config.path.os_mixer_bypassed` | a sessão relata a taxa que concedeu e o stream abre nela; quando a taxa concedida difere da pedida, `config.rate` mostra a concedida e nenhum resample silencioso do app é alegado como bit-perfect (TASK-E20-01-AC-3) | `coreaudio_ios.rs::usb_dac_negotiates_24_96_and_24_192_or_reports_session_rate` (`#[ignore]`, hardware) |
| OUTPUT-BK-AC-19 | um DAC USB removido com o stream em `Running`, em cada um dos cinco alvos de `OUTPUT-AC-16` e `OUTPUT-BK-AC-15` a `OUTPUT-BK-AC-18`, com o alto-falante interno do sistema exposto como default | o cabo é removido e o cliente lê o `StreamState` final e a lista de dispositivos que o host tentou abrir ou abriu | nenhum alvo cai para o alto-falante default por conta própria; a perda chega como `DeviceLost` e a recuperação é `resume` explícito depois que o dispositivo volta (TASK-E04-05-AC-3) | `device_lost_is_not_a_fallback` (`#[ignore]`, hardware, uma variante por alvo) |

> **NOTA DE CONFLITO — `OUTPUT-BK-AC-11` e `OUTPUT-BK-AC-12` não são
> satisfeitos no Windows, e o mantenedor precisa ratificar qual texto vale.**
> As duas linhas acima estão no texto aprovado em 2026-11-15 e foram restauradas
> literalmente: `hosts()` devolve só `Null` e `default_host()` devolve `Null`
> com `skipped: []`. `OUTPUT-AC-16`, aprovada depois no mesmo dia e mais
> específica, exige o contrário no Windows: o host WASAPI já está na lista de
> `hosts()`. As duas não podem valer juntas nesse alvo. A implementação segue a
> AC-16 no Windows (WASAPI antes de `Null`) e a AC-11/AC-12 em todos os outros
> alvos, e os testes afirmam exatamente isso — `facade.rs` e
> `hosts_preference.rs` —, sem reescrever o `Then` aprovado para caber no
> código. A decisão de qual critério prevalece, e a edição correspondente nesta
> tabela, são do mantenedor; até lá esta nota é a única alteração aqui.

> **`OUTPUT-BK-AC-15` a `OUTPUT-BK-AC-19` foram autorados em 2026-11-15 e
> aprovados pelo mantenedor no mesmo dia**, com uma extensão de escopo explícita:
> os critérios de DAC valem para Windows, Linux, macOS, Android **e** iOS, não só
> para o WASAPI. As cinco linhas acima cobrem as quatro plataformas que não o
> Windows; o critério do Windows é `OUTPUT-AC-16` em `specs/output.md` e **não** é
> repetido aqui — duas cópias do mesmo `Then` em duas specs divergem na primeira
> edição. A matriz inteira é `OUTPUT-AC-16` mais `OUTPUT-BK-AC-15` a `AC-18`.
>
> Os cinco critérios medem a mesma coisa — 24/96 e 24/192 negociados num DAC USB
> pela rota nativa de cada OS — e todos exigem hardware real. Nenhum roda neste
> ambiente, e nenhum teste correspondente existe: os backends `wasapi`, `alsa`,
> `pipewire`, `coreaudio` e `aaudio` estão em `-` nas seis colunas de
> `docs/support-matrix.md`, porque `support()` devolve `None` para os cinco.
>
> O que **não** está prometido aqui, e a diferença importa: `Exclusive` é pedido,
> não garantido. Windows/WASAPI e Linux/ALSA têm caminho exclusivo conhecido;
> Linux/PipeWire, macOS/CoreAudio e ambos os móveis relatam o modo e a taxa que o
> sistema **concedeu** — o `Then` de cada linha exige o relato fiel do modo
> efetivo, não a obtenção do exclusivo. É a mesma regra que
> `reference/audio/05-platform-backends.md` fixa: "transparência bit a bit é
> teste por conjunto, não consequência de pedir flag de exclusive".
>
> **Divisão de dono, para não duplicar entrega.** O host mora neste módulo, mas o
> backend de cada OS não nasce nesta TASK: `TASK-E04-05` entrega o WASAPI
> event-exclusive e declara explicitamente no Boundary que **não** implementa
> ALSA, PipeWire, Pulse, CoreAudio nem AAudio. Linux nativo é `TASK-E12-01`,
> macOS Intel/Silicon é `TASK-E12-02`, Android é `TASK-E13-01`/`TASK-E13-10` e iOS
> é `TASK-E20-01`. As cinco linhas acima são o critério que cada uma dessas TASKs
> mede no alvo real; a linha `AC-19` (unplug sem fallback) existe porque nenhuma
> das TASKs de plataforma nomeava esse caso fora do Windows.

### Critérios que esta spec **não** fixa

- Comportamento de cada backend do OS além dos cinco critérios acima: só será
  fixado quando o backend existir, com teste de integração no alvo. Os critérios
  de recuperação a busy e sleep por backend continuam sem linha própria — a
  `OUTPUT-AC-17` de `specs/output.md` cobre o caso no Windows, e cada backend novo
  fixa o seu quando existir.
- Ordem de preferência entre backends de uma mesma plataforma (por exemplo,
  PipeWire antes de Pulse): decisão da TASK E04-06.

## 6. Erros e cancelamento

| Situação | Erro | Observação |
|---|---|---|
| enumeração passou do prazo | `Error::Timeout` | a thread fica abandonada e contada, nunca morta |
| a thread de enumeração não pôde ser criada | `Error::Os { call: "std::thread::Builder::spawn", .. }` | |
| `enumerate` entrou em panic | `Error::Os { code: -1, call: "Host::enumerate panicked" }` | |
| dispositivo desconhecido | `Error::DeviceNotFound` | |
| `VirtualClockHandle` já dirige outro stream | `Error::InvalidRequest { what: "virtual clock already drives a stream" }` | um stream por handle |
| arquivo de captura ou manifesto não pode ser escrito | `Error::Os` | |
| backend do OS ausente nesta build | não aparece em `hosts()`; `support` devolve `None` | não é erro em tempo de execução |

Os demais erros (`FormatUnsupported`, `ExclusiveUnavailable`, `DeviceLost` etc.)
são do contrato de stream e estão em `specs/output.md` §6. Cancelamento é
explícito: `Stream::stop`/`drop`; não existe timeout escondido além do de
`enumerate_with_timeout`, que o chamador escolhe.

## 7. Fora de escopo

Selecao de faixa, volume de aplicacao, EQ; casting e AirPlay sao do player (E12-09)

## 8. Dependências

Depende de: `vynamp-dsp` (opcional, feature `resample`)

É dependido por: engine

## 9. Testes e fixtures

`cargo test --workspace --all-features` na raiz de `libs/output`.

| Arquivo | Cobre |
|---|---|
| `crates/vynamp-output-core/tests/enumerate_timeout.rs` | prazo de enumeração |
| `crates/vynamp-output-core/tests/null_host.rs` | dispositivo virtual, capacidades, período, DoP, relógio real |
| `crates/vynamp-output-core/tests/stream.rs` | relógio virtual (um stream por vez) |
| `crates/vynamp-output-core/tests/capture.rs` | gravação bit a bit e erros de arquivo |
| `crates/vynamp-output/tests/facade.rs` | `hosts` e `default_host` |
| `crates/vynamp-output/tests/support_matrix.rs` | matriz gerada = matriz commitada |
| `crates/vynamp-output-testkit/tests/counting_alloc.rs` | alocador contador usado nas provas de zero alocação |

Nenhum teste depende de hardware nem de arquivo grande. O oráculo do Capture é
o próprio buffer gerado pelo teste, comparado byte a byte.

## 10. Limitações conhecidas

| Limitação | Consequência | Quem cobre |
|---|---|---|
| Só os hosts `Null` e `Capture` existem | o player ainda não toca em dispositivo real | E04-05 (WASAPI), E04-06 (demais) |
| Todo par (host, feature) está em `LibraryVerified` | a matriz não afirma nada sobre hardware | `docs/hardware-matrix.md`, quando houver backend |
| `default_host()` sempre devolve `Null` com `skipped` vazio | `HostSelection` ainda não informa nada útil | E04-05/E04-06 |
| Thread de enumeração abandonada vive até o processo acabar | um driver travado segura uma thread e uma referência ao host | aceito por desenho; contada em `abandoned_enumeration_threads` |
| Sem perfis por dispositivo | a mesma política vale para todo dispositivo | E04-09 |
| Sem Bluetooth | — | E17-19 |
