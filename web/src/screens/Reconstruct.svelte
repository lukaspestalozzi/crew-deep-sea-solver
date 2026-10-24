<script lang="ts">
  // Enter tricks card by card. Reconstruct: hands unknown, cards only checked
  // for not being played twice. Live: hands sealed, each card is checked
  // against the player's hand and the follow-suit rule; owners stay hidden.
  import Header from '../components/Header.svelte';
  import CardGrid from '../components/CardGrid.svelte';
  import Card from '../components/Card.svelte';
  import { ALL_CARDS, legalCards, winningPosition } from '../lib/cards';
  import { i18n } from '../lib/i18n.svelte';
  import { router, session } from '../lib/state.svelte';

  let { live }: { live: boolean } = $props();

  const n = session.players;
  let error = $state<string | null>(null);
  // Reconstruct needs the first leader (the captain) to be named. Each route
  // mounts its own instance, so reading `live` once is intended.
  // svelte-ignore state_referenced_locally
  let firstLeader = $state<number | null>(live ? session.captain() : (session.played[0]?.leader ?? null));

  const played = $derived(new Set(session.played.flatMap((t) => t.cards)));
  const current = $derived(session.played.length && session.played.at(-1)!.cards.length < n ? session.played.at(-1)! : null);
  const complete = $derived(session.played.filter((t) => t.cards.length === n));
  const leader = $derived.by(() => {
    if (current) return current.leader;
    const lastFull = complete.at(-1);
    if (!lastFull) return firstLeader;
    return (lastFull.leader + winningPosition(lastFull.cards)) % n;
  });
  const toPlay = $derived(leader === null ? null : (leader + (current?.cards.length ?? 0)) % n);
  const totalTricks = Math.floor(40 / n);
  const gameOver = $derived(complete.length === totalTricks);

  function winnerOf(t: { leader: number; cards: string[] }) {
    return (t.leader + winningPosition(t.cards)) % n;
  }

  function tap(card: string) {
    error = null;
    if (toPlay === null || leader === null || played.has(card) || gameOver) return;
    if (live) {
      const hand = ALL_CARDS.filter((c) => session.owner[c] === toPlay && !played.has(c));
      if (!hand.includes(card)) return (error = i18n.t('notInHand', { c: card, p: i18n.player(toPlay) }));
      if (!legalCards(hand, current?.cards ?? []).includes(card)) return (error = i18n.t('mustFollow', { p: i18n.player(toPlay) }));
    }
    if (current) current.cards = [...current.cards, card];
    else session.played = [...session.played, { leader, cards: [card] }];
    session.played = [...session.played];
  }

  function undo() {
    error = null;
    const lastT = session.played.at(-1);
    if (!lastT) return;
    if (lastT.cards.length <= 1) session.played = session.played.slice(0, -1);
    else {
      lastT.cards = lastT.cards.slice(0, -1);
      session.played = [...session.played];
    }
  }

  function toHands() {
    // Played cards belong to whoever played them.
    const owner: Record<string, number> = { ...session.owner };
    for (const t of session.played) t.cards.forEach((c, i) => (owner[c] = (t.leader + i) % n));
    session.owner = owner;
    router.go('/deal');
  }

  function finishLive() {
    session.sealed = false;
    router.go('/tasks');
  }
</script>

<main class="screen">
  <Header title={live ? i18n.t('liveLog') : i18n.t('reconstruct')} back={live ? '/sealed' : '/'}>
    {#snippet right()}
      <button class="icon-btn small" style="color: var(--accent); width: auto; padding: 0 6px" onclick={undo}>{i18n.t('undo')}</button>
    {/snippet}
  </Header>

  {#if firstLeader === null}
    <p>{i18n.t('whoLeadsFirst')}</p>
    <div class="cols-2">
      {#each Array(n) as _, p}
        <button class="btn" onclick={() => (firstLeader = p)}>{i18n.player(p)}</button>
      {/each}
    </div>
  {:else}
    <div style="display: flex; flex-direction: column; gap: 4px">
      {#each complete as t, i}
        <div class="row panel" style="padding: 6px 10px; flex-direction: row; gap: 8px">
          <span class="small muted" style="width: 20px">{i + 1}</span>
          <span class="row grow" style="gap: 4px; flex-wrap: wrap">{#each t.cards as c}<Card card={c} />{/each}</span>
          <span class="small muted">→ {i18n.player(winnerOf(t))}</span>
        </div>
      {/each}
    </div>

    {#if !gameOver && toPlay !== null && leader !== null}
      <div class="panel" style="border: 2px solid var(--accent); background: transparent">
        <div class="row between small">
          <span><strong>{i18n.t('trickN', { n: complete.length + 1 })}</strong> · {i18n.t('leads', { p: i18n.player(leader) })}</span>
        </div>
        <div style="display: grid; grid-template-columns: repeat({n}, minmax(0, 1fr)); gap: 6px">
          {#each Array(n) as _, i}
            {@const p = (leader + i) % n}
            {@const c = current?.cards[i]}
            <div style="display: flex; flex-direction: column; align-items: center; gap: 2px; min-width: 0">
              <span style="font-size: 10.5px" class="muted">{i18n.player(p)}</span>
              {#if c}<Card card={c} />{:else}<span class="chip-card" style="border: 2px dashed var(--accent); color: var(--accent)">?</span>{/if}
            </div>
          {/each}
        </div>
      </div>
      <p class="small">
        {i18n.t('tapCardPlayed', { p: i18n.player(toPlay) })}
        {#if live}<span class="muted">{i18n.t('ownersHidden')}</span>{/if}
      </p>
    {/if}

    {#if error}<div class="error">{error}</div>{/if}
    <CardGrid compact cell={(c) => (played.has(c) ? 'gone' : 'free')} ontap={tap} />
    <div class="grow"></div>
    {#if live}
      <div class="cols-2">
        <button class="btn" onclick={finishLive}>{i18n.t('missionFailed')}</button>
        <button class="btn" onclick={finishLive}>{i18n.t('missionWon')}</button>
      </div>
    {:else}
      <button class="btn primary" disabled={session.played.length === 0} onclick={toHands}>{i18n.t('nextHands')}</button>
    {/if}
  {/if}
</main>
