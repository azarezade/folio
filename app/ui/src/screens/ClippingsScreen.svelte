<script lang="ts">
  // My Clippings (Clippings board): books on the left, their highlights,
  // notes and bookmarks on the right, from the Kindle's My Clippings.txt.
  import { app } from '../lib/state.svelte';
  import { asset } from '../lib/api';
  import { parseClippings, type ClipBook } from '../lib/clippings';
  import { plural } from '../lib/format';
  import { Segmented, SearchField, Button, EmptyState, Icon, activate } from '../lib/ui';
  import Toolbar from '../shell/Toolbar.svelte';

  let books = $state<ClipBook[] | null>(null);
  let error = $state<string | null>(null);
  let pick = $state<string | null>(null);
  let show = $state<'all' | 'Highlight' | 'Note' | 'Bookmark'>('all');
  let query = $state('');

  $effect(() => {
    const files = app.clippingFiles;
    let live = true;
    Promise.all(files.map((f) => fetch(asset(`file/${f.id}`)).then((r) => (r.ok ? r.text() : ''))))
      .then((texts) => live && (books = parseClippings(texts.join('\n==========\n'))))
      .catch((e) => live && (error = String(e?.message ?? e)));
    return () => (live = false);
  });
  $effect(() => {
    if (app.openAt?.loc === 'clippings') {
      query = app.openAt.query;
      app.openAt = null;
    }
  });

  const total = $derived(books?.reduce((n, b) => n + b.clips.length, 0) ?? 0);
  const q = $derived(query.trim().toLowerCase());
  const shownBooks = $derived(
    (books ?? []).filter((b) => !q || b.title.toLowerCase().includes(q) || b.author.toLowerCase().includes(q) || b.clips.some((c) => c.text.toLowerCase().includes(q))),
  );
  const current = $derived(shownBooks.find((b) => key(b) === pick) ?? shownBooks[0] ?? null);
  const clips = $derived(
    (current?.clips ?? []).filter((c) => (show === 'all' || c.kind === show) && (!q || c.text.toLowerCase().includes(q) || current!.title.toLowerCase().includes(q))),
  );
  const libBook = $derived(current ? app.items.book.find((b) => b.title.toLowerCase() === current.title.toLowerCase()) : undefined);
  const count = (b: ClipBook, k: string) => b.clips.filter((c) => c.kind === k).length;

  function key(b: ClipBook) {
    return `${b.title}\u0000${b.author}`;
  }
  function tint(title: string) {
    return `var(--fo-cover-${(Array.from(title).reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7) % 6) + 1})`;
  }
  function copyAll() {
    if (!current) return;
    const text = [`${current.title}${current.author ? ` — ${current.author}` : ''}`, '', ...clips.map((c) => `${c.kind === 'Note' ? 'Note: ' : ''}${c.text}\n  (${[c.where, c.added].filter(Boolean).join(' · ')})\n`)].join('\n');
    navigator.clipboard?.writeText(text).then(() => app.toast(`Copied ${plural(clips.length, 'clipping')}`, 'ok'));
  }
</script>

<div class="screen">
  <Toolbar title="My Clippings" subtitle={books ? (total ? `${plural(total, 'clipping')} from ${plural(books.length, 'book')}` : 'No clippings yet') : ''}>
    {#if total}
      <Segmented label="Show" options={[{ value: 'all', label: 'All' }, { value: 'Highlight', label: 'Highlights' }, { value: 'Note', label: 'Notes' }, { value: 'Bookmark', label: 'Bookmarks' }]} bind:value={show} />
      <Button variant="plain" onclick={copyAll} disabled={!clips.length}>Copy</Button>
      <div class="search"><SearchField placeholder="Search clippings" label="Search clippings" bind:value={query} /></div>
    {/if}
  </Toolbar>

  {#if error}
    <div class="empty"><EmptyState icon="warning" title="My Clippings could not be read" body={error} /></div>
  {:else if !books}
    <div class="empty"><span class="fo-spinner"></span></div>
  {:else if !total}
    <div class="empty"><EmptyState icon="clippings" title="No clippings yet" body="Highlights you make on the Kindle show up after the next sync." /></div>
  {:else}
    <div class="split">
      <div class="books" role="listbox" aria-label="Books">
        {#each shownBooks as b (key(b))}
          {@const sel = current && key(b) === key(current)}
          {@const lb = app.items.book.find((x) => x.title.toLowerCase() === b.title.toLowerCase())}
          <div role="option" tabindex="0" aria-selected={!!sel} class="book" class:sel onclick={() => (pick = key(b))} onkeydown={activate}>
            {#if lb?.has_cover}
              <img class="cover" src={asset(`thumb/${lb.id}`)} alt="" />
            {:else}
              <span class="cover" style:background={tint(b.title)}></span>
            {/if}
            <span class="t">
              <span class="title">{b.title}</span>
              <span class="sub">{b.author || 'Unknown author'}</span>
            </span>
            <span class="n">{b.clips.length}</span>
          </div>
        {:else}
          <p class="none">No books match.</p>
        {/each}
      </div>
      <section class="clips">
        {#if current}
          <div class="head">
            <div class="ht">
              <h2>{current.title}</h2>
              <p>{[current.author, plural(count(current, 'Highlight'), 'highlight'), plural(count(current, 'Note'), 'note'), plural(count(current, 'Bookmark'), 'bookmark')].filter(Boolean).join(' · ')}</p>
            </div>
            {#if libBook && !libBook.blocked}<Button onclick={() => app.open(libBook)}>Open Book</Button>{/if}
          </div>
          {#each clips as c}
            <article>
              {#if c.kind === 'Note'}
                <div class="note"><Icon name="note" color="var(--fo-accent)" /><p>{c.text}</p></div>
              {:else if c.kind === 'Bookmark'}
                <div class="mark"><Icon name="bookmark" /><p>{c.where || 'Bookmark'}</p></div>
              {:else}
                <p class="quote">{c.text}</p>
              {/if}
              <div class="meta">
                <span class="chip {c.kind.toLowerCase()}">{c.kind}</span>
                <span>{[c.where, c.added].filter(Boolean).join(' · ')}</span>
              </div>
            </article>
          {:else}
            <p class="none">Nothing to show with this filter.</p>
          {/each}
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .screen { display: flex; flex-direction: column; height: 100%; min-width: 0; }
  .search { width: 200px; }
  .empty { flex: 1; display: flex; align-items: center; justify-content: center; }
  .split { flex: 1; display: flex; min-height: 0; }
  .books { width: 300px; flex: none; border-right: 1px solid var(--fo-separator); padding: 8px; overflow-y: auto; display: flex; flex-direction: column; gap: 2px; }
  .books .book { display: flex; min-width: 0; outline: none; align-items: center; gap: 10px; padding: 8px; border: 0; border-radius: 8px; background: transparent; color: var(--fo-label); text-align: left; }
  .books .book:focus-visible { box-shadow: 0 0 0 2px var(--fo-accent); }
  .books .book:hover:not(.sel) { background: var(--fo-fill); }
  .books .book.sel { background: var(--fo-accent); color: var(--fo-on-accent); }
  .cover { width: 28px; height: 38px; flex: none; border-radius: 3px; object-fit: cover; box-shadow: 0 0 0 1px var(--fo-separator); }
  .t { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .title { font: 500 13px/16px var(--fo-font); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sub, .n { font: var(--fo-text-caption); color: var(--fo-label-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sel .sub, .sel .n { color: inherit; opacity: 0.85; }
  .n { font-variant-numeric: tabular-nums; }
  .clips { flex: 1; min-width: 0; padding: 24px 40px; overflow-y: auto; }
  .head { display: flex; align-items: flex-start; gap: 16px; padding-bottom: 16px; border-bottom: 1px solid var(--fo-separator); }
  .ht { flex: 1; display: flex; flex-direction: column; gap: 4px; }
  h2 { font: var(--fo-text-large-title); letter-spacing: -0.01em; }
  .ht p { font: var(--fo-text-callout); color: var(--fo-label-2); }
  article { padding: 16px 0; border-bottom: 1px solid var(--fo-separator); display: flex; flex-direction: column; gap: 8px; }
  .quote { font: 400 16px/1.5 var(--fo-font-reading); white-space: pre-wrap; }
  .note, .mark { display: flex; gap: 10px; align-items: flex-start; }
  .note p { font: 400 14px/1.5 var(--fo-font); white-space: pre-wrap; }
  .mark { color: var(--fo-label-2); align-items: center; }
  .meta { display: flex; align-items: center; gap: 8px; font: var(--fo-text-caption); color: var(--fo-label-2); }
  .chip { height: 18px; padding: 0 7px; border-radius: 9px; font-weight: 600; display: flex; align-items: center; background: var(--fo-fill); color: var(--fo-label-2); }
  .chip.highlight { background: var(--fo-highlight); color: var(--fo-label); }
  .chip.note { background: var(--fo-fill-2); color: var(--fo-accent-text); }
  .none { padding: 16px 8px; font: var(--fo-text-callout); color: var(--fo-label-2); }
</style>
