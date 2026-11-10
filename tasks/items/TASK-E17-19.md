# TASK-E17-19 — Expor capacidades Bluetooth e headroom por rota

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E17-19` |
| Módulo dono | `vynamp-output` |
| Repo | `jimmykedlyn/vynamp-output` |
| REQ coberto | `REQ-E17-19` |
| Critérios da spec | pendente — `specs/output.md` §5 |
| Fase | M3 |
| Prioridade | P0 |
| Estado | planned |
| Depende de | `TASK-E13-03`, `TASK-E14-02`, `TASK-E04-08` |
| Bloqueia | `TASK-E17-20` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | alto |

## Entrega

Diagnóstico de rota OS, codec negociado quando exposto e volume/headroom por dispositivo.

## Boundary

- **Não** implementa o guard de true-peak nem a medição do teto no PCM final. A QA-027 entra aqui só por referência: o guard depois de mix/master/resampler é `TASK-E17-07` e o harness que mede o buffer do backend é `TASK-E17-08`; a confusão é plausível porque "headroom por rota" soa como o teto final, e esta TASK apenas reporta o que a rota declara, sem tocar em nenhum dos dois.
- **Não** abre stream, trata audio focus nem recupera a callback na rota nova. Isso é `TASK-E13-03`, dona de AudioManager, foco, noisy e reconexão fora da callback no Android; aqui só se lê o que ele reporta, e trazer a reabertura para cá faria dois donos da mesma transição de rota da AC-3.
- **Não** registra extensão nem associação de shell. O gerador de declarações de OS por container e parser é `TASK-E16-09` e a certificação de associações nos cinco alvos é `TASK-E16-07`; a AC-1 existe justamente para que nenhum `.lc3`, `.ldac` ou `.aptx` saia do diagnóstico de rota para esses geradores.
- **Não** implementa backend exclusivo nem negociação de formato por dispositivo. `TASK-E04-05` é dona de WASAPI event-exclusive, IDs estáveis, capabilities e device profiles; aqui se consome o que o host declara e não se decide política de abertura, fallback nem formato da reabertura da AC-3.
- **Não** persiste volume, EQ ou headphone preset por dispositivo. `TASK-E04-09` é dona dos perfis e do import de preset e `TASK-E04-01` do ganho e ReplayGain; aqui o `EndpointVolume` é o valor que o backend reporta, nunca um valor que a aplicação escreve na rota nem um perfil salvo entre sessões.
- **Não** publica o manifest de capacidades por artefato nem concede selo de hi-res ou bit-perfect. O manifest é `TASK-E17-12` e o modo transparente com os diagnósticos hi-res é `TASK-E04-06`; a AC-3 proíbe prometer hi-res na troca de rota, e prometer seria exatamente o trabalho dessas duas.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e17-19-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E17-19-AC-1` | as entradas `bt-classic` (perfis SBC, AAC, aptX, aptX HD, aptX Adaptive, aptX Lossless, LDAC) e `bt-le` (LC3 LE Audio, LC3plus) de `config/format-catalog.json` rev 0.7.0, ambas com `policy: conditional`, `operations: ["observe"]` e `association_allowed: false`, e uma rota de teste cujo `Transport` é `Bluetooth` | o diagnóstico dessa rota é consultado e o app enumera as extensões e MIME que poderia associar a partir do codec de transporte que ela reporta | SBC/AAC/aptX/LDAC/LC3 não geram associações |
| `TASK-E17-19-AC-2` | um host de teste com `DeviceInfo.transport` igual a `Transport::Bluetooth` e `PathProperties` com `endpoint_volume: Unknown` e nenhum campo de codec negociado, no molde do `NullHost` de `crates/vynamp-output-core/src/null.rs:353`, que hoje só constrói `Transport::Virtual` | o diagnóstico de rota desse dispositivo é consultado | API ausente mostra desconhecido |
| `TASK-E17-19-AC-3` | duas rotas de saída no host: o DAC USB com `EndpointVolume::Unity` e a rota Bluetooth limitada a 48 kHz com `EndpointVolume::Unknown`, com o stream tocando 44,1 kHz em 16 bits e volume de aplicação 1,0 | a rota de saída muda de USB para Bluetooth, o host abre um stream novo no dispositivo Bluetooth e o diagnóstico das duas rotas é lido | troca de rota não aumenta volume nem garante hi-res. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-output` | verde no CI Gate |
| Oráculo independente | `cargo test --workspace --locked -p vynamp-output-core` sobre um host de teste que reporta `Transport::Bluetooth` e `EndpointVolume::Unknown`, mais `cargo run -q -p output-probe -- --host null --rate 44100` para imprimir o `PathProperties` real, mais a leitura das entradas `bt-classic`, `bt-le` e `lc3-files` de `config/format-catalog.json` | esperado: as três asserções numéricas são `transport == Bluetooth`, `endpoint_volume` igual antes e depois da troca (Unity no USB, Unknown no Bluetooth, em nenhuma das duas `NotUnity`) e extensões associáveis iguais a zero itens, sem `.lc3`, `.ldac` nem `.aptx`, com o codec em `Unknown` porque nenhum campo o carrega. Hoje nada disso existe: as três entradas do catálogo já estão com `association_allowed: false` e `state: planned`, mas nenhum dos 42 `.rs` de `libs/output` abre o catálogo (grep por `format-catalog`: 0 ocorrências), `Transport::Bluetooth` aparece só na declaração de `crates/vynamp-output-core/src/config.rs:26` e nenhum host o constrói, `PathProperties` em `config.rs:42` não tem campo de codec negociado nem de headroom, e `support()` devolve `None` para os seis hosts de OS, com `LibraryVerified` só para Null e Capture. O probe imprime hoje, em 2026-11-10, `endpoint_volume: Unity, transport: Virtual` e `callbacks=200 frames_delivered=51200 source_underruns=0`, sem nenhuma linha de codec. Baseline medido: `cargo test --workspace --locked` dá 133 passed, 0 failed e 1 ignored (só o doctest `crates/vynamp-output-testkit/src/alloc.rs - alloc (line 8)`) em 18 suítes, número que não cobre Bluetooth algum e é só o ponto de partida; `tests/null_host.rs:208` depende de `thread::sleep(60 ms)` antes de `s.timing()` e passou 5 de 5 execuções hoje, então não serve como evidência de estabilidade sob carga. |
| Hardware | ausente para a rota real: a AC-1 e a AC-3 pedem um headset Bluetooth Classic (SBC ou AAC, de preferência com aptX e LDAC) e um LE Audio (LC3) pareados e tocando, e esta máquina é Windows 11 sem endpoint de áudio Bluetooth ativo — `Get-PnpDevice -Class AudioEndpoint` com Status OK devolve só `Alto-falante (Conexant ISST Audio)` e `Microfone interno (Conexant ISST Audio)`, enquanto `S22 Ultra de Jimmy`, `EDIFIER R1700BT` e `BT9000x` aparecem em Status Unknown e o único `Headset (USB 384KZ HIFI Audio)` também está Unknown. O transporte, o codec desconhecido e o headroom são exercitáveis no host de teste; a leitura do codec negociado e do volume por dispositivo no OS real, não | ausência é limitação registrada, não aprovação |

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
