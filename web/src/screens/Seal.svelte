<script lang="ts">
  // Pass the phone: each player privately taps their own hand. Other players'
  // choices are never shown; conflicts are reported only at the end.
  import Header from '../components/Header.svelte';
  import CardGrid from '../components/CardGrid.svelte';
  import { ALL_CARDS, sortCards } from '../lib/cards';
  import { i18n } from '../lib/i18n.svelte';
  import { router, session } from '../lib/state.svelte';

  const n = session.players;
  const base = Math.floor(40 / n);
  let step = $state(0);
  let revealed = $state(false);
  let picks = $state<string[][]>(Array.from({ length: n }, () => []));
  let error = $state<string | null>(null);

  const last = $derived(step === n - 1);
  // The last player gets every card nobody picked; they only confirm.
  const rest = $derived(ALL_CARDS.filter((c) => !picks.slice(0, n - 1).some((h) => h.includes(c))));
  const mine = $derived(last ? rest : picks[step]);
  const sizeOk = $derived(mine.length === base || (40 % n !== 0 && mine.length === base + 1));
  const holdsS4 = $derived(mine.includes('S4'));

  function tap(card: string) {
    if (last) return;
    const h = picks[step];
    picks[step] = h.includes(card) ? h.filter((c) => c !== card) : h.length < base + (40 % n ? 1 : 0) ? [...h, card] : h;
  }

  function done() {
    if (!last) {
      step += 1;
      revealed = false;
      return;
    }
    picks[n - 1] = rest;
    const seen = new Map<string, number>();
    const dup: string[] = [];
    picks.forEach((h) => h.forEach((c) => seen.set(c, (seen.get(c) ?? 0) + 1)));
    for (const [c, k] of seen) if (k > 1) dup.push(c);
    const extra = picks.filter((h) => h.length > base).length;
    if (dup.length || extra > 40 % n) {
      error = i18n.t('duplicateClaims', { cards: sortCards(dup).join(' ') || '–' });
      step = 0;
      revealed = false;
      return;
    }
    const owner: Record<string, number> = {};
    picks.forEach((h, p) => h.forEach((c) => (owner[c] = p)));
    session.owner = owner;
    session.sealed = true;
    router.go('/sealed');
  }
</script>

<main class="screen">
  <Header title="{i18n.player(step)} · {step + 1}/{n}" back="/" />
  {#if error}<div class="error">{error}</div>{/if}

  {#if !revealed}
    <div class="grow" style="display: flex; flex-direction: column; justify-content: center; gap: 16px; text-align: center">
      <h1>{i18n.t('passTo', { p: i18n.player(step) })}</h1>
      <button class="btn primary" onclick={() => (revealed = true)}>{i18n.t('imReady', { p: i18n.player(step) })}</button>
    </div>
  {:else}
    <div style="display: flex; flex-direction: column; gap: 4px">
      <h1>{i18n.t('selectYourHand', { p: i18n.player(step) })}</h1>
      <p class="small muted">{i18n.t('onlyLook', { p: i18n.player(step) })}</p>
    </div>
    <CardGrid cell={(c) => (mine.includes(c) ? 'mine' : last ? 'disabled' : 'free')} ontap={tap} />
    <div class="panel row between">
      <strong>{mine.length} / {base}{40 % n ? '–' + (base + 1) : ''}</strong>
      {#if holdsS4}<span style="color: var(--accent)" class="small">{i18n.t('youAreCaptain')}</span>{/if}
    </div>
    <div class="grow"></div>
    <button class="btn primary" disabled={!sizeOk} onclick={done}>
      {last ? i18n.t('doneSealing') : i18n.t('hideAndPass', { p: i18n.player(step + 1) })}
    </button>
  {/if}
</main>
