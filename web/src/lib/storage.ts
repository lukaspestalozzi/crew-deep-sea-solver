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

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE, { keyPath: 'id' });
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function tx<T>(mode: IDBTransactionMode, fn: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const db = await open();
  return new Promise((resolve, reject) => {
    const req = fn(db.transaction(STORE, mode).objectStore(STORE));
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

export const newId = () => `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
