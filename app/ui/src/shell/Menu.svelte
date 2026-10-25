<script lang="ts">
  // A context menu at the pointer: arrows move, Enter picks, Esc closes.
  import { onMount } from 'svelte';

  export interface MenuItem {
    label?: string;
    run?: () => void;
    danger?: boolean;
    disabled?: boolean;
    separator?: boolean;
  }
  let { x, y, items, onclose }: { x: number; y: number; items: MenuItem[]; onclose: () => void } = $props();
  let el: HTMLDivElement;
  let left = $state(0);
  let top = $state(0);

  onMount(() => {
    const r = el.getBoundingClientRect();
    left = Math.min(x, innerWidth - r.width - 8);
    top = Math.min(y, innerHeight - r.height - 8);
    el.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
  });
  function key(e: KeyboardEvent) {
    const bs = [...el.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
    const i = bs.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === 'ArrowDown') bs[(i + 1) % bs.length]?.focus();
    else if (e.key === 'ArrowUp') bs[(i - 1 + bs.length) % bs.length]?.focus();
    else if (e.key === 'Escape' || e.key === 'Tab') onclose();
    else return;
    e.preventDefault();
  }
</script>

<div class="scrim" role="presentation" onclick={onclose} oncontextmenu={(e) => (e.preventDefault(), onclose())}></div>
<div class="menu" role="menu" tabindex="-1" bind:this={el} style:left="{left || x}px" style:top="{top || y}px" onkeydown={key}>
  {#each items as it}
    {#if it.separator}
      <div class="sep" role="separator"></div>
    {:else}
      <button type="button" role="menuitem" class:danger={it.danger} disabled={it.disabled} onclick={() => { onclose(); it.run?.(); }}>{it.label}</button>
    {/if}
  {/each}
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 70; }
  .menu { position: fixed; z-index: 71; min-width: 200px; padding: 5px; border-radius: 8px; background: var(--fo-bg-popover); box-shadow: var(--fo-shadow-popover); display: flex; flex-direction: column; }
  button { height: 24px; padding: 0 10px; border: 0; border-radius: 5px; background: transparent; color: var(--fo-label); font: var(--fo-text-body); text-align: left; }
  button:hover:not(:disabled), button:focus-visible { background: var(--fo-accent); color: var(--fo-on-accent); outline: none; }
  button:disabled { color: var(--fo-label-3); }
  .danger { color: var(--fo-danger-text); }
  .sep { height: 1px; margin: 4px 6px; background: var(--fo-separator); }
</style>
