// The mission being analysed, and every analysis run on it. One instance for
// the app, so results stay while moving between Result and Analysis (and
// between tabs); they reset only when the mission text changes. The verdict
// runs on its own worker; the analyses on demand share a worker pool and run
// one at a time.
import { i18n } from './i18n.svelte';
import { inspector } from './inspector.svelte';
import type { DealPairingRow } from './pairing';
import {
  Cancelled,
  SolverClient,
  SolverPool,
  mergeDrafts,
  type DraftRow,
  type DraftsOut,
  type NoReturnOut,
  type PairConflict,
  type SolveDraftsOut,
  type SolveOut,
  type TightnessOut,
  type TraceEvent,
} from './solver';
import { check, parse, toText, type Situation } from './wasm';

/** Drafts decided per count at most (sampled above). */
const LIMIT = 2000;
/** Random deals for "Test deals" and for the pairing. */
export const DEALS = 100;

/** State of one computation: progress, cancel, error. */
export class Job {
  /** Name in the Inspector's work lists. */
  constructor(readonly name: string) {}
  running = $state(false);
  progress = $state('');
  /** Last progress line, kept to show what was known when cancelled. */
  last = $state('');
  cancelled = $state(false);
  error = $state<string | null>(null);

  reset() {
    this.running = this.cancelled = false;
    this.progress = this.last = '';
    this.error = null;
  }

  report(line: string) {
    this.progress = this.last = line;
  }
}

/** Items arriving from the workers, shown in batches (every 300 ms). */
class Stream<T> {
  items = $state.raw<T[]>([]);
  private buffer: T[] = [];
  private timer: ReturnType<typeof setTimeout> | null = null;

  push = (x: T) => {
    this.buffer.push(x);
    this.timer ??= setTimeout(() => this.flush(), 300);
  };

  flush() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    if (this.buffer.length) this.items = this.items.concat(this.buffer);
    this.buffer = [];
  }

  reset() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.buffer = [];
    this.items = [];
  }
}

export type Verdict = 'feasible' | 'infeasible' | 'redeal' | null;

class Mission {
  private client = new SolverClient();
  private pool = new SolverPool();

  text = $state('');
  /** Tab on the Analysis screen (kept across missions). */
  tab = $state<'deal' | 'random' | 'position'>('deal');
  situation = $state<Situation | null>(null);
  difficulty = $state(0);
  redeal = $state<string | null>(null);

  // Verdict and its explanations.
  verdictJob = new Job('verdict');
  solveOut = $state<SolveOut | null>(null);
  draftsOut = $state<SolveDraftsOut | null>(null);
  conflict = $state<number[] | null>(null);
  noReturn = $state<NoReturnOut>(null);

  // This deal.
  draftsJob = new Job('drafts');
  drafts = $state<DraftsOut | null>(null);
  rows = new Stream<DraftRow>();
  /** Drafts the count plans to decide (sample or all). */
  planned = $state(0);
  pairsJob = new Job('pairs');
  pairs = $state<PairConflict[] | null>(null);
  tightJob = new Job('tightness');
  tight = $state<TightnessOut | null>(null);

  // Random deals.
  dealsJob = new Job('deals');
  robustness = $state<{ solvable: number; undecided: number; deals: number } | null>(null);
  pairingJob = new Job('pairing');
  pairing = new Stream<DealPairingRow>();

  /** Some owner is `?`: the verdict is over all drafts. */
  unknownOwners = $derived(!!this.situation?.tasks.some((t) => t.owner === '?'));
  /** Tasks with an owner other than the crew (rule cards). */
  hasOwners = $derived(!!this.situation?.tasks.some((t) => t.owner !== 'crew'));
  verdict = $derived<Verdict>(
    this.redeal
      ? 'redeal'
      : this.unknownOwners
        ? this.draftsOut?.verdict === 'feasible' || this.draftsOut?.verdict === 'infeasible'
          ? this.draftsOut.verdict
          : null
        : this.solveOut?.verdict === 'feasible' || this.solveOut?.verdict === 'infeasible'
          ? this.solveOut.verdict
          : null,
  );
  /** An analysis on the pool is running (one at a time). */
  busy = $derived(
    this.draftsJob.running || this.pairsJob.running || this.tightJob.running || this.dealsJob.running || this.pairingJob.running,
  );

  /** Analyse `text`, unless it is the mission already loaded. */
  load(text: string) {
    if (text === this.text && !this.verdictJob.cancelled && !this.verdictJob.error) return;
    this.text = text;
    this.client.cancel();
    this.pool.cancel();
    inspector.reset();
    for (const j of [this.verdictJob, this.draftsJob, this.pairsJob, this.tightJob, this.dealsJob, this.pairingJob]) j.reset();
    this.situation = this.redeal = null;
    this.solveOut = this.draftsOut = this.drafts = null;
    this.conflict = this.pairs = this.tight = this.robustness = null;
    this.noReturn = null;
    this.rows.reset();
    this.pairing.reset();
    this.planned = 0;
    void this.runVerdict(text);
  }

  private async runVerdict(text: string) {
    const job = this.verdictJob;
    const p = parse(text);
    if ('error' in p) {
      job.error = (p.error.line ? `line ${p.error.line}: ` : '') + p.error.msg;
      return;
    }
    const s = (this.situation = p.ok[0]);
    inspector.mission = s.title ?? '';
    const sink = inspector.on ? inspector.sink(this.verdictJob.name) : null;
    const trace = sink ? (e: TraceEvent) => sink(0, e) : undefined;
    const c = check(text);
    if ('error' in c) {
      job.error = c.error.msg;
      return;
    }
    this.difficulty = c.ok.difficulty;
    if (c.ok.redeal) {
      this.redeal = c.ok.redeal;
      return;
    }
    job.running = true;
    const nodes = (k: number) => job.report(i18n.t('positions', { n: (k / 1e6).toFixed(0) + 'M' }));
    try {
      if (this.unknownOwners) {
        this.draftsOut = await this.client.call<SolveDraftsOut>('solve_drafts', [text], nodes, undefined, trace);
      } else {
        this.solveOut = await this.client.call<SolveOut>('solve', [text], nodes, undefined, trace);
        if (this.solveOut.verdict === 'infeasible') {
          this.conflict = await this.client.call<number[] | null>('conflict', [text]);
          if (s.played.length) this.noReturn = await this.client.call<NoReturnOut>('no_return', [text]);
        }
      }
    } catch (e) {
      if (e instanceof Cancelled) job.cancelled = true;
      else job.error = String(e instanceof Error ? e.message : e);
    }
    job.running = false;
    job.progress = '';
  }

  cancelVerdict() {
    this.client.cancel();
    inspector.stopRunning(this.verdictJob.name);
    this.verdictJob.running = false;
    this.verdictJob.cancelled = true;
  }

  /** Run an analysis on the pool with progress and cancel. */
  private async run(job: Job, f: () => Promise<void>) {
    if (this.busy) return;
    job.reset();
    job.running = true;
    this.pool.trace = inspector.on ? inspector.sink(job.name) : null;
    try {
      await f();
    } catch (e) {
      if (e instanceof Cancelled) job.cancelled = true;
      else job.error = String(e instanceof Error ? e.message : e);
    }
    this.rows.flush();
    this.pairing.flush();
    inspector.stopRunning(job.name);
    job.running = false;
    job.progress = '';
  }

  /** Stop the analysis running on the pool; what has arrived stays. */
  cancel() {
    this.pool.cancel();
    this.rows.flush();
    this.pairing.flush();
  }

  /**
   * Decide every draft: with owners `?`, the drafts of those tasks; with
   * owners entered, all other drafts from the deal (would another have worked?).
   */
  countDrafts = () =>
    this.run(this.draftsJob, async () => {
      this.rows.reset();
      this.pairs = null;
      this.planned = 0;
      const progress = (a: number, b: number, w: number, open: number) => {
        this.planned = b;
        this.draftsJob.report(a < b ? i18n.t('draftsChecked', { a, b, w, n: open }) : i18n.t('draftsSettling', { w, n: open }));
      };
      const parts = await this.pool.all<DraftsOut>('drafts', [this.text, !this.unknownOwners, LIMIT], progress, (d) =>
        this.rows.push(d as DraftRow),
      );
      this.drafts = mergeDrafts(parts);
    });

  /** Each pair of open tasks on one player without the rest. */
  checkPairs = () =>
    this.run(this.pairsJob, async () => {
      const parts = await this.pool.all<PairConflict[] | null>('pair_conflicts', [this.text, !this.unknownOwners], (a, b) =>
        this.pairsJob.report(i18n.t('pairsChecked', { a, b })),
      );
      this.pairs = parts.flatMap((p) => p ?? []).sort((x, y) => x.tasks[0] - y.tasks[0] || x.tasks[1] - y.tasks[1]);
    });

  /** Critical decisions and tightness along the winning line. */
  computeTightness = () =>
    this.run(this.tightJob, async () => {
      const s = this.situation;
      if (!s) return;
      if (this.solveOut?.verdict === 'feasible') {
        this.tight = await this.pool.one<TightnessOut | null>('tightness', [this.text, JSON.stringify(this.solveOut.line)]);
      } else if (this.draftsOut?.verdict === 'feasible') {
        const owners = this.draftsOut.owners;
        const owned = toText({ ...s, tasks: s.tasks.map((t, i) => ({ ...t, owner: owners[i] })) });
        this.tight = await this.pool.one<TightnessOut | null>('tightness', [owned, JSON.stringify(this.draftsOut.line)]);
      }
    });

  /** Share of random deals on which some draft works. */
  testDeals = () =>
    this.run(this.dealsJob, async () => {
      // Shown live: each deal is reported when decided or left open, and
      // again when the second pass settles it.
      const outcome = new Map<number, boolean | null>();
      const tell = (e: unknown) => {
        const { deal, solvable } = e as { deal: number; solvable: boolean | null };
        outcome.set(deal, solvable);
        const all = [...outcome.values()];
        const open = all.filter((x) => x === null).length;
        this.robustness = { solvable: all.filter((x) => x === true).length, undecided: open, deals: all.length };
        this.dealsJob.report(i18n.t('dealsTested', { a: all.length, b: DEALS }) + (open ? ' · ' + i18n.t('dealsOpen', { n: open }) : ''));
      };
      this.robustness = null;
      await this.pool.all('deal_robustness', [this.text, DEALS, seed()], undefined, tell);
    });

  /** Which tasks share a player in the working drafts of random deals. */
  pairDeals = () =>
    this.run(this.pairingJob, async () => {
      this.pairing.reset();
      let done = 0;
      // Draft count of the deals in progress, summed over the workers.
      let current = [0, 0];
      const show = () =>
        this.pairingJob.report(i18n.t('pairingProgress', { d: done, n: DEALS, a: current[0], b: current[1] }));
      const tell = (d: unknown) => {
        this.pairing.push(d as DealPairingRow);
        done++;
        show();
      };
      const progress = (a: number, b: number) => {
        current = [a, b];
        show();
      };
      await this.pool.all<boolean>('deal_pairing', [this.text, DEALS, seed()], progress, tell);
    });
}

const seed = () => Math.floor(Math.random() * 1e9);

export const mission = new Mission();
