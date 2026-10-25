<script lang="ts">
  let {
    value,
    state = 'active',
    size = 'regular',
    label,
  }: {
    /** 0…1, or null for indeterminate. */
    value: number | null;
    state?: 'active' | 'failed' | 'done';
    size?: 'regular' | 'small';
    label?: string;
  } = $props();
  const pct = $derived(value === null ? null : Math.round(Math.min(1, Math.max(0, value)) * 100));
</script>

<span
  class="fo-progress {state} {size}"
  class:indeterminate={pct === null}
  role="progressbar"
  aria-label={label}
  aria-valuemin="0"
  aria-valuemax="100"
  aria-valuenow={pct ?? undefined}
>
  <span class="bar" style:width={pct === null ? '30%' : `${pct}%`}></span>
</span>

<style>
  .fo-progress { display: flex; height: 6px; border-radius: 3px; background: var(--fo-fill-2); overflow: hidden; }
  .small { height: 4px; border-radius: 2px; }
  .bar { border-radius: inherit; background: var(--fo-accent); transition: width var(--fo-dur-popover) var(--fo-ease); }
  .failed .bar { background: var(--fo-danger); }
  .done .bar { background: var(--fo-success-solid); }
  .indeterminate .bar { animation: slide 1.2s var(--fo-ease) infinite; }
  @keyframes slide { from { transform: translateX(-100%); } to { transform: translateX(340%); } }
  @media (prefers-reduced-motion: reduce) { .indeterminate .bar { animation: none; width: 100% !important; opacity: 0.4; } }
</style>
