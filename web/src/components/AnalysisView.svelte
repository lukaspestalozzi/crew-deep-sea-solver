<script lang="ts">
  // The analyses on demand, in two tabs: this deal, and random deals with the
  // same tasks. Results live in `mission` and stay while switching.
  import AnalysisCard from './AnalysisCard.svelte';
  import DraftExplorer from './DraftExplorer.svelte';
  import PairingTable from './PairingTable.svelte';
  import TrickList from './TrickList.svelte';
  import WorkList from './WorkList.svelte';
  import SlowLog from './SlowLog.svelte';
  import { inspector } from '../lib/inspector.svelte';
  import { inspect, positionOnLine } from '../lib/position.svelte';
  import PositionView from './PositionView.svelte';

  /** In the desk view: a third tab with the Position Inspector. */
  let { desk = false }: { desk?: boolean } = $props();
  const tab = $derived(!desk && mission.tab === 'position' ? 'deal' : mission.tab);
  import { i18n } from '../lib/i18n.svelte';
  import { DEALS, mission } from '../lib/mission.svelte';
  import type { DraftsOut } from '../lib/solver';
  import { isInferred, textOf } from '../lib/tasks';

  const m = mission;
  /** Tasks the drafts move (and the pair check pairs up). */
  const draftable = $derived(m.situation?.tasks.filter((t) => (m.unknownOwners ? t.owner === '?' : t.owner !== 'crew')).length ?? 0);

  // One entry per task: the players for whom it is impossible alone.
  function impossibleByTask(d: DraftsOut): [number, number[]][] {
    const by = new Map<number, number[]>();
    for (const { task, player } of d.impossible_pairs) by.set(task, [...(by.get(task) ?? []), player]);
    return [...by];
  }

  const draftLine = (d: DraftsOut) =>
    (d.sampled
      ? i18n.t('draftsWorkSampled', { pct: Math.round((100 * d.working) / Math.max(1, d.checked)), n: d.total, k: d.checked })
      : i18n.t('draftsWork', { k: d.working, n: d.total })) + (d.undecided ? ' · ' + i18n.t('undecidedN', { n: d.undecided }) : '');
</script>

{#if m.situation && (m.verdict === 'feasible' || m.verdict === 'infeasible')}
  {@const s = m.situation}
  <div class="row between">
    <div class="seg" role="radiogroup" aria-label={i18n.t('analysis')}>
      <button role="radio" aria-checked={tab === 'deal'} onclick={() => (m.tab = 'deal')}>{i18n.t('tabThisDeal')}</button>
      <button role="radio" aria-checked={tab === 'random'} onclick={() => (m.tab = 'random')}>{i18n.t('tabRandom')}</button>
      {#if desk}
        <button role="radio" aria-checked={tab === 'position'} onclick={() => (m.tab = 'position')}>{i18n.t('tabPosition')}</button>
      {/if}
    </div>
    <label class="row small">
      <input type="checkbox" checked={inspector.on} onchange={(e) => inspector.setOn((e.target as HTMLInputElement).checked)} style="accent-color: var(--accent)" />
      {i18n.t('details')}
    </label>
  </div>

  {#if tab === 'position'}
    {#if desk}<PositionView />{/if}
  {:else if tab === 'deal'}
    {#if m.hasOwners}
      <AnalysisCard
        title={m.unknownOwners ? i18n.t('draftsThatWork') : i18n.t('otherDrafts')}
        sub={m.unknownOwners ? i18n.t('draftsSub') : i18n.t('otherDraftsSub')}
        job={m.draftsJob}
        show={!!m.drafts || m.rows.items.length > 0}
        onrun={m.countDrafts}
      >
        {#if m.drafts}
          {@const d = m.drafts}
          <span>{draftLine(d)}</span>
          {#if d.impossible_pairs.length}
            <span class="small muted">{i18n.t('impossibleAlone')}:</span>
            {#each impossibleByTask(d) as [task, players]}
              <span class="small muted">{textOf(s.tasks[task].id)} — {players.map((p) => i18n.player(p)).join(', ')}</span>
            {/each}
          {/if}
          {#if !m.unknownOwners && d.closest?.distance && m.verdict === 'infeasible'}
            <span class="small"><strong>{i18n.t('smallestChange')}</strong> ({i18n.t('changesOwner', { n: d.closest.distance })}):</span>
            {#each s.tasks as t, i}
              {#if d.closest.owners[i] !== t.owner}
                <span class="small">{i18n.owner(t.owner)} → <strong>{i18n.owner(d.closest.owners[i])}</strong> · {textOf(t.id)}</span>
              {/if}
            {/each}
            <span class="row between">
              <span class="small muted">{i18n.t('lineWithFix')}</span>
              <button class="btn small" onclick={() => inspect(positionOnLine(s, d.closest!.owners, d.closest!.tricks, 0, true))}>{i18n.t('inspect')}</button>
            </span>
            <TrickList tricks={d.closest.tricks} players={s.hands.length} />
          {/if}
          {#if draftable < 2}
            <!-- no pairs to check -->
          {:else if m.pairs}
            <span class="small"><strong>{m.pairs.length ? i18n.t('pairsClash') : i18n.t('pairsNone')}</strong></span>
            {#each m.pairs as c}
              <span class="small">
                {textOf(s.tasks[c.tasks[0]].id)} + {textOf(s.tasks[c.tasks[1]].id)}{c.players.length < c.tried.length
                  ? ': ' + c.players.map((p) => i18n.player(p)).join(', ')
                  : ''}
              </span>
            {/each}
          {:else if m.pairsJob.running}
            <span class="row between">
              <span class="small muted">{m.pairsJob.progress}</span>
              <button class="btn small" onclick={() => m.cancel()}>{i18n.t('cancel')}</button>
            </span>
          {:else}
            <span class="pairs">
              <span class="small muted">{i18n.t('pairsSub')}</span>
              <button class="btn small outline-accent" disabled={m.busy} onclick={m.checkPairs}>{i18n.t('checkPairs')}</button>
            </span>
          {/if}
        {/if}
        {#if m.rows.items.some((r) => r.status === 'works')}
          <DraftExplorer situation={s} rows={m.rows.items} open={Math.max(0, m.planned - m.rows.items.length)} />
        {/if}
      </AnalysisCard>
    {/if}

    {#if m.verdict === 'feasible'}
      <AnalysisCard
        title="{i18n.t('criticalDecisions')} · {i18n.t('tightness')}"
        sub={i18n.t('tightSub')}
        job={m.tightJob}
        show={!!m.tight}
        onrun={m.computeTightness}
      >
        {#if m.tight}
          <div class="figures">
            <span class="fig"><span class="small muted">{i18n.t('criticalDecisions')}</span><strong>{m.tight.critical} / {m.tight.choices}</strong></span>
            <span class="fig"><span class="small muted">{i18n.t('tightness')}</span><strong>{Math.round(m.tight.tightness * 100)}%</strong></span>
          </div>
        {/if}
      </AnalysisCard>
    {/if}
  {:else}
    <div class="panel">
      <span class="fig">
        <span class="small muted">{i18n.t('printedSum')}</span>
        <strong>{s.tasks.some((t) => isInferred(t.id)) ? '≈' : ''}{m.difficulty}</strong>
      </span>
    </div>

    <AnalysisCard
      title={i18n.t('randomDeals')}
      sub={i18n.t('dealsSub', { n: DEALS })}
      job={m.dealsJob}
      show={!!m.robustness}
      action={i18n.t('testDeals', { n: DEALS })}
      onrun={m.testDeals}
    >
      {#if m.robustness}
        <span class="fig">
          <strong>{m.robustness.solvable} / {m.robustness.deals}</strong>
          {#if m.robustness.undecided}
            <span class="small muted">{i18n.t(m.dealsJob.running ? 'dealsOpen' : 'undecidedN', { n: m.robustness.undecided })}</span>
          {/if}
        </span>
      {/if}
    </AnalysisCard>

    {#if m.hasOwners}
      <AnalysisCard
        title={i18n.t('pairingTitle')}
        sub={i18n.t('pairingSub')}
        job={m.pairingJob}
        show={m.pairing.items.length > 0}
        action={i18n.t('start')}
        onrun={m.pairDeals}
      >
        <PairingTable situation={s} rows={m.pairing.items} />
      </AnalysisCard>
    {/if}
  {/if}

  {#if inspector.on}
    {#if inspector.rows.some((r) => r.job === 'verdict')}
      <section class="panel">
        <span class="label">{i18n.t('verdictWork')}</span>
        <WorkList job="verdict" />
      </section>
    {/if}
    <SlowLog />
  {/if}
{/if}

<style>
  .pairs {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
  }
  .figures {
    display: flex;
    gap: 24px;
  }
  .fig {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .fig strong {
    font-family: var(--mono);
    font-size: 16px;
  }
</style>
