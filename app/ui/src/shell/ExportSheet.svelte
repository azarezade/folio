<script lang="ts">
  // Export notebooks to PDF (Reader board, export sheet): format, resolution,
  // pages, what to include, where to save. Defaults come from Settings.
  import { app } from '../lib/state.svelte';
  import { call, chooseFolder, isTauri } from '../lib/api';
  import { Button, Radio, PopUp, Checkbox, Icon } from '../lib/ui';
  import Sheet from './Sheet.svelte';

  const ids = app.exportFor!;
  const single = ids.length === 1;
  const item = single ? app.items.notebook.find((n) => n.id === ids[0]) : null;
  const pages = item?.page_count ?? null;
  let mode = $state<'vector' | 'raster'>(app.settings.export_mode === 'raster' ? 'raster' : 'vector');
  let longEdge = $state(app.settings.export_long_edge || '2480');
  let range = $state<'all' | 'current' | 'custom'>('all');
  let from = $state('1');
  let to = $state(String(pages ?? 1));
  let dest = $state(app.settings.export_dir || app.exportDirDefault);
  let force = $state(false);
  let busy = $state(false);
  const sizes = [1240, 1860, 2480, 3720, 4960].map((v) => ({ value: String(v), label: `${Math.round((v / 2480) * 300)} dpi` }));
  const folderName = $derived(dest.split('/').filter(Boolean).pop() ?? dest);

  async function pick() {
    const d = await chooseFolder(dest);
    if (d) dest = d;
  }
  async function go() {
    busy = true;
    const p = !single ? undefined : range === 'current' ? String(app.readerPage + 1) : range === 'custom' ? `${from}-${to}` : undefined;
    const r = await app.run('Export', () => call<number[]>('export', { ids, mode, long_edge: Number(longEdge), pages: p, dest_dir: dest, force }));
    busy = false;
    if (r) {
      if (single && r[0] != null) app.exportJob = r[0];
      else app.toast(`Exporting ${ids.length} notebooks — follow them in Activity`, 'info');
      app.exportFor = null;
      app.picked = new Set();
    }
  }
</script>

<Sheet label="Export notebook" width={540} onclose={() => (app.exportFor = null)}>
  <h2>Export {single ? `“${item?.title ?? 'notebook'}”` : `${ids.length} notebooks`}</h2>
  <div class="form">
    <span class="l">Format:</span>
    <Radio label="Format" options={[{ value: 'vector', label: 'Vector PDF — sharp paths, pencil as solid grey' }, { value: 'raster', label: 'Raster PDF — pencil texture, as images' }]} bind:value={mode} />
    <span class="l">Resolution:</span>
    <div class="row">
      <PopUp label="Resolution" options={sizes} bind:value={longEdge} disabled={mode === 'vector'} />
      <span class="hint">{mode === 'vector' ? 'Raster only — vectors stay sharp at any size' : `${longEdge} px long edge`}</span>
    </div>
    {#if single}
      <span class="l">Pages:</span>
      <div class="col">
        <Radio label="Pages" options={[{ value: 'all', label: pages ? `All ${pages} pages` : 'All pages' }, { value: 'current', label: `Current page (${app.readerPage + 1})` }, { value: 'custom', label: 'From' }]} bind:value={range} />
        <div class="range">
          <input aria-label="First page" inputmode="numeric" bind:value={from} onfocus={() => (range = 'custom')} />
          <span>to</span>
          <input aria-label="Last page" inputmode="numeric" bind:value={to} onfocus={() => (range = 'custom')} />
        </div>
      </div>
    {/if}
    <span class="l">Include:</span>
    <div class="col">
      <Checkbox checked={app.settings.draw_shapes !== 'false'} onchange={(v) => app.setSetting('draw_shapes', String(v))}>Convert-to-shape lines</Checkbox>
      <Checkbox bind:checked={force}>Export again even if the PDF is up to date</Checkbox>
    </div>
    <span class="l">Save to:</span>
    <div class="row">
      {#if isTauri}
        <button type="button" class="folder" onclick={pick}><Icon name="folder" color="var(--fo-accent)" />{folderName} <span aria-hidden="true">⌃⌄</span></button>
      {:else}
        <input class="path" aria-label="Folder" bind:value={dest} />
      {/if}
    </div>
  </div>
  <div class="foot">
    <span class="hint">Defaults come from Settings › Rendering</span>
    <Button onclick={() => (app.exportFor = null)}>Cancel</Button>
    <Button variant="primary" disabled={busy || (range === 'custom' && !(Number(from) >= 1 && Number(to) >= Number(from)))} onclick={go}>Export</Button>
  </div>
</Sheet>

<style>
  .form { display: grid; grid-template-columns: 90px 1fr; gap: 14px 12px; align-items: start; }
  .l { font: var(--fo-text-body); color: var(--fo-label); text-align: right; padding-top: 1px; }
  .row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .col { display: flex; flex-direction: column; gap: 8px; }
  .hint { font: var(--fo-text-caption); color: var(--fo-label-2); }
  .range { display: flex; align-items: center; gap: 6px; padding-left: 22px; font: var(--fo-text-body); }
  .range input, .path { width: 52px; height: 22px; border: 1px solid var(--fo-control-border); border-radius: 5px; background: var(--fo-control); color: var(--fo-label); font: var(--fo-text-body); padding: 0 6px; }
  .path { width: 100%; }
  .folder { height: 22px; display: inline-flex; align-items: center; gap: 6px; padding: 0 8px; border: 1px solid var(--fo-control-border); border-radius: var(--fo-radius-button); background: var(--fo-control); color: var(--fo-label); font: var(--fo-text-callout); }
  .folder span { color: var(--fo-label-2); font-size: 10px; }
  .foot { display: flex; align-items: center; gap: 8px; padding-top: 6px; }
  .foot .hint { flex: 1; }
</style>
