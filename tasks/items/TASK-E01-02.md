# TASK-E01-02 — Provar callback e output clock

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. Critério de aceite: **PENDENTE** de autoria —
> a coluna `Then` é o requisito aprovado, copiado literal; `Given` e `When` ainda
> não existem.

| Campo | Valor |
|---|---|
| ID | `TASK-E01-02` |
| Módulo dono | `vynamp-output` |
| Repo | `jimmykedlyn/vynamp-output` |
| REQ coberto | `REQ-E01-02` |
| Critérios da spec | pendente — `specs/output.md` §5 |
| Fase | M0 |
| Prioridade | P0 |
| Estado | in_progress |
| Depende de | `TASK-E11-01` |
| Bloqueia | `TASK-E01-03`, `TASK-E04-02`, `TASK-E04-05`, `TASK-E13-01` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | alto |

## Entrega

Spike shared output, ring SPSC e harness de callback com instrumentation.

## Boundary

<!-- PENDENTE: o que esta TASK não faz, mesmo parecendo relacionado. -->

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
| Teste automatizado | `cargo test` no repo `vynamp-output` | verde no CI Gate |
| Oráculo independente | PENDENTE | comparação numérica, não "parece certo" |
| Hardware | PENDENTE | ausência é limitação registrada, não aprovação |

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
