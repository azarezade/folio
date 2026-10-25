<script lang="ts">
  // The transfer queue (Device board, transfers): every copy to and from the
  // Kindle with progress, state, and cancel / retry / show.
  import { app } from '../lib/state.svelte';
  import { call } from '../lib/api';
  import { bytes, speed } from '../lib/format';
  import type { Transfer } from '../lib/types';
  import { Button, Banner, Progress, StatusChip, EmptyState, Icon } from '../lib/ui';
  import Toolbar from '../shell/Toolbar.svelte';
  import ActivityButton from '../shell/ActivityButton.svelte';

  const list = $derived(Object.values(app.transfers).sort((a, b) => b.id - a.id).slice(0, 300));
  const running = $derived(list.filter((t) => t.state === 'running').length);
  const queued = $derived(list.filter((t) => t.state === 'queued').length);
  const failed = $derived(list.filter((t) => t.state === 'failed'));

  async function clear() {
    await call('clear_transfers');
    app.transfers = Object.fromEntries(Object.entries(app.transfers).filter(([, t]) => t.state === 'queued' || t.state === 'running'));
  }
  const retry = (t: Transfer) => app.run('Retry', () => call('retry_transfer', { id: t.id }));
  function detail(t: Transfer) {
    if (t.state === 'running') return `${bytes(t.done)} of ${bytes(t.size)}${t.bytes_per_sec ? ` · ${speed(t.bytes_per_sec)}` : ''}`;
    if (t.state === 'failed') return `Stopped at ${bytes(t.done)} of ${bytes(t.size)}`;
    return bytes(t.size);
  }
</script>

<div class="screen">
  <Toolbar title="Transfers" subtitle={`${app.device.name ?? 'Kindle'}${running ? ` · ${running} running` : ''}`}>
    <ActivityButton />
  </Toolbar>
  <div class="body">
    {#if failed.length}
      <Banner tone="danger" title={`${failed.length} transfer${failed.length === 1 ? '' : 's'} failed.`}>
        {failed[0].error ?? 'The Kindle didn’t accept the file.'} Nothing on the device changed — retry when it is idle.
        {#snippet actions()}<Button onclick={() => failed.forEach(retry)}>Retry Failed</Button>{/snippet}
      </Banner>
    {/if}
    <div class="summary">
      <span>{list.length} transfer{list.length === 1 ? '' : 's'}{running ? ` · ${running} running` : ''}{queued ? ` · ${queued} queued` : ''}</span>
      <Button variant="plain" disabled={!list.length} onclick={clear}>Clear Finished</Button>
    </div>
    {#if !list.length}
      <div class="center"><EmptyState icon="transfers" title="No transfers" body="Files you send to the Kindle, and copies made from it, show up here." /></div>
    {:else}
      <div class="table" role="table" aria-label="Transfer queue">
        {#each list as t (t.id)}
          <div class="row" role="row">
            <span role="cell" class="dir" title={t.direction === 'up' ? 'To Kindle' : 'From Kindle'}>
              <Icon name={t.direction === 'up' ? 'upload' : 'download'} color={t.direction === 'up' ? 'var(--fo-accent)' : 'var(--fo-label-2)'} label={t.direction === 'up' ? 'To Kindle' : 'From Kindle'} />
            </span>
            <span role="cell" class="name">
              <span class="n">{t.name}</span>
              <span class="note" class:bad={t.state === 'failed'}>{t.state === 'failed' ? (t.error ?? 'Failed') : t.direction === 'up' ? 'To the Kindle' : 'To the library'}</span>
            </span>
            <span role="cell" class="prog">
              {#if t.state === 'running' || t.state === 'failed' || t.state === 'done'}
                <Progress value={t.size ? t.done / t.size : null} size="small" state={t.state === 'failed' ? 'failed' : t.state === 'done' ? 'done' : 'active'} label="{t.name} progress" />
              {/if}
              <span>{detail(t)}</span>
            </span>
            <span role="cell"><StatusChip state={t.state} /></span>
            <span role="cell" class="act">
              {#if t.state === 'running' || t.state === 'queued'}<Button variant="plain" onclick={() => call('cancel_job', { id: t.job })}>Cancel</Button>
              {:else if t.state === 'failed'}<Button variant="plain" onclick={() => retry(t)}>Retry</Button>{/if}
            </span>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .screen { display: flex; flex-direction: column; height: 100%; min-width: 0; }
  .body { flex: 1; overflow-y: auto; padding: 20px 28px; display: flex; flex-direction: column; gap: 12px; }
  .center { flex: 1; display: flex; align-items: center; justify-content: center; }
  .summary { display: flex; align-items: center; justify-content: space-between; font: var(--fo-text-callout); color: var(--fo-label-2); }
  .table { display: flex; flex-direction: column; border-top: 1px solid var(--fo-separator); }
  .row { display: grid; grid-template-columns: 28px minmax(0, 2fr) minmax(0, 1.5fr) 90px 80px; gap: 12px; align-items: center; padding: 10px 6px; border-bottom: 1px solid var(--fo-separator); }
  .name { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .n { font: 500 13px/16px var(--fo-font); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .note { font: var(--fo-text-caption); color: var(--fo-label-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .note.bad { color: var(--fo-danger); }
  .prog { display: flex; flex-direction: column; gap: 4px; font: var(--fo-text-caption); color: var(--fo-label-2); }
  .act { text-align: right; }
</style>
