# TASK-E04-06 — Entregar modo transparente e diagnósticos hi-res

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E04-06` |
| Módulo dono | `vynamp-output` |
| Repo | `jimmykedlyn/vynamp-output` |
| REQ coberto | `REQ-E04-06` |
| Critérios da spec | pendente — `specs/output.md` §5 |
| Fase | M3 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E04-05`, `TASK-E04-03` |
| Bloqueia | `TASK-E01-10`, `TASK-E02-07`, `TASK-E04-10`, `TASK-E14-04` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 5–15 dias úteis |
| Risco | alto |

## Entrega

Integer direct path, active conditions, source/output labels e validated combinations.

## Boundary

- **Não** implementa o backend exclusivo nem o perfil de dispositivo que a TASK habilita. Quem abre o stream em exclusivo e negocia 24/96 e 24/192 é `TASK-E04-05`; aqui só se consome a `StreamConfig` já negociada e se decide o caminho inteiro de inteiros. A confusão é plausível porque o selo transparente só pode ser exibido depois que o exclusivo abriu.
- **Não** implementa EQ, ReplayGain nem volume digital. O grafo é `TASK-E04-03` e o EQ paramétrico com presets é `TASK-E04-04`; o modo transparente exige que esses desliguem, não que esta TASK os implemente. A confusão é plausível porque 'desligar o EQ' parece parte do modo transparente, mas é condição de entrada, não entrega.
- **Não** produz os rótulos da interface. Quem desenha o selo 'Modo transparente ativo' e o audio-chain inspector é `TASK-E02-07`; esta TASK entrega os dados e as condições que o rótulo consome.
- **Não** importa AutoEQ nem decide a precedência de perfis. O import com origem e licença é `TASK-E04-09`; ligar o EQ fora do modo transparente não pode importar nada nem mexer na origem do preset.
- **Não** valida o DSP numericamente nem faz o soak de 8 h. Quem mede curva, alias e budget de callback é `TASK-E04-10` e `TASK-E01-10`; aqui o oráculo é a comparação byte a byte da captura digital contra o arquivo.
- **Não** ressampleia nem converte. O resampler e o packing PCM24/32 já foram provados em `TASK-E04-02`; em modo transparente a conversão de precisão é proibida e um PCM24 dentro de container de 32 bits não vira origem de 32 bits.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e04-06-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E04-06-AC-1` | um DAC USB com saída digital (S/PDIF ou interface de captura) ligado a outra máquina, e um arquivo PCM de teste com 24 bits válidos em 96 kHz e um com 32 bits válidos | o arquivo é reproduzido em modo exclusivo pelo DAC, o lado de captura digital grava o fluxo recebido em disco, e os dois arquivos são comparados byte a byte, um caso em I24Packed e outro em I32 valid_bits 32, com `Conversion::Forbid` nos dois | PCM24/32 capture exata em hardware validado |
| `TASK-E04-06-AC-2` | o mesmo DAC USB de antes, com o stream exclusivo aberto em I24Packed a 96 kHz, o EQ10 ligado com ganho de +3 dB na banda de 1 kHz, o ReplayGain com preamp de -6 dB ativo e o volume digital da aplicação em 0,5 | o cliente desliga o EQ mantendo o stream tocando por 5 segundos, depois desliga o ReplayGain e por fim leva o volume digital a 1,0, e em cada etapa lê a lista de condições ativas devolvida pelo módulo e compara a captura digital daquela etapa com o arquivo original | EQ/RG/volume digital desligados |
| `TASK-E04-06-AC-3` | um arquivo MP3 de 320 kbps e um FLAC de 24 bits, ambos tocados no mesmo DAC USB em modo exclusivo, com o EQ e o ReplayGain desligados | os dois arquivos são abertos no player e o cliente lê os rótulos de origem e de saída devolvidos pelo módulo para cada um deles | fonte lossy não recebe selo errado. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-output` | verde no CI Gate |
| Oráculo independente | `cargo test --workspace --locked` em `libs/output` mais a comparação numérica do arquivo capturado contra o arquivo fonte (sha256 dos dois, e diff byte a byte quando os hashes diferem), com o DAC USB e a interface de captura digital ligados | **medido em 2026-11-10**: existe só metade automatizável. O `CaptureHost` já grava os bytes entregues e o compara bit a bit com o oráculo `ChannelRamp` em `crates/vynamp-output-core/tests/capture.rs:114` (I24Packed) e `:139` (I32), com manifesto `format=I24Packed, container_bytes=3, valid_bits=24`. Mas `CaptureHost` não passa por WASAPI: é um backend virtual de teste, e `cargo run -p output-probe -- --host capture --capture-file saida.bin` rodou com `path.os_mixer_bypassed: No`, o que já impede DoP. Não existe hoje código de modo transparente, de bit-perfect ou de badge no repo (0 ocorrências de `transparent`, `bit_perfect` e `Badge` em `crates/`), então o caminho inteiro de inteiros, a lista de condições ativas e os rótulos de origem e de saída ainda são a entregar. Evidência esperada ao fim da TASK: sha256 igual entre a captura digital e o arquivo fonte nos dois formatos, `PathProperties` com `os_mixer_bypassed: Yes`, `endpoint_volume: Unity` e `library_conversion: None`, e um rótulo de saída que mostre taxa e bits válidos negociados |
| Hardware | precisa de DAC USB com saída digital (S/PDIF óptico ou coaxial) e de uma interface de captura digital na outra ponta do cabo, para comparar byte a byte fora do PC; este ambiente só tem 'Conexant ISST Audio' e 'Audio Intel(R) para telas', que são codecs internos de entrada analógica e não servem de prova de caminho exclusivo | ausência é limitação registrada, não aprovação |

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
