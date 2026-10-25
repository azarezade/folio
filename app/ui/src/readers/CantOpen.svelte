<script lang="ts">
  // A book that can't be opened (DocReader board, error): the cover, the
  // reason in plain words, the details, and what can still be done.
  import { app } from '../lib/state.svelte';
  import { asset } from '../lib/api';
  import { bytes, dateTime } from '../lib/format';
  import { revealItem, deleteFromKindle } from '../lib/actions';
  import type { Item } from '../lib/types';
  import { Button, Icon } from '../lib/ui';
  import Toolbar from '../shell/Toolbar.svelte';

  let { item }: { item: Item } = $props();
  const reason = $derived(item.blocked ?? `${item.format} files can't be opened here.`);
  const details = $derived(
    [
      `File      ${item.device_path ?? item.rel_path} · ${bytes(item.size)}`,
      `Format    ${item.format}${item.class ? ` · ${item.class}` : ''}`,
      `Reason    ${reason}`,
      `Synced    ${dateTime(item.modified)}`,
    ].join('\n'),
  );
</script>

<div class="screen">
  <Toolbar title={item.title} subtitle={`Book · ${item.format} · ${bytes(item.size)}`} back={() => app.close()} />
  <div class="body">
    <div class="card">
      {#if item.has_cover}
        <img class="cover" src={asset(`thumb/${item.id}`)} alt="" />
      {:else}
        <Icon name="book" size={48} weight={1} color="var(--fo-label-2)" />
      {/if}
      <h2>This book can't be opened</h2>
      <p>“{item.title}”: {reason} The file is still in your library{item.device_path ? ' and on the Kindle' : ''}.</p>
      <div class="details">
        <span class="dh"><Icon name="info" size={14} />Details</span>
        <pre>{details}</pre>
      </div>
      <div class="buttons">
        <Button onclick={() => revealItem(item)}>Show in Finder</Button>
        <Button onclick={() => navigator.clipboard?.writeText(details).then(() => app.toast('Copied the details', 'ok'))}>Copy Details</Button>
      </div>
      {#if item.device_path}
        <Button variant="plain" disabled={!app.device.connected || app.device.busy} onclick={() => deleteFromKindle(item)}>Delete from Kindle…</Button>
      {/if}
    </div>
  </div>
</div>

<style>
  .screen { display: flex; flex-direction: column; height: 100%; min-width: 0; }
  .body { flex: 1; overflow-y: auto; display: flex; align-items: center; justify-content: center; padding: 32px; background: var(--fo-bg-group); }
  .card { max-width: 520px; display: flex; flex-direction: column; align-items: center; text-align: center; gap: 12px; }
  .cover { width: 120px; max-height: 180px; object-fit: contain; border-radius: 3px; box-shadow: var(--fo-shadow-window); }
  h2 { font: var(--fo-text-large-title); }
  p { font: var(--fo-text-body); line-height: 1.5; color: var(--fo-label-2); }
  .details { width: 100%; text-align: left; border-radius: var(--fo-radius-group); background: var(--fo-bg-window); box-shadow: 0 0 0 1px var(--fo-separator); padding: 10px 12px; display: flex; flex-direction: column; gap: 6px; }
  .dh { display: flex; align-items: center; gap: 6px; font: var(--fo-text-headline); color: var(--fo-label); }
  pre { margin: 0; font: var(--fo-text-mono); color: var(--fo-label-2); white-space: pre-wrap; overflow-wrap: anywhere; }
  .buttons { display: flex; gap: 8px; }
</style>
