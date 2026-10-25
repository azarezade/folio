<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';

  let {
    tone = 'info',
    title,
    children,
    actions,
  }: { tone?: 'warning' | 'danger' | 'info'; title?: string; children?: Snippet; actions?: Snippet } = $props();
</script>

<div class="fo-banner {tone}" role={tone === 'info' ? 'status' : 'alert'}>
  <span class="icon"><Icon name={tone === 'info' ? 'info' : 'warning'} /></span>
  <span class="text">{#if title}<strong>{title}</strong>{/if} {@render children?.()}</span>
  {#if actions}<span class="actions">{@render actions()}</span>{/if}
</div>

<style>
  .fo-banner {
    padding: 10px 12px;
    border-radius: 8px;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    font: var(--fo-text-callout);
    line-height: 1.4;
    color: var(--fo-label);
    background: var(--fo-fill);
  }
  .icon { color: var(--fo-accent); display: flex; }
  .warning { background: var(--fo-warning-fill); }
  .warning .icon { color: var(--fo-warning); }
  .danger { background: var(--fo-danger-fill); }
  .danger .icon { color: var(--fo-danger); }
  .text { flex: 1; }
  strong { font-weight: 600; }
  .actions { display: flex; gap: 8px; align-self: center; }
</style>
