<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon, { type IconName } from './Icon.svelte';

  let { icon, title, body, actions }: { icon: IconName; title: string; body?: string; actions?: Snippet } = $props();
</script>

<div class="fo-empty-state">
  <Icon name={icon} size={32} weight={1} color="var(--fo-label-2)" />
  <span class="title">{title}</span>
  {#if body}<span class="body">{body}</span>{/if}
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</div>

<style>
  .fo-empty-state { display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 10px 0; text-align: center; color: var(--fo-label); }
  .title { font: var(--fo-text-headline); }
  .body { font: var(--fo-text-callout); color: var(--fo-label-2); max-width: 320px; }
  .actions { display: flex; gap: var(--fo-space-2); margin-top: var(--fo-space-2); }
</style>
