import { i18n } from './i18n.svelte';
import { getCatalogue, taskById, type TaskCard } from './wasm';

export const CATEGORIES = [
  'cards',
  'none',
  'counts',
  'values',
  'trick',
  'with',
  'tricks',
  'not-first',
  'number',
  'row',
  'captain',
  'lead',
  'other',
  'rule',
] as const;

const CATEGORY_NAMES: Record<string, [string, string]> = {
  cards: ['Win cards', 'Karten gewinnen'],
  none: ['Win none', 'Nichts gewinnen'],
  counts: ['Colour counts', 'Farbanzahl'],
  values: ['Value counts', 'Werteanzahl'],
  trick: ['Trick property', 'Stich-Eigenschaft'],
  with: ['Win with', 'Gewinnen mit'],
  tricks: ['Specific tricks', 'Bestimmte Stiche'],
  'not-first': ['Not the first', 'Nicht die ersten'],
  number: ['Trick count', 'Stichzahl'],
  row: ['In a row', 'In Folge'],
  captain: ['Vs captain', 'Gegen Kapitän'],
  lead: ['Leading', 'Ausspielen'],
  other: ['Other', 'Andere'],
  rule: ['Rule cards', 'Regelkarten'],
};

export const categoryName = (c: string) => CATEGORY_NAMES[c]?.[i18n.lang === 'de' ? 1 : 0] ?? c;

/** Display text; German falls back to English until the cards are transcribed. */
export function taskText(t: TaskCard | undefined): string {
  if (!t) return '?';
  return i18n.lang === 'de' && t.text_de ? t.text_de : t.text_en;
}

export const textOf = (id: string) => taskText(taskById(id));

export function difficultyOf(id: string, players: number): number {
  const t = taskById(id);
  return t?.difficulty ? t.difficulty[players - 3] : 0;
}

/** Draw task cards at random until the difficulty reaches `target` (skipping overshoots). */
export function drawToTarget(current: string[], players: number, target: number): string[] {
  const pool = getCatalogue().filter((t) => !t.rule && !current.includes(t.id));
  for (let attempt = 0; attempt < 50; attempt++) {
    const deck = [...pool].sort(() => Math.random() - 0.5);
    let sum = current.reduce((a, id) => a + difficultyOf(id, players), 0);
    const out = [...current];
    for (const t of deck) {
      const d = t.difficulty![players - 3];
      if (sum + d <= target) {
        out.push(t.id);
        sum += d;
      }
      if (sum === target) return out;
    }
  }
  return current;
}
