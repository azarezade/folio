// Transport to the app core. Inside the Tauri app: one `call` command, the
// `folio://` scheme and the `folio-event` event. In a browser (development):
// the `folio-cli serve` bridge over HTTP and server-sent events. Both reach the
// same `folio_core::api`.
import type { AppEvent } from './types';

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export async function call<T = any>(name: string, args: Record<string, unknown> = {}): Promise<T> {
  if (isTauri) {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<T>('call', { name, args });
  }
  const res = await fetch(`/api/${name}`, { method: 'POST', body: JSON.stringify(args) });
  const body = await res.json();
  if (!body.ok) throw new Error(body.error);
  return body.value as T;
}

/** URL for page images and files: `notebook/<id>/page/<n>?w=`, `thumb/<id>`, `file/<id>`. */
export function asset(path: string): string {
  return isTauri ? `folio://localhost/${path}` : `/folio/${path}`;
}

export function onEvent(cb: (e: AppEvent) => void): () => void {
  if (isTauri) {
    let stop = () => {};
    import('@tauri-apps/api/event').then(({ listen }) =>
      listen<AppEvent>('folio-event', (e) => cb(e.payload)).then((un) => (stop = un)),
    );
    return () => stop();
  }
  const es = new EventSource('/events');
  es.onmessage = (m) => cb(JSON.parse(m.data));
  return () => es.close();
}

/** Native menu choices (Tauri only). */
export function onMenu(cb: (id: string) => void): () => void {
  if (!isTauri) return () => {};
  let stop = () => {};
  import('@tauri-apps/api/event').then(({ listen }) =>
    listen<string>('menu', (e) => cb(e.payload)).then((un) => (stop = un)),
  );
  return () => stop();
}

/** Files dropped onto the window (Tauri gives real paths). */
export function onFileDrop(cb: (paths: string[]) => void): () => void {
  if (!isTauri) return () => {};
  let stop = () => {};
  import('@tauri-apps/api/webview').then(({ getCurrentWebview }) =>
    getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === 'drop') cb(e.payload.paths);
      })
      .then((un) => (stop = un)),
  );
  return () => stop();
}

/** Choose files (Tauri dialog); returns [] when cancelled or in a browser. */
export async function chooseFiles(extensions: string[]): Promise<string[]> {
  if (!isTauri) return [];
  const { open } = await import('@tauri-apps/plugin-dialog');
  const r = await open({ multiple: true, filters: [{ name: 'Documents', extensions }] });
  return r === null ? [] : Array.isArray(r) ? r : [r];
}

export async function chooseFolder(start?: string): Promise<string | null> {
  if (!isTauri) return null;
  const { open } = await import('@tauri-apps/plugin-dialog');
  const r = await open({ directory: true, defaultPath: start });
  return typeof r === 'string' ? r : null;
}

export async function saveAs(defaultName: string): Promise<string | null> {
  if (!isTauri) return null;
  const { save } = await import('@tauri-apps/plugin-dialog');
  return (await save({ defaultPath: defaultName })) ?? null;
}

/** Opens a web page in the default browser. */
export async function openUrl(url: string): Promise<void> {
  if (!isTauri) {
    window.open(url, '_blank', 'noopener');
    return;
  }
  const { openUrl } = await import('@tauri-apps/plugin-opener');
  await openUrl(url);
}
