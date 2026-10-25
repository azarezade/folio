<script lang="ts">
  export type ChipState = 'queued' | 'running' | 'done' | 'failed' | 'cancelled';
  let { state, label }: { state: ChipState; label?: string } = $props();
  const text = $derived(label ?? state[0].toUpperCase() + state.slice(1));
</script>

<span class="fo-status-chip {state}">{text}</span>

<style>
  .fo-status-chip {
    display: inline-flex;
    align-items: center;
    height: 18px;
    padding: 0 8px;
    border-radius: 9px;
    font: 600 11px/1 var(--fo-font);
    white-space: nowrap;
    background: var(--fo-fill);
    color: var(--fo-label-2);
  }
  .running { background: var(--fo-fill-2); color: var(--fo-label); }
  .done { background: var(--fo-success-fill); color: var(--fo-success); }
  .failed { background: var(--fo-danger-fill); color: var(--fo-danger); }
</style>
