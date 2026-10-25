<script lang="ts">
  // Component gallery for reviewing the Folio set (open the UI with ?gallery).
  import './lib/styles/tokens.css';
  import {
    Banner, Button, ConfirmSheet, EmptyState, ItemCard, JobRow, Progress, SearchField, Segmented, SidebarRow, StatusChip, Switch, Toast, ToolbarButton,
  } from './lib/ui';

  let theme = $state<'light' | 'dark'>(matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
  $effect(() => {
    document.documentElement.dataset.theme = theme;
  });

  let layout = $state<'page' | 'book' | 'zen'>('page');
  let query = $state('');
  let scope = $state<'titles' | 'text'>('titles');
  let on = $state(true);
  let off = $state(false);
  let selected = $state('all');
  let sheet = $state(false);
  let toasts = $state([
    { id: 1, tone: 'success' as const, text: 'Exported 12 pages', action: 'Show' },
    { id: 2, tone: 'danger' as const, text: 'Kindle disconnected mid-transfer', action: 'Details' },
  ]);
</script>

<div class="page">
  <header>
    <div>
      <p class="eyebrow">Folio · components · {theme === 'dark' ? 'Dark' : 'Light'}</p>
      <h1>Components</h1>
      <p class="lede">Svelte components in <code>src/lib/ui</code>. Every colour comes from a <code>--fo-*</code> token.</p>
    </div>
    <Segmented label="Appearance" options={[{ value: 'light', label: 'Light' }, { value: 'dark', label: 'Dark' }]} bind:value={theme} />
  </header>

  <div class="grid">
    <div class="col">
      <section>
        <code>&lt;Button variant="primary | secondary | danger | plain"&gt;</code>
        <div class="row">
          <Button variant="primary">Export</Button>
          <Button>Cancel</Button>
          <Button variant="danger">Delete</Button>
          <Button variant="plain">Show Log</Button>
          <Button variant="primary" disabled>Disabled</Button>
        </div>
        <code>&lt;ToolbarButton icon label showLabel?&gt;</code>
        <div class="row tight">
          <ToolbarButton icon="sync" label="Sync" />
          <ToolbarButton icon="sync" label="Sync (active)" active />
          <ToolbarButton icon="export" label="Export…" showLabel />
          <ToolbarButton icon="search" label="Search" />
        </div>
      </section>

      <section>
        <code>&lt;Segmented options bind:value&gt;</code>
        <Segmented label="Layout" options={[{ value: 'page', label: 'Page' }, { value: 'book', label: 'Book' }, { value: 'zen', label: 'Zen' }]} bind:value={layout} />
        <code>&lt;SearchField bind:value scopes? bind:scope&gt;</code>
        <SearchField placeholder="Search titles" bind:value={query} scopes={[{ value: 'titles', label: 'Titles' }, { value: 'text', label: 'Text' }]} bind:scope />
        <code>&lt;Switch bind:checked label&gt;</code>
        <div class="row">
          <Switch label="On" bind:checked={on} />
          <Switch label="Off" bind:checked={off} />
          <Switch label="Disabled" checked disabled />
        </div>
      </section>

      <section>
        <code>&lt;StatusChip state&gt;</code>
        <div class="row tight">
          <StatusChip state="queued" /><StatusChip state="running" /><StatusChip state="done" /><StatusChip state="failed" /><StatusChip state="cancelled" />
        </div>
        <code>&lt;Progress value state size?&gt;</code>
        <div class="stack">
          <Progress value={0.58} label="Export" />
          <Progress value={0.4} state="failed" label="Failed" />
          <Progress value={1} state="done" label="Done" />
          <Progress value={null} label="Working" />
        </div>
      </section>
    </div>

    <div class="col">
      <section>
        <code>&lt;SidebarRow icon label count? sub? tone? selected&gt;</code>
        <div class="sidebar">
          <SidebarRow icon="all" label="All Items" count={348} selected={selected === 'all'} onclick={() => (selected = 'all')} />
          <SidebarRow icon="notebook" label="Notebooks" count={48} selected={selected === 'nb'} onclick={() => (selected = 'nb')} />
          <SidebarRow icon="clippings" label="My Clippings" count="1,204" selected={selected === 'cl'} onclick={() => (selected = 'cl')} />
          <SidebarRow icon="kindle" label="Kindle Scribe" sub="Held by another process" tone="warning" />
          <SidebarRow icon="kindle" label="Kindle Scribe" sub="Connected · 12.4 GB free" />
          <SidebarRow icon="transfers" label="Transfers" indent>
            {#snippet trailing()}<span class="badge">4</span>{/snippet}
          </SidebarRow>
        </div>
      </section>

      <section>
        <code>&lt;ItemCard kind cover? state? badge? selected&gt;</code>
        <div class="cards">
          <ItemCard kind="notebook" title="Meeting notes" subtitle="12 pages" selected>
            {#snippet preview()}
              <svg width="100%" height="100%" viewBox="0 0 120 160" fill="none" stroke="var(--fo-ink)" stroke-width="1.6" stroke-linecap="round" aria-hidden="true"><path d="M14 30c3-5 6 4 9 0s6-5 9 0 6 4 9 0 6-5 9 0M14 46c3-4 6 4 9 0s6-4 9 0 6 4 8 0M18 70h44v30H18zM68 85h28" /></svg>
            {/snippet}
          </ItemCard>
          <ItemCard kind="book" title="Middlemarch" subtitle="EPUB" />
          <ItemCard kind="book" title="The Odyssey" subtitle="AZW" state="failed" badge="Can't open" />
        </div>
      </section>

      <section>
        <code>&lt;JobRow job oncancel? onretry?&gt;</code>
        <div class="popover">
          <JobRow job={{ title: 'Syncing from Kindle Scribe', detail: '214 of 562 files · 2 min left', state: 'running', progress: 0.38, icon: 'sync' }} oncancel={() => {}} />
          <JobRow job={{ title: 'Sending “Block C.pdf”', detail: 'Failed — the Kindle was busy', state: 'failed', icon: 'upload' }} onretry={() => {}} />
        </div>
      </section>
    </div>

    <div class="col">
      <section>
        <code>&lt;Toast tone action? persistent? ondismiss?&gt;</code>
        <div class="stack">
          {#each toasts as t (t.id)}
            <Toast tone={t.tone} text={t.text} action={{ label: t.action, run: () => {} }} persistent ondismiss={() => (toasts = toasts.filter((x) => x.id !== t.id))} />
          {/each}
          <Toast tone="info" text="Indexing books for search" />
        </div>
      </section>

      <section>
        <code>&lt;Banner tone="warning | danger | info"&gt;</code>
        <Banner tone="warning" title="Kindle is held by another process.">
          Free it to sync.
          {#snippet actions()}<Button>Free the Kindle</Button>{/snippet}
        </Banner>
        <Banner tone="danger" title="Sync failed.">2 files couldn't be copied.</Banner>
        <Banner tone="info">Handwriting from the Kindle is shown on 3 pages.</Banner>
        <code>&lt;ConfirmSheet destructive&gt;</code>
        <div class="row"><Button variant="danger" onclick={() => (sheet = true)}>Clear Cache…</Button></div>
      </section>

      <section>
        <code>&lt;EmptyState icon title body actions?&gt;</code>
        <EmptyState icon="clippings" title="No clippings yet" body="Highlights you make on the Kindle show up after the next sync." />
        <EmptyState icon="kindle" title="Kindle not connected" body="Plug in your Kindle Scribe with a USB cable to sync.">
          {#snippet actions()}<Button variant="primary">Sync All</Button>{/snippet}
        </EmptyState>
      </section>
    </div>
  </div>
</div>

{#if sheet}
  <ConfirmSheet
    title="Clear the cache?"
    body="Removes 1.1 GB of rendered pages. Your library isn't touched; pages render again when opened."
    confirmLabel="Clear Cache"
    destructive
    oncancel={() => (sheet = false)}
    onconfirm={() => (sheet = false)}
  />
{/if}

<style>
  :global(body) { margin: 0; background: var(--fo-bg-group); }
  .page { box-sizing: border-box; min-height: 100vh; padding: 40px 48px; background: var(--fo-bg-group); color: var(--fo-label); font: var(--fo-text-body); display: flex; flex-direction: column; gap: 24px; }
  header { display: flex; justify-content: space-between; align-items: flex-end; gap: 16px; flex-wrap: wrap; }
  .eyebrow { margin: 0; font: var(--fo-text-headline); color: var(--fo-label-2); }
  h1 { margin: 6px 0 0; font: 700 34px/1.1 var(--fo-font); letter-spacing: -0.02em; }
  .lede { margin: 8px 0 0; font: 400 15px/1.4 var(--fo-font); color: var(--fo-label-2); }
  .lede code { font: 13px var(--fo-font-mono); }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(320px, 1fr)); gap: 20px; align-items: start; }
  .col { display: flex; flex-direction: column; gap: 20px; min-width: 0; }
  section { background: var(--fo-bg-window); border-radius: var(--fo-radius-window); box-shadow: 0 0 0 1px var(--fo-separator); padding: 16px; display: flex; flex-direction: column; gap: 12px; }
  section > code { font: 11.5px var(--fo-font-mono); color: var(--fo-label-2); }
  .row { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .tight { gap: 6px; }
  .stack { display: flex; flex-direction: column; gap: 8px; }
  .sidebar { display: flex; flex-direction: column; gap: 2px; padding: 8px; border-radius: 8px; background: var(--fo-bg-sidebar); }
  .badge { min-width: 18px; height: 16px; padding: 0 5px; box-sizing: border-box; border-radius: 8px; background: var(--fo-fill-2); font: 600 11px/16px var(--fo-font); color: var(--fo-label); text-align: center; }
  .cards { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; }
  .popover { border-radius: var(--fo-radius-group); background: var(--fo-bg-popover); box-shadow: var(--fo-shadow-popover); }
  @media (max-width: 520px) { .page { padding: 24px 16px; } }
</style>
