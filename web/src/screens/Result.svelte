<script lang="ts">
  import Header from '../components/Header.svelte';
  import ResultView from '../components/ResultView.svelte';
  import ExportSheet from '../components/ExportSheet.svelte';
  import { i18n } from '../lib/i18n.svelte';
  import { router, session } from '../lib/state.svelte';
  import { saveCurrent } from '../lib/saved';
  import { toText } from '../lib/wasm';

  let verdict = $state<'feasible' | 'infeasible' | 'redeal' | null>(null);
  let saved = $state(false);
  let exporting = $state(false);

  const text = $derived.by(() => {
    try {
      return toText(session.situation());
    } catch (e) {
      return '';
    }
  });

  async function save() {
    await saveCurrent(verdict);
    saved = true;
  }
</script>

<main class="screen">
  <Header title={i18n.t('result')} back="/owners" />
  {#if text}
    <ResultView {text} onverdict={(v) => (verdict = v)}>
      {#snippet actions()}
        {#if verdict === 'feasible' || verdict === 'infeasible'}
          <button class="btn" onclick={() => router.go('/analysis')}>{i18n.t('openAnalysis')}</button>
        {/if}
      {/snippet}
    </ResultView>
  {/if}
  <div class="grow"></div>
  <div class="cols-2">
    <button class="btn" onclick={save}>{saved ? i18n.t('savedOk') : i18n.t('save')}</button>
    <button class="btn" onclick={() => (exporting = true)}>{i18n.t('export')}</button>
  </div>
</main>

{#if exporting}
  <ExportSheet {text} title={session.title ?? i18n.t('result')} onclose={() => (exporting = false)} />
{/if}
