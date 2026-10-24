<script lang="ts">
  import { SUIT_COLOR, suitOf } from '../lib/cards';
  import { i18n } from '../lib/i18n.svelte';
  import type { TrickOut } from '../lib/solver';

  let { tricks, players, notes = () => '' }: { tricks: TrickOut[]; players: number; notes?: (t: TrickOut) => string } = $props();
</script>

<div class="list">
  {#each tricks as t (t.n)}
    <div class="trick">
      <div class="row">
        <span class="small muted" style="width: 54px">{i18n.t('trickN', { n: t.n })}</span>
        <div class="cards" style="grid-template-columns: repeat({players}, minmax(0, 1fr))">
          {#each t.cards as c, i}
            {@const p = (t.leader + i) % players}
            <div class="slot">
              <span class="who">{i === 0 ? '▸ ' : ''}{i18n.player(p)}</span>
              <span class="card" class:win={p === t.winner} style="background: {SUIT_COLOR[suitOf(c)]}">{c}</span>
            </div>
          {/each}
        </div>
      </div>
      {#if notes(t)}<span class="small" style="color: var(--good)">{notes(t)}</span>{/if}
    </div>
  {/each}
</div>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .trick {
    background: var(--surface);
    border-radius: 12px;
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .cards {
    display: grid;
    gap: 6px;
    flex-grow: 1;
  }
  .slot {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    min-width: 0;
  }
  .who {
    font-size: 10.5px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .card {
    font-family: var(--mono);
    font-weight: 700;
    font-size: 14px;
    width: 100%;
    text-align: center;
    padding: 6px 0;
    border-radius: 8px;
    color: var(--accent-ink);
    opacity: 0.8;
  }
  .card.win {
    opacity: 1;
    outline: 3px solid #fff;
    outline-offset: -3px;
  }
</style>
