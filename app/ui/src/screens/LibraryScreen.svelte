<script lang="ts">
  // Library (Library board): items of the chosen kind as a grid of covers in
  // date sections, or as a list. Click selects, double-click or Enter opens,
  // ⌘-click picks notebooks for a batch export. First launch (FirstLaunch
  // board) shows how to fill the library.
  import { app, type Place } from '../lib/state.svelte';
  import { asset } from '../lib/api';
  import { bytes, dateBucket, plural, shortDate } from '../lib/format';
  import { openMenu } from '../lib/actions';
  import type { Item } from '../lib/types';
  import { ItemCard, Segmented, PopUp, ToolbarButton, SearchField, EmptyState, Button, Banner, Checkbox, Icon } from '../lib/ui';
  import Toolbar from '../shell/Toolbar.svelte';
  import ActivityButton from '../shell/ActivityButton.svelte';

  const titles: Record<string, string> = { all: 'All Items', notebook: 'Notebooks', book: 'Books', document: 'Documents', screenshot: 'Screenshots' };
  const nouns: Record<string, [string, string]> = { all: ['item', 'items'], notebook: ['notebook', 'notebooks'], book: ['book', 'books'], document: ['document', 'documents'], screenshot: ['screenshot', 'screenshots'] };

  const list = $derived(app.visible);
  const total = $derived(app.place === 'all' ? app.counts.all : app.counts[app.place as 'notebook' | 'book' | 'document' | 'screenshot']);
  const libraryEmpty = $derived(app.counts.all + app.counts.screenshot === 0);
  const syncing = $derived(app.runningJobs.some((j) => j.kind === 'sync'));
  const sections = $derived.by(() => {
    if (app.sort !== 'added') return [{ name: '', items: list }];
    const out: { name: string; items: Item[] }[] = [];
    for (const it of list) {
      const name = dateBucket(it.modified);
      if (out.at(-1)?.name !== name) out.push({ name, items: [] });
      out.at(-1)!.items.push(it);
    }
    return out;
  });
  let scope = $state<'titles' | 'text'>('titles');

  function kindOf(it: Item): 'notebook' | 'book' | 'pdf' {
    return it.kind === 'notebook' ? 'notebook' : it.kind === 'screenshot' || it.format === 'PDF' ? 'pdf' : 'book';
  }
  function cover(it: Item) {
    if (it.kind === 'notebook' || it.kind === 'screenshot') return asset(`thumb/${it.id}`);
    return it.has_cover ? asset(`thumb/${it.id}`) : null;
  }
  function sub(it: Item) {
    if (it.blocked) return "Can't open";
    if (it.kind === 'notebook') return `Notebook · ${it.page_count ? plural(it.page_count, 'page') : bytes(it.size)}`;
    if (it.kind === 'screenshot') return shortDate(it.modified);
    if (it.format === 'PDF') return `PDF · ${bytes(it.size)}`;
    return it.authors.length ? it.authors.join(', ') : `${it.format} · ${bytes(it.size)}`;
  }
  function click(e: MouseEvent, it: Item) {
    if (e.metaKey && it.kind === 'notebook') {
      const s = new Set(app.picked);
      s.has(it.id) ? s.delete(it.id) : s.add(it.id);
      app.picked = s;
      return;
    }
    app.selectedId = it.id;
    app.picked = new Set();
  }
  function key(e: KeyboardEvent) {
    if ((e.target as HTMLElement)?.closest('input, select, textarea')) return;
    const i = list.findIndex((x) => x.id === app.selectedId);
    if (e.key === 'Enter' && app.selected) app.open(app.selected);
    else if ((e.key === 'ArrowRight' || e.key === 'ArrowDown') && list.length) app.selectedId = list[Math.min(list.length - 1, i + 1)].id;
    else if ((e.key === 'ArrowLeft' || e.key === 'ArrowUp') && list.length) app.selectedId = list[Math.max(0, i - 1)].id;
    else return;
    e.preventDefault();
  }
  function search(q: string) {
    if (scope === 'text' && q.trim()) {
      app.searchQuery = q.trim();
      app.go('search');
    }
  }
  const setLayout = (v: 'grid' | 'list') => app.setSetting('library_view', v).catch(() => {});
  const setSort = (v: 'added' | 'title' | 'kind') => app.setSetting('library_sort', v).catch(() => {});
</script>

<svelte:window onkeydown={key} />

<div class="screen">
  <Toolbar title={titles[app.place] ?? 'Library'} subtitle={libraryEmpty ? 'No items yet' : app.filter ? `${list.length} of ${plural(total, ...nouns[app.place])}` : plural(total, ...nouns[app.place])}>
    {#if !libraryEmpty}
      <Segmented label="View" options={[{ value: 'grid', label: 'Grid', icon: 'grid' }, { value: 'list', label: 'List', icon: 'list' }]} bind:value={app.layout} onchange={setLayout} />
      <PopUp label="Sort by" plain options={[{ value: 'added', label: 'Date Added' }, { value: 'title', label: 'Title' }, { value: 'kind', label: 'Kind' }]} bind:value={app.sort} onchange={setSort} />
      <span class="toolbar-sep"></span>
    {/if}
    {#if app.picked.size > 1}
      <Button variant="primary" onclick={() => (app.exportFor = [...app.picked])}>Export {app.picked.size}…</Button>
    {/if}
    <ToolbarButton icon="sync" label={syncing ? 'Syncing…' : 'Sync Kindle now'} disabled={!app.device.connected || app.device.busy || syncing} onclick={() => app.sync()} />
    <ActivityButton />
    <div class="search">
      <SearchField placeholder={scope === 'text' ? 'Search inside books' : 'Search titles'} label="Search" bind:value={app.filter} scopes={[{ value: 'titles', label: 'Titles' }, { value: 'text', label: 'Full text' }]} bind:scope onsubmit={search} />
    </div>
  </Toolbar>

  <div class="body">
    {#if libraryEmpty}
      <div class="first">
        {#if app.device.connected && !app.device.busy}
          <EmptyState icon="kindle" title="Your library is empty" body="{app.device.name ?? 'Your Kindle'} is connected. Sync it to bring your notebooks, books, PDFs and clippings to this Mac. Later syncs only copy what changed.">
            {#snippet actions()}
              <Button variant="primary" disabled={syncing} onclick={() => app.sync()}>{syncing ? 'Syncing…' : `Sync ${app.device.name ?? 'Kindle'}`}</Button>
              <Button onclick={() => { app.settingsTab = 'general'; app.settingsOpen = true; }}>Library Folder…</Button>
            {/snippet}
          </EmptyState>
          <div class="note">
            <Banner tone="info"><strong>The first sync copies everything.</strong> It runs in the background — keep using the app and watch it in Activity.</Banner>
            <Checkbox checked={app.settings.auto_sync === 'true'} onchange={(v) => app.setSetting('auto_sync', String(v))}>Sync automatically whenever the Kindle is plugged in</Checkbox>
          </div>
        {:else if app.device.busy}
          <EmptyState icon="warning" title="Another process is holding the Kindle" body="macOS's photo service claimed the USB connection. Free it to sync.">
            {#snippet actions()}<Button variant="primary" onclick={() => app.go('device')}>Show Kindle</Button>{/snippet}
          </EmptyState>
        {:else}
          <EmptyState icon="kindle" title="Connect your Kindle" body="This app reads your notebooks, books and clippings straight from a Kindle Scribe over USB. Nothing is uploaded anywhere.">
            {#snippet actions()}
              <Button onclick={() => { app.settingsTab = 'general'; app.settingsOpen = true; }}>Library Folder…</Button>
            {/snippet}
          </EmptyState>
          <ol class="steps">
            <li><span>1</span>Plug the Kindle into this Mac with a USB cable.</li>
            <li><span>2</span>Wake and unlock it.</li>
            <li><span>3</span>It's found on its own — this page updates.</li>
          </ol>
          <p class="looking"><span class="fo-spinner small"></span>Looking for a Kindle…</p>
        {/if}
      </div>
    {:else if !list.length}
      <div class="first">
        <EmptyState icon="search" title={app.filter ? 'No matches' : `No ${nouns[app.place][1]} yet`} body={app.filter ? `Nothing here matches “${app.filter}”.` : 'They appear here after the next sync.'}>
          {#snippet actions()}
            {#if app.filter}
              <Button onclick={() => (app.filter = '')}>Clear Search</Button>
              <Button onclick={() => { app.searchQuery = app.filter; app.go('search'); }}>Search Inside Books</Button>
            {/if}
          {/snippet}
        </EmptyState>
      </div>
    {:else if app.layout === 'grid'}
      {#each sections as sec (sec.name)}
        {#if sec.name}
          <div class="section"><h2>{sec.name}</h2><span>{plural(sec.items.length, 'item')}</span></div>
        {/if}
        <div class="grid">
          {#each sec.items as it (it.id)}
            <div class="cell" role="presentation" ondblclick={() => app.open(it)}>
              <ItemCard
                kind={kindOf(it)}
                title={it.title}
                subtitle={sub(it)}
                cover={cover(it)}
                state={it.blocked ? 'failed' : undefined}
                badge={it.blocked ? "Can't open" : app.picked.has(it.id) ? 'Picked' : undefined}
                selected={app.selectedId === it.id || app.picked.has(it.id)}
                onclick={(e) => click(e, it)}
                oncontextmenu={(e) => openMenu(e, it)}
              >
                {#snippet preview()}
                  {#if it.kind === 'book' && it.format === 'PDF' && !it.has_cover}
                    <span class="pdf"><Icon name="document" size={28} weight={1} color="var(--fo-label-3)" /><span>PDF</span></span>
                  {:else if cover(it)}
                    <img class="art" class:contain={it.kind !== 'book'} src={cover(it)} alt="" loading="lazy" />
                  {:else}
                    <span class="spine"><span class="t">{it.title}</span>{#if it.authors.length}<span class="a">{it.authors.join(', ')}</span>{/if}</span>
                  {/if}
                {/snippet}
              </ItemCard>
            </div>
          {/each}
        </div>
      {/each}
    {:else}
      <div class="table" role="table" aria-label={titles[app.place]}>
        <div class="row head" role="row">
          <span role="columnheader">Name</span><span role="columnheader">Kind</span><span role="columnheader">Size</span><span role="columnheader">Date</span>
        </div>
        {#each list as it (it.id)}
          <div
            class="row"
            class:sel={app.selectedId === it.id || app.picked.has(it.id)}
            role="row"
            tabindex="-1"
            onclick={(e) => click(e, it)}
            ondblclick={() => app.open(it)}
            onkeydown={(e) => e.key === 'Enter' && app.open(it)}
            oncontextmenu={(e) => openMenu(e, it)}
          >
            <span role="cell" class="name">
              <Icon name={it.kind === 'notebook' ? 'notebook' : it.kind === 'screenshot' ? 'screenshot' : it.format === 'PDF' ? 'document' : 'book'} color={app.selectedId === it.id ? 'currentColor' : 'var(--fo-accent)'} />
              <span>{it.title}</span>
              {#if it.blocked}<span class="cant">Can't open</span>{/if}
            </span>
            <span role="cell">{it.kind === 'notebook' ? `Notebook${it.page_count ? ` · ${plural(it.page_count, 'page')}` : ''}` : it.kind === 'screenshot' ? 'Screenshot' : it.format}</span>
            <span role="cell">{bytes(it.size)}</span>
            <span role="cell">{shortDate(it.modified)}</span>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .screen { display: flex; flex-direction: column; height: 100%; min-width: 0; }
  .search { width: 230px; }
  .body { flex: 1; overflow-y: auto; padding: 20px 28px 32px; display: flex; flex-direction: column; gap: 14px; }
  .section { display: flex; align-items: baseline; gap: 10px; border-bottom: 1px solid var(--fo-separator); padding-bottom: 8px; margin-top: 6px; }
  .section h2 { font: var(--fo-text-headline); }
  .section span { font: var(--fo-text-callout); color: var(--fo-label-2); }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(132px, 1fr)); gap: 20px; }
  .cell { min-width: 0; }
  .art { width: 100%; height: 100%; object-fit: cover; display: block; }
  .art.contain { object-fit: contain; }
  .pdf { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px; font: var(--fo-text-caption); color: var(--fo-label-3); }
  .spine { position: absolute; inset: 0; padding: 18px 14px 14px; display: flex; flex-direction: column; justify-content: space-between; color: var(--fo-cover-text); }
  .spine .t { font: 400 16px/1.15 var(--fo-font-reading); overflow: hidden; }
  .spine .a { font: 9px/1.2 var(--fo-font); letter-spacing: 0.08em; text-transform: uppercase; opacity: 0.85; }
  .first { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 18px; padding: 40px 0; }
  .note { display: flex; flex-direction: column; gap: 12px; max-width: 460px; }
  .steps { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 10px; }
  .steps li { display: flex; align-items: center; gap: 10px; font: var(--fo-text-body); }
  .steps span { width: 22px; height: 22px; border-radius: 50%; background: var(--fo-fill); display: flex; align-items: center; justify-content: center; font: var(--fo-text-headline); }
  .looking { display: flex; align-items: center; gap: 8px; font: var(--fo-text-callout); color: var(--fo-label-2); }
  .looking :global(.fo-spinner) { width: 14px; height: 14px; }
  .table { display: flex; flex-direction: column; }
  .row { display: grid; grid-template-columns: minmax(0, 3fr) minmax(0, 1.2fr) 90px 150px; gap: 12px; align-items: center; height: var(--fo-row-list); padding: 0 10px; border-radius: var(--fo-radius-button); font: var(--fo-text-body); }
  .row:nth-child(even):not(.sel) { background: var(--fo-fill); }
  .row.head { font: var(--fo-text-caption); font-weight: 600; color: var(--fo-label-2); background: transparent !important; border-bottom: 1px solid var(--fo-separator); border-radius: 0; }
  .row.sel { background: var(--fo-accent); color: var(--fo-on-accent); }
  .row span { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .row:not(.head) span:not(.name) { color: inherit; }
  .row:not(.sel):not(.head) span[role='cell']:not(.name) { color: var(--fo-label-2); }
  .name { display: flex; align-items: center; gap: 8px; }
  .cant { font: 600 10px/16px var(--fo-font); padding: 0 6px; border-radius: 8px; background: var(--fo-danger-solid); color: var(--fo-on-accent); flex: none; }
</style>
