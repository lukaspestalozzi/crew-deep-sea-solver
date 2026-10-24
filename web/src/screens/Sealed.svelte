<script lang="ts">
  import Header from '../components/Header.svelte';
  import { i18n } from '../lib/i18n.svelte';
  import { router, session } from '../lib/state.svelte';
  import { saveCurrent } from '../lib/saved';

  let error = $state<string | null>(null);

  function reveal() {
    session.sealed = false;
    router.go('/tasks');
  }

  async function later() {
    try {
      await saveCurrent(null);
      router.go('/');
    } catch (e) {
      error = String(e);
    }
  }
</script>

<main class="screen">
  <Header title={i18n.t('sealedTitle')} back="/" />
  <div class="grow" style="display: flex; flex-direction: column; justify-content: center; gap: 12px">
    <h1>{i18n.t('sealedTitle')}</h1>
    <p class="muted">{i18n.t('sealedSub')}</p>
    {#if session.played.length}<p class="small muted">{i18n.t('playedTricks')}: {session.played.length}</p>{/if}
  </div>
  {#if error}<div class="error">{error}</div>{/if}
  <button class="btn" onclick={() => router.go('/live')}>{i18n.t('liveLog')}</button>
  <button class="btn" onclick={later}>{i18n.t('saveForLater')}</button>
  <button class="btn primary" onclick={reveal}>{i18n.t('reveal')}</button>
</main>
