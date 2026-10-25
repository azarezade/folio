<script lang="ts">
  import type { Snippet } from 'svelte';
  let {
    checked = $bindable(false),
    disabled = false,
    onchange,
    children,
  }: { checked?: boolean; disabled?: boolean; onchange?: (v: boolean) => void; children: Snippet } = $props();
</script>

<label class="fo-checkbox" class:disabled>
  <input type="checkbox" bind:checked {disabled} onchange={() => onchange?.(checked)} />
  <span class="box" aria-hidden="true">
    <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="var(--fo-on-accent)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M2 5.2 4.2 7.4 8 2.8" /></svg>
  </span>
  <span>{@render children()}</span>
</label>

<style>
  .fo-checkbox { display: inline-flex; align-items: center; gap: 8px; font: var(--fo-text-body); color: var(--fo-label); }
  .disabled { opacity: 0.45; }
  input { position: absolute; opacity: 0; width: 0; height: 0; }
  .box { width: 14px; height: 14px; flex: none; border-radius: var(--fo-radius-check); border: 1px solid var(--fo-control-border); background: var(--fo-control); display: flex; align-items: center; justify-content: center; }
  .box svg { visibility: hidden; }
  input:checked + .box { border: 0; background: var(--fo-accent); }
  input:checked + .box svg { visibility: visible; }
  input:focus-visible + .box { outline: 2px solid var(--fo-accent); outline-offset: 2px; }
</style>
