# Deep Sea Solver

Was a mission of **The Crew: Mission Deep Sea** possible, and how hard was it?

Given the hands of all players and the task cards, the solver searches every
legal line of play (perfect information) and answers:

- **Feasible / not feasible / re-deal** for the draft you entered, with a winning line you can replay trick by trick.
- **Other drafts:** how many legal task drafts would have worked, and the smallest change of owners that fixes yours.
- **Why it fails:** the smallest set of tasks that cannot be fulfilled together.
- **Where it went wrong:** for games entered trick by trick, the card after which the mission was lost, and which cards would have kept it winnable.
- **Difficulty measures:** share of drafts that work, critical decisions and tightness along a winning line, printed difficulty, and how often the task set is solvable on random deals.

Mission Deep Sea only, 3–5 players. Communication and the distress signal are
not modelled: they do not change which plays are legal.

The design and every product decision are in [`spec/design-spec.md`](spec/design-spec.md),
including what is not implemented yet.

## Parts

| Path | What |
| --- | --- |
| `crates/crew-core` | Rules engine, task catalogue, solver, draft search, analyses, `crew 1` text format |
| `crates/crew-core/data/tasks.toml` | The 96 task cards and 5 logbook rule cards as data |
| `crates/crew-cli` | `crew` command line |
| `crates/crew-wasm` | WebAssembly bindings (JSON API) |
| `web` | Installable web app (Svelte), runs the solver in a Web Worker, works offline |

## Command line

```sh
cargo run --release -p crew-cli -- solve examples/mock.crew --analyse --all-drafts
cargo run --release -p crew-cli -- bench --players 4 --target 7 --count 20 -v
```

- `solve FILE`: every situation in the file. Tasks with owner `?` are searched over all legal drafts at once.
- `--count-drafts`: also count the drafts that work, exactly (sampled above `--limit`, default 2000): rounds with doubling node budgets per draft, winning lines replayed against the other drafts, and searches over all open drafts together that can prove them all infeasible at once.
- `--all-drafts`: also check every other draft. `--analyse`: critical decisions, smallest conflict, point of no return.
- `--dfs`, `--best-first`, `--task-aware`, `--deck-order`, `--zobrist`: search strategy, move order and position hashing (see Search).
- `bench`: random deals with tasks drawn to a difficulty sum and a random legal draft; prints solve times. `bench --load FILE` times the situations in a file.

## Search

`crates/crew-core/src/solver.rs` runs over plays. Between tricks every task is
evaluated for every player who may still own it (a (task, player) *pair*).
With fixed owners each task has one pair; with owner `?` one search covers
all legal drafts: a branch is lost once no complete legal draft of surviving
pairs remains, and won once some draft has all its tasks done.

- Pruning: a task fails as soon as it can no longer be met. Combined checks
  between tricks: every task's card requirements (cards to win or not to
  win, counts of a suit or value, tricks it still needs) give each player a
  window for its final trick count; with all trick-count rules (exact counts,
  vs. captain, most/fewest, …) they are checked jointly over the possible
  final splits and drafts; who may still win each card is checked across
  players; certain wins come from submarine pairing (an own submarine loses
  only to a higher one of another player in the same trick).
- Positions proven lost are cached (128-bit keys; by default the hands by
  relative rank, so positions differing only in interchangeable cards share
  a key). The cache has a size limit.
- Interchangeable cards are tried once.
- Pluggable through `Options`: a `MoveDecider` (`decider.rs`) orders the
  moves, and the same trait can drive a self-playing bot. `Lookahead` (default)
  ranks leads by playing the trick out and scoring the result; `TaskAware`
  judges each card by whether it can still be made to win or lose the trick
  (later players must follow suit) and what the players' tasks want;
  `DeckOrder` is the reference. The `Strategy` is `Portfolio` (default: depth-first
  restarts alternating the decider with deck order, node budgets doubling,
  lost-position cache kept), plain `Dfs`, or `BestFirst` over positions between
  tricks (falls back to depth-first above a position budget). A node limit
  makes a search stop as undecided.

## Text format (`crew 1`)

```text
crew 1
title: Mission 13
target: 8
players: 4
sealed: no

Player1: P2 P8 B4 B9 G1 G6 Y3 Y7 S2 S4
Player2: P1 P5 B1 B2 B6 G4 G9 Y2 Y6 S1
Player3: P3 P6 B3 B8 G2 G5 G8 Y1 Y8 S3
Player4: rest

tasks:
Player2: win B4
?:       win P9 Y8
Player1: first-trick
crew:    rule M12 no-lead pink sub

played:
trick1 Player1: G6 G4 G2 G3
```

- Cards: `P B G Y S` (pink, blue, green, yellow, submarine) + value. `rest` stands for one player's remaining cards.
- Hands are always the full hands as dealt; played cards are subtracted.
- Task owners: `PlayerN`, `?` (unknown, draft search) or `crew` (rule cards only).
- Tasks: an official ID from `tasks.toml`, or any task of the categories as a parametric ID (below).
- `played`: leader, then the cards clockwise. The last trick may be incomplete.
- `#` starts a comment. Several situations in one file are separated by a line `---`.

## Generated tasks

Besides the official cards, every task of the official categories can be
written as an ID. An ID that describes an official card resolves to it
(`win Y7 B6` is the card `win B6 Y7`). Generated tasks have a difficulty
inferred by a rule of thumb per category (shown as ≈) and no re-deal
clauses. Random draws ("Fill randomly", `bench`) use official cards only.
Rule cards are not generated.

| Family | Examples |
| --- | --- |
| Win cards | `win P3 Y7`, `win S2`, `win all 5s` |
| Win none | `no pink green`, `no subs`, `no 8s 9s` |
| Not leading | `no-lead blue green` |
| Win with | `win with 4`, `win 8 with 3` |
| Trick property | `trick below 5`, `trick above 6`, `trick even`, `trick sum >25`, `trick sum <10`, `trick sum 18-20`, `trick pink=green`, `win B6 with sub`, `win Y3 last` |
| Specific tricks | `first 4`, `first 2+last`, `only first 2`, `none of first 6` |
| Trick count | `tricks =3` |
| In a row | `row 4`, `exactly row 3` |
| Colour counts | `at least 6 green`, `exactly 2 pink 1 sub` |
| Value counts | `at least two 8s`, `exactly one 1s` |
| Other | `only S3`, `more green than blue`, `blue=yellow` |

The grammar is in `crates/crew-core/src/generate.rs`.

## Web app

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.100
cd web
npm ci
npm run wasm    # builds the solver into web/src/wasm
npm run dev     # or: npm run build
```

Pushes to `main` run the tests and deploy `web/dist` to GitHub Pages
(repository settings → Pages → Source: GitHub Actions).

## Task card data

The texts and difficulty values in `tasks.toml` are **placeholders** from a
fan-made list (BoardGameGeek thread 2631311, as used by
remigerme/the-crew-solver). Entries marked `doubtful = true` disagree between
sources or have unclear rules; the app marks them. They are to be replaced by
transcriptions of the German edition.

Rule interpretations that the cards leave open are documented next to each
entry (`note`) and in `crates/crew-core/src/task.rs`.

## Verification

`cargo test --release` runs, among others:

- a differential test: the optimised search (position cache, merged equivalent cards, joint trick-count check, task-aware move order) against a plain search on 400 random positions;
- the search over all drafts in one tree against solving each draft on its own; best-first and Zobrist keys against the defaults;
- a soundness test: every early "done" or "failed" verdict of every task checked against all (or sampled) endings of the game.

## Legal

The Crew: Mission Deep Sea is © KOSMOS / Thomas Sing. This is an unofficial
fan project, not affiliated with KOSMOS.
