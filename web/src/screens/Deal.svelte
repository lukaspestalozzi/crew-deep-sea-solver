<script lang="ts">
  import Header from '../components/Header.svelte';
  import CardGrid from '../components/CardGrid.svelte';
  import PlayerChips from '../components/PlayerChips.svelte';
  import { ALL_CARDS, type CellState } from '../lib/cards';
  import { i18n } from '../lib/i18n.svelte';
  import { router, session } from '../lib/state.svelte';

  let active = $state(0);
  // In a reconstructed game, cards already played are fixed to whoever played them.
  const locked = $derived(new Set(session.played.flatMap((t) => t.cards)));

  const counts = $derived(Array.from({ length: session.players }, (_, p) => ALL_CARDS.filter((c) => session.owner[c] === p).length));
  const base = $derived(Math.floor(40 / session.players));
  const extra = $derived(40 % session.players); // 1 for 3 players
  // With 3 players one hand has 14 cards: allow base + 1 while no one else has it.
  const max = (p: number) => {
    if (extra === 0) return base;
    const someoneElseHasExtra = counts.some((c, q) => q !== p && c > base);
    return someoneElseHasExtra ? base : base + 1;
  };
  const unassigned = $derived(40 - counts.reduce((a, b) => a + b, 0));
  const open = $derived(counts.map((c, p) => (c < max(p) ? p : -1)).filter((p) => p >= 0));
  const canFill = $derived(open.length === 1 && unassigned > 0 && unassigned <= max(open[0]) - counts[open[0]]);
  const complete = $derived(unassigned === 0);

  function tap(card: string) {
    if (locked.has(card)) return;
    const o = session.owner[card];
    const next = { ...session.owner };
    if (o === active) delete next[card];
    else if (counts[active] < max(active)) next[card] = active;
    else return;
    session.owner = next;
    // Move on to the next player once a hand is full.
    const after = ALL_CARDS.filter((c) => next[c] === active).length;
    if (after >= max(active) && o !== active) {
      const nextOpen = Array.from({ length: session.players }, (_, i) => (active + 1 + i) % session.players).find(
        (p) => ALL_CARDS.filter((c) => next[c] === p).length < max(p),
      );
      if (nextOpen !== undefined) active = nextOpen;
    }
  }

  function fill() {
    const next = { ...session.owner };
    for (const c of ALL_CARDS) if (next[c] === undefined) next[c] = open[0];
    session.owner = next;
  }

  function cellState(card: string): CellState {
    if (locked.has(card)) return 'gone';
    const o = session.owner[card];
    if (o === undefined) return 'free';
    return o === active ? 'mine' : 'other';
  }
</script>

<main class="screen">
  <Header title="{i18n.t('deal')} · {session.players} {i18n.t('players')}" back={locked.size ? '/reconstruct' : '/'} />
  {#if locked.size}<p class="small muted">{i18n.t('handsAfterTricks')}</p>{/if}
  <PlayerChips players={session.players} {active} {counts} {max} captain={session.captain()} onpick={(p) => (active = p)} />
  <p class="small muted">{@html i18n.t('tapToGive', { p: `<strong style="color: var(--text)">${i18n.player(active)}</strong>` })}</p>
  <CardGrid cell={cellState} tag={(c) => (session.owner[c] !== undefined ? String(session.owner[c] + 1) : '')} ontap={tap} />
  <div class="grow"></div>
  {#if canFill}
    <button class="btn outline-accent" style="border-style: dashed" onclick={fill}>
      {i18n.t('giveRest', { n: unassigned, p: i18n.player(open[0]) })}
    </button>
  {/if}
  <div class="row">
    <span class="small muted grow">{i18n.t('unassigned', { n: unassigned })}</span>
    <button class="btn primary" disabled={!complete} onclick={() => router.go('/tasks')}>{i18n.t('nextTasks')}</button>
  </div>
</main>
