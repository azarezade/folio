<script lang="ts">
  // Sideloaded PDFs with PDF.js, in the same reader shell: pages
  // are drawn to images at the display width; single-page mode adds the text
  // layer for selection, and the search box finds pages with a phrase.
  // the legacy build carries polyfills for the WebKit in macOS 12–15 (e.g. Map.getOrInsertComputed)
  import * as pdfjs from 'pdfjs-dist/legacy/build/pdf.mjs';
  import workerUrl from 'pdfjs-dist/legacy/build/pdf.worker.min.mjs?url';
  import '../pdf-text-layer.css';
  import { asset, call, isTauri, saveAs } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { AnnotatedPage, Item } from '../lib/types';
  import { ToolbarButton, SearchField, EmptyState, Button, activate } from '../lib/ui';
  import PagedReader from './PagedReader.svelte';

  pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

  let { item }: { item: Item } = $props();
  let doc = $state<pdfjs.PDFDocumentProxy | null>(null);
  let error = $state<string | null>(null);
  let aspects = $state<number[]>([]);
  let position = $state(0);
  let query = $state('');
  let results = $state<{ page: number; text: string }[] | null>(null);
  let searching = $state(false);
  let jumpTo = $state<number | null>(null);
  /** The current find hit (index into results). */
  let cur = $state(0);
  type Toc = { title: string; page: number | null; items: Toc[] };
  let outline = $state<Toc[]>([]);
  /** Handwriting from the Kindle, by PDF page. */
  let ink = $state<Map<number, AnnotatedPage>>(new Map());
  let showInk = $state(true);
  const cache = new Map<string, Promise<string>>();
  const texts = new Map<number, string>();
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    let live = true;
    let task: ReturnType<typeof pdfjs.getDocument> | null = null;
    (async () => {
      try {
        const [bytes, pos, annotated] = await Promise.all([
          fetch(asset(`file/${item.id}`)).then((r) => r.arrayBuffer()),
          call<number | null>('position', { id: item.id }),
          call<AnnotatedPage[]>('book_annotations', { id: item.id }).catch(() => []),
        ]);
        ink = new Map(annotated.map((a) => [a.pdf_page, a]));
        const data = (dir: string) => new URL(`pdfjs/${dir}/`, document.baseURI).href;
        task = pdfjs.getDocument({
          data: bytes,
          standardFontDataUrl: data('standard_fonts'),
          cMapUrl: data('cmaps'),
          cMapPacked: true,
          iccUrl: data('iccs'),
          wasmUrl: data('wasm'),
        });
        const d = await task.promise;
        if (!live) return;
        const first = await d.getPage(1);
        const v = first.getViewport({ scale: 1 });
        aspects = Array(d.numPages).fill(v.width / v.height);
        position = Math.min(pos ?? 0, d.numPages - 1);
        const at = app.openAt?.id === item.id ? app.openAt : null;
        app.openAt = null;
        if (at?.loc.startsWith('page:')) position = Math.min(Number(at.loc.slice(5)), d.numPages - 1);
        doc = d;
        loadOutline(d).then((o) => live && (outline = o)).catch(() => {});
        if (at?.query) {
          // phrase first, then the longest word (the library matches words anywhere on a page)
          query = at.query;
          await runSearch();
          if (!results?.length) {
            query = at.query.split(/\s+/).sort((a, b) => b.length - a.length)[0];
            await runSearch();
          }
        }
      } catch (e: any) {
        error = String(e?.message ?? e);
      }
    })();
    return () => {
      live = false;
      for (const p of cache.values()) p.then((u) => URL.revokeObjectURL(u)).catch(() => {});
      cache.clear();
      task?.destroy();
    };
  });

  // a new function when handwriting is toggled, so the reader asks for pages again
  const pageSrc = $derived(showInk ? (i: number, w: number) => src(i, w) : (i: number, w: number) => src(i, w));

  function src(i: number, width: number): Promise<string> {
    const w = Math.ceil(width / 128) * 128;
    const a = showInk ? ink.get(i) : undefined;
    const key = `${i}@${w}${a ? '+ink' : ''}`;
    let p = cache.get(key);
    if (!p) {
      p = (async () => {
        const page = await doc!.getPage(i + 1);
        const v1 = page.getViewport({ scale: 1 });
        if (Math.abs(v1.width / v1.height - aspects[i]) > 0.001) aspects[i] = v1.width / v1.height;
        const viewport = page.getViewport({ scale: w / v1.width });
        const canvas = document.createElement('canvas');
        canvas.width = Math.round(viewport.width);
        canvas.height = Math.round(viewport.height);
        await page.render({ canvas, viewport }).promise;
        if (a) {
          // fetched (not <img>) so the canvas stays exportable in the app
          const W = canvas.width, H = canvas.height, p = a.placement;
          const blob = await fetch(asset(`annotation/${item.id}/page/${a.notebook_page}?w=${Math.round(W * p.width)}`)).then((r) => (r.ok ? r.blob() : null));
          if (blob) {
            const bmp = await createImageBitmap(blob);
            canvas.getContext('2d')!.drawImage(bmp, p.left * W, p.top * H, p.width * W, p.height * H);
            bmp.close();
          }
        }
        const blob = await new Promise<Blob>((res, rej) => canvas.toBlob((b) => (b ? res(b) : rej(new Error('render failed'))), 'image/png'));
        return URL.createObjectURL(blob);
      })();
      cache.set(key, p);
      if (cache.size > 60) {
        const [oldKey, old] = cache.entries().next().value!;
        cache.delete(oldKey);
        old.then((u) => URL.revokeObjectURL(u)).catch(() => {});
      }
    }
    return p;
  }

  /** The PDF's bookmarks with their page indexes. */
  async function loadOutline(d: pdfjs.PDFDocumentProxy): Promise<Toc[]> {
    const raw = (await d.getOutline()) ?? [];
    const walk = async (items: any[]): Promise<Toc[]> =>
      Promise.all(
        items.map(async (o) => {
          let page: number | null = null;
          try {
            const dest = typeof o.dest === 'string' ? await d.getDestination(o.dest) : o.dest;
            if (dest?.[0]) page = typeof dest[0] === 'number' ? dest[0] : await d.getPageIndex(dest[0]);
          } catch {}
          return { title: o.title, page, items: await walk(o.items ?? []) };
        }),
      );
    return walk(raw);
  }

  function goPage(n: number) {
    jumpTo = null;
    queueMicrotask(() => (jumpTo = n));
  }
  function step(d: number) {
    if (!results?.length) return;
    cur = (cur + d + results.length) % results.length;
    goPage(results[cur].page);
  }

  async function pageText(i: number) {
    if (!texts.has(i)) {
      const page = await doc!.getPage(i + 1);
      const tc = await page.getTextContent();
      // items are words and spaces; line ends are flagged separately
      const raw = tc.items.map((x: any) => (x.str ?? '') + (x.hasEOL ? ' ' : '')).join('');
      texts.set(i, raw.replace(/\s+/g, ' '));
    }
    return texts.get(i)!;
  }

  function search(e: SubmitEvent) {
    e.preventDefault();
    runSearch();
  }

  async function runSearch() {
    const q = query.trim().replace(/\s+/g, ' ').toLowerCase();
    if (!q || !doc) return (results = null);
    searching = true;
    const out: { page: number; text: string }[] = [];
    for (let i = 0; i < doc.numPages && out.length < 200; i++) {
      const t = await pageText(i);
      const at = t.toLowerCase().indexOf(q);
      if (at >= 0) out.push({ page: i, text: '…' + t.slice(Math.max(0, at - 40), at + q.length + 60) + '…' });
    }
    results = out;
    searching = false;
    cur = 0;
    if (out.length) goPage(out[0].page);
  }

  /** PDF.js text layer for selecting and finding text on the shown page. */
  function textLayer(node: HTMLDivElement, args: { page: number; w: number }) {
    let task: any = null;
    async function draw({ page, w }: { page: number; w: number }) {
      task?.cancel?.();
      node.innerHTML = '';
      const p = await doc!.getPage(page + 1);
      const v1 = p.getViewport({ scale: 1 });
      const viewport = p.getViewport({ scale: w / v1.width });
      node.style.setProperty('--scale-factor', String(viewport.scale));
      node.style.setProperty('--total-scale-factor', String(viewport.scale));
      task = new pdfjs.TextLayer({ textContentSource: p.streamTextContent(), container: node, viewport });
      await task.render();
      // spans hold single words, so mark the words of the query
      const words = query.toLowerCase().split(/\s+/).filter((w) => w.length > 1);
      if (words.length)
        for (const span of node.querySelectorAll('span')) {
          const t = span.textContent?.toLowerCase().trim() ?? '';
          if (t && words.some((w) => t.includes(w))) span.classList.add('hit');
        }
    }
    draw(args);
    return { update: draw, destroy: () => task?.cancel?.() };
  }

  async function exportInk() {
    const base = item.rel_path.split('/').pop()!.replace(/\.pdf$/i, '');
    const dest = await saveAs(`${base} (annotated).pdf`);
    if (!dest) return;
    app.run('Export with handwriting', async () => {
      const r = await call<{ pages: number; path: string }>('export_annotated', { id: item.id, dest });
      app.toast(`Saved with handwriting on ${r.pages} page${r.pages === 1 ? '' : 's'}`, 'ok');
    });
  }

  function onPage(n: number) {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => call('set_position', { id: item.id, page: n }), 400);
  }
</script>

{#if error}
  <div class="center"><EmptyState icon="warning" title="This PDF could not be opened" body={error} /></div>
{:else if !doc}
  <div class="center"><span class="fo-spinner"></span></div>
{:else}
  <PagedReader
    title={item.title}
    subtitle={`PDF · ${doc.numPages} pages${ink.size ? ` · handwriting on ${ink.size} page${ink.size === 1 ? '' : 's'}` : ''}`}
    count={doc.numPages}
    aspect={(i) => aspects[i] ?? 0.75}
    src={pageSrc}
    coverOnRight
    initialPage={position}
    {onPage}
    {jumpTo}
    contents={outline.length ? contents : undefined}
  >
    {#snippet tools()}
      {#if ink.size}
        <ToolbarButton icon="pen" label={showInk ? 'Hide your handwriting' : 'Show your handwriting'} active={showInk} onclick={() => (showInk = !showInk)} />
        {#if isTauri}<ToolbarButton icon="export" label="Export with handwriting…" onclick={exportInk} />{/if}
      {/if}
      <form class="find" onsubmit={search}><SearchField placeholder="Find" label="Find in document" bind:value={query} /></form>
    {/snippet}
    {#snippet bar()}
      {#if results || searching}
        <div class="findbar" role="status">
          <span>Find:</span><strong>“{query.trim()}”</strong>
          <span class="muted">{searching ? 'Searching…' : results?.length ? `${cur + 1} of ${results.length} page${results.length === 1 ? '' : 's'} · p. ${results[cur].page + 1}` : 'No matches'}</span>
          <span class="grow"></span>
          <ToolbarButton icon="back" label="Previous match" disabled={!results?.length} onclick={() => step(-1)} />
          <ToolbarButton icon="forward" label="Next match" disabled={!results?.length} onclick={() => step(1)} />
          <Button variant="plain" onclick={() => { results = null; query = ''; }}>Done</Button>
        </div>
      {/if}
    {/snippet}
    {#snippet overlay(page, w, h)}
      <div class="textLayer" style="width:{w}px;height:{h}px" use:textLayer={{ page, w }}></div>
    {/snippet}
  </PagedReader>
{/if}

{#snippet contents()}
  {#snippet entries(list: Toc[], depth: number)}
    {#each list as t}
      <div role="button" class="toc" class:off={t.page == null} tabindex={t.page == null ? -1 : 0} aria-disabled={t.page == null} style:padding-left="{8 + depth * 14}px" onclick={() => t.page != null && goPage(t.page)} onkeydown={activate}>
        <span class="tt">{t.title}</span>{#if t.page != null}<span class="tp">{t.page + 1}</span>{/if}
      </div>
      {#if t.items.length}{@render entries(t.items, depth + 1)}{/if}
    {/each}
  {/snippet}
  {@render entries(outline, 0)}
{/snippet}

<style>
  .center { height: 100%; display: flex; align-items: center; justify-content: center; }
  .find { width: 190px; }
  .findbar { display: flex; align-items: center; gap: 8px; height: 36px; padding: 0 12px; border-bottom: 1px solid var(--fo-separator); background: var(--fo-bg-group); font: var(--fo-text-callout); }
  .findbar strong { font-weight: 600; }
  .muted { color: var(--fo-label-2); }
  .grow { flex: 1; }
  .toc { width: 100%; display: flex; align-items: center; gap: 8px; min-height: var(--fo-row-list); padding: 4px 8px; border: 0; border-radius: var(--fo-radius-button); background: transparent; color: var(--fo-label); font: var(--fo-text-body); text-align: left; }
  .toc:hover:not(.off) { background: var(--fo-fill); }
  .toc.off { color: var(--fo-label-2); }
  .tt { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tp { font: var(--fo-text-caption); color: var(--fo-label-2); font-variant-numeric: tabular-nums; }
  .textLayer { position: absolute; left: 0; top: 0; -webkit-user-select: text; user-select: text; }
  :global(.textLayer span.hit) { background: var(--fo-highlight); border-radius: 2px; }
</style>
