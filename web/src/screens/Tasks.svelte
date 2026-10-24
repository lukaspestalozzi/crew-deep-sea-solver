<script lang="ts">
  import Header from '../components/Header.svelte';
  import Card from '../components/Card.svelte';
  import { i18n } from '../lib/i18n.svelte';
  import { router, session } from '../lib/state.svelte';
  import { CATEGORIES, SYNTAX, categoryName, difficultyOf, drawToTarget, isInferred, taskText } from '../lib/tasks';
  import { getCatalogue, resolveTask, taskById } from '../lib/wasm';

  let query = $state('');
  let custom = $state('');
  const customResult = $derived(custom.trim() ? resolveTask(custom) : null);
  let category = $state<string | null>(null);

  const selected = $derived(new Set(session.tasks.map((t) => t.id)));
  const list = $derived(
    getCatalogue().filter((t) => {
      if (category ? t.category !== category : t.rule) return false;
      if (!query.trim()) return true;
      const q = query.trim().toLowerCase();
      return t.id.toLowerCase().includes(q) || taskText(t).toLowerCase().includes(q);
    }),
  );
  const taskCount = $derived(session.tasks.filter((t) => t.owner !== 'crew').length);
  const ruleCount = $derived(session.tasks.length - taskCount);
  const sum = $derived(session.tasks.reduce((a, t) => a + difficultyOf(t.id, session.players), 0));
  const approx = $derived(session.tasks.some((t) => isInferred(t.id)) ? '≈' : '');

  function addCustom() {
    if (!customResult || 'error' in customResult) return;
    const t = customResult.ok;
    if (!selected.has(t.id)) toggle(t.id, t.rule);
    custom = '';
  }

  function toggle(id: string, rule: boolean) {
    if (selected.has(id)) session.tasks = session.tasks.filter((t) => t.id !== id);
    else session.tasks = [...session.tasks, { id, owner: rule ? 'crew' : '?' }];
  }

  function fill() {
    if (!session.target) return;
    const ids = drawToTarget(
      session.tasks.filter((t) => t.owner !== 'crew').map((t) => t.id),
      session.players,
      session.target,
    );
    const known = new Set(session.tasks.map((t) => t.id));
    session.tasks = [...session.tasks, ...ids.filter((id) => !known.has(id)).map((id) => ({ id, owner: '?' }))];
  }
</script>

<main class="screen">
  <Header title={i18n.t('tasks')} back={session.played.length ? '/deal' : session.mode === 'sealed' ? '/sealed' : '/deal'} />

  <label class="small muted" style="display: flex; flex-direction: column; gap: 6px">
    {i18n.t('searchTasks')}
    <input class="input" type="search" placeholder={i18n.t('searchPlaceholder')} bind:value={query} />
  </label>

  <div class="row" style="flex-wrap: wrap; gap: 6px">
    <button class="cat" class:on={category === null} onclick={() => (category = null)}>{i18n.lang === 'de' ? 'Alle' : 'All'}</button>
    {#each CATEGORIES as c}
      <button class="cat" class:on={category === c} onclick={() => (category = c)}>{categoryName(c)}</button>
    {/each}
  </div>

  {#if session.tasks.length}
    <div class="panel">
      <span class="label">{i18n.t('selectedTasks')}</span>
      {#each session.tasks as s (s.id)}
        {@const t = taskById(s.id)}
        <div class="row" style="gap: 10px">
          <span class="diff" aria-label={i18n.t('difficulty')}>{t?.rule ? '–' : (isInferred(s.id) ? '≈' : '') + difficultyOf(s.id, session.players)}</span>
          <span class="grow" style="font-size: 15px; line-height: 1.3">{taskText(t)}</span>
          <button class="btn small" onclick={() => toggle(s.id, !!t?.rule)}>{i18n.t('remove')}</button>
        </div>
      {/each}
    </div>
  {/if}

  <div class="panel">
    <span class="label">{i18n.t('customTask')}</span>
    <span class="small muted">{i18n.t('customHint')}</span>
    <div class="row" style="gap: 8px">
      <input class="input grow" placeholder="win P3 Y7" bind:value={custom} onkeydown={(e) => e.key === 'Enter' && addCustom()} />
      <button class="btn small" disabled={!customResult || 'error' in customResult} onclick={addCustom}>{i18n.t('add')}</button>
    </div>
    {#if customResult}
      {#if 'error' in customResult}
        <span class="small" style="color: var(--bad-text)">{customResult.error.msg}</span>
      {:else}
        {@const t = customResult.ok}
        <div class="row" style="gap: 10px">
          <span class="diff">{t.rule ? '–' : (t.official ? '' : '≈') + t.difficulty![session.players - 3]}</span>
          <span class="grow" style="display: flex; flex-direction: column; gap: 2px">
            <span style="font-size: 15px; line-height: 1.3">{taskText(t)}</span>
            <span class="row small muted" style="gap: 4px; flex-wrap: wrap">
              {#each t.cards as c}<Card card={c} />{/each}
              <code>{t.id}</code>
            </span>
          </span>
        </div>
      {/if}
    {/if}
    <details class="small muted">
      <summary>{i18n.t('syntax')}</summary>
      <span class="row" style="gap: 6px; flex-wrap: wrap; margin-top: 6px">
        {#each SYNTAX as ex}<button class="cat" onclick={() => (custom = ex)}><code>{ex}</code></button>{/each}
      </span>
    </details>
  </div>

  {#if category === 'rule'}<p class="small muted">{i18n.t('rulesNote')}</p>{/if}

  <div style="display: flex; flex-direction: column">
    {#each list as t (t.id)}
      <div class="list-row">
        <span class="diff" aria-label={i18n.t('difficulty')}>{t.rule ? '–' : t.difficulty![session.players - 3]}</span>
        <span class="grow" style="display: flex; flex-direction: column; gap: 2px">
          <span style="font-size: 15px; line-height: 1.3">{taskText(t)}</span>
          <span class="row small muted" style="gap: 4px; flex-wrap: wrap">
            {#each t.cards as c}<Card card={c} />{/each}
            <code>{t.id}</code>
            {#if t.doubtful}<span title={t.note ?? ''} style="color: var(--accent)">· {i18n.t('doubtful')}</span>{/if}
          </span>
        </span>
        <button class="btn small" class:added={selected.has(t.id)} onclick={() => toggle(t.id, t.rule)}>
          {selected.has(t.id) ? i18n.t('added') : i18n.t('add')}
        </button>
      </div>
    {/each}
  </div>

  <div class="tray panel">
    <div class="row between small">
      <span>
        <strong>{i18n.t('nTasks', { n: taskCount })}{ruleCount ? ` + ${ruleCount} ${i18n.t('ruleCards')}` : ''}</strong>
        · {session.target ? i18n.t('difficultyOf', { sum: approx + sum, target: session.target }) : i18n.t('difficultySum', { sum: approx + sum })}
      </span>
      <label class="row small muted" style="gap: 4px">
        {i18n.t('target')}
        <input class="input" style="width: 64px; min-height: 36px; padding: 0 8px" type="number" min="1" max="40" bind:value={session.target} />
      </label>
    </div>
    {#if session.target}
      <div class="progress"><div style="width: {Math.min(100, (100 * sum) / session.target)}%"></div></div>
    {/if}
    <div class="cols-2">
      <button class="btn" disabled={!session.target || sum >= session.target} onclick={fill}>{i18n.t('fillRandomly')}</button>
      <button class="btn primary" style="font-size: 15px; min-height: 48px" disabled={taskCount === 0} onclick={() => router.go('/owners')}>{i18n.t('nextOwners')}</button>
    </div>
  </div>
</main>

<style>
  .cat {
    min-height: 36px;
    padding: 0 12px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: transparent;
    font-size: 14px;
  }
  .cat.on {
    background: var(--text);
    color: var(--bg);
    border-color: var(--text);
    font-weight: 600;
  }
  .diff {
    flex-shrink: 0;
    width: 28px;
    height: 28px;
    border-radius: 8px;
    background: var(--line);
    display: flex;
    align-items: center;
    justify-content: center;
    font-family: var(--mono);
    font-weight: 700;
    font-size: 14px;
  }
  .added {
    background: #2b4a33;
    color: var(--good);
    border-color: #3f6b4b;
  }
  .tray {
    position: sticky;
    bottom: 8px;
    box-shadow: 0 -8px 24px rgba(0, 0, 0, 0.35);
  }
</style>
