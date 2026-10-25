<script lang="ts">
  // A local title for a notebook (the Kindle's own titles can't be read over USB).
  import { app } from '../lib/state.svelte';
  import { call } from '../lib/api';
  import { Button } from '../lib/ui';
  import Sheet from './Sheet.svelte';

  const item = app.all.find((i) => i.id === app.renameId);
  let title = $state(item?.title ?? '');
  async function save(e?: SubmitEvent) {
    e?.preventDefault();
    await app.run('Rename', () => call('rename', { id: app.renameId, title: title.trim() || null }));
    app.renameId = null;
    await app.loadItems();
  }
</script>

<Sheet label="Rename notebook" width={420} onclose={() => (app.renameId = null)}>
  <h2>Rename notebook</h2>
  <form onsubmit={save}>
    <input aria-label="Title" bind:value={title} />
    <p>The name is kept in this library only. Leave it empty to go back to the original.</p>
    <div class="buttons">
      <Button onclick={() => (app.renameId = null)}>Cancel</Button>
      <Button variant="primary" type="submit">Rename</Button>
    </div>
  </form>
</Sheet>

<style>
  form { display: flex; flex-direction: column; gap: 10px; }
  input { height: 26px; border: 1px solid var(--fo-control-border); border-radius: var(--fo-radius-field); background: var(--fo-control); color: var(--fo-label); font: var(--fo-text-body); padding: 0 8px; }
  p { font: var(--fo-text-caption); color: var(--fo-label-2); }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; }
</style>
