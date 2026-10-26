<script lang="ts">
  // Who takes which task, and which tasks go together, over the decided
  // drafts. Tapping a cell fixes that choice; every figure then narrows to the
  // drafts that make it.
  import TrickList from './TrickList.svelte';
  import { draftStats, mark, type Count, type Filter } from '../lib/draftStats';
  import { i18n } from '../lib/i18n.svelte';
  import { mission } from '../lib/mission.svelte';
  import type { DraftRow } from '../lib/solver';
  import { textOf } from '../lib/tasks';
  import type { Situation } from '../lib/wasm';

  let { situation, rows, open }: { situation: Situation; rows: DraftRow[]; open: number } = $props();

  const pairs = $derived(mission.pairs);
  /** The pair cannot share any player who could take both, even alone. */
  const clashAlone = (t1: number, t2: number) =>
    pairs?.some((c) => c.tasks[0] === t1 && c.tasks[1] === t2 && c.players.length === c.tried.length) ?? false;
  /** A draft picked from the list, whose line is shown below it. */
  let picked = $state<DraftRow | null>(null);

  let filter = $state<Filter>(new Map());
  let shown = $state(20);
  const stats = $derived(draftStats(rows, filter));

  const letter = (t: number) => String.fromCharCode(65 + t);
  const pct = (x: number, of: number) => Math.round((100 * x) / Math.max(1, of));

  function toggle(t: number, o: string) {
    const f = new Map(filter);
    if (f.get(t) === o) f.delete(t);
    else f.set(t, o);
    filter = f;
    shown = 20;
  }

  function cellText(c: Count) {
    if (c.a === 0) return '–';
    const m = mark(c, stats.works.length, stats.all);
    return m ? i18n.t(m) : `${pct(c.w, stats.works.length)}%`;
  }

  // "Player1: A C · Player2: B"
  const summary = (r: DraftRow) =>
    stats.owners
      .map((o) => [o, stats.tasks.filter((t) => r.owners[t] === o)] as const)
      .filter(([, ts]) => ts.length)
      .map(([o, ts]) => `${i18n.owner(o)}: ${ts.map(letter).join(' ')}`)
      .join(' · ');
</script>

<div class="explorer">
  <span class="small"><strong>{i18n.t('whoTakesWhat')}</strong></span>
  <span class="small muted">{i18n.t('explorerSub')}</span>

  {#if filter.size}
    <div class="row wrap">
      {#each [...filter] as [t, o]}
        <button class="chip" onclick={() => toggle(t, o)}>{letter(t)} → {i18n.owner(o)} ×</button>
      {/each}
      <button class="btn small" onclick={() => (filter = new Map())}>{i18n.t('clearChoice')}</button>
    </div>
  {/if}

  <span class="small">
    {stats.works.length || !filter.size ? i18n.t('worksOfAll', { w: stats.works.length, a: stats.all }) : i18n.t('noneWithChoice')}
    {filter.size && stats.works.length ? i18n.t('withChoice') : ''}
  </span>
  {#if open > 0}<span class="small muted">{i18n.t('stillOpen', { n: open })}</span>{/if}

  <div class="scroll">
    <table>
      <thead>
        <tr>
          <th></th>
          {#each stats.owners as o}<th>{i18n.owner(o)}</th>{/each}
        </tr>
      </thead>
      <tbody>
        {#each stats.tasks as t}
          <tr>
            <th class="task" title={textOf(situation.tasks[t].id)}><strong>{letter(t)}</strong> {textOf(situation.tasks[t].id)}</th>
            {#each stats.owners as o}
              {@const c = stats.cell(t, o)}
              {@const m = mark(c, stats.works.length, stats.all)}
              <td>
                <button
                  class="cell"
                  class:must={m === 'must'}
                  class:never={m === 'never'}
                  class:chosen={filter.get(t) === o}
                  disabled={c.a === 0}
                  style="--heat: {m ? 0 : Math.round(pct(c.w, stats.works.length) / 2)}%"
                  onclick={() => toggle(t, o)}>{cellText(c)}</button
                >
              </td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  {#if stats.always.length}
    <span class="small"><strong>{i18n.t('alwaysTogether')}</strong></span>
    {#each stats.always as p}<span class="small">{letter(p.t1)} + {letter(p.t2)}</span>{/each}
  {/if}
  {#if stats.never.length}
    <span class="small"><strong>{i18n.t('neverTogether')}</strong></span>
    {#each stats.never as p}
      <span class="small">{letter(p.t1)} + {letter(p.t2)}{clashAlone(p.t1, p.t2) ? ` (${i18n.t('evenAlone')})` : ''}</span>
    {/each}
  {/if}

  {#if stats.tendencies.length}
    <span class="small"><strong>{i18n.t('tendencies')}</strong></span>
    {#each stats.tendencies as p}
      <span class="small">
        {letter(p.t1)} + {letter(p.t2)}: {i18n.t('togetherShare', { w: pct(p.same.w, stats.works.length), a: pct(p.same.a, stats.all) })}
      </span>
    {/each}
  {/if}

  {#if stats.works.length}
    <span class="small" style="margin-top: 4px"><strong>{i18n.t('workingDrafts')}</strong></span>
    {#each stats.works.slice(0, shown) as r}
      <button class="draft small" class:open={picked === r} onclick={() => (picked = picked === r ? null : r)}>{summary(r)}</button>
      {#if picked === r && r.tricks}
        <span class="small muted">{i18n.t('lineForDraft')}</span>
        <TrickList tricks={r.tricks} players={situation.hands.length} />
      {/if}
    {/each}
    {#if stats.works.length > shown}
      <button class="btn small" onclick={() => (shown += 50)}>{i18n.t('showMore', { n: Math.min(50, stats.works.length - shown) })}</button>
    {/if}
  {/if}
</div>

<style>
  .explorer {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 6px;
  }
  .draft.open {
    outline: 2px solid var(--accent);
  }
  .wrap {
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    font: inherit;
    font-size: 13px;
    color: var(--accent-ink);
    background: var(--accent);
    border: 0;
    border-radius: 999px;
    padding: 4px 10px;
    cursor: pointer;
  }
  .scroll {
    overflow-x: auto;
    margin: 6px 0;
  }
  table {
    border-collapse: collapse;
    font-size: 13px;
  }
  th {
    font-weight: 500;
    color: var(--muted);
    padding: 2px 4px;
    white-space: nowrap;
  }
  th.task {
    text-align: left;
    color: var(--text);
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  td {
    padding: 2px;
  }
  .cell {
    width: 52px;
    font: inherit;
    font-family: var(--mono);
    color: var(--text);
    background: color-mix(in srgb, var(--good) var(--heat), var(--surface));
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 4px 0;
    cursor: pointer;
  }
  .cell.must {
    background: var(--good-bg);
    border-color: var(--good);
    color: var(--good);
    font-weight: 700;
  }
  .cell.never {
    background: var(--bad-bg);
    border-color: var(--bad-border);
    color: var(--bad-text);
  }
  .cell.chosen {
    outline: 2px solid var(--accent);
  }
  .cell:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .draft {
    font: inherit;
    font-size: 13px;
    text-align: left;
    color: var(--text);
    background: var(--surface-2);
    border: 0;
    border-radius: 6px;
    padding: 4px 8px;
    cursor: pointer;
  }
</style>
