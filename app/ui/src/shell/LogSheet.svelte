<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { Button } from '../lib/ui';
  import Sheet from './Sheet.svelte';

  let box = $state<HTMLPreElement>();
  $effect(() => {
    void app.log.length;
    box?.scrollTo({ top: box.scrollHeight });
  });
  function copy() {
    navigator.clipboard?.writeText(app.log.map((l) => l.text).join('\n'));
  }
</script>

<Sheet label="Sync log" width={720} onclose={() => (app.logOpen = false)}>
  <h2>Sync log</h2>
  <pre bind:this={box}>{#each app.log as l}<span class={l.kind}>{l.text}</span>{'\n'}{:else}Nothing logged yet. Sync the Kindle to see what is copied.{/each}</pre>
  <div class="buttons">
    <Button onclick={copy}>Copy</Button>
    <Button onclick={() => (app.log = [])}>Clear</Button>
    <Button variant="primary" onclick={() => (app.logOpen = false)}>Done</Button>
  </div>
</Sheet>

<style>
  pre { margin: 0; height: 50vh; overflow: auto; padding: 10px 12px; border-radius: var(--fo-radius-group); background: var(--fo-fill); font: var(--fo-text-mono); color: var(--fo-label); white-space: pre-wrap; }
  .ok { color: var(--fo-success); }
  .err { color: var(--fo-danger); }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; }
</style>
