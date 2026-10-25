<script lang="ts" generics="T extends string">
  import Icon, { type IconName } from './Icon.svelte';

  let {
    options,
    value = $bindable(),
    label,
    onchange,
  }: { options: { value: T; label: string; icon?: IconName; disabled?: boolean }[]; value: T; label: string; onchange?: (v: T) => void } = $props();

  function pick(v: T) {
    value = v;
    onchange?.(v);
  }

  function key(e: KeyboardEvent) {
    const i = options.findIndex((o) => o.value === value);
    const step = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    for (let j = 1; j <= options.length; j++) {
      const o = options[(i + step * j + options.length) % options.length];
      if (!o.disabled) return pick(o.value);
    }
  }
</script>

<div class="fo-segmented" role="group" aria-label={label}>
  {#each options as o (o.value)}
    <button type="button" aria-pressed={o.value === value} disabled={o.disabled} tabindex={o.value === value ? 0 : -1} onclick={() => pick(o.value)} onkeydown={key} class:icon={!!o.icon} aria-label={o.icon ? o.label : undefined} title={o.icon ? o.label : undefined}>{#if o.icon}<Icon name={o.icon} size={15} />{:else}{o.label}{/if}</button>
  {/each}
</div>

<style>
  .fo-segmented {
    display: inline-flex;
    height: 26px;
    padding: 2px;
    box-sizing: border-box;
    border-radius: var(--fo-radius-field);
    background: var(--fo-fill);
  }
  button {
    padding: 0 12px;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--fo-label-2);
    font: 500 12px/1 var(--fo-font);
    cursor: default;
    transition: background var(--fo-dur-hover) var(--fo-ease), color var(--fo-dur-hover) var(--fo-ease);
  }
  button.icon { width: 32px; padding: 0; display: flex; align-items: center; justify-content: center; }
  button[aria-pressed='true'] { background: var(--fo-segment-on); box-shadow: var(--fo-shadow-segment); color: var(--fo-label); }
  button:disabled { opacity: 0.45; }
  button:focus-visible { outline: 2px solid var(--fo-accent); outline-offset: 1px; }
</style>
