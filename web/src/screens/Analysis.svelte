<script lang="ts">
  import { untrack } from 'svelte';
  import Header from '../components/Header.svelte';
  import AnalysisView from '../components/AnalysisView.svelte';
  import { i18n } from '../lib/i18n.svelte';
  import { mission } from '../lib/mission.svelte';
  import { session } from '../lib/state.svelte';
  import { toText } from '../lib/wasm';

  const text = $derived.by(() => {
    try {
      return toText(session.situation());
    } catch {
      return '';
    }
  });
  // The same mission as on Result (kept if already loaded there).
  $effect(() => {
    const t = text;
    if (t) untrack(() => mission.load(t));
  });
</script>

<main class="screen">
  <Header title={i18n.t('analysis')} back="/result" />
  {#if mission.verdictJob.running}
    <p class="muted">{i18n.t('computing')} <span class="small">{mission.verdictJob.progress}</span></p>
  {/if}
  <AnalysisView />
</main>
