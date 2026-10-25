<script lang="ts">
  // Bottom-centre toasts: 4 s with one action; errors stay until dismissed.
  import { app } from '../lib/state.svelte';
  import { Toast } from '../lib/ui';

  const tone = (k: string) => (k === 'ok' ? 'success' : k === 'error' ? 'danger' : 'info') as 'success' | 'danger' | 'info';
</script>

<div class="toasts" aria-live="polite">
  {#each app.toasts as t (t.id)}
    <div class="one">
      <Toast
        tone={tone(t.kind)}
        text={t.details ? `${t.text} — ${t.details}` : t.text}
        action={t.action}
        ondismiss={() => app.dismiss(t.id)}
      />
    </div>
  {/each}
</div>

<style>
  .toasts { position: fixed; left: 50%; bottom: calc(var(--fo-statusbar-height) + 16px); transform: translateX(-50%); z-index: 80; display: flex; flex-direction: column; gap: 8px; width: min(460px, calc(100vw - 32px)); pointer-events: none; }
  .one { pointer-events: auto; }
</style>
