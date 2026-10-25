<script lang="ts">
  // An image whose URL may still be coming (PDF pages are drawn on demand).
  // Shows the previous image until the new one has loaded, so pages never blink.
  interface Props {
    src: string | Promise<string>;
    alt: string;
  }
  let { src, alt }: Props = $props();
  let url = $state<string | null>(null);
  let failed = $state(false);

  $effect(() => {
    let live = true;
    failed = false;
    Promise.resolve(src).then(
      (u) => {
        if (!live) return;
        const img = new Image();
        img.onload = () => live && (url = u);
        img.onerror = () => {
          console.error('page image did not load', u);
          if (live) failed = true;
        };
        img.src = u;
      },
      (e) => {
        console.error('page could not be drawn', e);
        if (live) failed = true;
      },
    );
    return () => (live = false);
  });
</script>

{#if url}
  <img src={url} {alt} draggable="false" />
{:else if failed}
  <div class="failed">Page could not be drawn</div>
{:else}
  <div class="loading"></div>
{/if}

<style>
  img { display: block; width: 100%; height: 100%; object-fit: contain; background: var(--fo-paper); }
  .loading, .failed { width: 100%; height: 100%; background: var(--fo-paper); }
  .failed { display: flex; align-items: center; justify-content: center; color: var(--fo-danger); font: var(--fo-text-caption); }
</style>
