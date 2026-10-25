# vynamp-output — biblioteca própria de saída de áudio (design)

Data: 2026-10-23 · Autor: Claude (dono único do Vynamp) · Status: aprovado pelo mantenedor em 2026-10-25. Decisões do mantenedor (§1) e chamadas de Claude 1–5 (§5) são vinculantes; o que Claude considera arriscado nelas está em §10, sem alterá-las.

Local provisório: `D:\Projects\vynamp-media-player\design-vynamp-output.md`; destino: `docs/superpowers/specs/2026-10-23-vynamp-output-design.md` no repositório `jimmykedlyn/vynamp-output`.

## 1. Intenção

O spike E01-02 do Vynamp (PR #14, worktree `repo/.worktrees/claude/task-e01-02-callback`) toca em WASAPI shared pelo crate de terceiros CPAL 0.18.2, atrás da feature `cpal` de `crates/vynamp-output`. O mantenedor quer, como fez com a decodificação (`vynamp-decode`), uma biblioteca **própria** de saída de áudio: enumeração de dispositivos, abertura/início/parada de streams, callback de tempo real, modos exclusivo/bit-perfect, DSD via DoP, hot-plug e relatório de latência/posição. Sem bibliotecas de terceiros, documentada, num repositório privado separado (`jimmykedlyn/vynamp-output`), pronta para ser aberta depois sob MIT OR Apache-2.0.

Nome simples e descritivo (decisão do mantenedor): `vynamp-output`.

Fontes de produto que este design precisa satisfazer (todas em `D:\Projects\vynamp-media-player\repo\`):

| Fonte | O que exige daqui |
|---|---|
| `docs/audio/04-outputs-hires.md` | Perfis por stable ID; policies follow system/follow source/fixed; taxa/formato reais; exclusive busy sem fallback silencioso; Bluetooth sem presumir codec; DAC removido → pausa/flush; default changed só migra se autorizado; sleep/resume sem duplicar; valid bits ≠ container; "bit-perfect verificado" só com captura |
| `docs/audio/05-platform-backends.md` | Backend nativo em todos os alvos desde M1; enumeração por API do OS; clock/drain/error; route changes por OS; volume hardware ≠ digital; nenhum backend corta fim de faixa, toca buffer antigo ou sobe volume ao trocar rota |
| `docs/audio/01-engine.md` | Callback sem mutex/heap/I/O/log; posição apresentada = frames consumidos menos latência do dispositivo; PCM32 inteiro no modo transparente; troca de taxa em exclusivo pode quebrar gapless e deve ser mostrada; callback p99.9 < 25% do período |
| `docs/audio/07-dsp-signal-contract.md` | Nada de ganho/resampling depois da proteção final; dither/conversão são os únicos estágios posteriores e pertencem ao Vynamp; transparente bypassa tudo; rate change latency entra no clock |
| `docs/platforms/01-support-matrix.md`, `02-android.md` | Windows 11 x64, Linux x86_64 (Ubuntu 22.04+/Fedora), macOS 13+ Intel e Silicon, Android API 29+ arm64; Android: sem JNI/alloc/log no callback, AudioManager/rotas/focus ficam na ponte do app |
| `tasks/items/TASK-E01-02.md`, `TASK-E04-05.md`, `TASK-E04-06.md`, `TASK-E04-10.md`, `TASK-E01-09.md`, `TASK-E12-02.md`, `TASK-E12-04.md`, `TASK-E13-01.md`, `TASK-E13-10.md`, `TASK-E14-04.md`, `TASK-E17-18.md` | Callback/clock (E01-02); WASAPI event-exclusive, stable IDs, capabilities, busy/unplug/sleep com pausa, sem fallback para speaker (E04-05); captura PCM24/32 exata (E04-06); matriz de hardware 100/192k/rate switch (E04-10); DSD→PCM não rotulado native (E01-09); CoreAudio rate/device changes em Intel e Silicon (E12-02); DoP/native DSD/768 kHz, equipamento sem suporte falha sem conversão oculta (E12-04); AAudio sem JNI/alloc (E13-01); flag AAudio não é prova de bit-perfect (E13-10); exclusive não presumido (E14-04); DSD raw/DoP distinguíveis (E17-18) |
| `docs/evidence/PR/task-e01-02/README.md` (na worktree do spike) | Métricas de referência e o viés conhecido do relógio com silêncio enfileirado (§5.7 resolve) |

## 2. Objetivos

1. Uma API única e segura para tocar áudio em Windows, Linux, macOS (Intel e Silicon) e Android, cobrindo shared e exclusivo/direto, com camadas entregáveis (§6).
2. Zero dependências de crates.io (crates publicados **e** dev-dependencies, como na `vynamp-decode`). Só `std` e as bibliotecas de áudio do próprio sistema operacional, declaradas por FFI escrita por nós.
3. Contrato de tempo real verificável: a callback não aloca, não trava, não bloqueia, não faz I/O/log e não propaga panic (§5.5).
4. **Nunca cair silenciosamente**: o que foi pedido e não pôde ser honrado vira erro ou evento explícito de downgrade; o stream sempre informa o modo/formato **real** (§5.6).
5. Tempo exato: posição apresentada derivada do relógio do dispositivo, com latência e instante monotônico, no formato que o relógio do Vynamp consome (§5.7).
6. Melhor que o CPAL em pontos verificáveis: exclusivo/hog/hw direto de primeira classe, valid bits vs container, DoP, timestamps de apresentação em todos os backends, IDs estáveis com nível de estabilidade declarado, hot-plug por eventos, build Linux sem headers de desenvolvimento (carregamento dinâmico).
7. Testável sem hardware no CI (backends null e captura em arquivo), com integração real nos agentes Woodpecker Windows/Linux e matriz manual documentada.
8. Documentação completa para terceiros (§8).

## 3. Não-objetivos

- Captura/gravação como API pública (só um uso interno de teste, §5.10).
- Decodificação, resampling, DSP, ganho, dither, mixer, fila, gapless: pertencem à `vynamp-decode` e ao Vynamp (`07-dsp-signal-contract.md`). A biblioteca não altera amostras, exceto conversões de tipo explicitamente pedidas (§5.6) e o empacotamento DoP (§5.9).
- Audio focus, MediaSession, notificações, rotas Android via AudioManager: ficam na ponte Kotlin do app (`02-android.md`); a biblioteca recebe IDs/rotas de fora.
- ASIO no núcleo aberto (ver §5.4). Driver USB próprio no Android (`05-platform-backends.md` exige spike e requisito novo).
- Passthrough codificado (AC-3/DTS bitstream): M5 e fora desta biblioteca até haver requisito.
- `no_std`; MIDI; JACK nativo (coberto pelo PipeWire); Windows/Linux ARM64 e Android 32-bit (expansão separada, `01-support-matrix.md`).

## 4. Anti-objetivos (falha mesmo que "funcione")

- Downgrade silencioso: abrir shared quando pediram exclusivo, resampling do OS ligado implicitamente (`AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM`, plugins `plug`/`default` do ALSA quando pediram `hw`), mudar de dispositivo sozinho após unplug ou após mudança de default.
- Declarar "bit-perfect" por ter conseguido modo exclusivo. A biblioteca reporta propriedades do caminho (`os_mixer_bypassed`, `library_conversion`, `endpoint_volume`), nunca "bit-perfect verificado" (`04-outputs-hires.md`).
- Tocar buffers antigos após stop/seek/resume, cortar o fim de um stream drenado, ou aumentar volume ao trocar rota.
- Copiar ou traduzir código do CPAL, RtAudio, PortAudio, Oboe, miniaudio, SDKs ou headers do sistema. Declarações FFI escritas a partir da documentação pública da API; fontes consultadas registradas em `docs/sources/<backend>.md`. Projetos permissivos podem ser estudados como referência de arquitetura e de quirks, com crédito.
- Panic, aborto ou deadlock causados por driver lento, dispositivo removido ou servidor de áudio reiniciado.

## 5. Decisões de arquitetura

### 5.1 Linguagem e regras
- Rust estável, mesma toolchain do Vynamp (1.99 no spike), edition 2024.
- **Chamada 2 (vinculante):** `unsafe` só nos crates `*-sys` por plataforma (§5.3), cada bloco com comentário `// SAFETY:` que cite a regra da API (ponteiro válido, tempo de vida, thread, contagem COM). Esses crates usam `#![deny(unsafe_op_in_unsafe_fn)]` e `#![warn(clippy::undocumented_unsafe_blocks)]` como erro no CI. Todos os outros crates — API pública, núcleo, lógica de backend, ferramentas — têm `#![forbid(unsafe_code)]`. Toda mudança em crate `*-sys` exige revisão de um subagente Opus de contexto limpo, registrada no PR.
- Os crates `*-sys` expõem **wrappers seguros mínimos** (ex.: `AudioClient::initialize(&self, ...) -> Result<…>`): os invariantes ficam provados lá dentro, uma vez. Lógica de negociação, estados e eventos fica nos crates seguros.
- Layout das structs FFI verificado por ferramenta de dev (`tools/layout-check`): gera um programa C com `sizeof/offsetof` a partir dos headers do sistema no agente (MSVC + Windows SDK; gcc + headers ALSA/PipeWire/Pulse instalados só no agente de CI) e compara com `size_of`/`offset_of!` do Rust. Compiladores e headers são ferramentas do agente, não dependências.
- Ferramentas permitidas fora dos crates publicados: compilador C para layout-check, NDK para build Android, `snd-aloop` do kernel, FFmpeg/sox para gerar fixtures. Nenhuma entra como dependência Cargo.

### 5.2 Camadas da API
```
Host (backend do OS) → enumerate() → DeviceInfo → open(StreamRequest, RenderSource) → Stream
                         ↘ EventReceiver (hot-plug, default changed, device lost, downgrade, xrun, panic)
Stream: start / pause / resume / drain(timeout) / stop · config() real · timing() · counters()
```
- `HostId`: `Wasapi`, `Alsa`, `PipeWire`, `Pulse`, `CoreAudio`, `AAudio`, `Null`, `Capture`. `Host::default_for_platform()` escolhe por disponibilidade (Linux: PipeWire → Pulse → ALSA; ver `05-platform-backends.md`) e **informa** a escolha e o motivo (`selection: HostSelection { chosen, skipped: [(HostId, reason)] }`).
- `StreamRequest`: dispositivo (`DeviceId` ou `Default(Role)`), `ShareMode::{Shared, Exclusive}`, `SampleFormat`, `rate`, `channels` + `ChannelLayout`, `buffer` (frames ou duração; `None` = padrão do backend), `on_unavailable: {Error (padrão), Downgrade}`, `conversion: {Forbid (padrão), Allow(ConversionSpec)}`, `allow_rate_change_of_device: bool` (macOS/ALSA hw; padrão false).
- `SampleFormat`: `F32`, `F64` (onde o OS aceita), `I16`, `I24Packed` (3 bytes), `I24In32 { justify }`, `I32 { valid_bits }`, `U8`, `Dop { dsd_rate }`, `DsdNative { layout }` (camada 4). Container e valid bits sempre separados (`04-outputs-hires.md`).
- `StreamConfig` (o real, depois de negociar): host, device, modo, formato de container, valid bits, taxa, canais e mapa de canais, buffer/período, `PathProperties { os_mixer_bypassed: Yes|No|Unknown, os_resampling: bool, library_conversion: Option<ConversionSpec>, endpoint_volume: Unity|NotUnity(f32)|Unknown, transport: Usb|Bluetooth|Hdmi|BuiltIn|Virtual|Unknown, rt_priority: Granted(kind)|Denied(reason) }`.
- Erros estruturados, sem `String` como único conteúdo: `LibraryUnavailable { lib }`, `NoBackend`, `DeviceNotFound`, `DeviceBusy`, `ExclusiveUnavailable { reason }`, `FormatUnsupported { nearest: Option<StreamConfig> }`, `PermissionDenied`, `DeviceLost`, `Timeout`, `InvalidRequest`, `Os { backend, code, call: &'static str }`.
- Eventos: `DeviceAdded/Removed`, `DefaultChanged { role }`, `DeviceLost { stream }`, `SharedFormatChanged` (usuário mudou o mix format), `Downgraded { requested, actual, reason }`, `OsXrun { count }` (reportado pelo OS) separado de `SourceUnderrun` (fonte não tinha dados), `SourcePanicked`, `RtPriorityDenied`. Produzidos só por threads não-RT (thread de notificação do OS ou watcher da biblioteca, que converte flags atômicas da callback em eventos). Canal: `std::sync::mpsc` (fora da RT é permitido bloquear).

### 5.3 Crates
- `vynamp-output-core` (seguro): tipos, erros, eventos, trait `RenderSource`, lógica de negociação e de downgrade, máquina de estados do stream, empacotador DoP, conversões explícitas de sample, matemática de tempo, flags/contadores atômicos RT, backends `Null` e `Capture`.
- Crates FFI (`unsafe` permitido): `vynamp-output-sys-loader` (dlopen/dlsym e LoadLibraryW/GetProcAddress declarados por nós; relógio monotônico do OS: QPC, `clock_gettime(CLOCK_MONOTONIC)`, `mach_absolute_time`; prioridade de thread), `vynamp-output-sys-windows` (COM, MMDevice, WASAPI, IAudioClock, IMMNotificationClient implementado em Rust via vtable própria, avrt/MMCSS, propsys), `vynamp-output-sys-alsa`, `vynamp-output-sys-pipewire` (inclui codificação SPA POD própria, já que a API C é feita de macros/inline), `vynamp-output-sys-pulse`, `vynamp-output-sys-coreaudio` (HAL AudioObject + AudioUnit HAL output), `vynamp-output-sys-aaudio`.
- Backends seguros (`#![forbid(unsafe_code)]`): `vynamp-output-wasapi`, `vynamp-output-alsa`, `vynamp-output-pipewire`, `vynamp-output-pulse`, `vynamp-output-coreaudio`, `vynamp-output-aaudio`.
- Fachada `vynamp-output` com features por backend; padrão por `target_os` (Windows: `wasapi`; Linux: `alsa`, `pipewire`, `pulse`; macOS: `coreaudio`; Android: `aaudio`; todos: `null`, `capture`).
- Dev: `vynamp-output-testkit` (allocator de contagem por thread, como `vynamp-audio/src/rt.rs` do spike; fontes de rampa/oráculo; harness), `tools/output-probe` (sucessor do `output_probe` do spike), `tools/capture-verify`, `tools/layout-check`.

### 5.4 Backends por plataforma (chamada 1, vinculante)
- **Carregamento:** Linux carrega `libasound.so.2`, `libpipewire-0.3.so.0`, `libpulse.so.0` com dlopen escrito por nós; biblioteca ausente → `LibraryUnavailable` e o host é pulado com motivo. Assim o build Linux não precisa de `-dev` headers (o spike não compilava o adapter no CI por falta de headers ALSA). Windows e macOS ligam direto a componentes que sempre existem nos baselines (ole32/mmdevapi via COM; frameworks CoreAudio/AudioToolbox/CoreFoundation); `avrt.dll` é carregado dinamicamente (MMCSS opcional). Android liga `libaaudio.so` (sempre presente em API 29+), com símbolos de API > 29 resolvidos por dlsym.
- **Windows WASAPI:** shared event-driven no mix format do endpoint (sem `AUTOCONVERTPCM`; taxa diferente da fonte → erro ou conversão pedida e reportada); exclusive event-driven com `WAVEFORMATEXTENSIBLE` (sondagem de `IsFormatSupported` sobre lista de candidatos, alinhamento de período com `AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED` tratado), thread própria com MMCSS "Pro Audio", `IAudioClock::GetPosition` com QPC para timing, `IMMNotificationClient` para hot-plug/default, `AUDCLNT_E_DEVICE_INVALIDATED` → `DeviceLost`. `AUDCLNT_E_DEVICE_IN_USE` → `DeviceBusy`, nunca shared implícito.
- **Linux ALSA (primeira classe):** `hw:CARD=<id>,DEV=<n>` para direto/exclusivo (sem plugins; `SND_PCM_NO_AUTO_*` aplicados), `default`/nomes de plugin para shared; thread própria com `poll` nos descritores do PCM; `SCHED_FIFO` tentado via `pthread_setschedparam`, recusa reportada (rtkit fora do escopo inicial); `snd_pcm_status` com `audio_htstamp` para timing; xrun → `snd_pcm_prepare` + `OsXrun`; hot-plug por inotify em `/dev/snd` + reenumeração (camada 2), substituído por eventos do PipeWire quando ele está ativo.
- **Linux PipeWire nativo** (camada 3): `pw_thread_loop` + `pw_stream` (thread RT do próprio PipeWire), formato por SPA POD, `pw_stream_get_time_n` para timing, registry para hot-plug; "exclusivo" = node com `node.exclusive`/rate request reportado como `os_mixer_bypassed: Unknown` salvo prova. **PulseAudio** (fallback quando PipeWire ausente): `pa_threaded_mainloop` + `pa_stream` com timing info; sempre shared.
- **macOS CoreAudio:** AudioUnit HAL output no dispositivo escolhido; timing por `AudioTimeStamp` + `kAudioDevicePropertyLatency` + `kAudioStreamPropertyLatency` + safety offset + buffer; hot-plug e default por `AudioObjectAddPropertyListener`. Exclusivo = hog mode (`kAudioDevicePropertyHogMode`) + `kAudioStreamPropertyPhysicalFormat` inteiro onde o driver aceita ("integer mode" só onde suportado; ausência reportada, nunca presumida). Mudança de `kAudioDevicePropertyNominalSampleRate` só com `allow_rate_change_of_device` e restaurada ao fechar. Aggregate devices enumerados e marcados.
- **Android AAudio:** builder com `SHARED` (padrão) ou `EXCLUSIVE` pedido; o que foi obtido é lido de volta (`AAudioStream_getSharingMode`, formato, taxa, canais, performance mode) e reportado; data callback; error callback `DISCONNECTED` → evento, reabertura fora da callback; `AAudioStream_getTimestamp(CLOCK_MONOTONIC)`. Device IDs vêm da ponte (AudioManager) e são marcados `IdStability::Session`. **OpenSL ES não é implementado**: o baseline API 29 (`01-support-matrix.md`) sempre tem AAudio; a spec não exige OpenSL. Oboe (C++ de terceiros) não é usado; quirks conhecidos são reimplementados a partir de documentação, com crédito.
- **ASIO excluído do núcleo aberto:** o ASIO SDK é distribuído pela Steinberg sob licença própria (e, conforme anúncio recente a confirmar, também GPLv3), ambas incompatíveis com um núcleo MIT OR Apache-2.0 sem a escolha de licença do distribuidor; "ASIO" é marca registrada; declarar a interface sem o SDK continua derivado dele. No máximo um adapter privado opcional depois, fora deste repositório, ligado só pela trait pública de backend. `TASK-E04-05` cita "ASIO adapter opcional": continua opcional; a ausência não bloqueia o aceite (ver §10).

### 5.5 Contrato de tempo real (chamada 3, vinculante)
```rust
pub trait RenderSource: Send + 'static {
    /// Chamado na thread de áudio. Não aloca, não trava, não bloqueia, não faz I/O/log.
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext) -> Rendered;
}
```
- `DeviceBuffer` é um enum de slices tipadas intercaladas no formato **real** do stream (`F32(&mut [f32])`, `I32(&mut [i32])`, `I24Packed(&mut [[u8; 3]])`, `Dsd(&mut [u8])`, …). `Rendered { frames }`: a biblioteca preenche o restante com o **silêncio correto do formato** (zero PCM; padrão idle DSD `0x69` com marcadores DoP contínuos) e conta `SourceUnderrun`. Nunca repete amostras.
- `RenderContext`: `frames`, `device_frame_index` (contador monotônico de **todos** os frames entregues ao dispositivo, conteúdo + silêncio), `timing: Option<PresentationTiming>` (§5.7), `config` (referência imutável ao `StreamConfig`).
- Tudo que a callback toca é alocado em `open()`. A callback da biblioteca só usa atômicos, slices e o scratch pré-alocado; trampolins `extern "C"` ficam nos crates `-sys`.
- **Panic:** o trampolim envolve a chamada em `catch_unwind(AssertUnwindSafe(..))`; em panic, a callback entrega silêncio dali em diante, seta `source_panicked`, e o watcher emite `SourcePanicked`. Limites documentados: com `panic = "abort"` não há captura; o panic hook global (que imprime e aloca) é escolha do aplicativo — a biblioteca não o substitui; o Vynamp deve instalar um hook que não faça I/O na thread de áudio.
- **Compatível com o spike:** `vynamp-audio::callback::Renderer::render(&mut [f32], latency_frames)` é adaptado por um `RenderSource` de ~20 linhas no Vynamp (`F32` direto; `latency_frames` vem de `ctx.timing`). O ring do spike é `f32`; o modo transparente (PCM24/32 inteiro, `01-engine.md`) precisa de um ring inteiro no Vynamp — a biblioteca já aceita `I32/I24`, a mudança do ring é TASK do Vynamp (E04-06).
- Gates: zero eventos de heap em seção RT (allocator de contagem por thread) em null/capture no CI e em cada backend real executado; guard estático de tokens proibidos (como `tests/rt_source_guard.rs` do spike) nos módulos RT; overhead da biblioteca na callback medido pelo probe (p99.9 do glue < 5% do período; o gate de produto < 25% em `01-engine.md` é do Vynamp inteiro).

### 5.6 Modos, negociação e "nunca cair silenciosamente" (chamada 4, vinculante)
- Negociação: a biblioteca tenta exatamente o pedido. Se não der e `on_unavailable = Error` (padrão) → erro com `nearest` quando o backend souber. Se `Downgrade` foi pedido explicitamente → abre o mais próximo **e** emite `Downgraded { requested, actual, reason }` antes do primeiro `start`. `config()` sempre devolve o real.
- Conversões de tipo (ex.: fonte `F32` → dispositivo `I24`) só com `conversion: Allow(spec)`; arredondamento e clamp determinísticos, **sem dither** (dither é último estágio do Vynamp, `07-dsp-signal-contract.md`), reportada em `library_conversion`. Resampling nunca é feito pela biblioteca; resampling do OS só se pedido e então `os_resampling: true`.
- Policies do produto (`04-outputs-hires.md`) mapeiam assim: follow system = `Shared` no mix format reportado; follow source = `Exclusive` na taxa da fonte; fixed rate = `Exclusive`/`Shared` com taxa fixa; "auto highest compatible" = consulta a `capabilities()` feita pelo Vynamp, não pela biblioteca.
- Troca de taxa em exclusivo: `Stream::reconfigure(rate)` fecha e reabre o cliente; retorna `Reopened { gap: true }` para o Vynamp mostrar quebra de gapless (`01-engine.md`).
- Default changed, unplug, sleep: a biblioteca **nunca** migra sozinha; emite evento, o stream fica em `Lost`, e o Vynamp decide (pausar, conservar posição, reconectar). Stop/stop-por-perda descartam os buffers do dispositivo (`Reset`/`snd_pcm_drop`/`AudioUnitReset`/`requestFlush`) para não tocar áudio antigo ao reabrir.
- Volume: a biblioteca nunca aplica ganho. Volume de endpoint/hardware é lido (camada 2) e pode ser controlado (camada 4) por API separada, sempre reportado como hardware, nunca usado automaticamente.
- Bluetooth/transporte: reportado quando o OS expõe (ex.: enumerador `BTHENUM` no Windows, transport type no CoreAudio); codec de transporte nunca é presumido.

### 5.7 Latência e posição apresentada
- `PresentationTiming { device_frame: u64, at: MonotonicNanos, latency_frames: u64, source: Reported|Estimated }`: "o frame `device_frame` do contador de `RenderContext` está saindo no conversor no instante `at`". `MonotonicNanos` usa o mesmo relógio que o OS usa nos timestamps (QPC, `CLOCK_MONOTONIC`, host time Mach), com conversão para `Instant` documentada.
- Fontes por backend: WASAPI `IAudioClock::GetPosition`+QPC e `GetStreamLatency`; ALSA `snd_pcm_status` (delay + htstamp); CoreAudio `AudioTimeStamp` + latências de dispositivo/stream/safety offset; PipeWire `pw_time` (delay/ticks/rate); Pulse timing info; AAudio `getTimestamp`. Sem timestamp do OS → `Estimated` a partir de buffer/período, marcado.
- `Stream::timing()` também disponível fora da callback (snapshot por seqlock de escritor único, padrão do `clock.rs` do spike).
- **Resolve o viés do spike:** como `device_frame_index` conta conteúdo **e** silêncio, o Vynamp mapeia "posição de conteúdo apresentada" sem subestimar após underrun/drenagem (limitação registrada em `docs/evidence/PR/task-e01-02/README.md`).
- `Stream::drain(timeout)`: para de pedir dados à fonte quando ela sinaliza fim, espera o `device_frame` apresentado alcançar o último frame de conteúdo e só então para — nenhum fim cortado (`05-platform-backends.md`).

### 5.8 Dispositivos, IDs estáveis e hot-plug
- `DeviceInfo`: `id: DeviceId` (string opaca) + `id_stability: Persistent|Session`, nome, host, papel default, transporte, `capabilities(mode)` → taxas/formatos/canais com marca `Advertised` vs `Probed`.
- IDs: WASAPI endpoint ID; CoreAudio `kAudioDevicePropertyDeviceUID`; ALSA `CARD=<id>,DEV=<n>` + USB id de `/proc/asound/cardN/usbid` quando houver; PipeWire `node.name`; AAudio: ID de sessão vindo da ponte. Índice numérico de enumeração nunca é ID (`04-outputs-hires.md`).
- Enumeração pode bloquear em driver ruim: `Host::enumerate()` é bloqueante e documentado; `enumerate_with_timeout(d)` roda numa thread auxiliar e devolve `Timeout` (a thread travada é abandonada e contada, nunca morta).
- Hot-plug por eventos (§5.2). Perfis por dispositivo são do Vynamp; a biblioteca fornece ID + capacidades.

### 5.9 DSD: DoP e nativo
- DoP (camada 4): fonte entrega bytes DSD (`DeviceBuffer::Dsd`); a biblioteca empacota 16 bits DSD por canal em amostras de 24 bits com marcador alternado `0x05`/`0xFA` no byte mais significativo, em 176,4/352,8/705,6 kHz para DSD64/128/256, mantendo a fase do marcador também no silêncio. Só abre em caminho com `os_mixer_bypassed: Yes`, `library_conversion: None` e volume de endpoint unity ou desconhecido-com-aviso; senão `ExclusiveUnavailable`/`FormatUnsupported` — nunca DoP por mixer (vira ruído).
- Nativo: ALSA `SND_PCM_FORMAT_DSD_U8/U16/U32_{LE,BE}` onde o driver anuncia. WASAPI e CoreAudio não têm DSD nativo (só DoP). Android: não suportado, erro.
- `StreamConfig` distingue `Dop` de `DsdNative` (`TASK-E17-18`); DSD→PCM é decimação na `vynamp-decode`/Vynamp e chega aqui como PCM comum (`TASK-E01-09`).

### 5.10 Verificação (chamada 5, vinculante)
Níveis por backend × recurso, publicados numa matriz gerada (`docs/support-matrix.md`):
1. `capture-verified`: bytes entregues comparados bit a bit com o esperado, num ponto que inclui o caminho do OS. Linux: backend ALSA `hw:` → `snd-aloop` (`hw:Loopback,0`) e captura em `hw:Loopback,1` por ferramenta de teste que usa o mesmo crate `-sys-alsa` (captura só para testes). Windows: loopback WASAPI prova só o caminho shared/mixer (`04-outputs-hires.md`: loopback shared não prova exclusivo).
2. `hardware-verified`: matriz manual (`docs/hardware-matrix.md`) com dispositivo, driver, OS, comando, commit e hash da captura; bit-perfect físico só com captura digital (ex.: S/PDIF para interface de captura) ou equivalente documentado — indicador do DAC não é prova.
3. `integration`: abre/toca/drena/perde dispositivo na pilha real do OS em agente Woodpecker (Windows nativo; Linux), com timing coerente e zero alocação; sem dispositivo de áudio no agente o teste é **"não executado"**, nunca "passou".
4. `library-verified`: backends `Null` (relógio real ou virtual determinístico) e `Capture` (grava exatamente o que a callback entregou, com manifesto de formato) — provam packing 24 bits, DoP, silêncio correto, underrun sem replay, drain sem corte, estados e eventos. Roda em todo CI.
5. `compiled`: `cargo check` do alvo + layout-check onde houver headers. macOS fica `compiled` = **"não verificado"** até existir agente Mac; Android tem build com NDK no agente Linux e emulador como integração parcial — aparelho arm64 real exigido para `hardware-verified` (`02-android.md`, `TASK-E13-10`).

Fuzzing não é central (entrada não é arquivo), mas parsers de dados do OS (PODs PipeWire, `/proc/asound`, propriedades CoreAudio) têm testes de propriedade com gerador próprio.

### 5.11 Integração com o Vynamp
- O spike E01-02 (PR #14) fica como está: CPAL atrás da feature `cpal`, métricas e evidências preservadas. A troca é uma TASK própria após a Camada 1.
- No Vynamp, o crate do spike chamado `crates/vynamp-output` colide com o nome da biblioteca. Na TASK de troca, o adapter fino (`RenderSource` sobre `Renderer`, conversões do `convert.rs`, `latency.rs` vira `ctx.timing`) passa para o módulo `vynamp_audio::output` e `crates/vynamp-output` deixa o workspace; a dependência vem por `git` + `rev` fixo do repositório privado. CPAL e seus 51 pacotes saem do `Cargo.lock`.
- Regressão: os testes do spike (`ring`, `clock`, `callback`, `harness`, `rt_alloc`, `rt_source_guard`) continuam verdes; o probe re-executado no mesmo hardware (Windows 11, USB DAC 384 kHz, WASAPI shared) precisa repetir os resultados do README do spike (0 eventos de heap, 0 regressões de relógio, underruns só no stall, taxa do relógio ≈1,000×) e mostrar `device_frame_index` corrigindo o viés.
- `TASK-E04-05/06/10`, `E12-02`, `E12-04`, `E13-01`, `E14-04` passam a consumir camadas 2–4; cada uma cita a camada e o nível de verificação da matriz.

## 6. Camadas de entrega
Ordem de dependência, não serialização estrita: depois de a API do core ser congelada (fim do core da Camada 1), backends de camadas diferentes podem andar em paralelo.

1. **Base:** core, API, eventos, `Null` + `Capture`, loader dinâmico, relógio monotônico; WASAPI shared event-driven; ALSA (`default` e `hw:` em modo simples, estéreo); CoreAudio dispositivo default; enumeração básica; timing em todos; probe. → troca do CPAL no Vynamp (Windows medido; Linux integration; macOS compiled).
2. **Exclusivo e dispositivos:** WASAPI exclusive event-driven, ALSA `hw` direto sem plugins, CoreAudio hog/physical format/rate de dispositivo; `capabilities()` sondadas; `reconfigure(rate)`; hot-plug/default changed/device lost em todos; IDs estáveis; mapas de canais e multicanal; leitura de volume de endpoint; `capture-verified` via `snd-aloop`. → base de `TASK-E04-05/06`.
3. **Servidores e móvel:** PipeWire nativo, PulseAudio fallback, seleção de host com motivo; AAudio shared/exclusive com reabertura em disconnect. → Linux M1 completo (`05-platform-backends.md` pede PipeWire/PulseAudio), `TASK-E13-01`.
4. **DSD e avançado:** DoP em WASAPI exclusive/ALSA hw/CoreAudio hog; DSD nativo ALSA; 384/768 kHz onde o hardware aceitar; controle de volume de hardware explícito; latência baixa avançada com contadores de xrun. → `TASK-E12-04`, `E17-18`.

Cada camada fecha com: matriz de suporte atualizada com níveis reais, gates RT verdes, revisão Opus de todo `-sys` alterado, docs/rustdoc, CHANGELOG e release interna (tag pelo mantenedor).

## 7. Restrições
- Repositório privado `jimmykedlyn/vynamp-output`; abertura pública é decisão futura do mantenedor. Licença MIT OR Apache-2.0.
- Zero dependências de crates.io, inclusive dev-dependencies (harness de teste, gerador de propriedades, allocator de contagem, benchmark próprios).
- Mesmas regras de entrega do Vynamp (`AGENTS.md`): PR por TASK, revisão por subagente de contexto limpo (Opus obrigatório para `-sys`), CI Gate Woodpecker (agentes Windows e Linux, gate que imprime "NOT VERIFIED - macOS" como a `vynamp-decode`), merge pelo mantenedor, sem force push/tag/release automáticos.
- Baselines: Windows 11 x64, Linux x86_64 glibc (Ubuntu 22.04+/Fedora), macOS 13+ x86_64 e arm64, Android API 29+ arm64-v8a (`01-support-matrix.md`).
- Bibliotecas do sistema carregadas: LGPL (libasound, libpulse) por ligação dinâmica em tempo de execução, sem copiar headers; PipeWire MIT. Registrado em `LEGAL.md`.

## 8. Documentação
README; guia mdBook em `book/` (arquitetura, contrato RT, como escrever um `RenderSource`, como portar um backend, semântica de timing, modos e downgrade, DSD/DoP); rustdoc completo com exemplos testados no backend `Null`; matriz de suporte gerada; `docs/hardware-matrix.md`; `docs/sources/<backend>.md` (documentação oficial consultada por API, com data); `docs/ffi-safety.md` (invariantes por crate `-sys`, lista de revisões Opus); `CONTRIBUTING.md`, `SECURITY.md`, `LEGAL.md` (incluindo o porquê da exclusão do ASIO), `CHANGELOG.md`, ADRs.

## 9. Decisões complementares (de Claude)
1. `unsafe` em **crates** `-sys`, não em módulos dentro dos crates seguros: `forbid(unsafe_code)` é por crate e não pode ser reaberto por módulo; crates separados tornam a fronteira auditável e a regra da revisão Opus mecânica (diff toca `crates/*-sys/**`).
2. Windows/macOS ligam estaticamente aos componentes do sistema garantidos no baseline; dlopen é usado onde a ausência é real (Linux, MMCSS, símbolos AAudio novos). Atende o objetivo da chamada 1 (degradar com elegância) sem loader desnecessário.
3. Silêncio de underrun é responsabilidade da biblioteca, no formato do stream, para garantir DoP contínuo e contagem única de underrun.
4. A biblioteca não tem thread de controle própria além do watcher de eventos e das threads de dispositivo que o backend exige.
5. O ADR-001 do novo repositório registra este contrato (`RenderSource`, `StreamConfig`, `PresentationTiming`); o Vynamp vira adaptador, como na `vynamp-decode`.
6. Captura existe apenas como ferramenta de teste interna (feature `test-capture` não publicada na fachada).

## 10. Riscos/objeções (às chamadas vinculantes e ao plano)
1. **Ordem das camadas × marcos do Vynamp (chamada de camadas):** `05-platform-backends.md` exige backend nativo em todos os alvos desde M1 e `TASK-E13-01` (M0) já pede AAudio. Com AAudio e PipeWire/Pulse na Camada 3, Android M0/M1 e o gate Linux M1 ficam atrás das Camadas 1–2. Mitigação adotada em §6: paralelizar backends após o congelamento do core; recomendação: começar AAudio shared logo após o core.
2. **ALSA primeira classe (chamada 1):** em desktops atuais o `default` do ALSA passa pelo PipeWire (pipewire-alsa); sem o backend PipeWire, eventos de rota/servidor ausente/restart (`05-platform-backends.md` §Testes) ficam pobres até a Camada 3. ALSA `hw:` conflita com o PipeWire segurando o dispositivo → `DeviceBusy` frequente; precisa de texto explicativo no Vynamp.
3. **PipeWire nativo é caro:** a API pública é majoritariamente macros/inline e PODs; reimplementar o builder SPA POD é o maior trabalho FFI do projeto e o mais sujeito a quebra entre versões 0.3.x. Alternativa (não adotada): usar só ALSA via pipewire-alsa + Pulse para eventos.
4. **Panic na callback (chamada 3):** `catch_unwind` não funciona com `panic = "abort"`, e o hook padrão aloca e escreve no stderr na thread RT; o contrato "nunca panica" depende do aplicativo configurar o hook. Documentado; não resolvível só na biblioteca.
5. **Verificação (chamada 5):** o agente Linux em Docker pode não ter permissão para `modprobe snd-aloop`, e o agente Windows pode não ter endpoint de áudio (serviço Audiosrv desligado em Windows Server). Sem isso, `capture-verified` e `integration` viram "não executado". Exclusivo bit-perfect no Windows não tem prova automatizável sem driver virtual de terceiros (ferramenta, não dependência) ou captura digital física.
6. **ASIO:** `TASK-E04-05` e `04-outputs-hires.md` listam ASIO opcional; a exclusão do núcleo é compatível com "opcional", mas usuários de interfaces profissionais sem bom driver WASAPI exclusivo ficam sem caminho até um adapter privado existir.
7. **macOS sem agente:** CoreAudio (hog, physical format, rate) é exatamente onde erros de layout/semântica aparecem só em execução; `compiled` não detecta. `TASK-E12-02` exige Intel e Silicon reais.

## 11. Deixado ao construtor
Estrutura interna de módulos; tamanho do scratch e estratégia de buffers; algoritmo de sondagem de formatos e cache; detalhes do watcher de eventos; formato exato do manifesto de captura e da matriz; layout do `tools/layout-check`; heurísticas de período/buffer padrão por backend; lista exata de quirks por driver; divisão fina de TASKs por camada; textos de erro.
