// Cards are strings like "P7" (pink 7) or "S4" (submarine 4).

/** How a card looks in the card grid. */
export type CellState = 'free' | 'mine' | 'other' | 'gone' | 'disabled';

export const SUITS = ['P', 'B', 'G', 'Y', 'S'] as const;
export type Suit = (typeof SUITS)[number];

export const SUIT_COLOR: Record<Suit, string> = {
  P: '#F27BB0',
  B: '#5BA4F0',
  G: '#5CC98A',
  Y: '#F2D04B',
  S: '#C9D3DC',
};

export const maxValue = (s: Suit) => (s === 'S' ? 4 : 9);

export const ALL_CARDS: string[] = SUITS.flatMap((s) =>
  Array.from({ length: maxValue(s) }, (_, i) => `${s}${i + 1}`),
);

export const suitOf = (c: string) => c[0] as Suit;
export const valueOf = (c: string) => Number(c.slice(1));

/** Grid rows by value 1..9, one column per suit; null where no submarine exists. */
export const GRID: (string | null)[][] = Array.from({ length: 9 }, (_, i) =>
  SUITS.map((s) => (i + 1 <= maxValue(s) ? `${s}${i + 1}` : null)),
);

const ORDER = new Map(ALL_CARDS.map((c, i) => [c, i]));
export const sortCards = (cards: string[]) => [...cards].sort((a, b) => ORDER.get(a)! - ORDER.get(b)!);

/** Hand size per player: 40 cards dealt as evenly as possible. */
export function handSize(players: number, extraFor: number | null = null, p = 0): number {
  const base = Math.floor(40 / players);
  return 40 % players !== 0 && extraFor === p ? base + 1 : base;
}

/** Index of the card that wins a trick (cards in play order). */
export function winningPosition(cards: string[]): number {
  const led = suitOf(cards[0]);
  let best = 0;
  for (let i = 1; i < cards.length; i++) {
    const c = cards[i];
    const b = cards[best];
    const cSub = suitOf(c) === 'S';
    const bSub = suitOf(b) === 'S';
    const better = cSub ? !bSub || valueOf(c) > valueOf(b) : !bSub && suitOf(c) === led && valueOf(c) > valueOf(b);
    if (better) best = i;
  }
  return best;
}

/** Legal cards for a hand given the cards already in the trick. */
export function legalCards(hand: string[], trick: string[]): string[] {
  if (trick.length === 0) return hand;
  const led = suitOf(trick[0]);
  const follow = hand.filter((c) => suitOf(c) === led);
  return follow.length ? follow : hand;
}
