// Client for the solver worker. One call at a time per client; cancel()
// terminates the worker (a running WebAssembly call cannot be interrupted
// otherwise) and starts a fresh one.
import SolverWorker from './worker.ts?worker';

type Pending = {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
  progress?: (...p: number[]) => void;
};

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
      const { id, progress, result } = e.data;
      const p = this.pending.get(id);
      if (!p) return;
      if (progress) {
        p.progress?.(...progress);
        return;
      }
      this.pending.delete(id);
      if (result && 'error' in result) p.reject(new Error(result.error.msg));
      else p.resolve(result.ok);
    };
    return worker;
  }

  call<T>(fn: string, args: unknown[], progress?: (...p: number[]) => void): Promise<T> {
    const id = this.next++;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject, progress });
      this.worker.postMessage({ id, fn, args });
    });
  }

  cancel() {
    this.worker.terminate();
    for (const p of this.pending.values()) p.reject(new Cancelled());
    this.pending.clear();
    this.worker = this.spawn();
  }
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
