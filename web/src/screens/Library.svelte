<script lang="ts">
  import Header from '../components/Header.svelte';
  import ExportSheet from '../components/ExportSheet.svelte';
  import { i18n, type Key } from '../lib/i18n.svelte';
  import { openSaved, verdictPill } from '../lib/saved';
  import { deleteSaved, listSaved, newId, putSaved, type SavedRecord, type SituationType } from '../lib/storage';
  import { parse, toText, type Situation } from '../lib/wasm';

  let records = $state<SavedRecord[]>([]);
  let pasting = $state(false);
  let pasted = $state('');
  let error = $state<string | null>(null);
  let exportText = $state<{ text: string; title: string } | null>(null);

  const refresh = () => listSaved().then((r) => (records = r));
  refresh();

  const TYPE_KEY: Record<SituationType, Key> = {
    sealed: 'typeSealed',
    full: 'typeFull',
    reconstructed: 'typeReconstructed',
    explore: 'typeExplore',
  };

  function typeOf(s: Situation): SituationType {
    if (s.sealed) return 'sealed';
    if (s.played.length) return 'reconstructed';
    return 'full';
  }

  async function importText(text: string) {
    error = null;
    const r = parse(text);
    if ('error' in r) {
      error = (r.error.line ? `line ${r.error.line}: ` : '') + r.error.msg;
      return;
    }
    for (const s of r.ok) {
      await putSaved({
        id: newId(),
        title: s.title || `${i18n.t('import')} ${new Date().toLocaleString()}`,
        type: typeOf(s),
        players: s.hands.length,
        updated: Date.now(),
        text: toText(s),
        verdict: null,
      });
    }
    pasting = false;
    pasted = '';
    refresh();
  }

  async function onFile(e: Event) {
    const files = (e.target as HTMLInputElement).files;
    if (!files) return;
    for (const f of files) await importText(await f.text());
  }

  async function remove(r: SavedRecord) {
    await deleteSaved(r.id);
    refresh();
  }

  function exportAll() {
    exportText = { text: records.map((r) => r.text).join('---\n'), title: i18n.t('library') };
  }
</script>

<main class="screen">
  <Header title={i18n.t('library')} back="/" />
  <div class="cols-2">
    <label class="btn">
      {i18n.t('importFile')}
      <input type="file" accept=".crew,.txt,text/plain" multiple hidden onchange={onFile} />
    </label>
    <button class="btn" onclick={() => (pasting = !pasting)}>{i18n.t('pasteText')}</button>
  </div>
  {#if pasting}
    <textarea class="input" rows="8" placeholder={i18n.t('pasteHere')} bind:value={pasted}></textarea>
    <button class="btn primary" disabled={!pasted.trim()} onclick={() => importText(pasted)}>{i18n.t('import')}</button>
  {/if}
  {#if error}<div class="error">{error}</div>{/if}
  <p class="small muted">{i18n.t('deviceOnly')}</p>

  <div style="display: flex; flex-direction: column">
    {#each records as r (r.id)}
      {@const v = verdictPill(r.verdict)}
      <div class="list-row">
        <button class="open" onclick={() => openSaved(r)}>
          <span style="font-weight: 600">{r.title}</span>
          <span class="small muted">{i18n.t(TYPE_KEY[r.type])} · {r.players} {i18n.t('players')} · {new Date(r.updated).toLocaleDateString()}</span>
        </button>
        {#if r.type === 'sealed'}
          <span class="pill" style="background: transparent; color: var(--accent); border: 1px solid var(--accent)">{i18n.t('typeSealed')}</span>
        {:else if v}
          <span class="pill" style="background: {v.bg}">{i18n.t(v.key)}</span>
        {/if}
        <button class="icon-btn" aria-label={i18n.t('export')} onclick={() => (exportText = { text: r.text, title: r.title })}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12M7 8l5-5 5 5M5 15v4a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-4" /></svg>
        </button>
        <button class="icon-btn" aria-label={i18n.t('delete')} onclick={() => remove(r)}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13" /></svg>
        </button>
      </div>
    {:else}
      <p class="small muted" style="padding: 8px 0">{i18n.t('empty')}</p>
    {/each}
  </div>

  {#if records.length > 1}
    <button class="btn" onclick={exportAll}>{i18n.t('exportAll')}</button>
  {/if}
</main>

{#if exportText}
  <ExportSheet text={exportText.text} title={exportText.title} onclose={() => (exportText = null)} />
{/if}

<style>
  .open {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    text-align: left;
    background: transparent;
    border: 0;
    padding: 0;
    min-height: 44px;
    justify-content: center;
  }
</style>
