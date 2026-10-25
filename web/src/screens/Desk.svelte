<script lang="ts">
  import ResultView from '../components/ResultView.svelte';
  import { i18n } from '../lib/i18n.svelte';
  import { router, session } from '../lib/state.svelte';
  import { check, parse, toText } from '../lib/wasm';

  const SAMPLE = `crew 1
title: Example
players: 4
Player1: P2 P8 B4 B9 G1 G6 Y3 Y7 S2 S4
Player2: P1 P5 B1 B2 B6 G4 G9 Y2 Y6 S1
Player3: P3 P6 B3 B8 G2 G5 G8 Y1 Y8 S3
Player4: rest

tasks:
Player2: win B4
?:       win P9 Y8
Player1: first-trick
?:       tricks =2
`;

  function initial() {
    try {
      return toText(session.situation());
    } catch {
      return SAMPLE;
    }
  }

  let text = $state(initial());
  let analysed = $state<string | null>(null);
  let checkAll = $state(false);

  const status = $derived.by(() => {
    const r = check(text);
    return 'error' in r ? { error: r.error.msg } : { ok: r.ok };
  });

  function openInApp() {
    const p = parse(text);
    if ('error' in p) return;
    session.load(p.ok[0], 'explore');
    router.go('/result');
  }

  async function drop(e: DragEvent) {
    e.preventDefault();
    const f = e.dataTransfer?.files[0];
    if (f) text = await f.text();
  }
</script>

<main class="screen wide">
  <div class="row between" style="flex-wrap: wrap; gap: 12px; border-bottom: 1px solid var(--line); padding-bottom: 12px">
    <div class="row" style="gap: 12px">
      <a href="#/" style="font-size: 20px; font-weight: 700; color: var(--text); text-decoration: none">Deep Sea Solver</a>
      <span class="small muted">{i18n.t('deskTitle')}</span>
    </div>
    <div class="row" style="flex-wrap: wrap">
      <a class="btn small" href="#/library">{i18n.t('saved')}</a>
      <div class="seg" role="radiogroup" aria-label="Language">
        <button role="radio" aria-checked={i18n.lang === 'de'} onclick={() => i18n.set('de')}>DE</button>
        <button role="radio" aria-checked={i18n.lang === 'en'} onclick={() => i18n.set('en')}>EN</button>
      </div>
    </div>
  </div>

  <div class="cols">
    <section>
      <label class="small muted" style="display: flex; flex-direction: column; gap: 6px" ondragover={(e) => e.preventDefault()} ondrop={drop}>
        {i18n.t('editText')}
        <textarea class="input" spellcheck="false" rows="22" bind:value={text}></textarea>
      </label>
      {#if 'ok' in status && status.ok}
        <span class="small" style="color: var(--good)">
          ✓ {i18n.t('valid', { p: i18n.player(status.ok.captain), d: status.ok.difficulty })}
        </span>
      {:else if 'error' in status}
        <div class="error">{status.error}</div>
      {/if}
      <label class="row small">
        <input type="checkbox" bind:checked={checkAll} style="accent-color: var(--accent)" />
        {i18n.t('checkAllDrafts')}
      </label>
      <div class="row">
        <button class="btn primary" disabled={'error' in status} onclick={() => (analysed = text)}>{i18n.t('analyse')}</button>
        <button class="btn" disabled={'error' in status} onclick={openInApp}>{i18n.t('loadIntoApp')}</button>
      </div>
    </section>
    <section>
      <span class="label">{i18n.t('result')}</span>
      {#if analysed}
        {#key analysed + checkAll}
          <ResultView text={analysed} {checkAll} />
        {/key}
      {/if}
    </section>
  </div>
</main>

<style>
  .cols {
    display: flex;
    flex-wrap: wrap;
    gap: 24px;
  }
  section {
    flex: 1 1 380px;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
</style>
