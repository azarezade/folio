<script lang="ts">
  import type { Snippet } from 'svelte';
  import { activate } from './activate';

  let {
    kind,
    title,
    subtitle,
    cover,
    state: cardState,
    badge,
    selected = false,
    onclick,
    oncontextmenu,
    preview,
  }: {
    kind: 'notebook' | 'book' | 'pdf';
    title: string;
    subtitle?: string;
    /** Image URL (book cover or notebook thumbnail). */
    cover?: string | null;
    state?: 'failed';
    /** Text on the corner badge, e.g. "Can't open". */
    badge?: string;
    selected?: boolean;
    onclick?: (e: MouseEvent) => void;
    oncontextmenu?: (e: MouseEvent) => void;
    /** Custom artwork instead of `cover`. */
    preview?: Snippet;
  } = $props();

  let broken = $state(false);
  // a steady colour per title for books without a cover
  const tint = $derived(`var(--fo-cover-${(Array.from(title).reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7) % 6) + 1})`);
  const paper = $derived(kind === 'notebook' || kind === 'pdf');
</script>

<div role="button" tabindex="0" aria-pressed={selected} class="fo-item-card" class:selected class:failed={cardState === 'failed'} {onclick} {oncontextmenu} onkeydown={activate}>
  <span class="art" class:paper style={!paper && !(cover && !broken) ? `background:${tint}` : undefined}>
    {#if preview}
      {@render preview()}
    {:else if cover && !broken}
      <img src={cover} alt="" loading="lazy" onerror={() => (broken = true)} />
    {:else if !paper}
      <span class="cover-title">{title}</span>
    {/if}
    {#if badge}<span class="badge">{badge}</span>{/if}
  </span>
  <span class="name" title={title}>{title}</span>
  {#if subtitle}<span class="subtitle">{subtitle}</span>{/if}
</div>

<style>
  .fo-item-card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--fo-label);
    text-align: left;
    cursor: default;
    min-width: 0;
    outline: none;
  }
  .art {
    position: relative;
    display: block;
    aspect-ratio: 3 / 4;
    border-radius: 5px;
    overflow: hidden;
    box-shadow: var(--fo-shadow-page);
    transition: box-shadow var(--fo-dur-hover) var(--fo-ease);
  }
  .paper { background: var(--fo-paper); }
  .selected .art { box-shadow: 0 0 0 3px var(--fo-accent); }
  .fo-item-card:focus-visible .art { box-shadow: 0 0 0 3px var(--fo-accent); }
  .fo-item-card:focus-visible { outline: none; }
  img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .paper img { object-fit: contain; }
  .cover-title { position: absolute; inset: 0; padding: 10px; color: var(--fo-cover-text); font: 400 13px/1.3 var(--fo-font-reading); overflow: hidden; }
  .failed .cover-title, .failed img { opacity: 0.55; }
  .badge {
    position: absolute;
    top: 6px;
    right: 6px;
    height: 18px;
    padding: 0 6px;
    border-radius: 9px;
    background: var(--fo-danger-solid);
    color: var(--fo-on-accent);
    font: 600 10px/18px var(--fo-font);
  }
  .name, .subtitle { display: block; min-width: 0; max-width: 100%; }
  .name { font: 500 11px/14px var(--fo-font); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .failed .name { color: var(--fo-danger); }
  .subtitle { font: var(--fo-text-caption); color: var(--fo-label-2); margin-top: -4px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
