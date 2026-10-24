<script lang="ts">
  import { i18n } from '../lib/i18n.svelte';

  let {
    players,
    active,
    counts,
    max,
    captain,
    onpick,
  }: {
    players: number;
    active: number;
    counts: number[];
    max: (p: number) => number;
    captain: number | null;
    onpick: (p: number) => void;
  } = $props();
</script>

<div class="chips" style="grid-template-columns: repeat({players}, minmax(0, 1fr))">
  {#each Array(players) as _, p}
    <button class="chip" class:active={p === active} class:full={counts[p] >= max(p) && p !== active} onclick={() => onpick(p)}>
      <span class="name">
        {i18n.player(p)}
        {#if captain === p}
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" stroke-width="2.4" stroke-linejoin="round" role="img" aria-label={i18n.t('captain')}><path d="M3 18h18l-2-10-5 4-2-6-2 6-5-4z" /></svg>
        {/if}
      </span>
      <span class="count">{counts[p]}/{max(p)}</span>
    </button>
  {/each}
</div>

<style>
  .chips {
    display: grid;
    gap: 6px;
  }
  .chip {
    min-height: 52px;
    border-radius: 12px;
    background: var(--surface);
    border: 2px solid transparent;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    padding: 0 2px;
    min-width: 0;
  }
  .chip.active {
    background: var(--surface-2);
    border-color: var(--accent);
  }
  .chip.full {
    opacity: 0.7;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 3px;
    font-size: 12.5px;
    font-weight: 700;
    white-space: nowrap;
  }
  .count {
    font-family: var(--mono);
    font-size: 12px;
  }
</style>
