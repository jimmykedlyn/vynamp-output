# Spec — vynamp-output · backends

| Campo | Valor |
|---|---|
| Módulo | `vynamp-output` |
| Crate(s) | `vynamp-output-core` (trait `Host`, `NullHost`, `CaptureHost`, `support`), `vynamp-output` (fachada: `hosts`, `default_host`, `support_matrix_markdown`) |
| Tipo | lib |
| Versão da spec | 0.2.0 |
| Data | 2026-11-11 |
| Estado | **rascunho autorado, não aprovado** — §3 a §10 escritos a partir do código e dos testes existentes; a aprovação é do mantenedor, por `GOVERNANCE.md` |
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
| OUTPUT-BK-REQ-09 | Backend WASAPI (compartilhado e exclusivo). — planned (M3) | REQ de saída bit-perfect | E04-05 | — |
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
| OUTPUT-BK-AC-11 | a fachada | `hosts()` | só `Null` | `facade.rs::hosts_lists_null_only` |
| OUTPUT-BK-AC-12 | a fachada | `default_host()` | `Null`, com `HostSelection { chosen: Null, skipped: [] }` | `facade.rs::default_host_is_null_and_says_so` |
| OUTPUT-BK-AC-13 | `docs/support-matrix.md` commitado | gerar com `support_matrix_markdown()` | o texto é idêntico | `support_matrix.rs::committed_matrix_matches_generator` |
| OUTPUT-BK-AC-14 | o alocador contador do testkit | alocar nesta thread / não alocar / alocar em outra thread | conta 1 / conta 0 / conta 0 aqui | `counting_alloc.rs::{counts_an_allocation_on_this_thread, no_allocation_is_counted_without_allocating, allocations_on_another_thread_are_not_counted_here}` |

### Critérios que esta spec **não** fixa

- Comportamento de cada backend do OS: só será fixado quando o backend existir,
  com teste de integração no alvo.
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
