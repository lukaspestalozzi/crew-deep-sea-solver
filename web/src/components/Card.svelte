<script lang="ts">
  import { SUIT_COLOR, suitOf } from '../lib/cards';

  let { card, faded = false }: { card: string; faded?: boolean } = $props();
</script>

<span class="chip-card" style="background: {SUIT_COLOR[suitOf(card)]}; opacity: {faded ? 0.75 : 1}">{card}</span>
