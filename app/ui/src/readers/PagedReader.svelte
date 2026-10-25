<script lang="ts">
  // A paged reader for notebooks and PDFs (Reader and DocReader boards).
  // Pages are images at the exact display width (×devicePixelRatio), so zoom
  // stays sharp. A pages panel on the left (with an optional Contents tab),
  // Page / Book / Zen in the toolbar; book mode shows spreads with the 3D
  // flip from the Python app (books keep the cover alone on the right).
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';
  import { app } from '../lib/state.svelte';
  import { Segmented, ToolbarButton } from '../lib/ui';
  import Toolbar from '../shell/Toolbar.svelte';
  import PageImg from './PageImg.svelte';

  interface Props {
    title: string;
    subtitle?: string;
    count: number;
    /** width / height of page i */
    aspect: (i: number) => number;
    src: (i: number, width: number) => string | Promise<string>;
    coverOnRight?: boolean;
    initialPage?: number;
    onPage?: (n: number) => void;
    /** Toolbar controls after the zoom (Export…, Find). */
    tools?: Snippet;
    /** A bar under the toolbar (find results). */
    bar?: Snippet;
    /** A Contents tab in the side panel (PDF outline). */
    contents?: Snippet;
    /** Drawn over a single page (e.g. a PDF text layer). */
    overlay?: Snippet<[number, number, number]>;
    jumpTo?: number | null;
  }
  let { title, subtitle = '', count, aspect, src, coverOnRight = false, initialPage = 0, onPage, tools, bar, contents, overlay, jumpTo = null }: Props = $props();
  let panel = $state(true);
  let tab = $state<'contents' | 'pages'>('pages');

  let page = $state(0);
  let zoom = $state(1);
  let vw = $state(800);
  let vh = $state(600);
  let gotoText = $state('');
  let flip = $state<{ forward: boolean; leaving: number; arriving: number; left: number; top: number; w: number; h: number; go: boolean } | null>(null);
  let spreadEl = $state<HTMLDivElement>();
  let wrapperEl = $state<HTMLDivElement>();
  let strip = $state<HTMLDivElement>();
  let stripTop = $state(0);
  let stripHeight = $state(600);
  let zenShown = $state(true);
  let zenTimer: ReturnType<typeof setTimeout> | undefined;

  const bookAllowed = $derived(count >= 3);
  const book = $derived(app.bookMode && bookAllowed);
  const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;

  onMount(() => {
    page = Math.min(Math.max(0, initialPage), Math.max(0, count - 1));
    if (book) page = snap(page);
    window.addEventListener('folio-menu', onMenu);
    return () => window.removeEventListener('folio-menu', onMenu);
  });

  $effect(() => {
    if (jumpTo != null) show(jumpTo);
  });

  // ── spreads (as server.py) ────────────────────────────────────────────────
  function snap(n: number) {
    if (coverOnRight) return n <= 0 ? 0 : n % 2 === 1 ? n : n - 1;
    return n - (n % 2);
  }
  function spread(n: number): [number | null, number | null] {
    if (coverOnRight) return n === 0 ? [null, 0] : [n, n + 1 < count ? n + 1 : null];
    return [n, n + 1 < count ? n + 1 : null];
  }
  function spreadIndex(n: number) {
    return coverOnRight ? (n === 0 ? 0 : Math.floor((n + 1) / 2)) : Math.floor(n / 2);
  }
  const visiblePages = $derived(book ? (spread(page).filter((x) => x !== null) as number[]) : [page]);
  const atFirst = $derived(page === 0);
  const atLast = $derived(visiblePages[visiblePages.length - 1] >= count - 1);
  const indicator = $derived(
    visiblePages.length === 2 ? `${visiblePages[0] + 1}–${visiblePages[1] + 1} / ${count}` : `${visiblePages[0] + 1} / ${count}`,
  );

  /** One page's CSS size: fits the viewport (two across in book mode), times zoom. */
  function size(i: number, paired: boolean) {
    const pad = 56;
    const a = aspect(i) || 0.75;
    const availH = Math.max(100, vh - pad);
    const availW = Math.max(100, paired ? (vw - pad) / 2 : vw - pad);
    let h = availH;
    let w = h * a;
    if (w > availW) {
      w = availW;
      h = w / a;
    }
    return { w: w * zoom, h: h * zoom };
  }
  const px = (w: number) => Math.round(w * dpr);

  function show(n: number, animate = true) {
    if (n < 0 || n >= count) return;
    if (book) n = snap(n);
    const old = page;
    if (n === old) return;
    page = n;
    onPage?.(n);
    if (animate && book && shouldFlip(old, n)) startFlip(old, n);
    prefetch();
  }
  function shouldFlip(a: number, b: number) {
    if (Math.abs(zoom - 1) > 0.01) return false;
    if (Math.abs(spreadIndex(a) - spreadIndex(b)) !== 1) return false;
    const solo = (n: number) => spread(snap(n)).filter((x) => x !== null).length === 1;
    return !solo(a) && !solo(b);
  }
  function startFlip(oldN: number, newN: number) {
    const forward = newN > oldN;
    const os = spread(oldN);
    const ns = spread(newN);
    const leaving = forward ? (os[1] ?? os[0]) : (os[0] ?? os[1]);
    const arriving = forward ? (ns[0] ?? ns[1]) : (ns[1] ?? ns[0]);
    if (leaving == null || arriving == null || !spreadEl || !wrapperEl) return;
    const r = spreadEl.getBoundingClientRect();
    const wr = wrapperEl.getBoundingClientRect();
    const s = size(leaving, true);
    flip = { forward, leaving, arriving, left: r.left - wr.left + (forward ? r.width / 2 : 0), top: r.top - wr.top, w: s.w, h: s.h, go: false };
    requestAnimationFrame(() => requestAnimationFrame(() => flip && (flip.go = true)));
    setTimeout(() => (flip = null), 650);
  }
  function go(delta: number) {
    if (!book) return show(page + delta);
    if (coverOnRight) {
      if (delta > 0) show(page === 0 ? 1 : page + 2);
      else show(page <= 2 ? 0 : page - 2);
    } else show(delta > 0 ? page + 2 : Math.max(0, page - 2));
  }
  function setZoom(z: number) {
    zoom = Math.round(Math.max(0.3, Math.min(3, z)) * 100) / 100;
  }
  function prefetch() {
    const next = book ? [page + 2, page + 3, page - 2, page - 1] : [page + 1, page - 1];
    for (const n of next) {
      if (n < 0 || n >= count) continue;
      const s = src(n, px(size(n, book).w));
      if (typeof s === 'string') new Image().src = s;
    }
  }

  // ── page thumbnails in the side panel (virtualised) ───────────────────────
  const TH = 176;
  const thumbFirst = $derived(Math.max(0, Math.floor(stripTop / TH) - 4));
  const thumbLast = $derived(Math.min(count, Math.ceil((stripTop + stripHeight) / TH) + 4));
  $effect(() => {
    const p = visiblePages[0];
    if (!strip) return;
    const y = p * TH;
    if (y < strip.scrollTop || y > strip.scrollTop + strip.clientHeight - TH) strip.scrollTo({ top: y - strip.clientHeight / 2 + TH / 2, behavior: 'smooth' });
  });
  const mode = $derived<'page' | 'book' | 'zen'>(app.zen ? 'zen' : book ? 'book' : 'page');
  async function setMode(m: 'page' | 'book' | 'zen') {
    if (m === 'zen') return void (app.zen = true);
    app.zen = false;
    if ((m === 'book') !== book) await toggleBook();
  }

  // ── keys and zen ──────────────────────────────────────────────────────────
  function onKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    switch (e.key) {
      case 'ArrowLeft': case 'ArrowUp': go(-1); break;
      case 'ArrowRight': case 'ArrowDown': go(1); break;
      case '+': case '=': setZoom(zoom + 0.15); break;
      case '-': setZoom(zoom - 0.15); break;
      case '0': setZoom(1); break;
      case 'z': case 'Z': app.zen = !app.zen; break;
      case 'b': case 'B': if (bookAllowed) toggleBook(); break;
      case 'Escape': if (app.zen) app.zen = false; else return; break;
      default: return;
    }
    e.preventDefault();
  }
  function onMenu(e: Event) {
    const id = (e as CustomEvent).detail;
    if (id === 'zoom_in') setZoom(zoom + 0.15);
    if (id === 'zoom_out') setZoom(zoom - 0.15);
    if (id === 'zoom_reset') setZoom(1);
  }
  async function toggleBook() {
    await app.toggleBookMode();
    page = book ? snap(page) : page;
  }
  function zenMove() {
    zenShown = true;
    clearTimeout(zenTimer);
    zenTimer = setTimeout(() => (zenShown = false), 2200);
  }
  function gotoPage(e: SubmitEvent) {
    e.preventDefault();
    const n = parseInt(gotoText, 10);
    if (n >= 1 && n <= count) show(n - 1, false);
    gotoText = '';
  }
</script>

<svelte:window onkeydown={onKey} onmousemove={app.zen ? zenMove : undefined} />

<div class="reader" class:zen={app.zen}>
  {#if !app.zen}
    <Toolbar {title} subtitle={subtitle.replace(/^·\s*/, '')} back={() => app.close()}>
      <ToolbarButton icon="pages" label={panel ? 'Hide pages' : 'Show pages'} active={panel} onclick={() => (panel = !panel)} />
      <span class="toolbar-sep"></span>
      <Segmented
        label="Layout"
        options={[{ value: 'page', label: 'Page' }, { value: 'book', label: 'Book', disabled: !bookAllowed }, { value: 'zen', label: 'Zen' }]}
        value={mode}
        onchange={setMode}
      />
      <div class="zoom" role="group" aria-label="Zoom">
        <button type="button" aria-label="Zoom out" title="Zoom out (−)" onclick={() => setZoom(zoom - 0.15)}>−</button>
        <button type="button" class="pct" title="Actual size (0)" onclick={() => setZoom(1)}>{Math.round(zoom * 100)}%</button>
        <button type="button" aria-label="Zoom in" title="Zoom in (+)" onclick={() => setZoom(zoom + 0.15)}>+</button>
      </div>
      <form class="goto" onsubmit={gotoPage}>
        <input aria-label="Page" inputmode="numeric" placeholder={String(visiblePages[0] + 1)} bind:value={gotoText} />
        <span>of {count}</span>
      </form>
      {#if tools}<span class="toolbar-sep"></span>{@render tools()}{/if}
    </Toolbar>
    {@render bar?.()}
  {/if}

  <div class="main">
    {#if panel && !app.zen}
      <aside class="side" aria-label="Pages">
        {#if contents}
          <div class="tabs">
            <Segmented label="Side panel" options={[{ value: 'contents', label: 'Contents' }, { value: 'pages', label: 'Pages' }]} bind:value={tab} />
          </div>
        {/if}
        {#if contents && tab === 'contents'}
          <div class="contents">{@render contents()}</div>
        {:else}
          <div class="thumbs" bind:this={strip} bind:clientHeight={stripHeight} onscroll={(e) => (stripTop = (e.currentTarget as HTMLDivElement).scrollTop)}>
            <div style="height:{count * TH}px;position:relative">
              {#each Array.from({ length: thumbLast - thumbFirst }, (_, k) => thumbFirst + k) as i (i)}
                <button type="button" class="thumb" class:active={visiblePages.includes(i)} style="top:{i * TH}px" onclick={() => show(i, false)} aria-label="Page {i + 1}" aria-current={visiblePages.includes(i) ? 'page' : undefined}>
                  <span class="art" style="aspect-ratio:{aspect(i) || 0.75}"><PageImg src={src(i, 192)} alt="" /></span>
                  <span class="n">{i + 1}</span>
                </button>
              {/each}
            </div>
          </div>
        {/if}
      </aside>
    {/if}

    <div class="stage">
    <div class="viewport" bind:clientWidth={vw} bind:clientHeight={vh}>
      <div class="wrapper" bind:this={wrapperEl}>
        {#if book}
          {@const [l, r] = spread(page)}
          <div class="spread" bind:this={spreadEl}>
            {#each [l, r] as p, side}
              {#if p !== null}
                {@const s = size(p, true)}
                <div class="spread-page" class:left={side === 0 && l !== null && r !== null} class:right={side === 1 && l !== null && r !== null} style="width:{s.w}px;height:{s.h}px">
                  <PageImg src={src(p, px(s.w))} alt="Page {p + 1}" />
                </div>
              {/if}
            {/each}
          </div>
          {#if flip}
            <div class="flip-page" class:back-origin={!flip.forward} class:forward={flip.go && flip.forward} class:backward={flip.go && !flip.forward} style="left:{flip.left}px;top:{flip.top}px;width:{flip.w}px;height:{flip.h}px">
              <div class="face front"><PageImg src={src(flip.leaving, px(flip.w))} alt="" /></div>
              <div class="face back"><PageImg src={src(flip.arriving, px(flip.w))} alt="" /></div>
            </div>
          {/if}
        {:else}
          {@const s = size(page, false)}
          <div class="single" style="width:{s.w}px;height:{s.h}px">
            <PageImg src={src(page, px(s.w))} alt="Page {page + 1}" />
            {@render overlay?.(page, s.w, s.h)}
          </div>
        {/if}
      </div>
    </div>
    {#if !app.zen}
      <div class="label" aria-hidden="true">{indicator}</div>
    {/if}
    </div>
  </div>

  {#if app.zen}
    <div class="hud" class:shown={zenShown}>
      <button type="button" aria-label="Previous page" disabled={atFirst} onclick={() => go(-1)}>‹</button>
      <span>Page {indicator}</span>
      <button type="button" aria-label="Next page" disabled={atLast} onclick={() => go(1)}>›</button>
      <span class="hint">← → turn pages · Esc leaves Zen</span>
      <button type="button" aria-label="Leave Zen" onclick={() => (app.zen = false)}>✕</button>
    </div>
  {/if}
</div>

<style>
  .reader { display: flex; flex-direction: column; height: 100%; min-height: 0; min-width: 0; position: relative; background: var(--fo-bg-canvas); }
  .zoom { display: flex; align-items: center; height: 26px; border-radius: var(--fo-radius-field); background: var(--fo-fill); padding: 0 2px; }
  .zoom button { height: 22px; min-width: 24px; border: 0; border-radius: 5px; background: transparent; color: var(--fo-label); font: var(--fo-text-callout); padding: 0 4px; }
  .zoom button:hover { background: var(--fo-fill-2); }
  .zoom .pct { min-width: 46px; font-variant-numeric: tabular-nums; color: var(--fo-label-2); }
  .goto { display: flex; align-items: center; gap: 6px; font: var(--fo-text-callout); color: var(--fo-label-2); white-space: nowrap; }
  .goto input { width: 42px; height: 22px; border: 1px solid var(--fo-control-border); border-radius: 5px; background: var(--fo-control); color: var(--fo-label); font: var(--fo-text-callout); text-align: center; padding: 0 4px; }
  .main { flex: 1; display: flex; min-height: 0; }
  .side { width: var(--fo-panel-pages); flex: none; display: flex; flex-direction: column; background: var(--fo-bg-sidebar); border-right: 1px solid var(--fo-separator); }
  .side:has(.contents) { width: var(--fo-panel-contents); }
  .tabs { padding: 10px; display: flex; justify-content: center; border-bottom: 1px solid var(--fo-separator); }
  .contents { flex: 1; overflow-y: auto; padding: 6px; }
  .thumbs { flex: 1; overflow-y: auto; }
  .thumb { position: absolute; left: 0; right: 0; height: 176px; border: 0; background: transparent; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px; padding: 8px 0; }
  .thumb .art { display: block; height: 130px; max-width: 120px; background: var(--fo-paper); border-radius: 2px; overflow: hidden; box-shadow: 0 0 0 1px var(--fo-separator), 0 1px 2px var(--fo-fill-2); }
  .thumb.active .art { box-shadow: 0 0 0 3px var(--fo-accent); }
  .thumb .n { font: var(--fo-text-caption); color: var(--fo-label-2); padding: 0 6px; border-radius: 8px; }
  .thumb.active .n { background: var(--fo-accent); color: var(--fo-on-accent); }
  .stage { flex: 1; position: relative; display: flex; min-width: 0; }
  .viewport { flex: 1; overflow: auto; display: flex; padding: 28px 28px 44px; min-width: 0; }
  .zen .viewport { padding: 20px; }
  .wrapper { position: relative; margin: auto; }
  .single { box-shadow: var(--fo-shadow-page); border-radius: 2px; background: var(--fo-paper); position: relative; }
  .spread { display: flex; justify-content: center; perspective: 2400px; }
  .spread-page { position: relative; background: var(--fo-paper); box-shadow: var(--fo-shadow-page); overflow: hidden; border-radius: 2px; }
  .spread-page.left { border-radius: 2px 0 0 2px; }
  .spread-page.right { border-radius: 0 2px 2px 0; }
  .spread-page.left::after, .spread-page.right::after { content: ''; position: absolute; top: 0; bottom: 0; width: 18px; pointer-events: none; }
  .spread-page.left::after { right: 0; background: linear-gradient(90deg, transparent, var(--fo-fill-2)); }
  .spread-page.right::after { left: 0; background: linear-gradient(270deg, transparent, var(--fo-fill-2)); }
  .flip-page { position: absolute; transform-style: preserve-3d; transition: transform 0.6s cubic-bezier(0.4, 0.05, 0.3, 1); transform-origin: left center; z-index: 5; pointer-events: none; }
  .flip-page.forward { transform: rotateY(-180deg); }
  .flip-page.back-origin { transform-origin: right center; }
  .flip-page.backward { transform: rotateY(180deg); }
  .face { position: absolute; inset: 0; backface-visibility: hidden; -webkit-backface-visibility: hidden; background: var(--fo-paper); overflow: hidden; }
  .face.back { transform: rotateY(180deg); }
  .label { position: absolute; bottom: 12px; left: 50%; transform: translateX(-50%); padding: 3px 10px; border-radius: 10px; background: var(--fo-bg-popover); box-shadow: var(--fo-shadow-popover); font: var(--fo-text-caption); color: var(--fo-label-2); white-space: nowrap; pointer-events: none; opacity: 0.9; }
  .hud { position: absolute; left: 50%; bottom: 24px; transform: translateX(-50%); display: flex; align-items: center; gap: 10px; padding: 6px 10px; border-radius: 10px; background: var(--fo-bg-popover); box-shadow: var(--fo-shadow-popover); color: var(--fo-label); font: var(--fo-text-callout); opacity: 0; transition: opacity 0.3s; }
  .hud.shown, .hud:hover { opacity: 1; }
  .hud button { border: 0; background: transparent; color: var(--fo-label); font-size: 16px; padding: 0 6px; }
  .hud button:disabled { opacity: 0.3; }
  .hud .hint { color: var(--fo-label-2); font: var(--fo-text-caption); }
  @media (prefers-reduced-motion: reduce) { .flip-page { transition: none; } }
</style>
