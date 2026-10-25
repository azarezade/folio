<script lang="ts">
  // Files on the Kindle (Device board): a path bar, a table, delete and
  // copy-to-library for the selection, and sending files to this folder.
  import { app } from '../lib/state.svelte';
  import { call, chooseFiles, isTauri } from '../lib/api';
  import { bytes } from '../lib/format';
  import { Icon, ToolbarButton, Button, EmptyState } from '../lib/ui';

  interface Entry { name: string; size: number; folder: boolean }
  let path = $state('documents');
  let entries = $state<Entry[] | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let sel = $state<string | null>(null);

  const crumbs = $derived(path ? path.split('/') : []);
  const inDocs = $derived(/^documents(\/|$)/i.test(path));
  const deletable = $derived(inDocs || /^screenshots(\/|$)/i.test(path));
  const selected = $derived(entries?.find((e) => e.name === sel) ?? null);
  const join = (name: string) => (path ? `${path}/${name}` : name);

  async function load() {
    loading = true;
    error = null;
    try {
      entries = await call<Entry[]>('device_ls', { path });
      entries.sort((a, b) => Number(b.folder) - Number(a.folder) || a.name.localeCompare(b.name));
    } catch (e: any) {
      error = String(e?.message ?? e);
      entries = null;
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    void path;
    sel = null;
    app.pushTarget = inDocs ? path : 'documents';
    if (app.device.connected && !app.device.busy) load();
  });
  $effect(() => {
    const done = Object.values(app.jobs).filter((j) => (j.kind === 'push' || j.kind === 'sync') && j.state === 'done').length;
    if (done && app.device.connected && !app.device.busy) load();
  });

  function kind(e: Entry) {
    if (e.folder) return 'Folder';
    const ext = e.name.split('.').pop()?.toLowerCase() ?? '';
    return ({ pdf: 'PDF', epub: 'Book (EPUB)', mobi: 'Book (MOBI)', azw: 'Book (AZW)', azw3: 'Book (AZW3)', kfx: 'Book (KFX)', txt: 'Text', docx: 'Word', png: 'Image', jpg: 'Image' } as Record<string, string>)[ext] ?? (ext.toUpperCase() || 'File');
  }
  function remove(e: Entry) {
    app.confirm = {
      title: `Delete “${e.name}” from the Kindle?`,
      body: 'It will be removed from the Kindle right away and can’t be recovered there. If it was synced, its copy in this library is removed too.',
      confirm: 'Delete',
      danger: true,
      run: async () => {
        await app.run('Delete', () => call('device_delete', { path: join(e.name) }));
        await load();
      },
    };
  }
  async function mkdir() {
    const name = prompt('New folder name');
    if (!name?.trim()) return;
    await app.run('New folder', () => call('device_mkdir', { path: join(name.trim()) }));
    await load();
  }
  async function send() {
    const files = await chooseFiles(['pdf', 'epub', 'mobi', 'txt', 'docx', 'rtf']);
    if (files.length) app.push(files, path);
    else if (!isTauri) app.sendOpen = true;
  }
  function key(e: KeyboardEvent) {
    if (!entries?.length || (e.target as HTMLElement)?.closest('input')) return;
    const i = entries.findIndex((x) => x.name === sel);
    if (e.key === 'ArrowDown') sel = entries[Math.min(entries.length - 1, i + 1)].name;
    else if (e.key === 'ArrowUp') sel = entries[Math.max(0, i - 1)].name;
    else if (e.key === 'Enter' && selected?.folder) path = join(selected.name);
    else if ((e.key === 'Backspace' || e.key === 'Delete') && selected && !selected.folder && deletable) remove(selected);
    else return;
    e.preventDefault();
  }
</script>

<section class="files" aria-label="Files on the Kindle">
  <div class="bar">
    <ToolbarButton icon="back" label="Up one folder" disabled={!path} onclick={() => (path = crumbs.slice(0, -1).join('/'))} />
    <nav class="crumbs" aria-label="Path">
      <button type="button" onclick={() => (path = '')}>{app.device.name ?? 'Kindle'}</button>
      {#each crumbs as c, i}
        <span aria-hidden="true">›</span>
        {#if i === crumbs.length - 1}<strong>{c}</strong>{:else}<button type="button" onclick={() => (path = crumbs.slice(0, i + 1).join('/'))}>{c}</button>{/if}
      {/each}
    </nav>
    <span class="count">{entries ? (sel ? `1 of ${entries.length} selected` : `${entries.length} item${entries.length === 1 ? '' : 's'}`) : ''}</span>
    {#if inDocs}<ToolbarButton icon="folderPlus" label="New folder" onclick={mkdir} />{/if}
    <ToolbarButton icon="download" label="Copy to library" disabled={!selected || selected.folder} onclick={() => selected && app.run('Copy', () => call('device_pull', { path: join(selected.name) }))} />
    <ToolbarButton icon="trash" label="Delete from Kindle" disabled={!selected || selected.folder || !deletable} onclick={() => selected && remove(selected)} />
    <ToolbarButton icon="sync" label="Refresh" onclick={load} />
  </div>

  {#if error}
    <div class="msg"><EmptyState icon="warning" title="The Kindle didn’t answer" body={error}>{#snippet actions()}<Button onclick={load}>Try Again</Button>{/snippet}</EmptyState></div>
  {:else if loading && !entries}
    <div class="msg"><span class="fo-spinner"></span></div>
  {:else if entries}
    <div class="table" role="grid" aria-label={path || 'Kindle'} tabindex="0" onkeydown={key}>
      <div class="row head" role="row"><span role="columnheader">Name</span><span role="columnheader">Kind</span><span role="columnheader">Size</span></div>
      {#each entries as e (e.name)}
        <div
          class="row"
          class:sel={sel === e.name}
          role="row"
          tabindex="-1"
          aria-selected={sel === e.name}
          onclick={() => (sel = e.name)}
          ondblclick={() => e.folder && (path = join(e.name))}
          onkeydown={(k) => k.key === 'Enter' && e.folder && (path = join(e.name))}
        >
          <span role="gridcell" class="name"><Icon name={e.folder ? 'folder' : 'document'} color={sel === e.name ? 'currentColor' : e.folder ? 'var(--fo-accent)' : 'var(--fo-label-2)'} />{e.name}</span>
          <span role="gridcell">{kind(e)}</span>
          <span role="gridcell">{e.folder ? '—' : bytes(e.size)}</span>
        </div>
      {:else}
        <p class="none">This folder is empty.</p>
      {/each}
    </div>
    {#if inDocs}
      <button type="button" class="drop" onclick={send}>
        <Icon name="upload" color="var(--fo-label-2)" />
        {isTauri ? `Drop PDFs, EPUBs or documents on the window to send them here, or click to choose` : 'Click to send files to this folder'}
      </button>
    {/if}
  {/if}
</section>

<style>
  .files { display: flex; flex-direction: column; min-height: 0; flex: 1; border-radius: var(--fo-radius-window); background: var(--fo-bg-window); box-shadow: 0 0 0 1px var(--fo-separator); overflow: hidden; }
  .bar { display: flex; align-items: center; gap: 4px; padding: 6px 10px; border-bottom: 1px solid var(--fo-separator); }
  .crumbs { flex: 1; display: flex; align-items: center; gap: 6px; font: var(--fo-text-body); min-width: 0; overflow: hidden; white-space: nowrap; }
  .crumbs button { border: 0; background: none; padding: 2px 4px; border-radius: 4px; color: var(--fo-label-2); }
  .crumbs button:hover { background: var(--fo-fill); color: var(--fo-label); }
  .crumbs span { color: var(--fo-label-3); }
  .count { font: var(--fo-text-caption); color: var(--fo-label-2); margin-right: 6px; white-space: nowrap; }
  .msg { padding: 32px; display: flex; justify-content: center; }
  .table { flex: 1; overflow-y: auto; padding: 4px 6px; outline: none; }
  .row { display: grid; grid-template-columns: minmax(0, 3fr) minmax(0, 1fr) 90px; gap: 12px; align-items: center; height: var(--fo-row-list); padding: 0 10px; border-radius: var(--fo-radius-button); font: var(--fo-text-body); }
  .row:nth-child(even):not(.sel) { background: var(--fo-fill); }
  .row.head { font: var(--fo-text-caption); font-weight: 600; color: var(--fo-label-2); background: transparent !important; }
  .row.sel { background: var(--fo-accent); color: var(--fo-on-accent); }
  .row span { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .row:not(.sel):not(.head) span[role='gridcell']:not(.name) { color: var(--fo-label-2); }
  .name { display: flex; align-items: center; gap: 8px; }
  .none { padding: 16px 10px; font: var(--fo-text-callout); color: var(--fo-label-2); }
  .drop { margin: 8px 10px 10px; padding: 12px; display: flex; align-items: center; justify-content: center; gap: 8px; border: 1.5px dashed var(--fo-control-border); border-radius: var(--fo-radius-group); background: transparent; color: var(--fo-label-2); font: var(--fo-text-callout); }
  .drop:hover { background: var(--fo-fill); }
</style>
