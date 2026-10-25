// Which tasks go to whom, and which go together, from the decided drafts.
// Every figure compares the working drafts with all drafts the draft rules
// allow, so patterns forced by the rules alone are not reported.
import type { DraftRow } from './solver';

/** Chosen owners: task index -> owner ("Player2"). */
export type Filter = Map<number, string>;

export interface Count {
  /** Working drafts with this property. */
  w: number;
  /** All decided drafts with this property. */
  a: number;
}

export interface Pair {
  t1: number;
  t2: number;
  /** Drafts where both tasks have the same owner. */
  same: Count;
}

export interface DraftStats {
  /** Tasks whose owner varies between drafts. */
  tasks: number[];
  /** Owners that occur for those tasks, sorted. */
  owners: string[];
  /** Working and all drafts that match the filter. */
  works: DraftRow[];
  all: number;
  /** cell(task, owner): drafts in which that owner has the task. */
  cell: (t: number, o: string) => Count;
  always: Pair[];
  never: Pair[];
  /** Pairs whose share of "same owner" among working drafts differs most from all drafts. */
  tendencies: Pair[];
}

export const matches = (r: DraftRow, f: Filter) => [...f].every(([t, o]) => r.owners[t] === o);

/** "must": every working draft has it while the rules allow otherwise; "never": none has it though the rules allow it. */
export function mark(c: Count, works: number, all: number): 'must' | 'never' | null {
  if (works === 0) return null;
  if (c.w === works && c.a < all) return 'must';
  if (c.w === 0 && c.a > 0) return 'never';
  return null;
}

/** Minimum gap between the two shares for a tendency, and how many are shown. */
const GAP = 0.25;
const TOP = 5;

export function draftStats(rows: DraftRow[], f: Filter): DraftStats {
  const n = rows[0]?.owners.length ?? 0;
  const tasks = [...Array(n).keys()].filter((t) => new Set(rows.map((r) => r.owners[t])).size > 1);
  const owners = [...new Set(tasks.flatMap((t) => rows.map((r) => r.owners[t])))].sort();
  const match = rows.filter((r) => matches(r, f));
  const works = match.filter((r) => r.status === 'works');
  const count = (p: (r: DraftRow) => boolean): Count => ({ w: works.filter(p).length, a: match.filter(p).length });

  const cells = new Map<string, Count>();
  for (const t of tasks) for (const o of owners) cells.set(`${t}/${o}`, count((r) => r.owners[t] === o));

  const always: Pair[] = [];
  const never: Pair[] = [];
  const rest: Pair[] = [];
  for (const [i, t1] of tasks.entries())
    for (const t2 of tasks.slice(i + 1)) {
      const p = { t1, t2, same: count((r) => r.owners[t1] === r.owners[t2]) };
      const m = mark(p.same, works.length, match.length);
      if (m === 'must') always.push(p);
      else if (m === 'never') never.push(p);
      else if (works.length && p.same.a) rest.push(p);
    }
  const gap = (p: Pair) => Math.abs(p.same.w / works.length - p.same.a / match.length);
  const tendencies = rest
    .filter((p) => gap(p) >= GAP)
    .sort((x, y) => gap(y) - gap(x))
    .slice(0, TOP);

  return {
    tasks,
    owners,
    works,
    all: match.length,
    cell: (t, o) => cells.get(`${t}/${o}`) ?? { w: 0, a: 0 },
    always,
    never,
    tendencies,
  };
}
