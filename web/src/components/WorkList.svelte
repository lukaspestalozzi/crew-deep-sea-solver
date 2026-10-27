<script lang="ts">
  // The Inspector's rows of one analysis: what the solver works on, how long,
  // how many positions; export or open any of them.
  import ExportSheet from './ExportSheet.svelte';
  import { i18n } from '../lib/i18n.svelte';
  import { count, duration, inspector, type WorkRow } from '../lib/inspector.svelte';
  import { router, session } from '../lib/state.svelte';
  import { parse } from '../lib/wasm';
  import { inspect, inspectable } from '../lib/position.svelte';

  let { job }: { job: string } = $props();

  let shown = $state(12);
  let exporting = $state<{ text: string; title: string } | null>(null);
  const slowMs = $derived(inspector.slowMinutes * 60_000);

  // Running first (longest first), then the slowest finished.
  const rows = $derived.by(() => {
    const mine = inspector.rows.filter((r) => r.job === job);
    const running = mine.filter((r) => r.end === null).sort((a, b) => a.start - b.start);
    const done = mine.filter((r) => r.end !== null).sort((a, b) => b.end! - b.start - (a.end! - a.start));
    return { list: [...running, ...done], running: running.length, done: done.length };
  });
  const slow = $derived(rows.list.filter((r) => inspector.elapsed(r) >= slowMs).length);

  const rate = (r: WorkRow) => (r.search && inspector.elapsed(r) > 0 ? count(Math.round((r.nodes * 1000) / inspector.elapsed(r))) : '');

  function openInDesk(text: string) {
    const p = parse(text);
    if ('error' in p) return;
    session.load(p.ok[0], 'explore');
    router.go('/desk');
  }
</script>

{#if rows.list.length}
  <div class="work">
    <span class="small muted">{i18n.t('workSummary', { r: rows.running, d: rows.done, s: slow })}</span>
    {#each rows.list.slice(0, shown) as r (r.key)}
      {@const ms = inspector.elapsed(r)}
      <div class="row-item" class:running={r.end === null} class:slow={ms >= slowMs}>
        <span class="what" title={inspector.path(r)}>{inspector.path(r) || r.label}</span>
        <span class="facts">
          {r.result ?? i18n.t('running')} · {duration(ms)}
          · {count(inspector.nodes(r))} {i18n.t('positionsShort')}
          {#if r.search}· {rate(r)}/s{/if}
          {#if r.restart}· {i18n.t('restartN', { n: r.restart, b: count(r.budget) })}{/if}
          {#if r.cache}· {i18n.t('cacheN', { n: count(r.cache) })}{/if}
        </span>
        <span class="acts">
          <button class="btn small" onclick={() => (exporting = { text: r.text, title: inspector.path(r) || r.label })}>{i18n.t('export')}</button>
          <button class="btn small" onclick={() => openInDesk(r.text)}>{i18n.t('openInDesk')}</button>
          {#if inspectable(r.text)}<button class="btn small" onclick={() => inspect(r.text)}>{i18n.t('inspect')}</button>{/if}
        </span>
      </div>
    {/each}
    {#if rows.list.length > shown}
      <button class="btn small" onclick={() => (shown += 30)}>{i18n.t('showMore', { n: Math.min(30, rows.list.length - shown) })}</button>
    {/if}
  </div>
{/if}

{#if exporting}
  <ExportSheet text={exporting.text} title={exporting.title} onclose={() => (exporting = null)} />
{/if}

<style>
  .work {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 6px;
    border-top: 1px dashed var(--border);
    padding-top: 8px;
  }
  .row-item {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 8px;
    border-radius: 8px;
    background: var(--bg);
    border-left: 3px solid var(--border);
  }
  .row-item.running {
    border-left-color: var(--accent);
  }
  .row-item.slow {
    border-left-color: var(--bad-border);
    background: var(--bad-bg);
  }
  .what {
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .facts {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--muted);
  }
  .acts {
    display: flex;
    gap: 6px;
    margin-top: 2px;
  }
</style>
