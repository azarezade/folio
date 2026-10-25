<script lang="ts">
  // Export progress (Reader board): a sheet with the bar; it can carry on in
  // the background (Activity has it then).
  import { app } from '../lib/state.svelte';
  import { call } from '../lib/api';
  import { Button, Progress } from '../lib/ui';
  import Sheet from './Sheet.svelte';

  const job = $derived(app.exportJob != null ? app.jobs[app.exportJob] : null);
  const frac = $derived(job?.total ? job.done / job.total : null);
</script>

{#if job}
  <Sheet label="Exporting" width={460} onclose={() => (app.exportJob = null)}>
    <h2>{job.title.replace(/^Export /, 'Exporting ')}</h2>
    <Progress value={frac} label="Export progress" />
    <div class="line"><span>{job.total ? `Rendering page ${job.done} of ${job.total}` : 'Starting…'}</span><span>{job.message}</span></div>
    <div class="buttons">
      <Button onclick={() => call('cancel_job', { id: job.id })}>Cancel Export</Button>
      <Button variant="primary" onclick={() => (app.exportJob = null)}>Continue in Background</Button>
    </div>
  </Sheet>
{/if}

<style>
  .line { display: flex; justify-content: space-between; gap: 12px; font: var(--fo-text-caption); color: var(--fo-label-2); }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; }
</style>
