# TASK-E12-04 — Entregar DSD nativo/DoP e PCM 768 kHz

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E12-04` |
| Módulo dono | `vynamp-output` |
| Repo | `jimmykedlyn/vynamp-output` |
| REQ coberto | `REQ-E12-04` |
| Critérios da spec | pendente — `specs/output.md` §5 |
| Fase | M5 |
| Prioridade | P2 |
| Estado | planned |
| Depende de | `TASK-E04-10`, `TASK-E01-09` |
| Bloqueia | `TASK-E17-18` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 10–25 dias úteis |
| Risco | alto |

## Entrega

Backends condicionados ao DAC, packing DoP e capabilities de taxa/DSD.

## Boundary

- **Não** implementa a decodificação de DSF/DFF nem a decimação DSD para PCM. O decimator e os mapas mono/stereo/5.1/7.1 são `TASK-E01-09`, dependência declarada desta TASK; aqui os bytes DSD chegam crus em `DeviceBuffer::Dsd` ou o PCM já vem decimado, e pôr a decimação na output criaria um segundo caminho de conversão além do dono.
- **Não** decide se o grafo DSP está ativo nem desliga nó algum. O grafo e o swap em fronteira de bloco são `TASK-E04-03`, e o vitrola que se autodesativa com DSD é `TASK-E19-07`; esta TASK lê a declaração de caminho e recusa abrir DSD com processamento declarado ativo, o que é plausível confundir com executar o bypass, mas executá-lo aqui faria dois donos do mesmo estado do grafo.
- **Não** entrega o backend de OS por onde 768 kHz e DSD saem no DAC físico. WASAPI exclusive e perfis de dispositivo são `TASK-E04-05`, e hoje as células de wasapi, alsa, pipewire, pulse, coreaudio e aaudio estão todas em `-` em `docs/support-matrix.md`; aqui se declara capability e se empacota DoP sobre o que o host expõe, e abrir dispositivo real não cabe nesta TASK.
- **Não** edita `docs/support-matrix.md` nem promove célula de `docs/hardware-matrix.md`. A declaração de combinação validada é `TASK-E04-06`, e `crates/vynamp-output/tests/support_matrix.rs` exige que o arquivo seja saída literal do gerador; `hardware-verified` exige captura digital, que este ambiente não tem, então o que aqui é medido em host virtual permanece `library-verified`.
- **Não** implementa o resampler que levaria uma fonte de 44,1, 48 ou 96 kHz a 768 kHz nem o dither de redução. Resampler e conversão de amostras são `TASK-E04-02`, provados em M0; esta TASK declara a taxa na capability e a negocia, e se o pedido de 768 kHz não casar com a fonte quem converte e quem registra a transformação é o resampler.
- **Não** cria as fixtures nem certifica raw, DST, PCM decimado e DoP lado a lado, e não trata SACD ISO. `TASK-E17-18` é a dona dessa certificação e é bloqueada por esta TASK; ela consome o que aqui se declara, e duplicar a matriz de arquivos deixaria duas verdades sobre o mesmo corpus.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e12-04-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E12-04-AC-1` | um dispositivo virtual criado por `NullHost::with_capabilities` que anuncia DSD nativo DSD64 a 2 822 400 Hz em modo exclusivo por uma `CapabilityEntry` no formato DSD nativo que `SampleFormat` ainda não tem (hoje só existe `Dop { dsd_rate }`), e dois `StreamRequest` nesse formato em exclusivo com `on_unavailable: OnUnavailable::Error` e `conversion: Conversion::Forbid`, um com a cadeia DSP declarada ativa e outro com ela declarada desligada | as duas aberturas são tentadas em sequência e, em cada uma, o `StreamConfig` devolvido ou o `Error` e o `PathProperties` do caminho são lidos, incluindo o que o caminho declara sobre processamento DSP e `path.library_conversion` | DSP desligado em DSD nativo |
| `TASK-E12-04-AC-2` | um DSD64 a 2 822 400 Hz em 2 canais, período ímpar de 7 frames, 3 períodos mais uma chamada curta de 3 frames que simula underrun, abertos no `CaptureHost` com a fonte de padrão `dsd_byte(f, c, k)` de `tests/dop.rs` — uma vez em `SampleFormat::Dop { dsd_rate: 2_822_400 }` no contêiner `I32` MSB-justified e outra vez no formato DSD nativo ainda ausente | o stream é iniciado, os períodos são entregues com o underrun no meio, o stream é drenado e o arquivo de captura é relido: no contêiner DoP palavra de 24 bits por palavra contra o marcador e o padrão de bytes, e no contêiner DSD nativo byte a byte contra o padrão, sem byte de marcador | markers/bytes validados |
| `TASK-E12-04-AC-3` | três dispositivos virtuais em modo exclusivo: (a) `NullHost::with_capabilities` só com entradas PCM — F32, I16, I24Packed, I32 de 24 e I32 de 32 valid bits —, (b) `NullHost` com as capabilities padrão e `with_os_mixer_bypassed(Tri::No)`, e (c) `NullHost::with_capabilities` com uma única entrada DoP DSD64 a 176 400 Hz; e cinco pedidos: em (a) DoP DSD64 a 176 400 Hz duas vezes, uma com `Conversion::Forbid` e outra com `Conversion::Allow` de `Dop` para `I24Packed`, ambos com `OnUnavailable::Error`; em (b) o mesmo DoP DSD64 com `Conversion::Forbid`; em (c) DoP DSD128 a 352 800 Hz uma vez com `OnUnavailable::Error` e outra com `OnUnavailable::Downgrade` e `Conversion::Forbid` | as cinco aberturas são tentadas e, em cada uma, o `Error` devolvido, o `StreamConfig` real, o valor de `path.library_conversion` e o evento `Downgraded` recebido são lidos e comparados com o que o pedido autorizou | equipamento sem suporte falha sem conversão oculta. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-output` | verde no CI Gate |
| Oráculo independente | `cd libs/output && cargo test -p vynamp-output-core --test dop --locked` (11 testes) e `cargo test -p vynamp-output-core --test negotiate --locked` (32 testes) e `cargo test -p vynamp-output --test support_matrix --locked` como linha de base, mais testes novos alimentados por `CapabilityEntry` montada em `NullHost::with_capabilities` e pela fonte `DsdPattern` de `tests/dop.rs` | Esperado: em AC-2, no contêiner DoP cada palavra de 24 bits capturada igual ao marcador deslocado 24 bits unido ao primeiro byte em 16 e ao segundo em 8, com marcador 0x05/0xFA alternando por frame de portadora e fase contínua através de 3 períodos de 7 frames, do underrun de 3 frames e do silêncio (idle 0x69) — é o que os 11 testes de `tests/dop.rs` e os 9 unitários de `src/dop.rs` já provam no host Capture; no contêiner DSD nativo, os bytes capturados iguais ao padrão e nenhum byte de marcador; em AC-3, as cinco aberturas devolvem: (a) `Error::FormatUnsupported { nearest: None }` nas duas, mesmo com `Conversion::Allow` de `Dop` para `I24Packed`, porque `negotiate.rs` recusa converter DoP em PCM e não há entrada DoP no dispositivo; (b) `Error::ExclusiveUnavailable { .. }`; (c) `Error::FormatUnsupported` com `nearest` em DOP64 a 176 400 Hz e, sob `OnUnavailable::Downgrade`, `StreamConfig` de DOP64 a 176 400 Hz com `Event::Downgraded` de motivo `rate not supported` e `path.library_conversion` `None` — os três comportamentos já são asseridos hoje em `tests/dop.rs`. O que ainda não existe: `SampleFormat` não tem variante de DSD nativo (format.rs:102 comenta que DSD_U8/U16/U32 'adds its own width here' e nada foi adicionado), `Feature` em `support.rs` não tem item de DSD nativo, `StreamRequest` em `request.rs` não tem campo por onde declarar a cadeia DSP ativa, o Null oferece DoP só a 176 400, 352 800 e 705 600 Hz, 768 000 Hz aparece apenas como taxa de PCM em `null.rs` e não há backend de OS — sem a variante DSD nativo e o campo de DSP no pedido, AC-1 não roda hoje |
| Hardware | um DAC com DSD nativo DSD64/128/256 por ASIO ou WASAPI exclusive, mais captura digital para provar os bytes no fio; nada disso existe aqui — este ambiente é Windows 11 x86_64 sem placa pro e sem DAC DSD, o único dispositivo já medido é o `Headset (USB 384KZ HIFI Audio)` em WASAPI shared a 384 kHz pelo spike da `TASK-E01-02`, e não há aparelho Android, iOS nem máquina macOS. Packing DoP, negociação e recusa sem conversão oculta rodam no Null e no Capture sem hardware; DSD nativo por DAC real fica como limitação registrada | ausência é limitação registrada, não aprovação |

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
