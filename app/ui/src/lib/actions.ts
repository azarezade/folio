// What can be done to a library item: the context menu and its commands.
import { app, isClippingsFile } from './state.svelte';
import { call } from './api';
import type { Item } from './types';
import type { MenuItem } from '../shell/Menu.svelte';

export function revealItem(it: Item) {
  app.run('Show in Finder', () => call('reveal', { id: it.id }));
}

export function deleteFromKindle(it: Item) {
  app.confirm = {
    title: `Delete “${it.title}” from the Kindle?`,
    body: `It will be removed from the Kindle right away and can't be recovered there. Its copy in this library is removed too.`,
    confirm: 'Delete',
    danger: true,
    run: () =>
      app.run('Delete', async () => {
        await call('delete', { id: it.id });
        if (app.openId === it.id) app.close();
        await app.loadItems();
      }),
  };
}

export function itemMenu(it: Item): MenuItem[] {
  const items: MenuItem[] = [{ label: 'Open', run: () => app.open(it) }];
  if (it.kind === 'notebook') {
    items.push({ label: 'Rename…', run: () => (app.renameId = it.id) });
    items.push({ label: 'Export as PDF…', run: () => (app.exportFor = app.picked.has(it.id) && app.picked.size > 1 ? [...app.picked] : [it.id]) });
  }
  items.push({ label: 'Show in Finder', run: () => revealItem(it) });
  if (it.kind !== 'notebook' && it.device_path && !isClippingsFile(it)) {
    items.push({ separator: true });
    items.push({ label: 'Delete from Kindle…', danger: true, disabled: !app.device.connected || app.device.busy, run: () => deleteFromKindle(it) });
  }
  return items;
}

export function openMenu(e: MouseEvent, it: Item) {
  e.preventDefault();
  app.selectedId = it.id;
  app.menu = { x: e.clientX, y: e.clientY, items: itemMenu(it) };
}
