<script lang="ts">
  import type { Snippet } from 'svelte';
  import { i18n } from '../lib/i18n.svelte';

  let { title, back, right }: { title: string; back?: string; right?: Snippet } = $props();
</script>

<header class="row between">
  {#if back}
    <a class="icon-btn" href={'#' + back} aria-label={i18n.t('back')}>
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M15 18l-6-6 6-6" /></svg>
    </a>
  {:else}
    <span style="width: 44px"></span>
  {/if}
  <div style="font-size: 17px; font-weight: 700; text-align: center">{title}</div>
  {#if right}{@render right()}{:else}<span style="width: 44px"></span>{/if}
</header>
