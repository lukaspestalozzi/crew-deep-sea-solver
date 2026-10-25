<script lang="ts">
  // Runs the analyses for a situation text in the worker and shows them.
  // The verdict comes first; costly extras (draft count, other drafts,
  // critical decisions, random deals) run when asked for.
  import { onDestroy, untrack } from 'svelte';
  import TrickList from './TrickList.svelte';
  import Card from './Card.svelte';
  import { winningPosition } from '../lib/cards';
  import { i18n } from '../lib/i18n.svelte';
  import { Cancelled, SolverClient, type DraftsOut, type NoReturnOut, type SolveDraftsOut, type SolveOut, type TightnessOut, type TrickOut } from '../lib/solver';
  import { isInferred, textOf } from '../lib/tasks';
  import { check, parse, toText, type Situation } from '../lib/wasm';

  type Verdict = 'feasible' | 'infeasible' | 'redeal' | null;

  let {
    text,
    checkAll = false,
    onverdict = () => {},
  }: { text: string; checkAll?: boolean; onverdict?: (v: Verdict) => void } = $props();

  const LIMIT = 2000;
  const client = new SolverClient();
  onDestroy(() => client.cancel());

  let running = $state(false);
  let cancelled = $state(false);
  /** Last progress line, kept to show what was known when cancelled. */
  let lastProgress = $state('');
  let progress = $state('');
  let error = $state<string | null>(null);
  let situation = $state<Situation | null>(null);
  let difficulty = $state(0);
  let redeal = $state<string | null>(null);
  let solveOut = $state<SolveOut | null>(null);
  let draftsOut = $state<SolveDraftsOut | null>(null);
  let drafts = $state<DraftsOut | null>(null);
  let altDrafts = $state<DraftsOut | null>(null);
  let conflict = $state<number[] | null>(null);
  let noReturn = $state<NoReturnOut>(null);
  let tight = $state<TightnessOut | null>(null);
  let robustness = $state<{ solvable: number; undecided: number; deals: number } | null>(null);
  let robustRunning = $state(false);
  let robustProgress = $state('');
  let step = $state<number | null>(null);

  const unknownOwners = $derived(!!situation?.tasks.some((t) => t.owner === '?'));
  const verdict = $derived<Verdict>(
    redeal
      ? 'redeal'
      : unknownOwners
        ? draftsOut?.verdict === 'feasible' || draftsOut?.verdict === 'infeasible'
          ? draftsOut.verdict
          : null
        : solveOut?.verdict === 'feasible' || solveOut?.verdict === 'infeasible'
          ? solveOut.verdict
          : null,
  );
  $effect(() => onverdict(verdict));

  // The line to show: the entered draft's, or a working draft's.
  const shownLine = $derived.by((): { tricks: TrickOut[]; owners?: string[]; fromFix?: boolean } | null => {
    if (solveOut?.verdict === 'feasible') return { tricks: solveOut.tricks };
    if (unknownOwners && draftsOut?.verdict === 'feasible') return { tricks: draftsOut.tricks, owners: draftsOut.owners };
    if (altDrafts?.closest) return { ...altDrafts.closest, fromFix: true };
    return null;
  });
  const playedTricks = $derived.by((): TrickOut[] => {
    if (!situation) return [];
    const n = situation.hands.length;
    return situation.played
      .filter((t) => t.cards.length === n)
      .map((t, i) => ({ n: i + 1, leader: t.leader, cards: t.cards, winner: (t.leader + winningPosition(t.cards)) % n }));
  });

  async function run(text: string) {
    client.cancel();
    running = true;
    cancelled = false;
    lastProgress = '';
    error = null;
    progress = '';
    solveOut = drafts = altDrafts = null;
    draftsOut = null;
    conflict = null;
    noReturn = null;
    tight = null;
    robustness = null;
    redeal = null;
    step = null;
    const p = parse(text);
    if ('error' in p) {
      error = (p.error.line ? `line ${p.error.line}: ` : '') + p.error.msg;
      running = false;
      return;
    }
    situation = p.ok[0];
    const c = check(text);
    if ('error' in c) {
      error = c.error.msg;
      running = false;
      return;
    }
    difficulty = c.ok.difficulty;
    if (c.ok.redeal) {
      redeal = c.ok.redeal;
      running = false;
      return;
    }
    const nodes = (k: number) => (progress = i18n.t('positions', { n: (k / 1e6).toFixed(0) + 'M' }));
    const draftProgress = draftProgressFn;
    try {
      if (unknownOwners) {
        draftsOut = await client.call<SolveDraftsOut>('solve_drafts', [text], nodes);
      } else {
        solveOut = await client.call<SolveOut>('solve', [text], nodes);
        if (solveOut.verdict === 'infeasible') {
          conflict = await client.call<number[] | null>('conflict', [text]);
          if (situation.played.length) noReturn = await client.call<NoReturnOut>('no_return', [text]);
        }
        if (checkAll && situation.tasks.some((t) => t.owner !== 'crew')) {
          altDrafts = await client.call<DraftsOut>('drafts', [text, true, LIMIT], draftProgress);
        }
      }
    } catch (e) {
      if (e instanceof Cancelled) cancelled = true;
      else error = String(e instanceof Error ? e.message : e);
    }
    running = false;
    progress = '';
  }
  // Re-run only when the input changes, not when results update.
  $effect(() => {
    const t = text;
    void checkAll;
    untrack(() => run(t));
  });

  function cancel() {
    client.cancel();
    cancelled = true;
    running = false;
  }

  /** Run an on-demand analysis with progress and cancel. */
  async function extra(f: () => Promise<void>) {
    running = true;
    cancelled = false;
    lastProgress = '';
    try {
      await f();
    } catch (e) {
      if (e instanceof Cancelled) cancelled = true;
      else error = String(e instanceof Error ? e.message : e);
    }
    running = false;
    progress = '';
  }

  // (checked, planned, working so far, drafts still open)
  function draftProgressFn(a: number, b: number, w: number, open: number) {
    progress = a < b ? i18n.t('draftsChecked', { a, b, w, n: open }) : i18n.t('draftsSettling', { w, n: open });
    lastProgress = progress;
  }

  /** Count the drafts that work (owners ?), or check all other drafts (owners fixed). */
  const countDrafts = () =>
    extra(async () => {
      if (unknownOwners) drafts = await client.call<DraftsOut>('drafts', [text, false, LIMIT], draftProgressFn);
      else altDrafts = await client.call<DraftsOut>('drafts', [text, true, LIMIT], draftProgressFn);
    });

  /** Critical decisions and tightness along the shown line. */
  const computeTightness = () =>
    extra(async () => {
      if (!situation) return;
      if (solveOut?.verdict === 'feasible') {
        tight = await client.call<TightnessOut | null>('tightness', [text, JSON.stringify(solveOut.line)]);
      } else if (draftsOut?.verdict === 'feasible') {
        const owners = draftsOut.owners;
        const owned = toText({ ...situation, tasks: situation.tasks.map((t, i) => ({ ...t, owner: owners[i] })) });
        tight = await client.call<TightnessOut | null>('tightness', [owned, JSON.stringify(draftsOut.line)]);
      }
    });

  async function testDeals() {
    robustRunning = true;
    robustProgress = '';
    cancelled = false;
    lastProgress = '';
    try {
      robustness = await client.call('deal_robustness', [text, 100, Math.floor(Math.random() * 1e9)], (a, b) => {
        robustProgress = i18n.t('dealsTested', { a, b });
        lastProgress = robustProgress;
      });
    } catch (e) {
      if (!(e instanceof Cancelled)) error = String(e instanceof Error ? e.message : e);
    }
    robustRunning = false;
  }

  function cancelDeals() {
    client.cancel();
    robustRunning = false;
    cancelled = true;
  }

  // One entry per task: the players for whom it is impossible alone.
  function impossibleByTask(d: DraftsOut): [number, number[]][] {
    const m = new Map<number, number[]>();
    for (const { task, player } of d.impossible_pairs) m.set(task, [...(m.get(task) ?? []), player]);
    return [...m];
  }

  const draftLine = (d: DraftsOut) =>
    (d.sampled
      ? i18n.t('draftsWorkSampled', { pct: Math.round((100 * d.working) / Math.max(1, d.checked)), n: d.total, k: d.checked })
      : i18n.t('draftsWork', { k: d.working, n: d.total })) + (d.undecided ? ' · ' + i18n.t('undecidedN', { n: d.undecided }) : '');
</script>

{#if error}
  <div class="error">{error}</div>
{:else}
  {#if verdict === 'feasible'}
    <div class="verdict good">
      <div class="row">
        <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="var(--good)" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7" /></svg>
        <span class="v">{i18n.t('feasible')}</span>
      </div>
      <span class="sub">{unknownOwners ? i18n.t('feasibleAnyDraft') : i18n.t('feasibleSub')}</span>
    </div>
  {:else if verdict === 'infeasible'}
    <div class="verdict bad">
      <div class="row">
        <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="var(--bad-text)" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
        <span class="v">{i18n.t('notFeasible')}</span>
      </div>
      <span class="sub">{unknownOwners ? i18n.t('notFeasibleAnyDraft') : i18n.t('notFeasibleSub')}</span>
    </div>
  {:else if verdict === 'redeal'}
    <div class="verdict neutral">
      <span class="v">{i18n.t('redeal')}</span>
      <span class="sub">{i18n.t('redealSub', { task: textOf(redeal ?? '') })}</span>
    </div>
  {/if}

  {#if running}
    <div class="panel row between" style="flex-direction: row">
      <span>{i18n.t('computing')} <span class="small muted">{progress}</span></span>
      <button class="btn small" onclick={cancel}>{i18n.t('cancel')}</button>
    </div>
  {:else if cancelled}
    <div class="panel"><span class="muted">{lastProgress ? i18n.t('cancelledAt', { state: lastProgress }) : i18n.t('cancelled')}</span></div>
  {/if}

  {#if situation && (drafts || altDrafts)}
    {@const d = (altDrafts ?? drafts)!}
    <div class="panel">
      <span class="label">{i18n.t('otherDrafts')}</span>
      <span>{draftLine(d)}</span>
      {#if d.impossible_pairs.length}
        <span class="small muted" style="margin-top: 4px">{i18n.t('impossibleAlone')}:</span>
        {#each impossibleByTask(d) as [task, players]}
          <span class="small muted">{textOf(situation.tasks[task].id)} — {players.map((p) => i18n.player(p)).join(', ')}</span>
        {/each}
      {/if}
      {#if altDrafts?.closest && altDrafts.closest.distance && verdict === 'infeasible'}
        <span class="small" style="margin-top: 4px"><strong>{i18n.t('smallestChange')}</strong> ({i18n.t('changesOwner', { n: altDrafts.closest.distance })}):</span>
        {#each situation.tasks as t, i}
          {#if altDrafts.closest.owners[i] !== t.owner}
            <span class="small">{i18n.owner(t.owner)} → <strong>{i18n.owner(altDrafts.closest.owners[i])}</strong> · {textOf(t.id)}</span>
          {/if}
        {/each}
      {/if}
    </div>
  {/if}

  {#if situation && conflict && conflict.length}
    <div class="panel">
      <span class="label">{i18n.t('whyFails')}</span>
      <span class="small muted">{conflict.length === 1 ? i18n.t('impossibleAlone') : i18n.t('conflictSub')}</span>
      {#each conflict as i}
        <span class="item">{i18n.owner(situation.tasks[i].owner)} · {textOf(situation.tasks[i].id)}</span>
      {/each}
    </div>
  {/if}

  {#if noReturn}
    <div class="panel dashed">
      <span class="label">{i18n.t('whereWrong')}</span>
      {#if noReturn.kind === 'at'}
        <span>{i18n.t('lostAt', { t: noReturn.trick + 1, p: i18n.player(noReturn.player), c: noReturn.card })}</span>
        {#if noReturn.alternatives.length}
          <span class="row small" style="gap: 4px; flex-wrap: wrap">{i18n.t('wouldHaveKept', { cards: '' })}{#each noReturn.alternatives as c}<Card card={c} />{/each}</span>
        {:else}
          <span class="small muted">{i18n.t('noAlternative')}</span>
        {/if}
      {:else if noReturn.kind === 'from_start'}
        <span>{i18n.t('neverWinnable')}</span>
      {:else}
        <span>{i18n.t('stillWinnable')}</span>
      {/if}
    </div>
  {/if}

  {#if situation && (verdict === 'feasible' || verdict === 'infeasible')}
    <div class="cols-2">
      <div class="panel">
        <span class="label" style="font-size: 12px">{i18n.t('thisDeal')}</span>
        {#if altDrafts || drafts}
          {@const d = (altDrafts ?? drafts)!}
          <span class="m"><span class="small muted">{i18n.t('draftsThatWork')}</span><strong>{d.working} / {d.sampled ? d.checked : d.total}</strong></span>
        {:else if situation.tasks.some((t) => t.owner !== 'crew')}
          <span class="m">
            <span class="small muted">{i18n.t('draftsThatWork')}</span>
            <button class="btn small outline-accent" disabled={running || robustRunning} onclick={countDrafts}>{i18n.t('compute')}</button>
          </span>
        {/if}
        {#if tight}
          <span class="m"><span class="small muted">{i18n.t('criticalDecisions')}</span><strong>{tight.critical} / {tight.choices}</strong></span>
          <span class="m"><span class="small muted">{i18n.t('tightness')}</span><strong>{Math.round(tight.tightness * 100)}%</strong></span>
        {:else if verdict === 'feasible'}
          <span class="m">
            <span class="small muted">{i18n.t('criticalDecisions')} · {i18n.t('tightness')}</span>
            <button class="btn small outline-accent" disabled={running || robustRunning} onclick={computeTightness}>{i18n.t('compute')}</button>
          </span>
        {/if}
      </div>
      <div class="panel">
        <span class="label" style="font-size: 12px">{i18n.t('taskSet')}</span>
        <span class="m"><span class="small muted">{i18n.t('printedSum')}</span><strong>{situation.tasks.some((t) => isInferred(t.id)) ? '≈' : ''}{difficulty}</strong></span>
        <span class="m">
          <span class="small muted">{i18n.t('randomDeals')}</span>
          {#if robustness}
            <strong>{robustness.solvable} / {robustness.deals}</strong>
            {#if robustness.undecided}<span class="small muted">{i18n.t('undecidedN', { n: robustness.undecided })}</span>{/if}
          {:else if robustRunning}
            <span class="row small" style="gap: 8px">
              <span class="muted">{robustProgress || i18n.t('computing')}</span>
              <button class="btn small" onclick={cancelDeals}>{i18n.t('cancel')}</button>
            </span>
          {:else}
            <button class="btn small outline-accent" disabled={running || robustRunning} onclick={testDeals}>{i18n.t('testDeals', { n: 100 })}</button>
          {/if}
        </span>
      </div>
    </div>
  {/if}

  {#if situation && playedTricks.length}
    <span class="label">{i18n.t('playedTricks')}</span>
    <TrickList tricks={playedTricks} players={situation.hands.length} />
  {/if}

  {#if situation && shownLine && shownLine.tricks.length}
    <div class="row between">
      <span class="label">
        {shownLine.fromFix ? (altDrafts?.closest?.distance ? i18n.t('lineWithFix') : i18n.t('lineFromDeal')) : i18n.t('aWinningLine')}
      </span>
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
    {#if shownLine.owners && (unknownOwners || shownLine.fromFix)}
      <div class="panel small">
        {#each situation.tasks as t, i}
          <span>{i18n.owner(shownLine.owners[i])} · {textOf(t.id)}</span>
        {/each}
      </div>
    {/if}
    <TrickList tricks={step === null ? shownLine.tricks : shownLine.tricks.slice(0, step)} players={situation.hands.length} />
    {#if step === null && shownLine.tricks.length && shownLine.tricks.at(-1)!.n < Math.floor(40 / situation.hands.length)}
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
  .m {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .m strong {
    font-family: var(--mono);
    font-size: 16px;
  }
</style>
