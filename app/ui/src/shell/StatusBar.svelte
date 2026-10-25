<script lang="ts">
  // Library size · device state · running jobs (StatusBar board).
  import { app } from '../lib/state.svelte';
  import { bytes, plural } from '../lib/format';
  import { activate } from '../lib/ui';

  const d = $derived(app.device);
  const items = $derived(app.counts.all + app.counts.screenshot);
  const size = $derived(app.all.reduce((s, i) => s + i.size, 0));
  const cap = $derived(Number(app.settings.cache_cap_mb || 2048) * 1024 * 1024);
  const running = $derived(app.runningJobs);
  const sync = $derived(running.find((j) => j.kind === 'sync'));
  const first = $derived(running[0]);
  const pct = $derived(first?.total ? Math.round((first.done / first.total) * 100) : null);
  const device = $derived(
    d.busy ? 'Kindle held by another process' : !d.connected ? 'Kindle not connected' : sync ? `${d.name ?? 'Kindle'} · syncing` : `${d.name ?? 'Kindle'} connected`,
  );
  const dot = $derived(d.busy ? 'var(--fo-warning)' : !d.connected ? 'var(--fo-label-3)' : sync ? 'var(--fo-accent)' : 'var(--fo-success-solid)');
  const jobText = $derived(
    first ? `${first.title}${pct != null ? ` ${pct}%` : ''}${running.length > 1 ? ` · ${running.length - 1} more job${running.length > 2 ? 's' : ''}` : ''}` : '',
  );
</script>

<footer class="status">
  <span>{items ? `${plural(items, 'item')} · ${bytes(size)} · cache ${bytes(app.status?.cache_bytes ?? 0)} of ${bytes(cap)}` : 'Library is empty'}</span>
  <span class="sep"></span>
  <span class="device"><span class="dot" style:background={dot}></span><span class:warn={d.busy}>{device}</span></span>
  <span class="grow"></span>
  {#if first}
    <div role="button" tabindex="0" class="job" onclick={() => (app.activityOpen = true)} onkeydown={activate}>
      <span class="track"><span class="fill" style:width={`${pct ?? 25}%`}></span></span>
      <span class="jt">{jobText}</span>
    </div>
  {:else}
    <span>No background jobs</span>
  {/if}
</footer>

<style>
  .status {
    height: var(--fo-statusbar-height);
    flex: none;
    padding: 0 14px;
    display: flex;
    align-items: center;
    gap: 16px;
    background: var(--fo-bg-window);
    border-top: 1px solid var(--fo-separator);
    color: var(--fo-label-2);
    font: var(--fo-text-caption);
    white-space: nowrap;
    overflow: hidden;
  }
  .sep { width: 1px; height: 12px; background: var(--fo-separator); }
  .device { display: flex; align-items: center; gap: 6px; }
  .dot { width: 7px; height: 7px; border-radius: 50%; }
  .warn { color: var(--fo-warning); }
  .grow { flex: 1; }
  .job { display: flex; align-items: center; gap: 8px; height: 20px; padding: 0 8px; border: 0; border-radius: 5px; background: var(--fo-fill); color: var(--fo-label); font: var(--fo-text-caption); overflow: hidden; }
  .job { min-width: 0; }
  .jt { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .track { width: 64px; height: 4px; flex: none; border-radius: 2px; background: var(--fo-fill-2); overflow: hidden; display: flex; }
  .fill { background: var(--fo-accent); border-radius: 2px; }
</style>
