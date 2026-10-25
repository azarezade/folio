<script lang="ts">
  // The activity ring in the toolbar and its popover (Device board): every
  // job with progress, cancel or retry, and the log.
  import { app } from '../lib/state.svelte';
  import { call } from '../lib/api';
  import { JobRow, Button } from '../lib/ui';
  import type { JobInfo } from '../lib/types';
  import type { IconName } from '../lib/ui/Icon.svelte';

  const jobs = $derived(Object.values(app.jobs).sort((a, b) => b.id - a.id).slice(0, 12));
  const running = $derived(app.runningJobs);
  const frac = $derived.by(() => {
    const r = running.filter((j) => j.total > 0);
    if (!running.length) return 0;
    if (!r.length) return 0.25;
    return r.reduce((s, j) => s + j.done / j.total, 0) / r.length;
  });
  const icons: Record<string, IconName> = { sync: 'sync', export: 'export', push: 'upload', pull: 'download', index: 'search', free: 'kindle', migrate: 'folder' };

  function row(j: JobInfo) {
    return {
      title: j.title,
      detail: j.state === 'failed' ? `Failed — ${j.message}` : j.state === 'done' ? 'Done' : j.state === 'cancelled' ? 'Cancelled' : [j.total ? `${j.done} of ${j.total}` : '', j.message].filter(Boolean).join(' · '),
      state: j.state,
      progress: j.total ? j.done / j.total : null,
      icon: icons[j.kind] ?? 'sync',
    };
  }
  function clearFinished() {
    app.jobs = Object.fromEntries(Object.entries(app.jobs).filter(([, j]) => j.state === 'running' || j.state === 'queued'));
  }
  function retry(j: JobInfo) {
    if (j.kind === 'sync') app.sync();
  }
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && app.activityOpen && (app.activityOpen = false)} />

<div class="wrap">
  <button
    type="button"
    class="ring"
    class:on={app.activityOpen}
    aria-label={running.length ? `Activity: ${running.length} job${running.length === 1 ? '' : 's'} running` : 'Activity'}
    aria-expanded={app.activityOpen}
    title="Activity"
    onclick={() => (app.activityOpen = !app.activityOpen)}
  >
    <svg width="18" height="18" viewBox="0 0 18 18" fill="none" aria-hidden="true">
      <circle cx="9" cy="9" r="6.5" stroke="var(--fo-fill-2)" stroke-width="2" />
      {#if running.length}
        <circle cx="9" cy="9" r="6.5" stroke="var(--fo-accent)" stroke-width="2" stroke-linecap="round" stroke-dasharray="{Math.max(2, frac * 40.8)} 40.8" transform="rotate(-90 9 9)" />
      {/if}
      <path d="M9 6v3.5" stroke="var(--fo-label-2)" stroke-width="1.4" stroke-linecap="round" />
    </svg>
  </button>
  {#if app.activityOpen}
    <div class="scrim" role="presentation" onclick={() => (app.activityOpen = false)}></div>
    <div class="popover" role="dialog" aria-label="Activity" tabindex="-1">
      <div class="head">
        <h2>Activity</h2>
        <Button variant="plain" onclick={clearFinished}>Clear Finished</Button>
      </div>
      <div class="jobs">
        {#each jobs as j (j.id)}
          <JobRow job={row(j)} oncancel={() => call('cancel_job', { id: j.id })} onretry={j.kind === 'sync' ? () => retry(j) : undefined} />
        {:else}
          <p class="none">Nothing has run yet.</p>
        {/each}
      </div>
      <div class="foot">
        <span>Jobs run in the background; you can close this.</span>
        <Button variant="plain" onclick={() => { app.logOpen = true; app.activityOpen = false; }}>Show Log</Button>
      </div>
    </div>
  {/if}
</div>

<style>
  .wrap { position: relative; }
  .ring { width: 30px; height: 28px; border: 0; border-radius: var(--fo-radius-button); background: transparent; display: flex; align-items: center; justify-content: center; padding: 0; }
  .ring:hover, .ring.on { background: var(--fo-fill); }
  .scrim { position: fixed; inset: 0; z-index: 60; }
  .popover {
    position: absolute;
    top: 36px;
    right: -8px;
    z-index: 61;
    width: 360px;
    max-height: 70vh;
    display: flex;
    flex-direction: column;
    border-radius: var(--fo-radius-window);
    background: var(--fo-bg-popover);
    box-shadow: var(--fo-shadow-popover);
    animation: pop var(--fo-dur-popover) var(--fo-ease);
  }
  @keyframes pop { from { opacity: 0; transform: translateY(-4px); } }
  .head { display: flex; align-items: center; justify-content: space-between; padding: 12px 12px 6px 14px; }
  h2 { font: var(--fo-text-headline); }
  .jobs { overflow-y: auto; }
  .none { padding: 14px; font: var(--fo-text-callout); color: var(--fo-label-2); }
  .foot { display: flex; align-items: center; gap: 8px; padding: 8px 8px 8px 14px; border-top: 1px solid var(--fo-separator); font: var(--fo-text-caption); color: var(--fo-label-2); }
  .foot span { flex: 1; }
</style>
