<script lang="ts">
  // A screenshot full size: ← → move between them, Esc closes.
  import { app } from '../lib/state.svelte';
  import { asset, call, isTauri, saveAs } from '../lib/api';
  import { bytes, dateTime } from '../lib/format';
  import { Button, ToolbarButton } from '../lib/ui';
  import { deleteFromKindle } from '../lib/actions';

  const shots = $derived(app.items.screenshot.slice().sort((a, b) => b.modified - a.modified));
  const i = $derived(shots.findIndex((s) => s.id === app.shotId));
  const cur = $derived(shots[i]);

  function key(e: KeyboardEvent) {
    if (e.key === 'ArrowLeft' && i > 0) app.shotId = shots[i - 1].id;
    else if (e.key === 'ArrowRight' && i < shots.length - 1) app.shotId = shots[i + 1].id;
    else if (e.key === 'Escape') app.shotId = null;
    else return;
    e.preventDefault();
  }
  async function save() {
    if (!cur) return;
    if (!isTauri) {
      const a = document.createElement('a');
      a.href = asset(`file/${cur.id}`);
      a.download = cur.title;
      a.click();
      return;
    }
    const dest = await saveAs(cur.title);
    if (dest) await app.run('Save', () => call('copy_file', { id: cur.id, dest }));
  }
</script>

<svelte:window onkeydown={key} />

{#if cur}
  <div class="viewer" role="dialog" aria-modal="true" aria-label={cur.title}>
    <div class="bar">
      <ToolbarButton icon="close" label="Close (Esc)" onclick={() => (app.shotId = null)} />
      <span class="name">{dateTime(cur.modified)} · {bytes(cur.size)}</span>
      <Button onclick={save}>Save As…</Button>
      {#if cur.device_path}<Button variant="danger" onclick={() => deleteFromKindle(cur)}>Delete from Kindle…</Button>{/if}
    </div>
    <div class="stage">
      <ToolbarButton icon="back" label="Previous" disabled={i <= 0} onclick={() => (app.shotId = shots[i - 1].id)} />
      <img src={asset(`file/${cur.id}`)} alt={cur.title} />
      <ToolbarButton icon="forward" label="Next" disabled={i >= shots.length - 1} onclick={() => (app.shotId = shots[i + 1].id)} />
    </div>
  </div>
{/if}

<style>
  .viewer { position: fixed; inset: 0; z-index: 85; background: var(--fo-bg-canvas); display: flex; flex-direction: column; }
  .bar { height: var(--fo-toolbar-height); display: flex; align-items: center; gap: 8px; padding: 0 12px; border-bottom: 1px solid var(--fo-separator); background: var(--fo-bg-window); }
  .name { flex: 1; font: var(--fo-text-body); color: var(--fo-label-2); }
  .stage { flex: 1; min-height: 0; display: flex; align-items: center; justify-content: center; gap: 12px; padding: 20px; }
  img { max-width: calc(100% - 100px); max-height: 100%; object-fit: contain; background: var(--fo-paper); box-shadow: var(--fo-shadow-window); border-radius: 2px; }
</style>
