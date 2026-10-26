// Client for the solver worker. One call at a time per client; cancel()
// terminates the worker (a running WebAssembly call cannot be interrupted
// otherwise) and starts a fresh one.
import SolverWorker from './worker.ts?worker';

type Pending = {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
  progress?: (...p: number[]) => void;
  /** Items streamed by the call (decided drafts, deals). */
  decided?: (d: unknown) => void;
  trace?: (e: TraceEvent) => void;
};

/** A solver trace event (see crew_core::trace), with the time it was sent (ms since 1970). */
export type TraceEvent =
  | { ev: 'start'; id: number; parent: number | null; label: string; text: string; t: number }
  | { ev: 'progress'; id: number; nodes: number; restart: number; budget: number; cache: number; t: number }
  | { ev: 'end'; id: number; result: string; stats: { nodes: number; cache_hits: number; cache_size: number } | null; t: number };

export class Cancelled extends Error {
  constructor() {
    super('cancelled');
  }
}

export class SolverClient {
  private worker: Worker;
  private next = 1;
  private pending = new Map<number, Pending>();

  constructor() {
    this.worker = this.spawn();
  }

  private spawn(): Worker {
    const worker = new SolverWorker();
    worker.onmessage = (e: MessageEvent) => {
      const { id, progress, decided, trace, result } = e.data;
      const p = this.pending.get(id);
      if (!p) return;
      if (progress) {
        p.progress?.(...progress);
        return;
      }
      if (decided) {
        p.decided?.(decided);
        return;
      }
      if (trace) {
        p.trace?.(trace);
        return;
      }
      this.pending.delete(id);
      if (result && 'error' in result) p.reject(new Error(result.error.msg));
      else p.resolve(result.ok);
    };
    return worker;
  }

  call<T>(
    fn: string,
    args: unknown[],
    progress?: (...p: number[]) => void,
    decided?: (d: unknown) => void,
    trace?: (e: TraceEvent) => void,
  ): Promise<T> {
    const id = this.next++;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject, progress, decided, trace });
      this.worker.postMessage({ id, fn, args, trace: !!trace });
    });
  }

  cancel() {
    this.worker.terminate();
    for (const p of this.pending.values()) p.reject(new Cancelled());
    this.pending.clear();
    this.worker = this.spawn();
  }
}

/** Workers for counting: one per core, keeping one core for the page. */
export const CORES = Math.max(1, Math.min(8, (navigator.hardwareConcurrency || 2) - 1));

/**
 * Several workers splitting one count. Worker k of n gets the arguments plus
 * (k, n) and decides its share; progress values are summed over the workers.
 * Workers start on first use.
 */
export class SolverPool {
  private clients: SolverClient[] = [];
  /** If set, every call reports trace events (worker index, event). */
  trace: ((worker: number, e: TraceEvent) => void) | null = null;

  private traceOf(k: number) {
    const t = this.trace;
    return t ? (e: TraceEvent) => t(k, e) : undefined;
  }

  /** A single call on the first worker. */
  one<T>(fn: string, args: unknown[], progress?: (...p: number[]) => void): Promise<T> {
    if (!this.clients.length) this.clients.push(new SolverClient());
    return this.clients[0].call<T>(fn, args, progress, undefined, this.traceOf(0));
  }

  all<T>(fn: string, args: unknown[], progress?: (...p: number[]) => void, decided?: (d: unknown) => void): Promise<T[]> {
    while (this.clients.length < CORES) this.clients.push(new SolverClient());
    const n = this.clients.length;
    const last: number[][] = Array.from({ length: n }, () => []);
    const report = (k: number, p: number[]) => {
      last[k] = p;
      progress?.(...p.map((_, i) => last.reduce((s, q) => s + (q[i] ?? 0), 0)));
    };
    return Promise.all(this.clients.map((c, k) => c.call<T>(fn, [...args, k, n], (...p) => report(k, p), decided, this.traceOf(k))));
  }

  cancel() {
    for (const c of this.clients) c.cancel();
  }
}

/** One draft count from the parts of a [`SolverPool`]. */
export function mergeDrafts(parts: DraftsOut[]): DraftsOut {
  const sum = (f: (d: DraftsOut) => number) => parts.reduce((s, d) => s + f(d), 0);
  const closest = parts
    .map((d) => d.closest)
    .filter((c): c is DraftLine => c !== null)
    .sort((a, b) => (a.distance ?? 0) - (b.distance ?? 0))[0];
  return {
    ...parts[0],
    checked: sum((d) => d.checked),
    working: sum((d) => d.working),
    undecided: sum((d) => d.undecided),
    example: parts.find((d) => d.example)?.example ?? null,
    closest: closest ?? null,
  };
}

// ---- result shapes ----

export interface TrickOut {
  n: number;
  leader: number;
  cards: string[];
  winner: number;
}

export type SolveOut =
  | { verdict: 'feasible'; line: string[]; tricks: TrickOut[]; stats: { nodes: number } }
  | { verdict: 'infeasible'; stats: { nodes: number } }
  | { verdict: 'redeal'; task: string }
  | { verdict: 'cancelled' };

export type SolveDraftsOut =
  | { verdict: 'feasible'; owners: string[]; line: string[]; tricks: TrickOut[]; stats: { nodes: number } }
  | { verdict: 'infeasible'; stats: { nodes: number } }
  | { verdict: 'redeal'; task: string }
  | { verdict: 'cancelled' | 'undecided' };

export interface DraftLine {
  owners: string[];
  line: string[];
  tricks: TrickOut[];
  distance?: number;
}

/** One decided draft, streamed while the count runs. */
export interface DraftRow {
  owners: string[];
  status: 'works' | 'fails';
  line?: string[];
  tricks?: TrickOut[];
}

/** Two tasks one player cannot fulfil together, even without the other tasks. */
export interface PairConflict {
  tasks: [number, number];
  /** Players (0-based) for whom the pair fails. */
  players: number[];
  /** Players for whom both tasks are possible alone. */
  tried: number[];
}

export interface DraftsOut {
  total: number;
  checked: number;
  working: number;
  undecided: number;
  sampled: boolean;
  any_works: boolean | null;
  example: DraftLine | null;
  closest: DraftLine | null;
  impossible_pairs: { task: number; player: number }[];
}

export type NoReturnOut =
  | null
  | { kind: 'from_start' }
  | { kind: 'still_winnable' }
  | { kind: 'at'; trick: number; player: number; card: string; alternatives: string[] };

export interface TightnessOut {
  critical: number;
  choices: number;
  tightness: number;
}
