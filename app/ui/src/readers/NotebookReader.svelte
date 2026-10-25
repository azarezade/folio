<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { asset, call } from '../lib/api';
  import { plural } from '../lib/format';
  import type { Item, NotebookInfo } from '../lib/types';
  import { ToolbarButton, EmptyState } from '../lib/ui';
  import PagedReader from './PagedReader.svelte';

  let { item }: { item: Item } = $props();
  let info = $state<NotebookInfo | null>(null);
  let error = $state<string | null>(null);
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    call<NotebookInfo>('notebook', { id: item.id }).then(
      (i) => {
        info = i;
        app.readerPage = i.position ?? 0;
      },
      (e) => (error = String(e?.message ?? e)),
    );
  });

  function onPage(n: number) {
    app.readerPage = n;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => call('set_position', { id: item.id, page: n }), 400);
  }
</script>

{#if error}
  <div class="center"><EmptyState icon="warning" title="This notebook could not be opened" body={error} /></div>
{:else if !info}
  <div class="center"><span class="fo-spinner"></span></div>
{:else if info.pages.length === 0}
  <div class="center"><EmptyState icon="notebook" title={item.title} body="This notebook has no pages." /></div>
{:else}
  <PagedReader
    title={item.title}
    subtitle={`Notebook · ${plural(info.pages.length, 'page')}`}
    count={info.pages.length}
    aspect={(i) => info!.pages[i][0] / info!.pages[i][1]}
    src={(i, w) => asset(`notebook/${item.id}/page/${i}?w=${w}`)}
    initialPage={info.position ?? 0}
    {onPage}
  >
    {#snippet tools()}
      <ToolbarButton icon="export" label="Export…" showLabel onclick={() => (app.exportFor = [item.id])} />
    {/snippet}
  </PagedReader>
{/if}

<style>
  .center { flex: 1; height: 100%; display: flex; align-items: center; justify-content: center; }
</style>
