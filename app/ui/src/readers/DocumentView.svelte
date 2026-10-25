<script lang="ts">
  // The open document: the right reader for its kind, or why it can't open.
  import type { Item } from '../lib/types';
  import NotebookReader from './NotebookReader.svelte';
  import PdfReader from './PdfReader.svelte';
  import EpubReader from './EpubReader.svelte';
  import TextReader from './TextReader.svelte';
  import CantOpen from './CantOpen.svelte';

  let { item }: { item: Item } = $props();
</script>

{#key item.id + item.content_hash}
  {#if item.kind === 'notebook'}
    <NotebookReader {item} />
  {:else if item.blocked}
    <CantOpen {item} />
  {:else if item.format === 'PDF'}
    <PdfReader {item} />
  {:else if item.format === 'Text'}
    <TextReader {item} />
  {:else if ['EPUB', 'MOBI', 'AZW', 'AZW3'].includes(item.format)}
    <EpubReader {item} />
  {:else}
    <CantOpen {item} />
  {/if}
{/key}
