<script lang="ts">
  // 5 suit columns x 9 value rows; every button is a 44 px touch target.
  import { GRID, SUIT_COLOR, suitOf, type CellState } from '../lib/cards';

  let {
    cell,
    tag = () => '',
    ontap,
    compact = false,
  }: {
    cell: (card: string) => CellState;
    tag?: (card: string) => string;
    ontap: (card: string) => void;
    compact?: boolean;
  } = $props();
</script>

<div class="grid" style="--h: {compact ? 40 : 44}px">
  {#each GRID as row}
    {#each row as card}
      {#if card === null}
        <span></span>
      {:else}
        {@const st = cell(card)}
        <button
          class="cell {st}"
          style="--suit: {SUIT_COLOR[suitOf(card)]}"
          disabled={st === 'disabled' || st === 'gone'}
          aria-label={card + (tag(card) ? ' ' + tag(card) : '')}
          aria-pressed={st === 'mine'}
          onclick={() => ontap(card)}
        >
          <span>{card}</span>
          {#if tag(card)}<span class="tag">{tag(card)}</span>{/if}
        </button>
      {/if}
    {/each}
  {/each}
</div>

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: 5px;
  }
  .cell {
    height: var(--h);
    border-radius: 8px;
    border: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    font-family: var(--mono);
    font-size: 15px;
    font-weight: 700;
    padding: 0;
  }
  .free {
    background: var(--suit);
    color: var(--accent-ink);
  }
  .mine {
    background: var(--suit);
    color: var(--accent-ink);
    outline: 3px solid #fff;
    outline-offset: -3px;
  }
  .mine .tag {
    font-size: 11px;
    background: var(--bg);
    color: #fff;
    border-radius: 999px;
    min-width: 16px;
    height: 16px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .other,
  .disabled {
    background: var(--surface);
    color: var(--faint);
    border: 1px solid var(--line);
    font-weight: 500;
  }
  .other .tag {
    font-size: 11px;
    color: var(--muted);
  }
  .gone {
    background: var(--surface);
    color: #4f667a;
    text-decoration: line-through;
    font-weight: 500;
  }
</style>
