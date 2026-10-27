<script lang="ts">
  // The Position Inspector: the board, every legal move solved, how each move
  // order ranks the moves, a move order playing on by itself, and a benchmark
  // of the move orders. State lives in `position`.
  import Card from './Card.svelte';
  import ExportSheet from './ExportSheet.svelte';
  import TrickList from './TrickList.svelte';
  import WorkList from './WorkList.svelte';
  import { i18n } from '../lib/i18n.svelte';
  import { inspector } from '../lib/inspector.svelte';
  import { benchSummary, position as P } from '../lib/position.svelte';
  import { textOf } from '../lib/tasks';

  let exporting = $state(false);
  let lineOf = $state<string | null>(null);

  const info = $derived(P.info && !('error' in P.info) ? P.info : null);
  const s = $derived(P.situation);
  // Moves in lookahead order (the default move order), solver result beside.
  const rows = $derived.by(() => {
    if (!info) return [];
    const order = info.rankings[2] ?? info.legal;
    return order.map((c) => ({ card: c, result: P.moves[c], ranks: info.rankings.map((r) => r.indexOf(c) + 1) }));
  });
  const anyOpen = $derived(rows.some((r) => r.result?.verdict === 'undecided'));
  const winning = $derived(new Set(rows.filter((r) => r.result?.verdict === 'feasible').map((r) => r.card)));
  const allDecided = $derived(rows.length > 0 && rows.every((r) => r.result && r.result.verdict !== 'undecided'));
  // Per move order: rank of its first winning move (when every move is decided).
  const firstWin = $derived(info && allDecided ? info.rankings.map((r) => r.findIndex((c) => winning.has(c)) + 1 || null) : null);
  const firstLine = $derived(rows.find((r) => r.result?.verdict === 'feasible')?.result?.line ?? null);

  const verdictText = (v: string | undefined) =>
    v === 'feasible' ? i18n.t('moveWins') : v === 'infeasible' ? i18n.t('moveLoses') : v === 'undecided' ? i18n.t('moveOpen') : v ? v : '…';
  const statusIcon = (x: string) => (x === 'done' ? '✓' : x === 'failed' ? '✗' : '○');
  const short = (name: string) => name.split(/[ -]/).map((w) => w[0].toUpperCase()).join('');
  const count = (n: number) => (n < 1000 ? String(n) : n < 1e6 ? `${Math.round(n / 1000)}k` : `${(n / 1e6).toFixed(1)}M`);
  const slowPositions = $derived(inspector.slow.map((r) => r.text).filter((t) => !/^\?:/m.test(t)));
</script>

{#if P.info && 'error' in P.info}
  <div class="error">{P.info.error}</div>
{:else if info && s}
  <div class="panel">
    <div class="row between">
      <strong>
        {info.over ? i18n.t('posOver') : i18n.t('posHeader', { t: info.trick, k: info.in_trick + 1, n: s.hands.length, p: i18n.player(info.to_play) })}
      </strong>
      <span class="row">
        <button class="btn small" disabled={!P.history.length} onclick={() => P.back()} aria-label={i18n.t('back')}>◀</button>
        <button class="btn small" onclick={() => (exporting = true)}>{i18n.t('export')}</button>
      </span>
    </div>
    <div class="tasks">
      {#each s.tasks as t, i}
        <span class="small task {info.status[i]}">{statusIcon(info.status[i])} {i18n.owner(t.owner)} · {textOf(t.id)}</span>
      {/each}
    </div>
    <div class="hands">
      {#each info.hands as hand, p}
        <div class="hand" class:turn={p === info.to_play && !info.over}>
          <span class="small who">{i18n.player(p)} <span class="muted">· {info.tricks_won[p]}</span></span>
          <span class="cards">{#each hand as c}<Card card={c} />{/each}</span>
        </div>
      {/each}
      {#if info.current.length}
        <div class="hand current">
          <span class="small who">{i18n.t('trickNow')}</span>
          <span class="cards">{#each info.current as c}<Card card={c} />{/each}</span>
        </div>
      {/if}
    </div>
  </div>

  {#if !info.over}
    <section class="panel">
      <span class="label">{i18n.t('movesTitle')}</span>
      <span class="small muted">{i18n.t('movesSub')}</span>
      <table>
        <thead>
          <tr>
            <th>{i18n.t('colMove')}</th>
            <th>{i18n.t('colSolver')}</th>
            {#each info.orders as o}<th title={o}>{short(o)}</th>{/each}
            <th>{i18n.t('positionsShort')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each rows as r (r.card)}
            <tr>
              <td><Card card={r.card} /></td>
              <td>
                {#if r.result?.verdict === 'feasible'}
                  <button class="link good" onclick={() => (lineOf = lineOf === r.card ? null : r.card)}>{verdictText('feasible')} ▾</button>
                {:else}
                  <span class={r.result?.verdict === 'infeasible' ? 'bad' : 'muted'}>{verdictText(r.result?.verdict)}</span>
                {/if}
              </td>
              {#each r.ranks as k}<td class="num" class:first={k === 1}>{k}</td>{/each}
              <td class="num muted">{r.result ? count(r.result.nodes) : ''}</td>
              <td><button class="btn small" onclick={() => P.play(r.card)} aria-label={i18n.t('playMove', { c: r.card })}>▸</button></td>
            </tr>
            {#if lineOf === r.card && r.result?.tricks}
              <tr>
                <td colspan={4 + info.orders.length}>
                  <div class="row">
                    <span class="small muted">{i18n.t('lineAfter', { c: r.card })}</span>
                    <button class="btn small" onclick={() => P.playAll(r.result!.line!)}>{i18n.t('followLine')}</button>
                  </div>
                  <TrickList tricks={r.result.tricks} players={s.hands.length} />
                </td>
              </tr>
            {/if}
          {/each}
        </tbody>
      </table>
      {#if firstWin}
        <span class="small">
          {#if winning.size}
            {i18n.t('firstWinRank')}: {info.orders.map((o, k) => `${short(o)} ${firstWin[k] ?? '–'}`).join(' · ')}
          {:else}
            {i18n.t('noMoveWins')}
          {/if}
        </span>
      {/if}
      {#if P.movesJob.running}
        <div class="row between">
          <span class="small muted">{P.movesJob.progress}</span>
          <button class="btn small" onclick={() => P.cancel()}>{i18n.t('cancel')}</button>
        </div>
      {:else if anyOpen}
        <div><button class="btn small outline-accent" disabled={P.busy} onclick={() => P.solveMoves(0)}>{i18n.t('continueOpen')}</button></div>
      {:else if P.movesJob.cancelled}
        <div><button class="btn small outline-accent" disabled={P.busy} onclick={() => P.solveMoves(2_000_000)}>{i18n.t('compute')}</button></div>
      {/if}
      {#if inspector.on}<WorkList job="position" />{/if}
    </section>

    <section class="panel">
      <span class="label">{i18n.t('letPlayTitle')}</span>
      <span class="small muted">{i18n.t('letPlaySub')}</span>
      <div class="row wrap">
        {#each info.orders as o, k}
          <button class="btn small outline-accent" disabled={P.busy} onclick={() => P.letPlay(k)}>{o}</button>
        {/each}
      </div>
      {#if P.playJob.running}
        <div class="row between">
          <span class="small muted">{P.playJob.progress}</span>
          <button class="btn small" onclick={() => P.cancel()}>{i18n.t('cancel')}</button>
        </div>
      {/if}
      {#if P.playOut}
        {@const po = P.playOut}
        <span>
          <strong>{info.orders[po.order]}:</strong>
          {#if po.winnable === false}
            {i18n.t('notWinnableHere')}
          {:else if po.lost_at !== null}
            {i18n.t('lostAtMove', { i: po.lost_at + 1, c: po.line[po.lost_at] })}
          {:else if po.status.every((x) => x === 'done')}
            {i18n.t('playFulfilled')}
          {:else}
            {i18n.t('playNotFulfilled')}
          {/if}
          {#if po.undecided}<span class="small muted">({i18n.t('someUndecided')})</span>{/if}
        </span>
        {#if po.lost_at !== null}
          <span class="row small wrap">
            {i18n.t('wouldHaveKept', { cards: '' })}{#each po.alternatives as c}<Card card={c} />{/each}
            <button class="btn small" onclick={() => P.playAll(po.line.slice(0, po.lost_at!))}>{i18n.t('goThere')}</button>
          </span>
        {/if}
        <span class="small muted">{i18n.t('finalStatus')}: {s.tasks.map((t, i) => `${statusIcon(po.status[i])} ${textOf(t.id)}`).join(' · ')}</span>
        <TrickList tricks={po.tricks} players={s.hands.length} />
      {/if}
    </section>

    <section class="panel">
      <span class="label">{i18n.t('benchTitle')}</span>
      <span class="small muted">{i18n.t('benchSub')}</span>
      <div class="row wrap">
        <button
          class="btn small outline-accent"
          disabled={P.busy || !firstLine}
          onclick={() => P.benchmark(i18n.t('benchLine'), P.positionsAlong(firstLine!))}>{i18n.t('benchLine')}</button
        >
        <button
          class="btn small outline-accent"
          disabled={P.busy || !slowPositions.length}
          onclick={() => P.benchmark(i18n.t('benchSlow'), slowPositions)}>{i18n.t('benchSlow')} ({slowPositions.length})</button
        >
      </div>
      {#if P.benchJob.running}
        <div class="row between">
          <span class="small muted">{P.benchJob.progress}</span>
          <button class="btn small" onclick={() => P.cancel()}>{i18n.t('cancel')}</button>
        </div>
      {/if}
      {#if P.bench}
        {@const sum = benchSummary(P.bench.rows, info.orders.length)}
        <span class="small">{P.bench.source}: {i18n.t('positionsDone', { a: P.bench.rows.length, b: P.bench.total })}</span>
        <table>
          <thead>
            <tr><th></th><th>{i18n.t('colCounted')}</th><th>{i18n.t('colTopWins')}</th><th>{i18n.t('colMeanRank')}</th></tr>
          </thead>
          <tbody>
            {#each info.orders as o, k}
              <tr>
                <td>{o}</td>
                <td class="num">{sum[k].counted}</td>
                <td class="num">{sum[k].counted ? Math.round((100 * sum[k].top) / sum[k].counted) + '%' : '–'}</td>
                <td class="num">{sum[k].counted ? sum[k].mean.toFixed(2) : '–'}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        <span class="small muted">{i18n.t('benchLegend')}</span>
      {/if}
    </section>
  {/if}
{/if}

{#if exporting && P.text}
  <ExportSheet text={P.text} title={i18n.t('positionTitle')} onclose={() => (exporting = false)} />
{/if}

<style>
  .tasks {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .task.done {
    color: var(--good);
  }
  .task.failed {
    color: var(--bad-text);
  }
  .hands {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .hand {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 4px 6px;
    border-radius: 8px;
  }
  .hand.turn {
    outline: 2px solid var(--accent);
  }
  .hand.current {
    background: var(--bg);
  }
  .who {
    width: 78px;
    flex: none;
  }
  .cards {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
  }
  table {
    border-collapse: collapse;
    font-size: 13px;
    width: 100%;
  }
  th {
    font-weight: 500;
    color: var(--muted);
    text-align: left;
    padding: 4px;
  }
  td {
    padding: 3px 4px;
    border-top: 1px solid var(--line);
  }
  .num {
    font-family: var(--mono);
    text-align: right;
  }
  .first {
    color: var(--accent);
    font-weight: 700;
  }
  .good {
    color: var(--good);
  }
  .bad {
    color: var(--bad-text);
  }
  .link {
    font: inherit;
    background: none;
    border: 0;
    padding: 0;
    cursor: pointer;
  }
  .wrap {
    flex-wrap: wrap;
    gap: 6px;
  }
</style>
