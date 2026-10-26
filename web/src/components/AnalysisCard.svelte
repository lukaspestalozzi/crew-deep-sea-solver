<script lang="ts">
  // One analysis: what it answers, a button to run it, progress and cancel,
  // and its result (`children`, shown when `show`, also while running).
  import type { Snippet } from 'svelte';
  import { i18n } from '../lib/i18n.svelte';
  import { mission, type Job } from '../lib/mission.svelte';
  import { inspector } from '../lib/inspector.svelte';
  import WorkList from './WorkList.svelte';

  let {
    title,
    sub,
    job,
    show,
    action = i18n.t('compute'),
    onrun,
    children,
  }: { title: string; sub: string; job: Job; show: boolean; action?: string; onrun: () => void; children?: Snippet } = $props();
</script>

<section class="panel">
  <span class="label">{title}</span>
  <span class="small muted">{sub}</span>
  {#if job.running}
    <div class="row between">
      <span class="small">{i18n.t('computing')} <span class="muted">{job.progress}</span></span>
      <button class="btn small" onclick={() => mission.cancel()}>{i18n.t('cancel')}</button>
    </div>
  {:else}
    {#if job.error}<div class="error">{job.error}</div>{/if}
    {#if job.cancelled}
      <span class="small muted">{job.last ? i18n.t('cancelledAt', { state: job.last }) : i18n.t('cancelled')}</span>
    {/if}
    {#if !show || job.cancelled}
      <div><button class="btn small outline-accent" disabled={mission.busy} onclick={onrun}>{action}</button></div>
    {/if}
  {/if}
  {#if show}{@render children?.()}{/if}
  {#if inspector.on}<WorkList job={job.name} />{/if}
</section>
