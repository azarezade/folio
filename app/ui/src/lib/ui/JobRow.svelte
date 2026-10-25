<script lang="ts">
  import Icon, { type IconName } from './Icon.svelte';
  import Progress from './Progress.svelte';

  export interface JobRowData {
    title: string;
    /** "214 of 562 files · 2 min left", or the error. */
    detail?: string;
    state: 'queued' | 'running' | 'done' | 'failed' | 'cancelled';
    /** 0…1 while running; null when unknown. */
    progress?: number | null;
    icon?: IconName;
  }

  let { job, oncancel, onretry }: { job: JobRowData; oncancel?: () => void; onretry?: () => void } = $props();
  const live = $derived(job.state === 'running' || job.state === 'queued');
</script>

<div class="fo-job-row" class:failed={job.state === 'failed'}>
  <span class="icon"><Icon name={job.icon ?? 'sync'} size={15} /></span>
  <span class="body">
    <span class="title">{job.title}</span>
    {#if job.state === 'running'}<Progress value={job.progress ?? null} size="small" label={job.title} />{/if}
    {#if job.detail}<span class="detail">{job.detail}</span>{/if}
  </span>
  {#if live && oncancel}
    <button type="button" aria-label="Cancel {job.title}" onclick={oncancel}><Icon name="close" size={10} weight={1.8} /></button>
  {:else if job.state === 'failed' && onretry}
    <button type="button" aria-label="Retry {job.title}" onclick={onretry}><Icon name="retry" size={11} weight={1.6} /></button>
  {:else}
    <span></span>
  {/if}
</div>

<style>
  .fo-job-row {
    display: grid;
    grid-template-columns: 28px minmax(0, 1fr) 22px;
    column-gap: 10px;
    align-items: center;
    padding: 10px 12px;
  }
  .fo-job-row + :global(.fo-job-row) { border-top: 1px solid var(--fo-separator); }
  .icon { width: 28px; height: 28px; border-radius: var(--fo-radius-field); background: var(--fo-fill); display: flex; align-items: center; justify-content: center; color: var(--fo-accent); }
  .failed .icon { color: var(--fo-danger); }
  .body { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .title { font: 600 12px/15px var(--fo-font); color: var(--fo-label); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .detail { font: var(--fo-text-caption); color: var(--fo-label-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .failed .detail { color: var(--fo-danger); }
  button { width: 22px; height: 22px; border: 0; border-radius: 50%; background: var(--fo-fill); color: var(--fo-label-2); padding: 0; display: flex; align-items: center; justify-content: center; cursor: default; }
  button:hover { background: var(--fo-fill-2); color: var(--fo-label); }
  button:focus-visible { outline: 2px solid var(--fo-accent); outline-offset: 1px; }
</style>
