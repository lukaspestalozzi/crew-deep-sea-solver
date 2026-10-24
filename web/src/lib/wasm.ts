// Fast, synchronous solver functions on the main thread (parse, write,
// check, catalogue). Long-running searches go through the worker client.
import init, * as w from '../wasm/crew_wasm.js';

export interface TaskCard {
  id: string;
  category: string;
  difficulty: [number, number, number] | null;
  text_en: string;
  text_de: string;
  doubtful: boolean;
  note: string | null;
  rule: boolean;
  captain_allowed: boolean;
}

export interface Trick {
  leader: number;
  cards: string[];
}

export interface Situation {
  title?: string | null;
  mission?: number | null;
  target?: number | null;
  sealed: boolean;
  hands: string[][];
  tasks: { id: string; owner: string }[];
  played: Trick[];
}

export type Result<T> = { ok: T } | { error: { line?: number; msg: string } };

let catalogue: TaskCard[] = [];
let byId = new Map<string, TaskCard>();

export async function loadWasm() {
  await init();
  catalogue = (JSON.parse(w.catalogue_json()) as { ok: TaskCard[] }).ok;
  byId = new Map(catalogue.map((t) => [t.id, t]));
}

export const getCatalogue = () => catalogue;
export const taskById = (id: string) => byId.get(id);

export const parse = (text: string): Result<Situation[]> => JSON.parse(w.parse(text));
export const write = (list: Situation[]): Result<string> => JSON.parse(w.write(JSON.stringify(list)));

export interface Check {
  captain: number;
  difficulty: number;
  tricks_played: number;
  to_play: number;
  redeal: string | null;
  drafts: number;
}
export const check = (text: string): Result<Check> => JSON.parse(w.check(text));

export const randomSituation = (players: number, target: number): Result<string> =>
  JSON.parse(w.random_situation(players, target, Math.floor(Math.random() * 2 ** 31)));

/** Write a single situation as text, or throw with the error message. */
export function toText(s: Situation): string {
  const r = write([s]);
  if ('error' in r) throw new Error(r.error.msg);
  return r.ok;
}
