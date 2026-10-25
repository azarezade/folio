<script lang="ts">
  // The Kindle (Device board): a summary with storage and sync, the files on
  // it, or why it can't be reached (held by macOS, not connected).
  import { app } from '../lib/state.svelte';
  import { call } from '../lib/api';
  import { bytes, plural } from '../lib/format';
  import { Button, ToolbarButton, Switch, Progress, Banner, EmptyState, Icon } from '../lib/ui';
  import Toolbar from '../shell/Toolbar.svelte';
  import ActivityButton from '../shell/ActivityButton.svelte';
  import FilesBrowser from './FilesBrowser.svelte';
  import { setPtpcamerad } from '../shell/ptpcamerad';

  type Summary = Record<string, { found: number; new: number; changed: number; up_to_date: number; bytes_to_copy: number }>;
  let { files = false }: { files?: boolean } = $props();
  let preview = $state<Summary | null>(null);
  let checking = $state(false);

  const d = $derived(app.device);
  const sync = $derived(app.runningJobs.find((j) => j.kind === 'sync'));
  const freeing = $derived(app.runningJobs.some((j) => j.kind === 'free'));
  const lastSync = $derived(Object.values(app.jobs).filter((j) => j.kind === 'sync' && j.state === 'done').sort((a, b) => b.id - a.id)[0]);
  const used = $derived(d.total_bytes && d.free_bytes != null ? d.total_bytes - d.free_bytes : 0);
  // what the library knows it copied, as a rough split of the used space
  const parts = $derived.by(() => {
    if (!d.total_bytes) return [];
    const sum = (xs: { size: number }[]) => xs.reduce((s, x) => s + x.size, 0);
    const nb = sum(app.items.notebook);
    const docs = sum(app.items.book.filter((b) => b.format === 'PDF'));
    const books = sum(app.items.book.filter((b) => b.format !== 'PDF'));
    const other = Math.max(0, used - nb - docs - books);
    return [
      { label: 'Notebooks', n: nb, color: 'var(--fo-accent)' },
      { label: 'Books', n: books, color: 'var(--fo-cover-4)' },
      { label: 'Documents', n: docs, color: 'var(--fo-cover-2)' },
      { label: 'System & other', n: other, color: 'var(--fo-label-3)' },
    ].filter((p) => p.n > 0);
  });
  const pending = $derived(preview ? Object.values(preview).reduce((n, s) => n + s.new + s.changed, 0) : 0);
  const pendingBytes = $derived(preview ? Object.values(preview).reduce((n, s) => n + s.bytes_to_copy, 0) : 0);
  const checked = $derived(preview ? Object.values(preview).reduce((n, s) => n + s.found, 0) : 0);

  async function check() {
    checking = true;
    try {
      preview = (await call<{ summary: Summary }>('sync_preview', {})).summary;
    } catch {
      preview = null;
    } finally {
      checking = false;
    }
  }
  $effect(() => {
    if (d.connected && !d.busy && !sync && !files) check();
  });
</script>

<div class="screen">
  <Toolbar
    title={files ? 'Files' : (d.name ?? 'Kindle Scribe')}
    subtitle={d.busy ? 'Held by another process' : !d.connected ? 'Not connected' : sync ? `Syncing${sync.total ? ` · ${Math.round((sync.done / sync.total) * 100)}%` : '…'}` : files ? `${d.name ?? 'Kindle'} · ${app.pushTarget}` : 'Connected via USB'}
  >
    <ToolbarButton icon="sync" label={sync ? 'Syncing…' : 'Sync Now'} showLabel disabled={!d.connected || d.busy || !!sync} onclick={() => app.sync()} />
    <ToolbarButton icon="upload" label="Send to Kindle…" showLabel disabled={!d.connected || d.busy} onclick={() => (app.sendOpen = true)} />
    <ActivityButton />
  </Toolbar>

  <div class="body">
    {#if d.busy}
      <section class="held" aria-labelledby="held-title">
        <Icon name="warning" size={40} weight={1} color="var(--fo-warning)" />
        <h2 id="held-title">Another process is holding the Kindle</h2>
        <p>macOS's photo service claimed the USB connection when the Kindle was plugged in, so this app can't reach it. Freeing it only stops that service's hold on this device — nothing else is closed.</p>
        <dl>
          <dt>Process</dt><dd>{d.holder?.split(' ')[0] ?? 'ptpcamerad'}</dd>
          {#if d.holder}<dt>Details</dt><dd>{d.holder}</dd>{/if}
          <dt>Belongs to</dt><dd>Image Capture (macOS)</dd>
        </dl>
        <div class="buttons">
          <Button variant="primary" disabled={freeing} onclick={() => app.run('Free the Kindle', () => call('free_kindle'))}>{freeing ? 'Freeing…' : 'Free the Kindle'}</Button>
          <Button onclick={() => app.run('Check', async () => (app.device = await call('device_status')))}>Try Again</Button>
        </div>
        <label class="stop">
          <Switch label="Stop macOS from grabbing the Kindle" checked={app.settings.ptpcamerad_disabled === 'true'} onchange={(v) => setPtpcamerad(v)} />
          <span><strong>Stop macOS from grabbing the Kindle</strong><br />Tells Image Capture to ignore devices like this one, so this won't happen again. You can turn it off in Settings › Sync.</span>
        </label>
      </section>
    {:else if d.error && !d.connected}
      <div class="center"><EmptyState icon="warning" title="The Kindle didn’t answer" body={d.error}>{#snippet actions()}<Button onclick={() => app.run('Check', async () => (app.device = await call('device_status')))}>Try Again</Button>{/snippet}</EmptyState></div>
    {:else if !d.connected}
      <div class="center"><EmptyState icon="kindle" title="Connect your Kindle" body="Plug it in with a USB cable, then wake and unlock it. It appears here on its own." /></div>
    {:else if files}
      <FilesBrowser />
    {:else}
      <section class="card summary" aria-label="Device summary">
        <div class="dev">
          <Icon name="kindle" size={36} weight={1} color="var(--fo-accent)" />
          <div>
            <p class="name">{d.name ?? 'Kindle Scribe'}</p>
            <p class="sub">Connected via USB · {bytes(d.total_bytes)}</p>
          </div>
        </div>
        {#if d.total_bytes}
          <div class="storage" role="img" aria-label="Storage: {bytes(used)} used, {bytes(d.free_bytes)} free">
            <div class="track">
              {#each parts as p}<span style:width="{(p.n / d.total_bytes) * 100}%" style:background={p.color}></span>{/each}
            </div>
            <div class="legend">
              {#each parts as p}<span><i style:background={p.color}></i>{p.label} {bytes(p.n)}</span>{/each}
              <span class="free">{bytes(d.free_bytes)} free</span>
            </div>
          </div>
        {/if}
      </section>

      <section class="card">
        {#if sync}
          <div class="syncing">
            <div class="line"><p class="h">Syncing to your library…</p><span>{sync.message}</span></div>
            <Progress value={sync.total ? sync.done / sync.total : null} label="Sync progress" />
            <p class="detail">{sync.total ? `Copying ${sync.done} of ${plural(sync.total, 'file')}` : 'Looking for changes'}</p>
            <div class="buttons">
              <Button onclick={() => call('cancel_job', { id: sync.id })}>Cancel Sync</Button>
              <Button variant="plain" onclick={() => (app.logOpen = true)}>Show Log</Button>
            </div>
          </div>
        {:else}
          <div class="state">
            {#if checking && !preview}
              <span class="fo-spinner"></span><p class="h">Checking what changed…</p>
            {:else if pending}
              <Icon name="sync" color="var(--fo-accent)" /><p class="h">{plural(pending, 'file')} to copy · {bytes(pendingBytes)}</p>
              <Button variant="primary" onclick={() => app.sync()}>Sync Now</Button>
            {:else}
              <Icon name="check" color="var(--fo-success)" /><p class="h">Library is up to date</p>
            {/if}
          </div>
          <p class="detail">{lastSync ? `Last synced this session · ${lastSync.message || 'done'}` : 'Not synced yet this session'}{preview ? ` · ${plural(checked, 'file')} checked` : ''}</p>
          <label class="auto">
            <span>Sync when the Kindle is plugged in</span>
            <Switch label="Sync when plugged in" checked={app.settings.auto_sync === 'true'} onchange={(v) => app.setSetting('auto_sync', String(v))} />
          </label>
        {/if}
      </section>

      <FilesBrowser />
    {/if}
  </div>
</div>

<style>
  .screen { display: flex; flex-direction: column; height: 100%; min-width: 0; }
  .body { flex: 1; overflow-y: auto; padding: 20px 28px; display: flex; flex-direction: column; gap: 16px; background: var(--fo-bg-group); }
  .center { flex: 1; display: flex; align-items: center; justify-content: center; }
  .card { border-radius: var(--fo-radius-window); background: var(--fo-bg-window); box-shadow: 0 0 0 1px var(--fo-separator); padding: 16px; display: flex; flex-direction: column; gap: 12px; }
  .summary { flex-direction: row; align-items: center; gap: 24px; flex-wrap: wrap; }
  .dev { display: flex; align-items: center; gap: 12px; min-width: 200px; }
  .name { font: var(--fo-text-toolbar-title); }
  .sub, .detail { font: var(--fo-text-callout); color: var(--fo-label-2); }
  .storage { flex: 1; min-width: 280px; display: flex; flex-direction: column; gap: 8px; }
  .track { height: 10px; border-radius: 5px; background: var(--fo-fill-2); display: flex; overflow: hidden; gap: 1px; }
  .legend { display: flex; flex-wrap: wrap; gap: 4px 14px; font: var(--fo-text-caption); color: var(--fo-label-2); }
  .legend span { display: inline-flex; align-items: center; gap: 5px; }
  .legend i { width: 8px; height: 8px; border-radius: 2px; }
  .legend .free { margin-left: auto; }
  .state, .line { display: flex; align-items: center; gap: 10px; }
  .line { justify-content: space-between; font: var(--fo-text-callout); color: var(--fo-label-2); }
  .h { font: var(--fo-text-headline); color: var(--fo-label); flex: 1; }
  .syncing { display: flex; flex-direction: column; gap: 10px; }
  .buttons { display: flex; gap: 8px; }
  .auto { display: flex; align-items: center; justify-content: space-between; padding-top: 10px; border-top: 1px solid var(--fo-separator); font: var(--fo-text-body); }
  .held { max-width: 560px; margin: 24px auto; display: flex; flex-direction: column; align-items: flex-start; gap: 12px; }
  .held h2 { font: var(--fo-text-large-title); }
  .held p { font: var(--fo-text-body); line-height: 1.5; color: var(--fo-label-2); }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 16px; margin: 0; padding: 12px 14px; border-radius: var(--fo-radius-group); background: var(--fo-fill); font: var(--fo-text-callout); width: 100%; }
  dt { color: var(--fo-label-2); }
  dd { margin: 0; font-family: var(--fo-font-mono); }
  .stop { display: flex; gap: 12px; align-items: flex-start; padding-top: 12px; border-top: 1px solid var(--fo-separator); font: var(--fo-text-callout); color: var(--fo-label-2); }
  .stop strong { color: var(--fo-label); font-weight: 600; }
</style>
