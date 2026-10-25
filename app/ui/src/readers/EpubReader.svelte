<script lang="ts">
  // Reflowable books (EPUB, MOBI, AZW3) with foliate-js (vendored, MIT): a
  // contents panel, themes, font size, search and a progress bar. The reading
  // location (an EPUB CFI) is saved in the index and restored on open.
  import '../vendor/foliate-js/view.js';
  import { asset, call } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { Item } from '../lib/types';
  import { ToolbarButton, SearchField, Segmented, Button, EmptyState } from '../lib/ui';
  import Toolbar from '../shell/Toolbar.svelte';

  type TocItem = { label: string; href: string; subitems?: TocItem[] };
  type Hit = { cfi: string; excerpt: { pre: string; match: string; post: string }; label: string };

  let { item }: { item: Item } = $props();
  let host: HTMLDivElement;
  let view: any = null;
  let error = $state<string | null>(null);
  let ready = $state(false);
  let toc = $state<TocItem[]>([]);
  let panel = $state(true);
  let tab = $state<'contents' | 'search'>('contents');
  let textOpen = $state(false);
  let fraction = $state(0);
  let chapter = $state('');
  let currentHref = $state('');
  let query = $state('');
  let hits = $state<Hit[] | null>(null);
  let searchProgress = $state(1);
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

  // auto follows the app's appearance; the others are fixed reading themes
  const THEMES = ['auto', 'paper', 'sepia', 'night'] as const;
  type Theme = (typeof THEMES)[number];
  const tok = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  function colours(t: Theme) {
    if (t === 'auto') return { bg: tok('--fo-bg-window'), fg: tok('--fo-label'), link: tok('--fo-accent-text'), dark: tok('color-scheme') === 'dark' };
    return { bg: tok(`--fo-reading-${t}-bg`), fg: tok(`--fo-reading-${t}-fg`), link: tok('--fo-accent-text'), dark: t === 'night' };
  }
  const stored = (k: string, d: string) => {
    try {
      return localStorage.getItem(k) ?? d;
    } catch {
      return d;
    }
  };
  const store = (k: string, v: string) => {
    try {
      localStorage.setItem(k, v);
    } catch {}
  };
  let theme = $state<Theme>((THEMES as readonly string[]).includes(stored('book.theme', 'auto')) ? (stored('book.theme', 'auto') as Theme) : 'auto');
  // follow appearance changes while on the auto theme
  let scheme = $state(0);
  $effect(() => {
    void app.settings.theme;
    const mq = matchMedia('(prefers-color-scheme: dark)');
    const bump = () => scheme++;
    mq.addEventListener('change', bump);
    requestAnimationFrame(bump);
    return () => mq.removeEventListener('change', bump);
  });
  let fontSize = $state(Number(stored('book.font', '100')) || 100);
  let spacing = $state(Number(stored('book.spacing', '1.5')) || 1.5);
  let layout = $state<'paginated' | 'scrolled'>(stored('book.layout', 'paginated') === 'scrolled' ? 'scrolled' : 'paginated');

  function css() {
    const t = colours(theme);
    return `
      html { color-scheme: ${t.dark ? 'dark' : 'light'}; }
      html, body { background: ${t.bg} !important; color: ${t.fg} !important; }
      body { font-size: ${fontSize}% !important; }
      a:link, a:visited { color: ${t.link} !important; }
      p, li, blockquote, dd { line-height: ${spacing}; text-align: start; hyphens: auto; -webkit-hyphens: auto; widows: 2; orphans: 2; }
      [align="center"] { text-align: center; } [align="right"] { text-align: right; }
      pre { white-space: pre-wrap !important; }
      img, svg { max-width: 100%; height: auto; }
      ${t.dark ? 'img { filter: brightness(0.9); }' : ''}
    `;
  }

  function applyStyles() {
    if (!view?.renderer) return;
    view.renderer.setStyles?.(css());
    view.renderer.setAttribute('flow', layout);
    view.renderer.setAttribute('gap', '6%');
    view.renderer.setAttribute('max-inline-size', '720px');
    view.renderer.setAttribute('max-column-count', '2');
  }

  $effect(() => {
    // re-apply when a setting changes
    void [theme, fontSize, spacing, layout, scheme];
    store('book.theme', theme);
    store('book.font', String(fontSize));
    store('book.spacing', String(spacing));
    store('book.layout', layout);
    applyStyles();
  });

  $effect(() => {
    let live = true;
    (async () => {
      try {
        const [blob, loc] = await Promise.all([
          fetch(asset(`file/${item.id}`)).then((r) => {
            if (!r.ok) throw new Error(`HTTP ${r.status}`);
            return r.blob();
          }),
          call<{ location: string; fraction: number | null } | null>('location', { id: item.id }),
        ]);
        if (!live) return;
        const name = item.rel_path.split('/').pop() ?? 'book';
        view = document.createElement('foliate-view');
        view.style.cssText = 'display:block;width:100%;height:100%';
        host.append(view);
        await view.open(new File([blob], name, { type: blob.type }));
        if (!live) return;
        toc = view.book.toc ?? [];
        view.addEventListener('relocate', (e: CustomEvent) => {
          const d = e.detail;
          fraction = d.fraction ?? 0;
          chapter = d.tocItem?.label ?? '';
          currentHref = d.tocItem?.href ?? '';
          clearTimeout(saveTimer);
          if (d.cfi) saveTimer = setTimeout(() => call('set_location', { id: item.id, location: d.cfi, fraction }), 500);
        });
        view.addEventListener('load', (e: CustomEvent) => e.detail.doc?.addEventListener('keydown', onKey));
        applyStyles();
        const at = app.openAt?.id === item.id ? app.openAt : null;
        app.openAt = null;
        await view.init({ lastLocation: at ? null : (loc?.location ?? null), showTextStart: !loc && !at });
        ready = true;
        if (at) await openAtHit(at);
      } catch (e: any) {
        if (live) error = String(e?.message ?? e);
      }
    })();
    return () => {
      live = false;
      clearTimeout(saveTimer);
      view?.close?.();
      view?.remove();
      view = null;
    };
  });

  /** Jump to a library search hit, then search the book for the words. */
  async function openAtHit(at: { loc: string; query: string }) {
    const [kind, v] = at.loc.split(':');
    if (kind === 'section') await view.goTo(Number(v));
    else if (kind === 'fraction') await view.goToFraction(Number(v));
    if (!at.query) return;
    // the library matches words anywhere in a passage; here try the phrase,
    // then the longest word
    const longest = at.query.split(/\s+/).sort((a, b) => b.length - a.length)[0];
    const index = kind === 'section' ? Number(v) : undefined;
    let found: Hit[] = [];
    for (const q of [...new Set([at.query, longest])]) {
      query = q;
      found = [];
      for await (const r of view.search({ query: q, index })) {
        if (r === 'done') break;
        if (r.cfi) found.push({ cfi: r.cfi, excerpt: r.excerpt, label: '' });
        if (r.subitems) for (const s of r.subitems) found.push({ ...s, label: r.label });
      }
      if (found.length) break;
    }
    hits = found.slice(0, 500);
    if (found.length && kind === 'section') await view.goTo(found[0].cfi);
  }

  function onKey(e: KeyboardEvent) {
    if ((e.target as HTMLElement)?.closest?.('input, textarea')) return;
    if (e.key === 'ArrowLeft' || e.key === 'PageUp') view?.goLeft();
    else if (e.key === 'ArrowRight' || e.key === 'PageDown' || e.key === ' ') view?.goRight();
    else if (e.key === 'Escape' && app.zen) app.zen = false;
    else return;
    e.preventDefault();
  }

  async function search(e?: SubmitEvent) {
    e?.preventDefault();
    const q = query.trim();
    panel = true;
    tab = 'search';
    view?.clearSearch();
    if (!q || !view) return (hits = null);
    hits = [];
    searchProgress = 0;
    const found: Hit[] = [];
    for await (const r of view.search({ query: q })) {
      if (r === 'done') break;
      if (r.progress != null) searchProgress = r.progress;
      if (r.subitems) {
        for (const s of r.subitems) found.push({ ...s, label: r.label });
        hits = found.slice(0, 500);
      }
    }
    searchProgress = 1;
    hits = found.slice(0, 500);
  }

  function closeSearch() {
    hits = null;
    view?.clearSearch();
  }

  function seek(e: Event) {
    view?.goToFraction(Number((e.currentTarget as HTMLInputElement).value));
  }

  const percent = $derived(Math.round(fraction * 100));
</script>

<svelte:window onkeydown={onKey} />

<div class="reader">
  {#if !app.zen}
    <Toolbar title={item.title} subtitle={item.authors.length ? item.authors.join(', ') : item.format} back={() => app.close()}>
      <ToolbarButton icon="pages" label={panel ? 'Hide sidebar' : 'Show sidebar'} active={panel} onclick={() => (panel = !panel)} />
      <span class="toolbar-sep"></span>
      <ToolbarButton icon="back" label="Previous page (←)" onclick={() => view?.goLeft()} />
      <ToolbarButton icon="forward" label="Next page (→)" onclick={() => view?.goRight()} />
      <div class="aa">
        <button type="button" class="aa-btn" class:on={textOpen} aria-label="Text settings" aria-expanded={textOpen} onclick={() => (textOpen = !textOpen)}>A<span>a</span></button>
        {#if textOpen}
          <div class="scrim" role="presentation" onclick={() => (textOpen = false)}></div>
          <div class="pop" role="dialog" aria-label="Text settings">
            <div class="size">
              <button type="button" aria-label="Smaller text" onclick={() => (fontSize = Math.max(70, fontSize - 10))}>A</button>
              <span>{fontSize}%</span>
              <button type="button" aria-label="Larger text" class="big" onclick={() => (fontSize = Math.min(200, fontSize + 10))}>A</button>
            </div>
            <div class="themes" role="radiogroup" aria-label="Theme">
              {#each THEMES as t (t)}
                <button type="button" role="radio" aria-checked={theme === t} class="sw {t}" onclick={() => (theme = t)}><span>{t === 'auto' ? 'Auto' : t[0].toUpperCase() + t.slice(1)}</span></button>
              {/each}
            </div>
            <label class="line">Line spacing <Segmented label="Line spacing" options={[{ value: '1.3', label: 'Tight' }, { value: '1.5', label: 'Normal' }, { value: '1.8', label: 'Loose' }]} value={String(spacing)} onchange={(v) => (spacing = Number(v))} /></label>
            <label class="line">Layout <Segmented label="Layout" options={[{ value: 'paginated', label: 'Pages' }, { value: 'scrolled', label: 'Scroll' }]} bind:value={layout} /></label>
            <Button variant="plain" onclick={() => { app.zen = true; textOpen = false; }}>Zen Mode</Button>
          </div>
        {/if}
      </div>
      <span class="toolbar-sep"></span>
      <form class="find" onsubmit={search}><SearchField placeholder="Search" label="Search in book" bind:value={query} /></form>
    </Toolbar>
  {/if}

  <div class="body">
    {#if panel && !app.zen}
      <aside class="side">
        <div class="tabs"><Segmented label="Sidebar" options={[{ value: 'contents', label: 'Contents' }, { value: 'search', label: 'Search' }]} bind:value={tab} /></div>
        {#if tab === 'contents'}
          <nav class="list" aria-label="Contents">
            {#snippet entries(list: TocItem[], depth: number)}
              {#each list as t}
                <button type="button" class="row" class:current={t.href === currentHref} style:padding-left="{8 + depth * 14}px" onclick={() => view?.goTo(t.href)}>{t.label}</button>
                {#if t.subitems?.length}{@render entries(t.subitems, depth + 1)}{/if}
              {/each}
            {/snippet}
            {#if toc.length}{@render entries(toc, 0)}{:else}<p class="none">This book has no table of contents.</p>{/if}
          </nav>
        {:else}
          <div class="list">
            {#if hits}
              <p class="count">{searchProgress < 1 ? `Searching… ${Math.round(searchProgress * 100)}%` : `${hits.length}${hits.length === 500 ? '+' : ''} result${hits.length === 1 ? '' : 's'} for “${query.trim()}”`}</p>
              {#each hits as h}
                <button type="button" class="hit" onclick={() => view?.goTo(h.cfi)}>
                  {#if h.label}<span class="where">{h.label}</span>{/if}
                  <span>{h.excerpt.pre}<mark>{h.excerpt.match}</mark>{h.excerpt.post}</span>
                </button>
              {/each}
              {#if hits.length}<Button variant="plain" onclick={closeSearch}>Clear Results</Button>{/if}
            {:else}
              <p class="none">Type in the search field to find words in this book.</p>
            {/if}
          </div>
        {/if}
      </aside>
    {/if}
    <div class="page">
      {#if error}
        <div class="center"><EmptyState icon="warning" title="This book could not be opened" body={error} /></div>
      {:else if !ready}
        <div class="center"><span class="fo-spinner"></span></div>
      {/if}
      <div class="host" class:hidden={!!error} bind:this={host}></div>
    </div>
  </div>

  {#if !app.zen}
    <div class="progress">
      <span class="chapter">{chapter}</span>
      <input type="range" min="0" max="1" step="0.001" value={fraction} onchange={seek} aria-label="Position in book" />
      <span class="pct">{percent}%</span>
    </div>
  {:else}
    <button type="button" class="zen-exit" aria-label="Leave Zen (Esc)" onclick={() => (app.zen = false)}>✕</button>
  {/if}
</div>

<style>
  .reader { display: flex; flex-direction: column; height: 100%; min-height: 0; min-width: 0; position: relative; background: var(--fo-bg-window); }
  .find { width: 190px; }
  .aa { position: relative; }
  .aa-btn { height: 28px; padding: 0 8px; border: 0; border-radius: var(--fo-radius-button); background: transparent; color: var(--fo-label); font: 600 15px/1 var(--fo-font-reading); }
  .aa-btn span { font-size: 11px; }
  .aa-btn:hover, .aa-btn.on { background: var(--fo-fill); }
  .scrim { position: fixed; inset: 0; z-index: 60; }
  .pop { position: absolute; top: 36px; right: -60px; z-index: 61; width: 280px; padding: 12px; border-radius: var(--fo-radius-window); background: var(--fo-bg-popover); box-shadow: var(--fo-shadow-popover); display: flex; flex-direction: column; gap: 12px; }
  .size { display: flex; align-items: center; justify-content: space-between; border-radius: var(--fo-radius-field); background: var(--fo-fill); }
  .size button { flex: 1; height: 30px; border: 0; background: transparent; color: var(--fo-label); font: 400 13px/1 var(--fo-font-reading); }
  .size button.big { font-size: 19px; }
  .size span { font: var(--fo-text-caption); color: var(--fo-label-2); }
  .themes { display: grid; grid-template-columns: repeat(4, 1fr); gap: 6px; }
  .sw { height: 44px; border: 0; border-radius: 8px; font: var(--fo-text-caption); display: flex; align-items: flex-end; justify-content: center; padding-bottom: 4px; box-shadow: 0 0 0 1px var(--fo-separator); }
  .sw.auto { background: var(--fo-bg-window); color: var(--fo-label); }
  .sw.paper { background: var(--fo-reading-paper-bg); color: var(--fo-reading-paper-fg); }
  .sw.sepia { background: var(--fo-reading-sepia-bg); color: var(--fo-reading-sepia-fg); }
  .sw.night { background: var(--fo-reading-night-bg); color: var(--fo-reading-night-fg); }
  .sw[aria-checked='true'] { box-shadow: 0 0 0 2px var(--fo-accent); }
  .line { display: flex; align-items: center; justify-content: space-between; font: var(--fo-text-callout); }
  .body { flex: 1; display: flex; min-height: 0; }
  .side { width: var(--fo-panel-contents); flex: none; display: flex; flex-direction: column; background: var(--fo-bg-sidebar); border-right: 1px solid var(--fo-separator); }
  .tabs { padding: 10px; display: flex; justify-content: center; border-bottom: 1px solid var(--fo-separator); }
  .list { flex: 1; overflow-y: auto; padding: 6px; display: flex; flex-direction: column; }
  .row { display: block; width: 100%; text-align: left; border: 0; border-radius: var(--fo-radius-button); background: transparent; padding: 6px 8px; font: var(--fo-text-body); color: var(--fo-label); }
  .row:hover { background: var(--fo-fill); }
  .row.current { background: var(--fo-accent); color: var(--fo-on-accent); }
  .count, .none { padding: 6px 8px; font: var(--fo-text-caption); color: var(--fo-label-2); }
  .hit { display: flex; flex-direction: column; gap: 2px; width: 100%; text-align: left; border: 0; border-radius: var(--fo-radius-button); background: transparent; padding: 8px; font: var(--fo-text-callout); color: var(--fo-label); line-height: 1.4; }
  .hit:hover { background: var(--fo-fill); }
  .where { font: var(--fo-text-caption); color: var(--fo-label-2); }
  mark { background: var(--fo-highlight); color: inherit; border-radius: 2px; }
  .page { flex: 1; position: relative; min-width: 0; }
  .host { position: absolute; inset: 0; padding: 12px 0; }
  .host.hidden { display: none; }
  .center { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; }
  .progress { display: flex; align-items: center; gap: 12px; height: var(--fo-statusbar-height); padding: 0 16px; border-top: 1px solid var(--fo-separator); font: var(--fo-text-caption); color: var(--fo-label-2); flex: none; }
  .progress input { flex: 1; accent-color: var(--fo-accent); }
  .chapter { max-width: 40%; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .pct { min-width: 34px; text-align: right; font-variant-numeric: tabular-nums; }
  .zen-exit { position: absolute; top: 12px; right: 12px; z-index: 10; width: 28px; height: 28px; border: 0; border-radius: 50%; background: var(--fo-bg-popover); box-shadow: var(--fo-shadow-popover); color: var(--fo-label-2); opacity: 0.6; }
  .zen-exit:hover { opacity: 1; }
</style>
