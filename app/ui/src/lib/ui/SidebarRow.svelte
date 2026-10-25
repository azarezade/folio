<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon, { type IconName } from './Icon.svelte';
  import { activate } from './activate';

  let {
    icon,
    label,
    count,
    sub,
    tone = 'normal',
    selected = false,
    indent = false,
    dimmed = false,
    onclick,
    trailing,
  }: {
    icon: IconName;
    label: string;
    count?: number | string;
    /** A second line (device status), which makes the row 40 px tall. */
    sub?: string;
    /** `warning` colours the icon and the second line (a held Kindle). */
    tone?: 'normal' | 'warning';
    selected?: boolean;
    indent?: boolean;
    /** Not available (a disconnected Kindle). */
    dimmed?: boolean;
    onclick?: () => void;
    trailing?: Snippet;
  } = $props();

  const iconColor = $derived(tone === 'warning' ? 'var(--fo-warning)' : dimmed ? 'var(--fo-label-2)' : 'var(--fo-accent)');
</script>

<div role="button" tabindex="0" onkeydown={activate} class="fo-sidebar-row" class:selected class:two={!!sub} class:indent class:dimmed aria-current={selected ? 'page' : undefined} {onclick}>
  <Icon name={icon} color={iconColor} />
  {#if sub}
    <span class="text">
      <span class="label">{label}</span>
      <span class="sub" class:warn={tone === 'warning'}>{sub}</span>
    </span>
  {:else}
    <span class="label grow">{label}</span>
  {/if}
  {#if count !== undefined && count !== ''}<span class="count">{count}</span>{/if}
  {@render trailing?.()}
</div>

<style>
  .fo-sidebar-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: var(--fo-row-list);
    padding: 0 8px;
    border: 0;
    border-radius: var(--fo-radius-button);
    background: transparent;
    color: var(--fo-label);
    font: var(--fo-text-body);
    text-align: left;
    cursor: default;
    min-width: 0;
  }
  .fo-sidebar-row:hover:not(.selected) { background: var(--fo-fill); }
  .fo-sidebar-row:focus-visible { outline: 2px solid var(--fo-accent); outline-offset: -2px; }
  .fo-sidebar-row:focus:not(:focus-visible) { outline: none; }
  .label { min-width: 0; }
  .selected { background: var(--fo-selection); font-weight: 600; }
  .two { height: auto; min-height: var(--fo-row-settings); padding: 4px 8px; }
  .indent { padding-left: 32px; }
  .grow { flex: 1; }
  .label { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .dimmed .label { color: var(--fo-label-2); }
  .text { flex: 1; display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .sub { font: var(--fo-text-caption); font-weight: 400; color: var(--fo-label-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sub.warn { color: var(--fo-warning); }
  .count { font: var(--fo-text-callout); font-weight: 400; color: var(--fo-label-2); }
</style>
