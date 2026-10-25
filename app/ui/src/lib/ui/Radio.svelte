<script lang="ts" generics="T extends string">
  // A radio group (Components board): accent dot, labels after.
  let {
    options,
    value = $bindable(),
    label,
    onchange,
  }: { options: { value: T; label: string; disabled?: boolean }[]; value: T; label: string; onchange?: (v: T) => void } = $props();
</script>

<div class="fo-radio" role="radiogroup" aria-label={label}>
  {#each options as o (o.value)}
    <label class:disabled={o.disabled}>
      <input type="radio" name={label} value={o.value} disabled={o.disabled} checked={value === o.value} onchange={() => { value = o.value; onchange?.(o.value); }} />
      <span class="dot" aria-hidden="true"></span>
      <span>{o.label}</span>
    </label>
  {/each}
</div>

<style>
  .fo-radio { display: flex; flex-direction: column; gap: 6px; }
  label { display: inline-flex; align-items: center; gap: 8px; font: var(--fo-text-body); color: var(--fo-label); }
  .disabled { opacity: 0.45; }
  input { position: absolute; opacity: 0; width: 0; height: 0; }
  .dot { width: 14px; height: 14px; flex: none; border-radius: 50%; border: 1px solid var(--fo-control-border); background: var(--fo-control); display: flex; align-items: center; justify-content: center; }
  input:checked + .dot { border: 0; background: var(--fo-accent); }
  input:checked + .dot::after { content: ''; width: 6px; height: 6px; border-radius: 50%; background: var(--fo-on-accent); }
  input:focus-visible + .dot { outline: 2px solid var(--fo-accent); outline-offset: 2px; }
</style>
