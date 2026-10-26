// The Inspector: what the solver is working on, from its trace events. One
// row per unit of work (a search, a draft count, a deal) with time,
// positions, restarts; searches running longer than a threshold go to the
// slow-case log (IndexedDB) to be exported and reproduced. Built to carry a
// bot's decision events later through the same channel.
import type { TraceEvent } from './solver';
import { listSlow, newId, putSlow, deleteSlow, clearSlow, type SlowRecord } from './storage';

export interface WorkRow {
  key: string;
  /** Analysis it belongs to ("verdict", "drafts", "deals", …). */
  job: string;
  worker: number;
  parent: string | null;
  label: string;
  /** The situation it works on, in crew 1 text. */
  text: string;
  start: number;
  end: number | null;
  nodes: number;
  restart: number;
  budget: number;
  cache: number;
  /** Verdict or summary once ended. */
  result: string | null;
  /** A search (has its own positions), not a group of searches. */
  search: boolean;
  /** Id in the slow-case log, once saved there. */
  slowId: string | null;
  /** Positions of the finished searches inside (groups; kept when those rows are dropped). */
  sub: number;
}

/** Rows kept at most; finished fast ones go first. */
const MAX_ROWS = 400;

function stored<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}

function store(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    /* storage unavailable */
  }
}

class Inspector {
  /** The Details switch. */
  on = $state(stored('inspector.on', false));
  /** A search is slow after this many minutes. */
  slowMinutes = $state(stored('inspector.slowMinutes', 2));
  /** Snapshot of the rows for display, refreshed about twice a second. */
  rows = $state.raw<WorkRow[]>([]);
  /** The clock for running rows (refreshed with the rows). */
  now = $state(Date.now());
  slow = $state.raw<SlowRecord[]>([]);
  /** Title of the mission, for the slow-case log. */
  mission = '';

  private map = new Map<string, WorkRow>();
  private run = 0;
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor() {
    void this.loadSlow();
  }

  setOn(v: boolean) {
    this.on = v;
    store('inspector.on', v);
  }

  setSlowMinutes(v: number) {
    this.slowMinutes = Math.max(0.1, v);
    store('inspector.slowMinutes', this.slowMinutes);
  }

  /** A sink for one run of an analysis (keys stay unique across worker restarts). */
  sink(job: string): (worker: number, e: TraceEvent) => void {
    const run = ++this.run;
    this.tick();
    return (worker, e) => this.event(job, `${run}:${worker}:`, worker, e);
  }

  /** End the rows of `job` still running (its workers were stopped). */
  stopRunning(job: string) {
    const now = Date.now();
    for (const r of this.map.values()) {
      if (r.job !== job || r.end !== null) continue;
      r.end = now;
      r.result = 'stopped';
      if (r.slowId) this.checkSlow(r, now);
    }
    this.rows = this.snapshot();
  }

  /** Positions of a row: its own, or for a group those of its searches (finished and running). */
  nodes(r: WorkRow): number {
    if (r.search) return r.nodes;
    let n = r.sub;
    for (const x of this.map.values()) if (x.parent === r.key && x.end === null) n += x.search ? x.nodes : this.nodes(x) - x.sub;
    return n;
  }

  /** Copies, so the lists redraw (the rows themselves change in place). */
  private snapshot = () => [...this.map.values()].map((r) => ({ ...r }));

  /** Forget the rows (a new mission). */
  reset() {
    this.map.clear();
    this.rows = [];
  }

  private event(job: string, prefix: string, worker: number, e: TraceEvent) {
    this.tick();
    const key = prefix + e.id;
    if (e.ev === 'start') {
      this.map.set(key, {
        key,
        job,
        worker,
        parent: e.parent === null ? null : prefix + e.parent,
        label: e.label,
        text: e.text,
        start: e.t,
        end: null,
        nodes: 0,
        restart: 0,
        budget: 0,
        cache: 0,
        result: null,
        search: false,
        slowId: null,
        sub: 0,
      });
      this.trim();
      return;
    }
    const r = this.map.get(key);
    if (!r) return;
    if (e.ev === 'progress') {
      Object.assign(r, { nodes: e.nodes, restart: e.restart, budget: e.budget, cache: e.cache, search: true });
    } else {
      r.end = e.t;
      r.result = e.result;
      if (e.stats) Object.assign(r, { nodes: e.stats.nodes, cache: e.stats.cache_size, search: true });
      // Count a finished search into every unit around it.
      if (r.search) for (let p = r.parent ? this.map.get(r.parent) : undefined; p; p = p.parent ? this.map.get(p.parent) : undefined) p.sub += r.nodes;
      this.checkSlow(r, e.t);
    }
  }

  /** Path of labels from the outermost unit, generic labels left out. */
  path(r: WorkRow): string {
    const parts: string[] = [];
    for (let x: WorkRow | undefined = r; x; x = x.parent ? this.map.get(x.parent) : undefined) {
      if (x.label !== 'search') parts.unshift(x.label);
    }
    return parts.join(' › ');
  }

  elapsed = (r: WorkRow) => (r.end ?? this.now) - r.start;

  private checkSlow(r: WorkRow, now: number) {
    const ms = (r.end ?? now) - r.start;
    if (ms < this.slowMinutes * 60_000) return;
    const rec: SlowRecord = {
      id: r.slowId ?? newId(),
      mission: this.mission,
      label: this.path(r),
      text: r.text,
      ms,
      nodes: this.nodes(r),
      result: r.result,
      date: Date.now(),
    };
    r.slowId = rec.id;
    void putSlow(rec).then(() => this.loadSlow());
  }

  private trim() {
    if (this.map.size <= MAX_ROWS) return;
    const done = [...this.map.values()].filter((r) => r.end !== null && !r.slowId).sort((a, b) => a.end! - b.end!);
    for (const r of done.slice(0, this.map.size - MAX_ROWS)) this.map.delete(r.key);
  }

  /** Refresh the snapshot while anything runs; save searches that became slow. */
  private tick() {
    this.timer ??= setInterval(() => {
      this.now = Date.now();
      let running = false;
      for (const r of this.map.values()) {
        if (r.end !== null) continue;
        running = true;
        // Saved once when crossing the threshold, updated when it ends.
        if (!r.slowId) this.checkSlow(r, this.now);
      }
      this.rows = this.snapshot();
      if (!running && this.timer) {
        clearInterval(this.timer);
        this.timer = null;
      }
    }, 500);
  }

  async loadSlow() {
    try {
      this.slow = await listSlow();
    } catch {
      this.slow = [];
    }
  }

  async deleteSlow(id: string) {
    await deleteSlow(id);
    await this.loadSlow();
  }

  async clearSlow() {
    await clearSlow();
    await this.loadSlow();
  }
}

export const inspector = new Inspector();

/** "850 ms", "12.3 s", "2 min 05 s". */
export function duration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  const s = Math.round(ms / 1000);
  return `${Math.floor(s / 60)} min ${String(s % 60).padStart(2, '0')} s`;
}

/** "950", "12k", "3.4M". */
export function count(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1e6) return `${Math.round(n / 1000)}k`;
  return `${(n / 1e6).toFixed(1)}M`;
}

// For the browser console and test scripts: `crewInspector.rows`.
(globalThis as { crewInspector?: Inspector }).crewInspector = inspector;
