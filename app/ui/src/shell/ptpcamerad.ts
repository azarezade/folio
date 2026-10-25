import { app } from '../lib/state.svelte';
import { call } from '../lib/api';

/** Turns macOS's ptpcamerad off (or back on) for this account, after a confirm. */
export function setPtpcamerad(disable: boolean) {
  app.confirm = {
    title: disable ? 'Stop macOS from grabbing the Kindle?' : 'Let macOS use USB cameras again?',
    body: disable
      ? 'This turns off Image Capture’s device service (ptpcamerad) for your account, so it no longer claims the Kindle when you plug it in. Photos and Image Capture won’t import from cameras or phones over USB until you turn it back on in Settings › Sync.'
      : 'Image Capture’s device service starts again and may claim the Kindle when you plug it in. Use Free the Kindle when that happens.',
    confirm: disable ? 'Stop Grabbing' : 'Turn Back On',
    danger: false,
    run: () =>
      app.run('Image Capture service', async () => {
        await call('ptpcamerad', { disable });
        await app.loadSettings();
      }),
  };
}
