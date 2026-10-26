<script lang="ts">
  // Which tasks sit with the same player in working drafts of random deals.
  import { i18n } from '../lib/i18n.svelte';
  import { pairTrends, type DealPairingRow } from '../lib/pairing';
  import { textOf } from '../lib/tasks';
  import type { Situation } from '../lib/wasm';

  let { situation, rows }: { situation: Situation; rows: DealPairingRow[] } = $props();

  const p = $derived(pairTrends(rows));
  const letter = (t: number) => String.fromCharCode(65 + t);
  const pct = (x: number) => `${Math.round(100 * x)}%`;
  /** Gap that counts as a clear tendency (as for one deal). */
  const GAP = 0.25;
  const tasks = $derived(situation.tasks.map((t, i) => [i, t] as const).filter(([, t]) => t.owner !== 'crew'));
</script>

<div class="pairing">
  <span class="small">{i18n.t('pairingStatus', { s: p.solvable, d: p.deals })}</span>
  {#if p.open}<span class="small muted">{i18n.t('pairingOpen', { n: p.open })}</span>{/if}
  {#if p.solvable && !p.pairs.length}
    <span class="small muted">{i18n.t('rulesNoPairs')}</span>
  {:else if p.pairs.length}
    <div class="scroll">
      <table>
        <thead>
          <tr>
            <th>{i18n.t('colPair')}</th>
            <th>{i18n.t('colWorking')}</th>
            <th>{i18n.t('colRules')}</th>
            <th>{i18n.t('colAlways')}</th>
            <th>{i18n.t('colNever')}</th>
          </tr>
        </thead>
        <tbody>
          {#each p.pairs as r}
            <tr class:together={r.works - r.all >= GAP} class:apart={r.all - r.works >= GAP}>
              <td><strong>{letter(r.t1)} + {letter(r.t2)}</strong></td>
              <td class="num">{pct(r.works)}</td>
              <td class="num muted">{pct(r.all)}</td>
              <td class="num">{r.always}</td>
              <td class="num">{r.never}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <span class="small muted">{i18n.t('pairingLegend')}</span>
  {/if}
  <div class="legend">
    {#each tasks as [i, t]}
      <span class="small"><strong>{letter(i)}</strong> {textOf(t.id)}</span>
    {/each}
  </div>
</div>

<style>
  .pairing {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 6px;
  }
  .scroll {
    overflow-x: auto;
  }
  table {
    border-collapse: collapse;
    font-size: 13px;
  }
  th {
    font-weight: 500;
    color: var(--muted);
    text-align: left;
    padding: 4px 6px;
    vertical-align: bottom;
  }
  td {
    padding: 4px 6px;
    border-top: 1px solid var(--line);
  }
  .num {
    font-family: var(--mono);
    text-align: right;
  }
  tr.together td:first-child,
  tr.together td:nth-child(2) {
    color: var(--good);
  }
  tr.apart td:first-child,
  tr.apart td:nth-child(2) {
    color: var(--bad-text);
  }
  .legend {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 4px;
  }
</style>
