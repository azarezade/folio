<script lang="ts">
  // The 52 px window toolbar: optional sidebar/back button, title over a
  // subtitle, then the screen's own controls on the right.
  import type { Snippet } from 'svelte';
  import { ToolbarButton } from '../lib/ui';
  import { app } from '../lib/state.svelte';

  let {
    title,
    subtitle,
    back,
    sidebarToggle = true,
    children,
  }: { title: string; subtitle?: string; back?: () => void; sidebarToggle?: boolean; children?: Snippet } = $props();
</script>

<header class="toolbar" data-tauri-drag-region>
  {#if back}
    <ToolbarButton icon="back" label="Back to Library" onclick={back} />
  {:else if sidebarToggle}
    <ToolbarButton icon="sidebar" label={app.sidebarHidden ? 'Show sidebar' : 'Hide sidebar'} onclick={() => (app.sidebarHidden = !app.sidebarHidden)} />
  {/if}
  <div class="titles" data-tauri-drag-region>
    <h1 title={title}>{title}</h1>
    {#if subtitle}<p>{subtitle}</p>{/if}
  </div>
  <div class="spacer" data-tauri-drag-region></div>
  {@render children?.()}
</header>

<style>
  .toolbar {
    height: var(--fo-toolbar-height);
    flex: none;
    padding: 0 12px 0 10px;
    display: flex;
    align-items: center;
    gap: 8px;
    border-bottom: 1px solid var(--fo-separator);
    background: var(--fo-bg-window);
    min-width: 0;
  }
  .titles { display: flex; flex-direction: column; padding-left: 4px; min-width: 0; flex-shrink: 1; }
  h1 { font: var(--fo-text-toolbar-title); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  p { font: var(--fo-text-caption); color: var(--fo-label-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .spacer { flex: 1; min-width: 8px; }
  :global(.toolbar-sep) { width: 1px; height: 18px; background: var(--fo-separator); margin: 0 2px; flex: none; }
</style>
