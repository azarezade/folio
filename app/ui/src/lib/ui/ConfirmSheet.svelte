<script lang="ts">
  import { onMount } from 'svelte';
  import Button from './Button.svelte';

  let {
    title,
    body,
    confirmLabel,
    cancelLabel = 'Cancel',
    destructive = false,
    oncancel,
    onconfirm,
  }: {
    title: string;
    body: string;
    confirmLabel: string;
    cancelLabel?: string;
    /** Destructive: Cancel is the default button and the action is red. */
    destructive?: boolean;
    oncancel: () => void;
    onconfirm: () => void;
  } = $props();

  let sheet: HTMLDivElement;
  onMount(() => {
    const prev = document.activeElement as HTMLElement | null;
    sheet.querySelector<HTMLButtonElement>('[data-default] button')?.focus();
    return () => prev?.focus?.();
  });

  function key(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      oncancel();
    } else if (e.key === 'Tab') {
      // keep focus inside the sheet
      const items = [...sheet.querySelectorAll<HTMLElement>('button')];
      const i = items.indexOf(document.activeElement as HTMLElement);
      e.preventDefault();
      items[(i + (e.shiftKey ? -1 : 1) + items.length) % items.length]?.focus();
    }
  }
</script>

<div class="fo-scrim" role="presentation" onclick={oncancel}></div>
<div class="fo-confirm-sheet" role="alertdialog" aria-modal="true" aria-labelledby="fo-sheet-title" aria-describedby="fo-sheet-body" tabindex="-1" bind:this={sheet} onkeydown={key}>
  <span id="fo-sheet-title" class="title">{title}</span>
  <span id="fo-sheet-body" class="body">{body}</span>
  <div class="buttons">
    {#if destructive}
      <span data-default style="display:contents"><Button variant="primary" onclick={oncancel}>{cancelLabel}</Button></span>
      <Button variant="danger" onclick={onconfirm}>{confirmLabel}</Button>
    {:else}
      <Button variant="secondary" onclick={oncancel}>{cancelLabel}</Button>
      <span data-default style="display:contents"><Button variant="primary" onclick={onconfirm}>{confirmLabel}</Button></span>
    {/if}
  </div>
</div>

<style>
  .fo-scrim { position: fixed; inset: 0; background: var(--fo-scrim); z-index: 90; animation: fade var(--fo-dur-sheet) var(--fo-ease); }
  .fo-confirm-sheet {
    position: fixed;
    top: 0;
    left: 50%;
    transform: translateX(-50%);
    width: min(420px, calc(100vw - 32px));
    box-sizing: border-box;
    z-index: 91;
    border-radius: 0 0 var(--fo-radius-window) var(--fo-radius-window);
    background: var(--fo-bg-popover);
    box-shadow: var(--fo-shadow-popover);
    padding: var(--fo-space-4);
    display: flex;
    flex-direction: column;
    gap: var(--fo-space-2);
    color: var(--fo-label);
    animation: drop var(--fo-dur-sheet) var(--fo-ease);
    outline: none;
  }
  .title { font: 700 13px/16px var(--fo-font); }
  .body { font: var(--fo-text-callout); line-height: 1.45; color: var(--fo-label-2); }
  .buttons { display: flex; justify-content: flex-end; gap: var(--fo-space-2); padding-top: var(--fo-space-1); }
  @keyframes drop { from { transform: translate(-50%, -100%); } }
  @keyframes fade { from { opacity: 0; } }
</style>
