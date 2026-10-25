<script lang="ts" generics="T extends string">
  // A pop-up button (Components board: "300 dpi ⌃⌄") over a native select,
  // so the menu, keyboard and screen readers behave as on macOS.
  let {
    options,
    value = $bindable(),
    label,
    disabled = false,
    plain = false,
    onchange,
  }: {
    options: { value: T; label: string }[];
    value: T;
    label: string;
    disabled?: boolean;
    /** Borderless, as in toolbars ("Date Added ⌄"). */
    plain?: boolean;
    onchange?: (v: T) => void;
  } = $props();
  const current = $derived(options.find((o) => o.value === value)?.label ?? '');
</script>

<span class="fo-popup" class:plain class:disabled>
  <span class="text">{current}</span>
  <span class="arrows" aria-hidden="true">{plain ? '⌄' : '⌃⌄'}</span>
  <select aria-label={label} {disabled} bind:value onchange={() => onchange?.(value)}>
    {#each options as o (o.value)}<option value={o.value}>{o.label}</option>{/each}
  </select>
</span>

<style>
  .fo-popup {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 14px;
    height: 22px;
    padding: 0 6px 0 10px;
    border: 1px solid var(--fo-control-border);
    border-radius: var(--fo-radius-button);
    background: var(--fo-control);
    color: var(--fo-label);
    font: var(--fo-text-callout);
    white-space: nowrap;
  }
  .plain { border-color: transparent; background: transparent; gap: 4px; height: 26px; padding: 0 8px 0 10px; }
  .plain:hover { background: var(--fo-fill); }
  .arrows { color: var(--fo-label-2); font-size: 10px; }
  .disabled { opacity: 0.45; }
  select { position: absolute; inset: 0; opacity: 0; width: 100%; font: inherit; }
  .fo-popup:focus-within { outline: 2px solid var(--fo-accent); outline-offset: 1px; }
</style>
