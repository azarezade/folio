<script lang="ts">
  // A window-attached sheet (Export, Send, Log): scrim, a panel hanging from
  // the top edge, Esc and the scrim close it.
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';

  let {
    label,
    width = 520,
    onclose,
    children,
  }: { label: string; width?: number; onclose: () => void; children: Snippet } = $props();
  let panel: HTMLElement;
  onMount(() => {
    const prev = document.activeElement as HTMLElement | null;
    panel.querySelector<HTMLElement>('[autofocus], input, select, button')?.focus();
    return () => prev?.focus?.();
  });
</script>

<div class="scrim" role="presentation" onclick={onclose}></div>
<div
  class="sheet"
  role="dialog"
  tabindex="-1"
  aria-modal="true"
  aria-label={label}
  style:width="min({width}px, calc(100vw - 32px))"
  bind:this={panel}
  onkeydown={(e) => e.key === 'Escape' && (e.stopPropagation(), onclose())}
>
  {@render children()}
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 90; background: var(--fo-scrim); animation: fade var(--fo-dur-sheet) var(--fo-ease); }
  .sheet {
    position: fixed;
    top: 0;
    left: 50%;
    transform: translateX(-50%);
    z-index: 91;
    max-height: calc(100vh - 40px);
    overflow-y: auto;
    border-radius: 0 0 var(--fo-radius-window) var(--fo-radius-window);
    background: var(--fo-bg-popover);
    box-shadow: var(--fo-shadow-popover);
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    animation: drop var(--fo-dur-sheet) var(--fo-ease);
  }
  .sheet :global(h2) { font: 700 13px/16px var(--fo-font); }
  @keyframes drop { from { transform: translate(-50%, -100%); } }
  @keyframes fade { from { opacity: 0; } }
</style>
