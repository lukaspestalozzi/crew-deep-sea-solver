// Which tasks share a player in the working drafts of random deals: the
// per-deal figures (from the solver) averaged over the solvable deals.

/** One random deal: see `deal_pairing` in the WASM API. */
export interface DealPairingRow {
  works: number;
  decided: number;
  open: number;
  /** [task i, task j, working drafts with both on one player, decided drafts with both on one player]. */
  same: [number, number, number, number][];
  always: [number, number][];
  never: [number, number][];
}

export interface PairTrend {
  t1: number;
  t2: number;
  /** Mean share of working drafts with both on one player (solvable deals where the rules allow it). */
  works: number;
  /** Mean share of all decided drafts with both on one player (same deals). */
  all: number;
  /** Deals in which the pair is always / never with the same player in working drafts. */
  always: number;
  never: number;
  /** Deals averaged over. */
  deals: number;
}

export interface Pairing {
  deals: number;
  solvable: number;
  /** Deals without a working draft found and with drafts left open (node limit). */
  open: number;
  /** Pairs, most together (compared with the rules) first. */
  pairs: PairTrend[];
}

export function pairTrends(rows: DealPairingRow[]): Pairing {
  const acc = new Map<string, PairTrend>();
  const key = (i: number, j: number) => `${i}/${j}`;
  const get = (i: number, j: number) => {
    let p = acc.get(key(i, j));
    if (!p) acc.set(key(i, j), (p = { t1: i, t2: j, works: 0, all: 0, always: 0, never: 0, deals: 0 }));
    return p;
  };
  let solvable = 0;
  let open = 0;
  for (const r of rows) {
    if (r.works === 0) {
      if (r.open > 0) open++;
      continue;
    }
    solvable++;
    for (const [i, j, w, a] of r.same) {
      const p = get(i, j);
      if (a === 0) continue; // the rules never put them together on this deal
      p.works += w / r.works;
      p.all += a / r.decided;
      p.deals++;
    }
    for (const [i, j] of r.always) get(i, j).always++;
    for (const [i, j] of r.never) get(i, j).never++;
  }
  const pairs = [...acc.values()]
    .filter((p) => p.deals > 0)
    .map((p) => ({ ...p, works: p.works / p.deals, all: p.all / p.deals }))
    .sort((x, y) => y.works - y.all - (x.works - x.all));
  return { deals: rows.length, solvable, open, pairs };
}
