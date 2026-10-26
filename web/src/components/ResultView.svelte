<script lang="ts">
  // The answer for a situation text: verdict, why it fails, a winning line.
  // Everything on demand is on the Analysis screen (AnalysisView).
  import { untrack, type Snippet } from 'svelte';
  import TrickList from './TrickList.svelte';
  import Card from './Card.svelte';
  import { winningPosition } from '../lib/cards';
  import { i18n } from '../lib/i18n.svelte';
  import { mission, type Verdict } from '../lib/mission.svelte';
  import type { TrickOut } from '../lib/solver';
  import { textOf } from '../lib/tasks';

  let {
    text,
    onverdict = () => {},
    actions,
  }: { text: string; onverdict?: (v: Verdict) => void; actions?: Snippet } = $props();

  // Load only when the text changes, not when the mission's state does.
  $effect(() => {
    const t = text;
    untrack(() => mission.load(t));
  });
  $effect(() => onverdict(mission.verdict));

  const m = mission;
  const job = m.verdictJob;
  let step = $state<number | null>(null);

  // The entered draft's line, or the working draft found.
  const shownLine = $derived.by((): { tricks: TrickOut[]; owners?: string[] } | null => {
    if (m.solveOut?.verdict === 'feasible') return { tricks: m.solveOut.tricks };
    if (m.unknownOwners && m.draftsOut?.verdict === 'feasible') return { tricks: m.draftsOut.tricks, owners: m.draftsOut.owners };
    return null;
  });
  const playedTricks = $derived.by((): TrickOut[] => {
    const s = m.situation;
    if (!s) return [];
    const n = s.hands.length;
    return s.played
      .filter((t) => t.cards.length === n)
      .map((t, i) => ({ n: i + 1, leader: t.leader, cards: t.cards, winner: (t.leader + winningPosition(t.cards)) % n }));
  });
</script>

{#if job.error}
  <div class="error">{job.error}</div>
{:else}
  {#if m.verdict === 'feasible'}
    <div class="verdict good">
      <div class="row">
        <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="var(--good)" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7" /></svg>
        <span class="v">{i18n.t('feasible')}</span>
      </div>
      <span class="sub">{m.unknownOwners ? i18n.t('feasibleAnyDraft') : i18n.t('feasibleSub')}</span>
    </div>
  {:else if m.verdict === 'infeasible'}
    <div class="verdict bad">
      <div class="row">
        <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="var(--bad-text)" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
        <span class="v">{i18n.t('notFeasible')}</span>
      </div>
      <span class="sub">{m.unknownOwners ? i18n.t('notFeasibleAnyDraft') : i18n.t('notFeasibleSub')}</span>
    </div>
  {:else if m.verdict === 'redeal'}
    <div class="verdict neutral">
      <span class="v">{i18n.t('redeal')}</span>
      <span class="sub">{i18n.t('redealSub', { task: textOf(m.redeal ?? '') })}</span>
    </div>
  {/if}

  {#if job.running}
    <div class="panel row between" style="flex-direction: row">
      <span>{i18n.t('computing')} <span class="small muted">{job.progress}</span></span>
      <button class="btn small" onclick={() => m.cancelVerdict()}>{i18n.t('cancel')}</button>
    </div>
  {:else if job.cancelled}
    <div class="panel"><span class="muted">{job.last ? i18n.t('cancelledAt', { state: job.last }) : i18n.t('cancelled')}</span></div>
  {/if}

  {@render actions?.()}

  {#if m.situation && m.conflict && m.conflict.length}
    <div class="panel">
      <span class="label">{i18n.t('whyFails')}</span>
      <span class="small muted">{m.conflict.length === 1 ? i18n.t('impossibleAlone') : i18n.t('conflictSub')}</span>
      {#each m.conflict as i}
        <span class="item">{i18n.owner(m.situation.tasks[i].owner)} · {textOf(m.situation.tasks[i].id)}</span>
      {/each}
    </div>
  {/if}

  {#if m.noReturn}
    {@const nr = m.noReturn}
    <div class="panel dashed">
      <span class="label">{i18n.t('whereWrong')}</span>
      {#if nr.kind === 'at'}
        <span>{i18n.t('lostAt', { t: nr.trick + 1, p: i18n.player(nr.player), c: nr.card })}</span>
        {#if nr.alternatives.length}
          <span class="row small" style="gap: 4px; flex-wrap: wrap">{i18n.t('wouldHaveKept', { cards: '' })}{#each nr.alternatives as c}<Card card={c} />{/each}</span>
        {:else}
          <span class="small muted">{i18n.t('noAlternative')}</span>
        {/if}
      {:else if nr.kind === 'from_start'}
        <span>{i18n.t('neverWinnable')}</span>
      {:else}
        <span>{i18n.t('stillWinnable')}</span>
      {/if}
    </div>
  {/if}

  {#if m.situation && playedTricks.length}
    <span class="label">{i18n.t('playedTricks')}</span>
    <TrickList tricks={playedTricks} players={m.situation.hands.length} />
  {/if}

  {#if m.situation && shownLine && shownLine.tricks.length}
    <div class="row between">
      <span class="label">{i18n.t('aWinningLine')}</span>
      {#if step === null}
        <button class="btn small outline-accent" onclick={() => (step = 1)}>{i18n.t('replay')}</button>
      {:else}
        <span class="row">
          <button class="btn small" disabled={step <= 1} onclick={() => (step = step! - 1)} aria-label="previous">◀</button>
          <span class="small muted">{step}/{shownLine.tricks.length}</span>
          <button class="btn small" disabled={step >= shownLine.tricks.length} onclick={() => (step = step! + 1)} aria-label="next">▶</button>
          <button class="btn small" onclick={() => (step = null)}>{i18n.t('showAll')}</button>
        </span>
      {/if}
    </div>
    {#if shownLine.owners}
      <div class="panel small">
        {#each m.situation.tasks as t, i}
          <span>{i18n.owner(shownLine.owners[i])} · {textOf(t.id)}</span>
        {/each}
      </div>
    {/if}
    <TrickList tricks={step === null ? shownLine.tricks : shownLine.tricks.slice(0, step)} players={m.situation.hands.length} />
    {#if step === null && shownLine.tricks.at(-1)!.n < Math.floor(40 / m.situation.hands.length)}
      <p class="small muted">{i18n.t('restIrrelevant', { t: shownLine.tricks.at(-1)!.n })}</p>
    {/if}
  {/if}
{/if}

<style>
  .verdict {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 16px;
    border-radius: 16px;
  }
  .verdict .v {
    font-size: 24px;
    font-weight: 700;
  }
  .verdict .sub {
    font-size: 14px;
    line-height: 1.45;
  }
  .good {
    background: var(--good-bg);
    border: 1px solid var(--good-border);
  }
  .good .sub {
    color: var(--good-text);
  }
  .bad {
    background: var(--bad-bg);
    border: 1px solid var(--bad-border);
  }
  .bad .sub {
    color: var(--bad-text);
  }
  .neutral {
    background: var(--surface);
    border: 1px solid var(--border);
  }
  .item {
    font-size: 15px;
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--bg);
  }
</style>
