<script lang="ts">
  import Header from '../components/Header.svelte';
  import PositionView from '../components/PositionView.svelte';
  import { i18n } from '../lib/i18n.svelte';
  import { position } from '../lib/position.svelte';
</script>

<main class="screen">
  <Header title={i18n.t('positionTitle')} back={position.from} />
  {#if position.text}
    <PositionView />
  {:else}
    <p class="muted">{i18n.t('noPosition')}</p>
  {/if}
</main>
