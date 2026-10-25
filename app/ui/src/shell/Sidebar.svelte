<script lang="ts">
  // The source list (Sidebar board): Library kinds, then the Kindle with
  // Files and Transfers while it is connected.
  import { app, type Place } from '../lib/state.svelte';
  import { SidebarRow, Icon } from '../lib/ui';
  import { bytes } from '../lib/format';

  const d = $derived(app.device);
  const sync = $derived(app.runningJobs.find((j) => j.kind === 'sync'));
  const pushing = $derived(Object.values(app.transfers).filter((t) => t.state === 'running' || t.state === 'queued'));
  const live = $derived(d.connected && !d.busy);
  const clips = $derived(app.clippingFiles.length);
  const sub = $derived(
    d.busy ? 'Held by another process'
    : !d.connected ? 'Not connected'
    : sync ? `Syncing${sync.total ? ` · ${Math.round((sync.done / sync.total) * 100)}%` : '…'}`
    : pushing.length ? `Sending ${pushing.length} item${pushing.length === 1 ? '' : 's'}`
    : `Connected${d.free_bytes != null ? ` · ${bytes(d.free_bytes)} free` : ''}`,
  );
  const lib: { place: 'all' | 'notebook' | 'book' | 'document' | 'screenshot'; icon: 'all' | 'notebook' | 'book' | 'document' | 'screenshot'; label: string }[] = [
    { place: 'all', icon: 'all', label: 'All Items' },
    { place: 'notebook', icon: 'notebook', label: 'Notebooks' },
    { place: 'book', icon: 'book', label: 'Books' },
    { place: 'document', icon: 'document', label: 'Documents' },
    { place: 'screenshot', icon: 'screenshot', label: 'Screenshots' },
  ];
  const here = (p: Place) => !app.openId && app.place === p;
</script>

<nav class="sidebar" aria-label="Sidebar">
  <div class="drag" data-tauri-drag-region></div>
  <div class="group">
    <p class="heading">Library</p>
    {#each lib as r (r.place)}
      <SidebarRow icon={r.icon} label={r.label} count={app.counts[r.place] || ''} selected={here(r.place)} onclick={() => app.go(r.place)} />
    {/each}
    <SidebarRow icon="clippings" label="My Clippings" dimmed={!clips} selected={here('clippings')} onclick={() => app.go('clippings')} />
    <SidebarRow icon="search" label="Search in Books" selected={here('search')} onclick={() => app.go('search')} />
  </div>

  <div class="group">
    <p class="heading">Kindle</p>
    <SidebarRow
      icon="kindle"
      label={d.name ?? 'Kindle Scribe'}
      {sub}
      tone={d.busy ? 'warning' : 'normal'}
      dimmed={!d.connected && !d.busy}
      selected={here('device')}
      onclick={() => app.go('device')}
    >
      {#snippet trailing()}
        {#if sync}
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" role="img" aria-label="Sync {sync.total ? Math.round((sync.done / sync.total) * 100) : 0} percent">
            <circle cx="8" cy="8" r="6" stroke="var(--fo-fill-2)" stroke-width="2" />
            <circle cx="8" cy="8" r="6" stroke="var(--fo-accent)" stroke-width="2" stroke-linecap="round" stroke-dasharray="{sync.total ? (sync.done / sync.total) * 37.7 : 4} 37.7" transform="rotate(-90 8 8)" />
          </svg>
        {:else if d.busy}
          <Icon name="warning" color="var(--fo-warning)" label="Needs attention" />
        {/if}
      {/snippet}
    </SidebarRow>
    {#if live}
      <SidebarRow icon="folder" label="Files" indent selected={here('files')} onclick={() => app.go('files')} />
      <SidebarRow icon="transfers" label="Transfers" indent selected={here('transfers')} onclick={() => app.go('transfers')}>
        {#snippet trailing()}
          {#if pushing.length}<span class="badge">{pushing.length}</span>{/if}
        {/snippet}
      </SidebarRow>
    {/if}
  </div>
  <div class="grow"></div>
  <div class="foot">
    <SidebarRow icon="gear" label="Settings" onclick={() => (app.settingsOpen = true)} />
  </div>
</nav>

<style>
  .sidebar {
    width: var(--fo-sidebar-width);
    flex: none;
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--fo-bg-sidebar);
    border-right: 1px solid var(--fo-separator);
    overflow-y: auto;
  }
  .drag { height: var(--fo-space-6); flex: none; }
  .group { display: flex; flex-direction: column; gap: 1px; padding: 4px 10px 10px; }
  .heading { padding: 6px 8px 4px; font: var(--fo-text-caption); font-weight: 600; color: var(--fo-label-2); }
  .grow { flex: 1; }
  .foot { padding: 6px 10px 10px; }
  .badge { min-width: 18px; height: 16px; padding: 0 5px; border-radius: 8px; background: var(--fo-fill-2); font: 600 11px/16px var(--fo-font); color: var(--fo-label); text-align: center; }
</style>
