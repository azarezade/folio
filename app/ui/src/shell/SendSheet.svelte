<script lang="ts">
  // Send files to the Kindle: pick files, choose the folder in documents/.
  import { app } from '../lib/state.svelte';
  import { chooseFiles, isTauri } from '../lib/api';
  import { Button, Icon } from '../lib/ui';
  import Sheet from './Sheet.svelte';

  const EXT = ['pdf', 'epub', 'mobi', 'txt', 'docx', 'rtf'];
  let files = $state<string[]>([]);
  let typed = $state('');
  let target = $state(app.pushTarget || 'documents');
  const bad = $derived(files.filter((f) => !EXT.includes(f.split('.').pop()?.toLowerCase() ?? '')));
  const ok = $derived(files.filter((f) => !bad.includes(f)));

  async function browse() {
    files = [...new Set([...files, ...(await chooseFiles(EXT))])];
  }
  function add(e: SubmitEvent) {
    e.preventDefault();
    if (typed.trim()) files = [...new Set([...files, typed.trim()])];
    typed = '';
  }
  async function send() {
    app.sendOpen = false;
    await app.push(ok, target);
    app.toast(`Sending ${ok.length} file${ok.length === 1 ? '' : 's'} to the Kindle`, 'info', { label: 'Show', run: () => app.go('transfers') });
  }
</script>

<Sheet label="Send to Kindle" width={520} onclose={() => (app.sendOpen = false)}>
  <h2>Send to Kindle</h2>
  <p class="hint">PDF, EPUB, MOBI, TXT, DOCX and RTF. A file with the same name already there is kept, and the new one gets “(2)”.</p>
  {#if isTauri}
    <button type="button" class="zone" onclick={browse}><Icon name="upload" color="var(--fo-label-2)" />Choose files, or drop them on the window</button>
  {:else}
    <form class="typed" onsubmit={add}>
      <input aria-label="File path" placeholder="/path/to/file.pdf" bind:value={typed} />
      <Button type="submit">Add</Button>
    </form>
  {/if}
  {#if files.length}
    <ul>
      {#each files as f (f)}
        <li class:bad={bad.includes(f)}><span>{f.split('/').pop()}</span>{#if bad.includes(f)}<em>not a format the Kindle reads</em>{/if}<button type="button" aria-label="Remove {f}" onclick={() => (files = files.filter((x) => x !== f))}>✕</button></li>
      {/each}
    </ul>
  {/if}
  <label class="to">Folder on the Kindle <input bind:value={target} /></label>
  <div class="buttons">
    <Button onclick={() => (app.sendOpen = false)}>Cancel</Button>
    <Button variant="primary" disabled={!ok.length} onclick={send}>Send {ok.length || ''}</Button>
  </div>
</Sheet>

<style>
  .hint { font: var(--fo-text-callout); color: var(--fo-label-2); }
  .zone { padding: 18px; display: flex; align-items: center; justify-content: center; gap: 8px; border: 1.5px dashed var(--fo-control-border); border-radius: var(--fo-radius-group); background: transparent; color: var(--fo-label-2); font: var(--fo-text-body); }
  .zone:hover { background: var(--fo-fill); }
  .typed { display: flex; gap: 8px; }
  input { flex: 1; height: 24px; border: 1px solid var(--fo-control-border); border-radius: var(--fo-radius-field); background: var(--fo-control); color: var(--fo-label); font: var(--fo-text-body); padding: 0 8px; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; max-height: 200px; overflow-y: auto; }
  li { display: flex; align-items: center; gap: 8px; padding: 4px 8px; border-radius: 6px; background: var(--fo-fill); font: var(--fo-text-callout); }
  li span { flex: 1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  li.bad { color: var(--fo-danger); }
  li em { font-style: normal; font: var(--fo-text-caption); }
  li button { border: 0; background: none; color: var(--fo-label-2); }
  .to { display: flex; align-items: center; gap: 10px; font: var(--fo-text-body); }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; }
</style>
