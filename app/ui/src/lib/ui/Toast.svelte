<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';

  let {
    tone = 'info',
    text,
    action,
    persistent,
    ondismiss,
  }: {
    tone?: 'success' | 'danger' | 'info';
    text: string;
    action?: { label: string; run: () => void };
    /** Stays until dismissed. Errors always stay; others go after 4 s. */
    persistent?: boolean;
    ondismiss?: () => void;
  } = $props();

  onMount(() => {
    if (persistent || tone === 'danger' || !ondismiss) return;
    const ms = Number(getComputedStyle(document.documentElement).getPropertyValue('--fo-toast-ms')) || 4000;
    const t = setTimeout(() => ondismiss?.(), ms);
    return () => clearTimeout(t);
  });
</script>

<div class="fo-toast" role={tone === 'danger' ? 'alert' : 'status'}>
  <span class="mark {tone}">
    {#if tone === 'success'}<Icon name="check" size={12} weight={1.8} color="var(--fo-on-accent)" />
    {:else if tone === 'danger'}<span class="bang">!</span>
    {:else}<span class="bang">i</span>{/if}
  </span>
  <span class="text">{text}</span>
  {#if action}<button type="button" class="action" onclick={action.run}>{action.label}</button>{/if}
  {#if ondismiss && (persistent || tone === 'danger')}
    <button type="button" class="close" aria-label="Dismiss" onclick={ondismiss}><Icon name="close" size={10} weight={1.8} /></button>
  {/if}
</div>

<style>
  .fo-toast {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 40px;
    padding: 0 8px 0 12px;
    box-sizing: border-box;
    border-radius: var(--fo-radius-window);
    background: var(--fo-bg-popover);
    box-shadow: var(--fo-shadow-popover);
    color: var(--fo-label);
    animation: rise var(--fo-dur-popover) var(--fo-ease);
  }
  @keyframes rise { from { opacity: 0; transform: translateY(6px); } }
  .mark { width: 18px; height: 18px; flex: none; border-radius: 50%; display: flex; align-items: center; justify-content: center; background: var(--fo-accent); }
  .mark.success { background: var(--fo-success-solid); }
  .mark.danger { background: var(--fo-danger-solid); }
  .bang { color: var(--fo-on-accent); font: 700 11px/1 var(--fo-font); }
  .text { flex: 1; font: var(--fo-text-callout); padding: 8px 0; }
  .action { height: 24px; padding: 0 8px; border: 0; border-radius: var(--fo-radius-button); background: var(--fo-fill); color: var(--fo-accent-text); font: var(--fo-text-callout); cursor: default; }
  .action:hover { background: var(--fo-fill-2); }
  .close { width: 20px; height: 20px; border: 0; border-radius: 50%; background: transparent; color: var(--fo-label-2); display: flex; align-items: center; justify-content: center; padding: 0; cursor: default; }
  .close:hover { background: var(--fo-fill); }
  @media (prefers-reduced-motion: reduce) { .fo-toast { animation-name: fade; } @keyframes fade { from { opacity: 0; } } }
</style>
