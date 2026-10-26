// Saved situations in IndexedDB (this device only).

export type SituationType = 'sealed' | 'full' | 'reconstructed' | 'explore';

export interface SavedRecord {
  id: string;
  title: string;
  type: SituationType;
  players: number;
  updated: number;
  /** The situation in crew 1 text. */
  text: string;
  /** Last verdict, for the list only; opening recomputes. */
  verdict?: 'feasible' | 'infeasible' | 'redeal' | null;
}

const DB = 'deep-sea-solver';
const STORE = 'situations';
/** Searches that took long (the Inspector's slow-case log). */
const SLOW = 'slow';

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 2);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(STORE)) db.createObjectStore(STORE, { keyPath: 'id' });
      if (!db.objectStoreNames.contains(SLOW)) db.createObjectStore(SLOW, { keyPath: 'id' });
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function tx<T>(mode: IDBTransactionMode, fn: (s: IDBObjectStore) => IDBRequest<T>, store = STORE): Promise<T> {
  const db = await open();
  return new Promise((resolve, reject) => {
    const req = fn(db.transaction(store, mode).objectStore(store));
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

export async function listSaved(): Promise<SavedRecord[]> {
  const all = await tx<SavedRecord[]>('readonly', (s) => s.getAll() as IDBRequest<SavedRecord[]>);
  return all.sort((a, b) => b.updated - a.updated);
}

export const putSaved = (r: SavedRecord) => tx('readwrite', (s) => s.put(r));
export const getSaved = (id: string) => tx<SavedRecord | undefined>('readonly', (s) => s.get(id));
export const deleteSaved = (id: string) => tx('readwrite', (s) => s.delete(id));

/** A slow search, kept to reproduce it later. */
export interface SlowRecord {
  id: string;
  /** Where it came from: the mission's title and the search's label path. */
  mission: string;
  label: string;
  /** The exact situation searched, in crew 1 text. */
  text: string;
  /** Time and positions so far (final once `result` is set). */
  ms: number;
  nodes: number;
  result: string | null;
  date: number;
}

export async function listSlow(): Promise<SlowRecord[]> {
  const all = await tx<SlowRecord[]>('readonly', (s) => s.getAll() as IDBRequest<SlowRecord[]>, SLOW);
  return all.sort((a, b) => b.date - a.date);
}

export const putSlow = (r: SlowRecord) => tx('readwrite', (s) => s.put(r), SLOW);
export const deleteSlow = (id: string) => tx('readwrite', (s) => s.delete(id), SLOW);
export const clearSlow = () => tx('readwrite', (s) => s.clear(), SLOW);

export const newId = () => `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
