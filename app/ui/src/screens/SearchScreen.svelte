<script lang="ts">
  // Full-text search across the library's books (SQLite FTS5, filled in the
  // background). A hit opens the book at that place with the words found.
  import { app, isClippingsFile } from '../lib/state.svelte';
  import { call } from '../lib/api';
  import { plural } from '../lib/format';
  import type { Item, TextHit } from '../lib/types';
  import { SearchField, EmptyState, Progress, Icon } from '../lib/ui';
  import Toolbar from '../shell/Toolbar.svelte';
  import ActivityButton from '../shell/ActivityButton.svelte';

  let input = $state<HTMLInputElement>();
  let hits = $state<TextHit[]>([]);
  let indexed = $state<{ books: number; chunks: number } | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let seq = 0;

  const books = $derived(new Map(app.items.book.map((b) => [b.id, b])));
  const searchable = $derived(app.items.book.filter((b) => !b.blocked).length);
  const indexing = $derived(app.runningJobs.find((j) => j.kind === 'index'));
  const groups = $derived.by(() => {
    const m = new Map<string, { item: Item; hits: TextHit[] }>();
    for (const h of hits) {
      const item = books.get(h.id);
      if (!item) continue;
      if (!m.has(h.id)) m.set(h.id, { item, hits: [] });
      m.get(h.id)!.hits.push(h);
    }
    return [...m.values()];
  });

  async function run(q: string) {
    const n = ++seq;
    if (!q.trim()) {
      hits = [];
      busy = false;
      return;
    }
    busy = true;
    try {
      const r = await call<{ hits: TextHit[]; books_indexed: number; chunks: number }>('search_text', { query: q, limit: 200 });
      if (n !== seq) return;
      hits = r.hits;
      indexed = { books: r.books_indexed, chunks: r.chunks };
      error = null;
    } catch (e: any) {
      if (n === seq) error = String(e?.message ?? e);
    } finally {
      if (n === seq) busy = false;
    }
  }

  $effect(() => {
    const q = app.searchQuery;
    void indexing;
    clearTimeout(timer);
    timer = setTimeout(() => run(q), 180);
  });
  $effect(() => {
    input?.focus();
  });

  function parts(snippet: string) {
    return snippet.split(/(\u0002[^\u0003]*\u0003)/).map((p) => (p.startsWith('\u0002') ? { hit: true, text: p.slice(1, -1) } : { hit: false, text: p }));
  }
  function open(h: TextHit) {
    const item = books.get(h.id);
    if (!item) return;
    if (isClippingsFile(item)) {
      app.openAt = { id: h.id, loc: 'clippings', query: app.searchQuery.trim() };
      return app.go('clippings');
    }
    app.openAt = { id: h.id, loc: h.loc, query: app.searchQuery.trim() };
    app.open(item);
  }
</script>

<div class="screen">
  <Toolbar title="Search in Books" subtitle={indexing ? `Indexing ${indexing.done} of ${indexing.total} books…` : indexed ? `${indexed.books} of ${plural(searchable, 'readable book')} indexed` : 'EPUB, MOBI, AZW3, PDF and text'}>
    <ActivityButton />
    <div class="search"><SearchField bind:input placeholder="Words or a phrase" label="Search inside books" bind:value={app.searchQuery} /></div>
  </Toolbar>
  {#if indexing}<Progress value={indexing.total ? indexing.done / indexing.total : null} size="small" label="Indexing books" />{/if}

  <div class="results">
    {#if error}
      <div class="empty"><EmptyState icon="warning" title="Search failed" body={error} /></div>
    {:else if !app.searchQuery.trim()}
      <div class="empty"><EmptyState icon="search" title="Search inside your books" body="Finds words in EPUB, MOBI, AZW3, PDF and text files. DRM-protected books and handwriting can't be searched." /></div>
    {:else if !busy && !groups.length}
      <div class="empty"><EmptyState icon="search" title="No matches" body={`Nothing in your books matches “${app.searchQuery.trim()}”.`} /></div>
    {/if}
    {#each groups as g (g.item.id)}
      <section>
        <div class="bh">
          <Icon name={g.item.format === 'PDF' ? 'document' : isClippingsFile(g.item) ? 'clippings' : 'book'} color="var(--fo-accent)" />
          <span class="bt">{isClippingsFile(g.item) ? 'My Clippings' : g.item.title}</span>
          {#if g.item.authors.length}<span class="ba">{g.item.authors.join(', ')}</span>{/if}
          <span class="bn">{g.hits.length}{g.hits.length === 200 ? '+' : ''} · {g.item.format}</span>
        </div>
        {#each g.hits as h}
          <button type="button" class="hit" onclick={() => open(h)}>
            <span class="where">{h.label || '—'}</span>
            <span class="snip">{#each parts(h.snippet) as p}{#if p.hit}<mark>{p.text}</mark>{:else}{p.text}{/if}{/each}</span>
          </button>
        {/each}
      </section>
    {/each}
  </div>
</div>

<style>
  .screen { display: flex; flex-direction: column; height: 100%; min-width: 0; }
  .search { width: 280px; }
  .results { flex: 1; overflow-y: auto; padding: 16px 28px 40px; }
  .empty { height: 100%; display: flex; align-items: center; justify-content: center; }
  section { max-width: 860px; margin-bottom: 18px; }
  .bh { display: flex; align-items: center; gap: 8px; padding: 10px 0 6px; border-bottom: 1px solid var(--fo-separator); }
  .bt { font: var(--fo-text-headline); }
  .ba { font: var(--fo-text-callout); color: var(--fo-label-2); flex: 1; }
  .bn { margin-left: auto; font: var(--fo-text-caption); color: var(--fo-label-2); }
  .hit { display: flex; gap: 12px; width: 100%; text-align: left; background: none; border: 0; border-bottom: 1px solid var(--fo-separator); padding: 9px 6px; color: var(--fo-label); font: var(--fo-text-body); line-height: 1.5; border-radius: 0; }
  .hit:hover { background: var(--fo-fill); }
  .where { flex-shrink: 0; min-width: 64px; font: var(--fo-text-caption); color: var(--fo-accent-text); padding-top: 2px; }
  mark { background: var(--fo-highlight); color: inherit; border-radius: 2px; padding: 0 1px; }
</style>
