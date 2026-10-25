<script lang="ts">
  // Plain-text books (My Clippings has its own screen).
  import { app } from '../lib/state.svelte';
  import { asset } from '../lib/api';
  import { bytes } from '../lib/format';
  import type { Item } from '../lib/types';
  import { EmptyState } from '../lib/ui';
  import Toolbar from '../shell/Toolbar.svelte';

  let { item }: { item: Item } = $props();
  let text = $state<string | null>(null);
  let error = $state<string | null>(null);
  let size = $state(100);

  $effect(() => {
    let live = true;
    fetch(asset(`file/${item.id}`))
      .then((r) => (r.ok ? r.text() : Promise.reject(new Error(`HTTP ${r.status}`))))
      .then((t) => live && (text = t.replace(/^﻿/, '').replace(/\r\n?/g, '\n')))
      .catch((e) => live && (error = String(e?.message ?? e)));
    return () => (live = false);
  });
</script>

<div class="reader">
  <Toolbar title={item.title} subtitle={`Text · ${bytes(item.size)}`} back={() => app.close()}>
    <div class="zoom" role="group" aria-label="Text size">
      <button type="button" aria-label="Smaller text" onclick={() => (size = Math.max(70, size - 10))}>A</button>
      <span>{size}%</span>
      <button type="button" aria-label="Larger text" class="big" onclick={() => (size = Math.min(200, size + 10))}>A</button>
    </div>
  </Toolbar>
  <div class="scroll" style:font-size="{size}%">
    {#if error}
      <EmptyState icon="warning" title="This file could not be opened" body={error} />
    {:else if text === null}
      <span class="fo-spinner"></span>
    {:else}
      <article>{text}</article>
    {/if}
  </div>
</div>

<style>
  .reader { display: flex; flex-direction: column; height: 100%; min-width: 0; }
  .zoom { display: flex; align-items: center; height: 26px; border-radius: var(--fo-radius-field); background: var(--fo-fill); padding: 0 2px; }
  .zoom button { height: 22px; min-width: 26px; border: 0; border-radius: 5px; background: transparent; color: var(--fo-label); font: 400 12px/1 var(--fo-font-reading); }
  .zoom button.big { font-size: 17px; }
  .zoom button:hover { background: var(--fo-fill-2); }
  .zoom span { font: var(--fo-text-caption); color: var(--fo-label-2); min-width: 40px; text-align: center; }
  .scroll { flex: 1; overflow-y: auto; padding: 40px 24px 64px; display: flex; justify-content: center; }
  article { max-width: 680px; width: 100%; white-space: pre-wrap; font: var(--fo-text-reading); font-size: 1.1em; color: var(--fo-label); }
</style>
