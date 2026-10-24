<script lang="ts">
  import { i18n } from '../lib/i18n.svelte';

  let { text, title, onclose }: { text: string; title: string; onclose: () => void } = $props();
  let copied = $state(false);

  const fileName = $derived(`${title.replace(/[^\w\- ]+/g, '').trim().replace(/\s+/g, '-') || 'situation'}.crew`);

  async function share() {
    const file = new File([text], fileName, { type: 'text/plain' });
    try {
      if (navigator.canShare?.({ files: [file] })) await navigator.share({ files: [file], title });
      else await navigator.share({ title, text });
    } catch {
      /* dismissed or unsupported */
    }
  }

  function download() {
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([text], { type: 'text/plain' }));
    a.download = fileName;
    a.click();
    URL.revokeObjectURL(a.href);
  }

  async function copy() {
    await navigator.clipboard.writeText(text);
    copied = true;
  }
</script>

<div class="backdrop" role="presentation" onclick={onclose}></div>
<div class="sheet" role="dialog" aria-label={i18n.t('exportTitle', { t: title })}>
  <div class="handle"></div>
  <div style="display: flex; flex-direction: column; gap: 2px">
    <strong style="font-size: 18px">{i18n.t('exportTitle', { t: title })}</strong>
    <span class="small muted">{i18n.t('exportSub')}</span>
  </div>
  <pre>{text}</pre>
  {#if 'share' in navigator}
    <button class="btn primary" onclick={share}>{i18n.t('share')}</button>
  {/if}
  <div class="cols-2">
    <button class="btn" onclick={download}>{i18n.t('download')}</button>
    <button class="btn" onclick={copy}>{copied ? i18n.t('copied') : i18n.t('copy')}</button>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(5, 10, 15, 0.6);
    z-index: 10;
  }
  .sheet {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    max-width: 480px;
    margin: 0 auto;
    background: var(--surface);
    border-radius: 20px 20px 0 0;
    padding: 12px 16px calc(20px + env(safe-area-inset-bottom));
    display: flex;
    flex-direction: column;
    gap: 12px;
    z-index: 11;
    max-height: 85dvh;
  }
  .handle {
    align-self: center;
    width: 40px;
    height: 4px;
    border-radius: 999px;
    background: var(--border);
  }
  pre {
    margin: 0;
    padding: 12px;
    background: var(--bg);
    border-radius: 10px;
    font-family: var(--mono);
    font-size: 12px;
    line-height: 1.55;
    color: #c7d4df;
    white-space: pre-wrap;
    overflow: auto;
    max-height: 30dvh;
  }
</style>
