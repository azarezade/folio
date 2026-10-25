<script lang="ts">
  let {
    checked = $bindable(false),
    label,
    disabled = false,
    onchange,
  }: { checked?: boolean; label: string; disabled?: boolean; onchange?: (v: boolean) => void } = $props();

  function toggle() {
    if (disabled) return;
    checked = !checked;
    onchange?.(checked);
  }
</script>

<button type="button" class="fo-switch" role="switch" aria-checked={checked} aria-label={label} {disabled} onclick={toggle}>
  <span class="knob"></span>
</button>

<style>
  .fo-switch {
    width: 32px;
    height: 18px;
    flex: none;
    border: 0;
    border-radius: 9px;
    padding: 0 2px;
    box-sizing: border-box;
    background: var(--fo-fill-2);
    display: inline-flex;
    align-items: center;
    cursor: default;
    transition: background var(--fo-dur-hover) var(--fo-ease);
  }
  .fo-switch[aria-checked='true'] { background: var(--fo-accent); }
  .fo-switch:disabled { opacity: 0.45; }
  .fo-switch:focus-visible { outline: 2px solid var(--fo-accent); outline-offset: 2px; }
  .knob {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--fo-knob);
    box-shadow: var(--fo-shadow-knob);
    transition: transform var(--fo-dur-hover) var(--fo-ease);
  }
  .fo-switch[aria-checked='true'] .knob { transform: translateX(14px); }
</style>
