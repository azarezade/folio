<script lang="ts">
  // Settings (Settings board): General · Rendering · Sync · About, as a
  // window over the library (⌘, from the menu; Esc closes).
  import { onMount } from 'svelte';
  import { app, type SettingsTab } from '../lib/state.svelte';
  import { call, chooseFolder, isTauri, openUrl } from '../lib/api';
  import { bytes, plural, tildify } from '../lib/format';
  import { Button, Segmented, PopUp, Switch, Progress, Icon } from '../lib/ui';
  import type { IconName } from '../lib/ui/Icon.svelte';
  import { setPtpcamerad } from './ptpcamerad';

  const SOURCE = 'https://github.com/azarezade/folio';
  const s = $derived(app.settings);
  const set = (k: string, v: string) => app.run('Setting', () => app.setSetting(k, v));
  const tabs: { id: SettingsTab; label: string; icon: IconName }[] = [
    { id: 'general', label: 'General', icon: 'gear' },
    { id: 'rendering', label: 'Rendering', icon: 'brush' },
    { id: 'sync', label: 'Sync', icon: 'sync' },
    { id: 'about', label: 'About', icon: 'info' },
  ];
  const titles: Record<SettingsTab, string> = { general: 'General', rendering: 'Rendering', sync: 'Sync', about: 'About Folio' };
  const cap = $derived(Number(s.cache_cap_mb || 2048) * 1024 * 1024);
  const cacheUsed = $derived(app.status?.cache_bytes ?? 0);
  const libSize = $derived(app.all.reduce((n, i) => n + i.size, 0));
  const looks = [
    { value: 'system', label: 'Same as macOS' },
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
  ];
  let win: HTMLElement;
  onMount(() => {
    win.focus();
    app.refreshStatus().catch(() => {});
  });

  function clearCache() {
    app.confirm = {
      title: 'Clear the cache?',
      body: `Removes ${bytes(cacheUsed)} of rendered pages. Your library isn't touched; pages render again when opened.`,
      confirm: 'Clear Cache',
      danger: true,
      run: async () => {
        const freed = await app.run('Clear cache', () => call<number>('clear_cache'));
        if (freed != null) app.toast(`Cleared ${bytes(freed)}`, 'ok');
        app.refreshStatus();
      },
    };
  }
  async function importOld() {
    const from = isTauri ? await chooseFolder() : prompt('Folder of the old library (…/scribe-library/library)');
    if (from) app.run('Import', () => call('migrate', { from }));
  }
  async function chooseExport() {
    const d = await chooseFolder(s.export_dir || app.exportDirDefault);
    if (d) set('export_dir', d);
  }
</script>

<div class="scrim" role="presentation" onclick={() => (app.settingsOpen = false)}></div>
<div class="window" role="dialog" aria-modal="true" aria-label="Settings" tabindex="-1" bind:this={win} onkeydown={(e) => e.key === 'Escape' && !app.confirm && (app.settingsOpen = false)}>
  <header>
    <button type="button" class="close" aria-label="Close settings" onclick={() => (app.settingsOpen = false)}></button>
    <h1>{titles[app.settingsTab]}</h1>
    <div class="tabs" role="tablist" aria-label="Settings">
      {#each tabs as t (t.id)}
        <button type="button" role="tab" aria-selected={app.settingsTab === t.id} onclick={() => (app.settingsTab = t.id)}>
          <Icon name={t.icon} size={20} weight={1.3} />
          {t.label}
        </button>
      {/each}
    </div>
  </header>

  <div class="content">
    {#if app.settingsTab === 'general'}
      <section>
        <h2>Library</h2>
        <div class="group">
          <div class="item">
            <Icon name="folder" size={28} weight={1.1} color="var(--fo-accent)" />
            <div class="grow"><p class="path">{app.status?.library_root ? tildify(app.status.library_root) : '…'}</p><p class="sub">{plural(app.all.length, 'item')} · {bytes(libSize)}</p></div>
            <Button onclick={() => call('reveal', { path: app.status?.library_root })}>Show in Finder</Button>
          </div>
          <div class="item">
            <div class="grow"><p>Import an old library</p><p class="sub">Copies notebooks, books and screenshots from the earlier Python app's folder. Files already here are skipped.</p></div>
            <Button onclick={importOld}>Import…</Button>
          </div>
        </div>
      </section>
      <section>
        <h2>Cache</h2>
        <div class="group">
          <div class="item"><div class="grow"><p>Location</p><p class="sub">{app.status?.cache_root ? tildify(app.status.cache_root) : '…'} — rendered pages and thumbnails</p></div></div>
          <div class="item col">
            <div class="line"><p>Using {bytes(cacheUsed)} of {bytes(cap)}</p><span class="sub">Oldest pages are removed first</span></div>
            <Progress value={cap ? cacheUsed / cap : 0} label="Cache usage" />
          </div>
          <div class="item">
            <p class="grow">Size limit</p>
            <PopUp label="Cache size limit" options={[['512', '512 MB'], ['1024', '1 GB'], ['2048', '2 GB'], ['5120', '5 GB'], ['10240', '10 GB']].map(([value, label]) => ({ value, label }))} value={s.cache_cap_mb || '2048'} onchange={(v) => set('cache_cap_mb', v)} />
            <Button variant="danger" onclick={clearCache}>Clear Cache…</Button>
          </div>
        </div>
      </section>
      <section>
        <h2>Appearance</h2>
        <div class="looks" role="radiogroup" aria-label="Appearance">
          {#each looks as l (l.value)}
            <button type="button" role="radio" aria-checked={(s.theme || 'system') === l.value} onclick={() => set('theme', l.value)}>
              <span class="swatch" class:auto={l.value === 'system'} class:dark={l.value === 'dark'}></span>
              {l.label}
            </button>
          {/each}
        </div>
      </section>
    {:else if app.settingsTab === 'rendering'}
      <section>
        <h2>In the reader</h2>
        <div class="group">
          <div class="item"><p class="grow">Pencil look</p><Segmented label="Pencil look" options={[{ value: 'texture', label: 'Texture' }, { value: 'gray', label: 'Solid grey' }]} value={s.reader_pencil === 'gray' ? 'gray' : 'texture'} onchange={(v) => set('reader_pencil', v)} /></div>
          <div class="item"><p class="grow">Texture fineness</p><Segmented label="Texture fineness" options={[{ value: '8', label: 'Coarse' }, { value: '4', label: 'Fine' }, { value: '2', label: 'Very fine' }]} value={s.pencil_scale || '4'} onchange={(v) => set('pencil_scale', v)} /></div>
          <div class="item">
            <div class="grow"><p>Draw “convert to shape” lines</p><p class="sub">Shows the clean lines, circles and boxes the Kindle made from your sketches</p></div>
            <Switch label="Draw convert to shape lines" checked={s.draw_shapes !== 'false'} onchange={(v) => set('draw_shapes', String(v))} />
          </div>
        </div>
      </section>
      <section>
        <h2>Export defaults</h2>
        <div class="group">
          <div class="item"><p class="grow">Format</p><Segmented label="Export format" options={[{ value: 'vector', label: 'Vector PDF' }, { value: 'raster', label: 'Raster PDF' }]} value={s.export_mode === 'raster' ? 'raster' : 'vector'} onchange={(v) => set('export_mode', v)} /></div>
          <div class="item">
            <div class="grow"><p>Resolution</p><p class="sub">For raster export</p></div>
            <PopUp label="Resolution" options={[1240, 1860, 2480, 3720, 4960].map((v) => ({ value: String(v), label: `${Math.round((v / 2480) * 300)} dpi` }))} value={s.export_long_edge || '2480'} onchange={(v) => set('export_long_edge', v)} />
          </div>
          <div class="item">
            <div class="grow"><p>Default folder</p><p class="sub path">{tildify(s.export_dir || app.exportDirDefault)}</p></div>
            {#if isTauri}<Button onclick={chooseExport}>Change…</Button>{/if}
          </div>
        </div>
      </section>
    {:else if app.settingsTab === 'sync'}
      <section>
        <h2>Syncing</h2>
        <div class="group">
          <div class="item">
            <div class="grow"><p>Sync automatically when the Kindle is plugged in</p><p class="sub">Unchanged files are skipped, so most syncs take seconds</p></div>
            <Switch label="Sync automatically" checked={s.auto_sync === 'true'} onchange={(v) => set('auto_sync', String(v))} />
          </div>
          <div class="item">
            <div class="grow"><p>Also copy the Kindle's sync database</p><p class="sub">About 1 GB, copied again whenever it changes, which makes syncs much slower. It holds no notebook titles.</p></div>
            <Switch label="Copy sync database" checked={s.sync_db === 'true'} onchange={(v) => set('sync_db', String(v))} />
          </div>
        </div>
      </section>
      <section>
        <h2>USB connection</h2>
        <div class="group">
          <div class="item">
            <div class="grow"><p>Stop macOS from grabbing the Kindle</p><p class="sub">macOS's photo service (Image Capture) sometimes claims the USB connection first. This tells it to ignore the Kindle.</p></div>
            <Switch label="Stop macOS from grabbing the Kindle" checked={s.ptpcamerad_disabled === 'true'} onchange={(v) => setPtpcamerad(v)} />
          </div>
          <div class="item">
            <div class="grow"><p>Log</p><p class="sub">What the last syncs copied, skipped and failed</p></div>
            <Button onclick={() => { app.settingsOpen = false; app.logOpen = true; }}>Show Log</Button>
          </div>
        </div>
      </section>
    {:else}
      <div class="about">
        <Icon name="notebook" size={64} weight={0.9} color="var(--fo-accent)" />
        <p class="name">Folio</p>
        <p class="sub">Version 0.1.0</p>
        <p class="lic">Folio is free software: you can redistribute it and modify it under the terms of the GNU General Public License, version 3 or later. It comes with no warranty.</p>
        <div class="buttons">
          <Button onclick={() => openUrl('https://www.gnu.org/licenses/gpl-3.0.html')}>View GPL-3.0 Licence</Button>
          <Button variant="plain" onclick={() => openUrl(SOURCE)}>Source Code ↗</Button>
        </div>
        <div class="ack">
          <p><strong>Acknowledgements</strong></p>
          <p class="sub">Notebook and book decoding ports John Howell's kfxlib. Thank you to its authors and contributors. Help › Licences lists every bundled component.</p>
        </div>
        <p class="sub small">Not affiliated with Amazon. Kindle is a trademark of Amazon.com, Inc.</p>
      </div>
    {/if}
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 88; background: var(--fo-scrim); }
  .window {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    z-index: 89;
    width: min(720px, calc(100vw - 32px));
    height: min(620px, calc(100vh - 32px));
    display: flex;
    flex-direction: column;
    border-radius: var(--fo-radius-window);
    background: var(--fo-bg-window);
    box-shadow: var(--fo-shadow-window);
    overflow: hidden;
    outline: none;
    animation: open var(--fo-dur-sheet) var(--fo-ease);
  }
  @keyframes open { from { opacity: 0; transform: translate(-50%, -48%) scale(0.98); } }
  header { position: relative; display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 10px 12px 8px; border-bottom: 1px solid var(--fo-separator); background: var(--fo-bg-sidebar); }
  .close { position: absolute; left: 14px; top: 13px; width: 12px; height: 12px; border-radius: 50%; border: 0; padding: 0; background: var(--fo-danger-solid); }
  h1 { font: var(--fo-text-headline); }
  .tabs { display: flex; gap: 2px; }
  .tabs button { width: 76px; height: 50px; border: 0; border-radius: var(--fo-radius-button); background: transparent; color: var(--fo-label-2); font: var(--fo-text-caption); display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 3px; }
  .tabs button:hover { background: var(--fo-fill); }
  .tabs button[aria-selected='true'] { background: var(--fo-selection); color: var(--fo-accent-text); }
  .content { flex: 1; overflow-y: auto; padding: 18px 28px 28px; display: flex; flex-direction: column; gap: 20px; background: var(--fo-bg-group); }
  h2 { font: var(--fo-text-headline); margin: 0 0 8px 4px; }
  .group { border-radius: var(--fo-radius-group); background: var(--fo-bg-window); box-shadow: 0 0 0 1px var(--fo-separator); }
  .item { min-height: var(--fo-row-settings); padding: 8px 14px; display: flex; align-items: center; gap: 12px; font: var(--fo-text-body); }
  .item + .item { border-top: 1px solid var(--fo-separator); }
  .item.col { flex-direction: column; align-items: stretch; gap: 8px; }
  .line { display: flex; justify-content: space-between; align-items: baseline; gap: 12px; }
  .grow { flex: 1; min-width: 0; }
  .sub { font: var(--fo-text-caption); color: var(--fo-label-2); }
  .path { font-family: var(--fo-font-mono); font-size: 11.5px; overflow-wrap: anywhere; }
  .looks { display: flex; gap: 16px; }
  .looks button { border: 0; background: transparent; display: flex; flex-direction: column; align-items: center; gap: 6px; font: var(--fo-text-callout); color: var(--fo-label); }
  /* fixed light and dark samples, whatever the current appearance */
  .swatch { width: 86px; height: 56px; border-radius: 8px; box-shadow: 0 0 0 1px var(--fo-separator); background: var(--fo-reading-paper-bg); }
  .swatch.dark { background: var(--fo-reading-night-bg); }
  .swatch.auto { background: linear-gradient(90deg, var(--fo-reading-paper-bg) 50%, var(--fo-reading-night-bg) 50%); }
  .looks button[aria-checked='true'] .swatch { box-shadow: 0 0 0 3px var(--fo-accent); }
  .about { display: flex; flex-direction: column; align-items: center; text-align: center; gap: 8px; padding-top: 12px; }
  .name { font: var(--fo-text-large-title); }
  .lic { max-width: 460px; font: var(--fo-text-body); line-height: 1.5; color: var(--fo-label); margin-top: 8px; }
  .buttons { display: flex; gap: 8px; }
  .ack { max-width: 460px; margin-top: 14px; padding: 12px 14px; border-radius: var(--fo-radius-group); background: var(--fo-bg-window); box-shadow: 0 0 0 1px var(--fo-separator); display: flex; flex-direction: column; gap: 4px; }
  .small { margin-top: 10px; }
</style>
