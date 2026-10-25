<script lang="ts">
  import Icon, { type IconName } from './Icon.svelte';

  let {
    icon,
    label,
    showLabel = false,
    active = false,
    disabled = false,
    onclick,
  }: { icon: IconName; label: string; showLabel?: boolean; active?: boolean; disabled?: boolean; onclick?: () => void } = $props();
</script>

<button class="fo-toolbar-button" class:with-label={showLabel} class:active {disabled} aria-label={showLabel ? undefined : label} title={label} aria-pressed={active || undefined} {onclick}>
  <Icon name={icon} size={showLabel ? 16 : 17} color={showLabel ? 'var(--fo-label-2)' : 'currentColor'} />
  {#if showLabel}<span>{label}</span>{/if}
</button>

<style>
  .fo-toolbar-button {
    height: 28px;
    min-width: 30px;
    padding: 0;
    border: 0;
    border-radius: var(--fo-radius-button);
    background: transparent;
    color: var(--fo-label-2);
    font: var(--fo-text-body);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    cursor: default;
    transition: background var(--fo-dur-hover) var(--fo-ease);
  }
  .with-label { padding: 0 10px 0 8px; color: var(--fo-label); }
  .fo-toolbar-button:hover:not(:disabled), .active { background: var(--fo-fill); color: var(--fo-label); }
  .fo-toolbar-button:disabled { opacity: 0.45; }
  .fo-toolbar-button:focus-visible { outline: 2px solid var(--fo-accent); outline-offset: 1px; }
</style>
