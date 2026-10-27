<script lang="ts">
  // Searches that took longer than the threshold, kept on this device to
  // reproduce them (one file for `crew bench --load`).
  import ExportSheet from './ExportSheet.svelte';
  import { i18n } from '../lib/i18n.svelte';
  import { count, duration, inspector } from '../lib/inspector.svelte';
  import { inspect, inspectable } from '../lib/position.svelte';

  let exporting = $state<{ text: string; title: string } | null>(null);
  const all = () => inspector.slow.map((r) => r.text.trimEnd() + '\n').join('---\n');
</script>

<section class="panel">
  <span class="label">{i18n.t('slowCases')}</span>
  <label class="row small">
    {i18n.t('slowAfter')}
    <input
      class="input"
      type="number"
      min="0.1"
      step="0.5"
      style="width: 80px"
      value={inspector.slowMinutes}
      onchange={(e) => inspector.setSlowMinutes(Number((e.target as HTMLInputElement).value))}
    />
    min
  </label>
  {#if inspector.slow.length}
    <div class="row">
      <button class="btn small outline-accent" onclick={() => (exporting = { text: all(), title: 'slow-cases' })}>{i18n.t('exportAll')}</button>
      <button class="btn small" onclick={() => inspector.clearSlow()}>{i18n.t('clearAll')}</button>
    </div>
    {#each inspector.slow as r (r.id)}
      <div class="item">
        <span class="small">{r.mission ? r.mission + ' › ' : ''}{r.label}</span>
        <span class="facts">{r.result ?? i18n.t('running')} · {duration(r.ms)} · {count(r.nodes)} {i18n.t('positionsShort')} · {new Date(r.date).toLocaleString()}</span>
        <span class="row">
          <button class="btn small" onclick={() => (exporting = { text: r.text, title: r.label })}>{i18n.t('export')}</button>
          {#if inspectable(r.text)}<button class="btn small" onclick={() => inspect(r.text)}>{i18n.t('inspect')}</button>{/if}
          <button class="btn small" onclick={() => inspector.deleteSlow(r.id)}>{i18n.t('delete')}</button>
        </span>
      </div>
    {/each}
  {:else}
    <span class="small muted">{i18n.t('noSlow')}</span>
  {/if}
</section>

{#if exporting}
  <ExportSheet text={exporting.text} title={exporting.title} onclose={() => (exporting = null)} />
{/if}

<style>
  .item {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 8px;
    border-radius: 8px;
    background: var(--bg);
  }
  .facts {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--muted);
  }
</style>
