import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// In development the UI can talk to `folio-cli serve` (the dev bridge) on :8137.
const bridge = 'http://127.0.0.1:8137';

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    proxy: { '/api': bridge, '/folio': bridge, '/events': bridge },
  },
  build: { target: 'safari16', outDir: 'dist', emptyOutDir: true, chunkSizeWarningLimit: 2000 },
});
