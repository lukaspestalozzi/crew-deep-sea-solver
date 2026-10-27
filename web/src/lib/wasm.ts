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
  /** Specific cards the task names, for chips. */
  cards: string[];
  /** A card of the game; generated tasks have an inferred difficulty. */
  official: boolean;
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
/** Official or generated task (parametric ID); undefined if the ID is invalid. */
export function taskById(id: string): TaskCard | undefined {
  const known = byId.get(id);
  if (known) return known;
  const r = resolveTask(id);
  if ('error' in r) return undefined;
  byId.set(id, r.ok);
  return r.ok;
}
export const resolveTask = (id: string): Result<TaskCard> => JSON.parse(w.task_info(id));

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

/** A position in play (owners fixed): see `position` in the WASM API. */
export interface PositionInfo {
  to_play: number;
  /** 1-based number of the trick in progress, cards already in it, its leader. */
  trick: number;
  in_trick: number;
  leader: number;
  current: string[];
  over: boolean;
  hands: string[][];
  tricks_won: number[];
  legal: string[];
  /** Names of the move orders, and the legal moves as each ranks them. */
  orders: string[];
  rankings: string[][];
  /** Per task, at the last trick boundary. */
  status: ('open' | 'done' | 'failed')[];
}

export const positionInfo = (text: string): Result<PositionInfo> => JSON.parse(w.position(text));

export const randomSituation = (players: number, target: number): Result<string> =>
  JSON.parse(w.random_situation(players, target, Math.floor(Math.random() * 2 ** 31)));

/** Write a single situation as text, or throw with the error message. */
export function toText(s: Situation): string {
  const r = write([s]);
  if ('error' in r) throw new Error(r.error.msg);
  return r.ok;
}
