<script lang="ts">
  import Header from '../components/Header.svelte';
  import { i18n } from '../lib/i18n.svelte';
  import { router, session } from '../lib/state.svelte';
  import { difficultyOf, textOf } from '../lib/tasks';
  import { check, taskById, toText } from '../lib/wasm';

  const captain = $derived(session.captain());
  const draftable = $derived(session.tasks.filter((t) => t.owner !== 'crew'));
  const n = session.players;
  const rule = $derived.by(() => {
    const t = draftable.length;
    if (t < n) return i18n.lang === 'de' ? 'höchstens 1 pro Person' : 'at most 1 each';
    const base = Math.floor(t / n);
    const extra = t % n;
    if (!extra) return i18n.lang === 'de' ? `${base} pro Person` : `${base} each`;
    return i18n.lang === 'de'
      ? `${base + 1} für die ersten ${extra} ab Kapitän, sonst ${base}`
      : `${base + 1} for the first ${extra} from the captain, ${base} for the rest`;
  });
  const info = $derived.by(() => {
    try {
      const r = check(toText(session.situation()));
      return 'error' in r ? { error: r.error.msg } : { ok: r.ok };
    } catch (e) {
      return { error: String(e) };
    }
  });

  function setOwner(i: number, owner: string) {
    session.tasks[i].owner = owner;
    session.tasks = [...session.tasks];
  }
</script>

<main class="screen">
  <Header title={i18n.t('owners')} back="/tasks" />

  <div class="panel small" style="line-height: 1.45">
    {#if captain !== null}<span>{i18n.t('captainPicksFirst', { p: i18n.player(captain) })}</span>{/if}
    <span class="muted">{i18n.t('eachTakes', { t: draftable.length, n, rule })}</span>
  </div>

  {#each session.tasks as t, i (t.id)}
    <div class="panel outline">
      <div class="row" style="gap: 6px; font-size: 15px">
        <span class="small muted" style="font-family: var(--mono); font-weight: 700">{t.owner === 'crew' ? '–' : difficultyOf(t.id, n)}</span>
        {textOf(t.id)}
      </div>
      {#if t.owner === 'crew'}
        <span class="small muted">{i18n.owner('crew')}</span>
      {:else}
        <div class="owners" role="radiogroup" aria-label={i18n.t('owners')} style="grid-template-columns: repeat({n + 1 > 5 ? 3 : n + 1}, minmax(0, 1fr))">
          {#each ['?', ...Array.from({ length: n }, (_, p) => `Player${p + 1}`)] as o, k}
            {@const forbidden = k > 0 && k - 1 === captain && !taskById(t.id)?.captain_allowed}
            <button role="radio" aria-checked={t.owner === o} disabled={forbidden} class:on={t.owner === o} onclick={() => setOwner(i, o)}>
              {i18n.owner(o)}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/each}

  {#if 'ok' in info && info.ok}
    {#if session.tasks.some((t) => t.owner === '?')}
      <p class="small muted">{i18n.t('unknownOwner', { n: info.ok.drafts })}</p>
    {/if}
  {:else if 'error' in info}
    <div class="error">{info.error}</div>
  {/if}

  <label class="panel row between" style="flex-direction: row">
    <span style="display: flex; flex-direction: column; gap: 2px">
      {i18n.t('checkAllDrafts')}
      <span class="small muted">{i18n.t('checkAllDraftsSub')}</span>
    </span>
    <input type="checkbox" bind:checked={session.checkAllDrafts} style="width: 24px; height: 24px; accent-color: var(--accent)" />
  </label>

  <div class="grow"></div>
  <button class="btn primary" disabled={'error' in info} onclick={() => router.go('/result')}>{i18n.t('analyse')}</button>
</main>

<style>
  .owners {
    display: grid;
    gap: 4px;
  }
  .owners button {
    min-height: 44px;
    border-radius: 10px;
    border: 1px solid var(--border);
    background: transparent;
    font-size: 12px;
    font-weight: 600;
    padding: 0 2px;
  }
  .owners button.on {
    background: var(--accent);
    color: var(--accent-ink);
    border-color: var(--accent);
  }
  .owners button:disabled {
    opacity: 0.3;
  }
</style>
