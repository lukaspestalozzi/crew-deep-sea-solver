// The situation being worked on, shared by all screens, plus the router.
import { ALL_CARDS, sortCards } from './cards';
import type { Situation, Trick } from './wasm';
import type { SituationType } from './storage';

export type Mode = 'full' | 'sealed' | 'reconstruct' | 'live' | 'explore';

export interface TaskEntry {
  id: string;
  /** "Player1".."Player5", "?" or "crew". */
  owner: string;
}

class Session {
  players = $state(4);
  mode = $state<Mode>('full');
  /** Card -> player index, while entering hands. */
  owner = $state<Record<string, number>>({});
  tasks = $state<TaskEntry[]>([]);
  played = $state<Trick[]>([]);
  title = $state<string | null>(null);
  mission = $state<number | null>(null);
  target = $state<number | null>(null);
  sealed = $state(false);
  savedId = $state<string | null>(null);

  reset(mode: Mode, players = this.players) {
    this.mode = mode;
    this.players = players;
    this.owner = {};
    this.tasks = [];
    this.played = [];
    this.title = null;
    this.mission = null;
    this.target = null;
    this.sealed = mode === 'sealed';
    this.savedId = null;
  }

  hand(p: number): string[] {
    return sortCards(ALL_CARDS.filter((c) => this.owner[c] === p));
  }

  hands(): string[][] {
    return Array.from({ length: this.players }, (_, p) => this.hand(p));
  }

  unassigned(): string[] {
    return ALL_CARDS.filter((c) => this.owner[c] === undefined);
  }

  /** Holder of S4, if assigned. */
  captain(): number | null {
    return this.owner['S4'] ?? null;
  }

  situation(): Situation {
    return {
      title: this.title,
      mission: this.mission,
      target: this.target,
      sealed: this.sealed,
      hands: this.hands(),
      tasks: this.tasks.map((t) => ({ ...t })),
      played: this.played.map((t) => ({ leader: t.leader, cards: [...t.cards] })),
    };
  }

  load(s: Situation, mode: Mode, savedId: string | null = null) {
    this.reset(mode, s.hands.length);
    const owner: Record<string, number> = {};
    s.hands.forEach((h, p) => h.forEach((c) => (owner[c] = p)));
    this.owner = owner;
    this.tasks = s.tasks.map((t) => ({ ...t }));
    this.played = s.played.map((t) => ({ leader: t.leader, cards: [...t.cards] }));
    this.title = s.title ?? null;
    this.mission = s.mission ?? null;
    this.target = s.target ?? null;
    this.sealed = s.sealed;
    this.savedId = savedId;
  }

  savedType(): SituationType {
    if (this.sealed && this.mode === 'sealed') return 'sealed';
    if (this.mode === 'reconstruct' || this.played.length > 0) return 'reconstructed';
    if (this.mode === 'explore') return 'explore';
    return 'full';
  }

  snapshot() {
    return {
      players: this.players,
      mode: this.mode,
      owner: this.owner,
      tasks: this.tasks,
      played: this.played,
      title: this.title,
      mission: this.mission,
      target: this.target,
      sealed: this.sealed,
      savedId: this.savedId,
    };
  }

  /** Restore a stored snapshot; throws (changing nothing) if it is not shaped like one. */
  restore(snap: unknown) {
    const s = snap as ReturnType<Session['snapshot']> | null;
    const isNum = (x: unknown) => typeof x === 'number' && Number.isFinite(x);
    const orNull = (x: unknown, ok: (x: unknown) => boolean) => x === null || ok(x);
    const valid =
      !!s &&
      typeof s === 'object' &&
      Number.isInteger(s.players) &&
      s.players >= 1 &&
      s.players <= 5 &&
      ['full', 'sealed', 'reconstruct', 'live', 'explore'].includes(s.mode) &&
      !!s.owner &&
      typeof s.owner === 'object' &&
      Object.values(s.owner).every(isNum) &&
      Array.isArray(s.tasks) &&
      s.tasks.every((t) => !!t && typeof t.id === 'string' && typeof t.owner === 'string') &&
      Array.isArray(s.played) &&
      s.played.every((t) => !!t && isNum(t.leader) && Array.isArray(t.cards) && t.cards.every((c) => typeof c === 'string')) &&
      orNull(s.title, (x) => typeof x === 'string') &&
      orNull(s.mission, isNum) &&
      orNull(s.target, isNum) &&
      typeof s.sealed === 'boolean' &&
      orNull(s.savedId, (x) => typeof x === 'string');
    if (!valid) throw new Error('bad snapshot');
    Object.assign(this, s);
  }
}

export const session = new Session();

// ---- router ----

function currentPath() {
  return location.hash.replace(/^#/, '') || '/';
}

class Router {
  path = $state(currentPath());

  constructor() {
    addEventListener('hashchange', () => {
      this.path = currentPath();
      scrollTo(0, 0);
    });
  }

  go(path: string) {
    location.hash = path;
  }
}

export const router = new Router();
