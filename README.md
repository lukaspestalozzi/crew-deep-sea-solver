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

- `solve FILE`: every situation in the file. Tasks with owner `?` are searched over all legal drafts (sampled above `--limit`, default 2000; the yes/no answer stays exact).
- `--all-drafts`: also check every other draft. `--analyse`: critical decisions, smallest conflict, point of no return.
- `bench`: random deals with tasks drawn to a difficulty sum and a random legal draft; prints solve times.

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
- Task owners: `PlayerN`, `?` (unknown, draft search) or `crew` (rule cards only). Task IDs are listed in `tasks.toml`.
- `played`: leader, then the cards clockwise. The last trick may be incomplete.
- `#` starts a comment. Several situations in one file are separated by a line `---`.

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

- a differential test: the optimised search (position cache, merged equivalent cards, joint trick-count check) against a plain search on 400 random positions;
- a soundness test: every early "done" or "failed" verdict of every task checked against all (or sampled) endings of the game.

## Legal

The Crew: Mission Deep Sea is © KOSMOS / Thomas Sing. This is an unofficial
fan project, not affiliated with KOSMOS.
