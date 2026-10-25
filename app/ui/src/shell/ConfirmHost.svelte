<script lang="ts">
  import { app } from '../lib/state.svelte';
  import { ConfirmSheet } from '../lib/ui';

  const c = app.confirm!;
</script>

<ConfirmSheet
  title={c.title}
  body={c.body}
  confirmLabel={c.confirm}
  destructive={!!c.danger}
  oncancel={() => (app.confirm = null)}
  onconfirm={() => {
    app.confirm = null;
    c.run();
  }}
/>
