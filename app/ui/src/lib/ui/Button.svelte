<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    variant = 'secondary',
    type = 'button',
    disabled = false,
    title,
    onclick,
    children,
  }: {
    variant?: 'primary' | 'secondary' | 'danger' | 'plain';
    type?: 'button' | 'submit';
    disabled?: boolean;
    title?: string;
    onclick?: (e: MouseEvent) => void;
    children: Snippet;
  } = $props();
</script>

<button class="fo-button {variant}" {type} {disabled} {title} {onclick}>{@render children()}</button>

<style>
  .fo-button {
    height: 24px;
    padding: 0 12px;
    border-radius: var(--fo-radius-button);
    font: var(--fo-text-body);
    white-space: nowrap;
    cursor: default;
    transition: filter var(--fo-dur-hover) var(--fo-ease), background var(--fo-dur-hover) var(--fo-ease);
  }
  .fo-button:focus-visible { outline: 2px solid var(--fo-accent); outline-offset: 2px; }
  .fo-button:disabled { opacity: 0.45; }
  .primary { border: 0; background: var(--fo-accent); color: var(--fo-on-accent); font-weight: 500; }
  .primary:hover:not(:disabled) { filter: brightness(1.08); }
  .primary:active:not(:disabled) { filter: brightness(0.92); }
  .secondary, .danger { border: 1px solid var(--fo-control-border); background: var(--fo-control); color: var(--fo-label); }
  .danger { color: var(--fo-danger-text); }
  .secondary:hover:not(:disabled), .danger:hover:not(:disabled) { background: var(--fo-fill-2); }
  .plain { border: 0; background: transparent; padding: 0 6px; color: var(--fo-accent-text); }
  .plain:hover:not(:disabled) { background: var(--fo-fill); }
</style>
