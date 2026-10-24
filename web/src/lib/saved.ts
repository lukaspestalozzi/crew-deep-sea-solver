// Opening and saving situations.
import type { Key } from './i18n.svelte';
import { router, session, type Mode } from './state.svelte';
import { newId, putSaved, type SavedRecord } from './storage';
import { parse, toText } from './wasm';

export function verdictPill(v: SavedRecord['verdict']): { key: Key; bg: string } | null {
  if (v === 'feasible') return { key: 'feasible', bg: '#5CC98A' };
  if (v === 'infeasible') return { key: 'notFeasible', bg: '#F27BB0' };
  if (v === 'redeal') return { key: 'redeal', bg: '#C9D3DC' };
  return null;
}

const MODE: Record<SavedRecord['type'], Mode> = {
  sealed: 'sealed',
  full: 'full',
  reconstructed: 'reconstruct',
  explore: 'explore',
};

export function openSaved(r: SavedRecord) {
  const p = parse(r.text);
  if ('error' in p) {
    alert(p.error.msg);
    return;
  }
  session.load(p.ok[0], MODE[r.type], r.id);
  router.go(r.type === 'sealed' ? '/sealed' : '/result');
}

/** Save the current session; returns the record. */
export async function saveCurrent(verdict: SavedRecord['verdict'] = null): Promise<SavedRecord> {
  const s = session.situation();
  const rec: SavedRecord = {
    id: session.savedId ?? newId(),
    title: s.title || `${new Date().toLocaleString()} · ${s.hands.length}p`,
    type: session.savedType(),
    players: s.hands.length,
    updated: Date.now(),
    text: toText(s),
    verdict,
  };
  await putSaved(rec);
  session.savedId = rec.id;
  return rec;
}
