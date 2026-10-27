// The Position Inspector's state: one position in play (owners fixed), its
// moves solved on the worker pool, move orders playing on by themselves,
// and a benchmark of the move orders over many positions. One instance, so
// the position survives switching screens.
import { i18n } from './i18n.svelte';
import { inspector } from './inspector.svelte';
import { Job, mission } from './mission.svelte';
import { Cancelled, SolverPool, type TrickOut } from './solver';
import { router } from './state.svelte';
import { parse, positionInfo, toText, type PositionInfo, type Situation } from './wasm';

/** Nodes per move in the first pass; open moves can be continued without a limit. */
export const BUDGET = 2_000_000;

export interface MoveResult {
  card: string;
  verdict: 'feasible' | 'infeasible' | 'undecided' | 'cancelled' | 'redeal';
  nodes: number;
  line?: string[];
  tricks?: TrickOut[];
}

export interface PlayOutResult {
  order: number;
  line: string[];
  tricks: TrickOut[];
  winnable: boolean | null;
  lost_at: number | null;
  alternatives: string[];
  undecided: boolean;
  status: ('open' | 'done' | 'failed')[];
}

export interface StatRow {
  index: number;
  legal: number;
  winning: string[];
  open: number;
  first_win: (number | null)[];
}

/** The situation after the player to move plays `card`. */
export function afterMove(s: Situation, info: PositionInfo, card: string): Situation {
  const played = s.played.map((t) => ({ ...t, cards: [...t.cards] }));
  const last = played.at(-1);
  if (last && last.cards.length < s.hands.length) last.cards.push(card);
  else played.push({ leader: info.leader, cards: [card] });
  return { ...s, played };
}

class PositionStore {
  private pool = new SolverPool();

  /** The position, as crew 1 text; the earlier ones for going back. */
  text = $state('');
  history = $state<string[]>([]);
  situation = $derived.by((): Situation | null => {
    const p = parse(this.text);
    return 'ok' in p ? p.ok[0] : null;
  });
  info = $derived.by((): PositionInfo | { error: string } | null => {
    if (!this.text) return null;
    const r = positionInfo(this.text);
    return 'ok' in r ? r.ok : { error: r.error.msg };
  });

  movesJob = new Job('position');
  moves = $state<Record<string, MoveResult>>({});
  playJob = new Job('position');
  playOut = $state<PlayOutResult | null>(null);
  /** Cards played so far by the move order playing on (live). */
  playing = $state<string[]>([]);
  benchJob = new Job('position');
  bench = $state<{ source: string; rows: StatRow[]; total: number } | null>(null);

  busy = $derived(this.movesJob.running || this.playJob.running || this.benchJob.running);

  /** Screen the position was opened from (for Back). */
  from = $state('/analysis');

  /** Open a position (resets the history). */
  open(text: string) {
    this.history = [];
    this.go(text);
  }

  /** Move to a position and solve its moves. */
  private go(text: string) {
    this.cancel();
    this.text = text;
    this.moves = {};
    this.playOut = null;
    this.playing = [];
    const info = this.info;
    if (info && !('error' in info) && !info.over) void this.solveMoves(BUDGET);
  }

  play(card: string) {
    const s = this.situation;
    const info = this.info;
    if (!s || !info || 'error' in info) return;
    this.history = [...this.history, this.text];
    this.go(toText(afterMove(s, info, card)));
  }

  back() {
    const prev = this.history.at(-1);
    if (prev === undefined) return;
    this.history = this.history.slice(0, -1);
    this.go(prev);
  }

  /** Play the given cards in turn (e.g. a line up to a point). */
  playAll(cards: string[]) {
    let s = this.situation;
    if (!s) return;
    const hist = [...this.history];
    let text = this.text;
    for (const c of cards) {
      const info = positionInfo(text);
      if ('error' in info) break;
      hist.push(text);
      s = afterMove(s, info.ok, c);
      text = toText(s);
    }
    this.history = hist;
    this.go(text);
  }

  cancel() {
    this.pool.cancel();
  }

  private async run(job: Job, f: () => Promise<void>) {
    job.reset();
    job.running = true;
    this.pool.trace = inspector.on ? inspector.sink(job.name) : null;
    try {
      await f();
    } catch (e) {
      if (e instanceof Cancelled) job.cancelled = true;
      else job.error = String(e instanceof Error ? e.message : e);
    }
    inspector.stopRunning(job.name);
    job.running = false;
    job.progress = '';
  }

  /** Solve the moves not decided yet (`budget` 0: to the end). */
  solveMoves = (budget: number) =>
    this.run(this.movesJob, async () => {
      const text = this.text;
      const info = this.info;
      if (!info || 'error' in info) return;
      const todo = info.legal.filter((c) => !this.moves[c] || this.moves[c].verdict === 'undecided');
      let done = 0;
      const tell = (m: unknown) => {
        const r = m as MoveResult;
        if (this.text !== text) return;
        this.moves = { ...this.moves, [r.card]: r };
        this.movesJob.report(i18n.t('movesSolved', { a: ++done, b: todo.length }));
      };
      this.movesJob.report(i18n.t('movesSolved', { a: 0, b: todo.length }));
      await this.pool.all('solve_moves', [text, todo.join(' '), budget], undefined, tell);
    });

  /** Let move order `order` play every seat from here. */
  letPlay = (order: number) =>
    this.run(this.playJob, async () => {
      this.playOut = null;
      this.playing = [];
      const tell = (m: unknown) => {
        const { card, still } = m as { card: string; still: boolean | null };
        this.playing = [...this.playing, card];
        this.playJob.report(i18n.t('playedN', { n: this.playing.length }) + (still === false ? ' · ' + i18n.t('lostShort') : ''));
      };
      const r = await this.pool.one<Omit<PlayOutResult, 'order'> | null>('play_out', [this.text, order, BUDGET], undefined, tell);
      if (r) this.playOut = { ...r, order };
    });

  /** The positions after each card of `line`, from here (the position itself first). */
  positionsAlong(line: string[]): string[] {
    const out = [this.text];
    let s = this.situation;
    let text = this.text;
    for (const c of line.slice(0, -1)) {
      const info = positionInfo(text);
      if (!s || 'error' in info) break;
      s = afterMove(s, info.ok, c);
      text = toText(s);
      out.push(text);
    }
    return out;
  }

  /** How the move orders do over `positions` (crew 1 texts). */
  benchmark = (source: string, positions: string[]) =>
    this.run(this.benchJob, async () => {
      this.bench = { source, rows: [], total: positions.length };
      const tell = (m: unknown) => {
        const b = this.bench!;
        this.bench = { ...b, rows: [...b.rows, m as StatRow] };
        this.benchJob.report(i18n.t('positionsDone', { a: this.bench.rows.length, b: positions.length }));
      };
      await this.pool.all('position_stats', [positions.map((t) => t.trimEnd() + '\n').join('---\n'), BUDGET], undefined, tell);
    });
}

export const position = new PositionStore();

/**
 * The position on a line: the situation with `owners`, its complete played
 * tricks (`fromDeal`: none), then the first `k` tricks of `tricks` (a line's
 * tricks, whose first may finish a trick in progress).
 */
export function positionOnLine(s: Situation, owners: string[] | undefined, tricks: TrickOut[], k: number, fromDeal = false): string {
  const n = s.hands.length;
  const played = fromDeal ? [] : s.played.filter((t) => t.cards.length === n);
  return toText({
    ...s,
    tasks: owners ? s.tasks.map((t, i) => ({ ...t, owner: owners[i] })) : s.tasks,
    played: [...played, ...tricks.slice(0, k).map((t) => ({ leader: t.leader, cards: t.cards }))],
  });
}

/** Open a position in the Inspector: the Position tab in the desk view, else its screen. */
export function inspect(text: string) {
  position.from = router.path === '/position' ? position.from : router.path;
  position.open(text);
  if (router.path === '/desk') mission.tab = 'position';
  else router.go('/position');
}

/** Positions need every owner fixed. */
export const inspectable = (text: string) => !/^\?:/m.test(text);

/** Per move order: positions that count (a choice and a winning move, all ranks known), first choice wins, mean first winning rank. */
export function benchSummary(rows: StatRow[], orders: number) {
  return [...Array(orders).keys()].map((k) => {
    const counted = rows.filter((r) => r.legal > 1 && r.winning.length > 0 && r.first_win[k] !== null);
    const top = counted.filter((r) => r.first_win[k] === 1).length;
    const mean = counted.reduce((s, r) => s + r.first_win[k]!, 0) / Math.max(1, counted.length);
    return { counted: counted.length, top, mean };
  });
}
