<script lang="ts">
  import { i18n } from '../lib/i18n.svelte';
  import { router, session, type Mode } from '../lib/state.svelte';
  import { listSaved, type SavedRecord } from '../lib/storage';
  import { parse, randomSituation } from '../lib/wasm';
  import { openSaved, verdictPill } from '../lib/saved';

  let saved = $state<SavedRecord[]>([]);
  let error = $state<string | null>(null);
  listSaved()
    .then((all) => (saved = all.slice(0, 3)))
    .catch(() => {});

  const hasWork = $derived(Object.keys(session.owner).length > 0 || session.tasks.length > 0);

  function start(mode: Mode, path: string) {
    session.reset(mode, session.players);
    router.go(path);
  }

  function random() {
    const r = randomSituation(session.players, 7);
    if ('error' in r) return (error = r.error.msg);
    const p = parse(r.ok);
    if ('error' in p) return (error = p.error.msg);
    session.load(p.ok[0], 'explore');
    router.go('/tasks');
  }
</script>

<main class="screen">
  <div class="row between">
    <div class="seg" role="radiogroup" aria-label={i18n.t('players')}>
      {#each [3, 4, 5] as n}
        <button role="radio" aria-checked={session.players === n} onclick={() => (session.players = n)}>{n}</button>
      {/each}
    </div>
    <div class="seg" role="radiogroup" aria-label="Language">
      <button role="radio" aria-checked={i18n.lang === 'de'} onclick={() => i18n.set('de')}>DE</button>
      <button role="radio" aria-checked={i18n.lang === 'en'} onclick={() => i18n.set('en')}>EN</button>
    </div>
  </div>

  <div style="display: flex; flex-direction: column; gap: 4px">
    <div class="label">The Crew · Mission Deep Sea</div>
    <h1 style="font-size: 30px">Deep Sea Solver</h1>
    <p class="muted">{i18n.t('tagline')}</p>
  </div>

  {#if hasWork}
    <button class="btn outline-accent" onclick={() => router.go(session.mode === 'sealed' && session.sealed ? '/sealed' : '/tasks')}>
      ↻ {session.title ?? i18n.t('deal')} · {i18n.t('next')}
    </button>
  {/if}

  <div style="display: flex; flex-direction: column; gap: 12px">
    <button class="big primary-card" onclick={() => start('sealed', '/seal')}>
      <span class="t">{i18n.t('recordBeforePlay')}</span>
      <span class="s">{i18n.t('recordBeforePlaySub')}</span>
    </button>
    <button class="big" onclick={() => start('reconstruct', '/reconstruct')}>
      <span class="t">{i18n.t('reconstruct')}</span>
      <span class="s muted">{i18n.t('reconstructSub')}</span>
    </button>
    <button class="big" onclick={() => start('full', '/deal')}>
      <span class="t">{i18n.t('fullDeal')}</span>
      <span class="s muted">{i18n.t('fullDealSub')}</span>
    </button>
    <div class="cols-2">
      <button class="btn" onclick={() => router.go('/library')}>{i18n.t('importText')}</button>
      <button class="btn" onclick={random}>{i18n.t('randomDeal')}</button>
    </div>
    <a class="btn" href="#/desk">{i18n.t('desk')}</a>
  </div>

  {#if error}<div class="error">{error}</div>{/if}

  <div style="display: flex; flex-direction: column">
    <div class="row between">
      <span class="label">{i18n.t('saved')}</span>
      <a href="#/library" class="small" style="min-height: 44px; display: flex; align-items: center">{i18n.t('allSaved')}</a>
    </div>
    {#each saved as r (r.id)}
      {@const v = verdictPill(r.verdict)}
      <button class="list-row saved" onclick={() => openSaved(r)}>
        <span style="display: flex; flex-direction: column; gap: 2px; flex-grow: 1; text-align: left">
          <span style="font-weight: 600">{r.title}</span>
          <span class="small muted">{r.players} {i18n.t('players')} · {new Date(r.updated).toLocaleDateString()}</span>
        </span>
        {#if v}<span class="pill" style="background: {v.bg}">{i18n.t(v.key)}</span>{/if}
      </button>
    {:else}
      <p class="small muted" style="padding: 8px 0">{i18n.t('empty')}</p>
    {/each}
  </div>
</main>

<style>
  .big {
    text-align: left;
    border-radius: 14px;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    background: var(--surface);
    border: 1px solid var(--border);
  }
  .big .t {
    font-size: 17px;
    font-weight: 600;
  }
  .big .s {
    font-size: 14px;
    line-height: 1.4;
  }
  .primary-card {
    background: var(--accent);
    color: var(--accent-ink);
    border: 0;
  }
  .primary-card .t {
    font-weight: 700;
  }
  .saved {
    background: transparent;
    border: 0;
    border-bottom: 1px solid var(--line);
    width: 100%;
  }
</style>
