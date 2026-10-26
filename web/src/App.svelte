<script lang="ts">
  import { router, session } from './lib/state.svelte';
  import Home from './screens/Home.svelte';
  import Deal from './screens/Deal.svelte';
  import Seal from './screens/Seal.svelte';
  import Sealed from './screens/Sealed.svelte';
  import Reconstruct from './screens/Reconstruct.svelte';
  import Tasks from './screens/Tasks.svelte';
  import Owners from './screens/Owners.svelte';
  import Result from './screens/Result.svelte';
  import Library from './screens/Library.svelte';
  import Desk from './screens/Desk.svelte';
  import Analysis from './screens/Analysis.svelte';

  // Keep the work in progress across reloads.
  try {
    const snap = localStorage.getItem('session');
    if (snap) session.restore(JSON.parse(snap));
  } catch {
    /* ignore a broken snapshot */
  }
  $effect(() => {
    const snap = JSON.stringify(session.snapshot());
    try {
      localStorage.setItem('session', snap);
    } catch {
      /* storage unavailable */
    }
  });
</script>

{#if router.path === '/deal'}
  <Deal />
{:else if router.path === '/seal'}
  <Seal />
{:else if router.path === '/sealed'}
  <Sealed />
{:else if router.path === '/reconstruct'}
  <Reconstruct live={false} />
{:else if router.path === '/live'}
  <Reconstruct live={true} />
{:else if router.path === '/tasks'}
  <Tasks />
{:else if router.path === '/owners'}
  <Owners />
{:else if router.path === '/result'}
  <Result />
{:else if router.path === '/library'}
  <Library />
{:else if router.path === '/analysis'}
  <Analysis />
{:else if router.path === '/desk'}
  <Desk />
{:else}
  <Home />
{/if}
