<script lang="ts">
  // The window (Main board): sidebar and the chosen place, or an open
  // document over them; status bar, toasts, sheets and Settings on top.
  import { onMount } from 'svelte';
  import { app } from './lib/state.svelte';
  import { call, onFileDrop, onMenu } from './lib/api';
  import { EmptyState } from './lib/ui';
  import Sidebar from './shell/Sidebar.svelte';
  import StatusBar from './shell/StatusBar.svelte';
  import Toasts from './shell/Toasts.svelte';
  import Menu from './shell/Menu.svelte';
  import ConfirmHost from './shell/ConfirmHost.svelte';
  import ExportSheet from './shell/ExportSheet.svelte';
  import ExportProgress from './shell/ExportProgress.svelte';
  import SendSheet from './shell/SendSheet.svelte';
  import RenameSheet from './shell/RenameSheet.svelte';
  import LogSheet from './shell/LogSheet.svelte';
  import SettingsWindow from './shell/SettingsWindow.svelte';
  import ScreenshotViewer from './shell/ScreenshotViewer.svelte';
  import LibraryScreen from './screens/LibraryScreen.svelte';
  import ClippingsScreen from './screens/ClippingsScreen.svelte';
  import SearchScreen from './screens/SearchScreen.svelte';
  import DeviceScreen from './screens/DeviceScreen.svelte';
  import TransfersScreen from './screens/TransfersScreen.svelte';
  import DocumentView from './readers/DocumentView.svelte';

  let error = $state<string | null>(null);

  onMount(() => {
    app.start().catch((e) => (error = String(e?.message ?? e)));
    const offDrop = onFileDrop((paths) => {
      app.push(paths, app.pushTarget);
      app.toast(`Sending ${paths.length} file${paths.length === 1 ? '' : 's'} to the Kindle`, 'info', { label: 'Show', run: () => app.go('transfers') });
    });
    const offMenu = onMenu((id) => {
      switch (id) {
        case 'export':
          if (app.opened?.kind === 'notebook') app.exportFor = [app.opened.id];
          else if (app.selected?.kind === 'notebook') app.exportFor = [app.selected.id];
          break;
        case 'push': app.sendOpen = true; break;
        case 'sync_all': app.sync(); break;
        case 'free': app.go('device'); app.run('Free the Kindle', () => call('free_kindle')); break;
        case 'device': app.go('device'); break;
        case 'settings': app.settingsOpen = true; break;
        case 'search_books': app.go('search'); break;
        case 'book_mode': app.toggleBookMode(); break;
        case 'zen': if (app.openId) app.zen = !app.zen; break;
        default: window.dispatchEvent(new CustomEvent('folio-menu', { detail: id }));
      }
    });
    return () => {
      offDrop();
      offMenu();
    };
  });

  $effect(() => {
    const t = app.settings.theme;
    if (t === 'light' || t === 'dark') document.documentElement.dataset.theme = t;
    else delete document.documentElement.dataset.theme;
  });

  function key(e: KeyboardEvent) {
    if (e.metaKey && e.key === ',') {
      app.settingsOpen = true;
      e.preventDefault();
    } else if (e.key === 'Escape' && app.openId && !app.zen && !app.settingsOpen && !app.confirm && !app.exportFor && !(e.target as HTMLElement)?.closest('input, textarea')) {
      app.close();
    }
  }
</script>

<svelte:window onkeydown={key} />

<div class="window" class:zen={app.zen}>
  <div class="row">
    {#if !app.sidebarHidden && !app.openId && !app.zen}<Sidebar />{/if}
    <main>
      {#if error}
        <div class="center"><EmptyState icon="warning" title="Could not start" body={error} /></div>
      {:else if app.opened}
        <DocumentView item={app.opened} />
      {:else if app.place === 'clippings'}
        <ClippingsScreen />
      {:else if app.place === 'search'}
        <SearchScreen />
      {:else if app.place === 'device'}
        <DeviceScreen />
      {:else if app.place === 'files'}
        <DeviceScreen files />
      {:else if app.place === 'transfers'}
        <TransfersScreen />
      {:else}
        <LibraryScreen />
      {/if}
    </main>
  </div>
  {#if !app.zen}<StatusBar />{/if}
</div>

<Toasts />
{#if app.shotId}<ScreenshotViewer />{/if}
{#if app.menu}<Menu x={app.menu.x} y={app.menu.y} items={app.menu.items} onclose={() => (app.menu = null)} />{/if}
{#if app.settingsOpen}<SettingsWindow />{/if}
{#if app.exportFor}<ExportSheet />{/if}
{#if app.exportJob != null}<ExportProgress />{/if}
{#if app.sendOpen}<SendSheet />{/if}
{#if app.renameId}<RenameSheet />{/if}
{#if app.logOpen}<LogSheet />{/if}
{#if app.confirm}<ConfirmHost />{/if}

<style>
  .window { display: flex; flex-direction: column; height: 100vh; overflow: hidden; background: var(--fo-bg-window); }
  .row { flex: 1; display: flex; min-height: 0; }
  main { flex: 1; min-width: 0; display: flex; flex-direction: column; overflow: hidden; }
  .center { flex: 1; display: flex; align-items: center; justify-content: center; }
</style>
