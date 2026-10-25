// App-wide state: library lists, selection, reader modes, jobs, transfers,
// device status, dialogs and toasts. Components read it directly; changes
// arrive from user actions and from core events.
import { call, onEvent } from './api';
import type { AppEvent, DeviceStatus, Item, JobInfo, Kind, Status, Transfer } from './types';

/** What the library window shows (the sidebar's rows). */
export type Place = 'all' | 'notebook' | 'book' | 'document' | 'screenshot' | 'clippings' | 'search' | 'device' | 'files' | 'transfers';
export type SettingsTab = 'general' | 'rendering' | 'sync' | 'about';

export interface Toast {
  id: number;
  text: string;
  kind: 'info' | 'ok' | 'error';
  action?: { label: string; run: () => void };
  details?: string;
}

export interface Confirm {
  title: string;
  body: string;
  confirm: string;
  danger?: boolean;
  run: () => void | Promise<void>;
}

/** The Kindle's "My Clippings.txt" (shown as My Clippings, not as a book). */
export const isClippingsFile = (it: Item) => it.kind === 'book' && /(^|\/)my clippings\.txt$/i.test(it.rel_path);
export const isDocument = (it: Item) => it.kind === 'book' && it.format === 'PDF';

class AppState {
  status = $state<Status | null>(null);
  items = $state<Record<Kind, Item[]>>({ notebook: [], book: [], screenshot: [] });
  place = $state<Place>('all');
  /** The open document (its own view over the library), or null. */
  openId = $state<string | null>(null);
  /** The screenshot shown full size, or null. */
  shotId = $state<string | null>(null);
  filter = $state('');
  sort = $state<'added' | 'title' | 'kind'>('added');
  layout = $state<'grid' | 'list'>('grid');
  sidebarHidden = $state(false);
  settingsOpen = $state(false);
  settingsTab = $state<SettingsTab>('general');
  activityOpen = $state(false);
  logOpen = $state(false);
  selectedId = $state<string | null>(null);
  /** Notebooks picked (⌘-click) for a batch export. */
  picked = $state<Set<string>>(new Set());
  bookMode = $state(false);
  zen = $state(false);
  device = $state<DeviceStatus>({ connected: false, name: null, busy: false, holder: null, free_bytes: null, total_bytes: null, error: null });
  jobs = $state<Record<number, JobInfo>>({});
  transfers = $state<Record<number, Transfer>>({});
  /** Sync log lines. */
  log = $state<{ text: string; kind: 'info' | 'ok' | 'err' }[]>([]);
  toasts = $state<Toast[]>([]);
  confirm = $state<Confirm | null>(null);
  exportFor = $state<string[] | null>(null);
  /** The export job the progress sheet follows. */
  exportJob = $state<number | null>(null);
  sendOpen = $state(false);
  /** The notebook being renamed. */
  renameId = $state<string | null>(null);
  /** The context menu, if open. */
  menu = $state<{ x: number; y: number; items: import('../shell/Menu.svelte').MenuItem[] } | null>(null);
  settings = $state<Record<string, string>>({});
  /** Where files are sent (the device browser's folder when inside documents/). */
  pushTarget = $state('documents');
  /** The page the notebook reader shows (for "export current page"). */
  readerPage = $state(0);
  exportDirDefault = $state('');
  /** Full-text search across books (the Search view). */
  searchQuery = $state('');
  /** Open a book at a search hit: the reader jumps to `loc` and searches `query`. */
  openAt = $state<{ id: string; loc: string; query: string } | null>(null);
  private toastId = 0;
  private syncJobs = new Set<number>();

  all = $derived([...this.items.notebook, ...this.items.book, ...this.items.screenshot]);
  opened = $derived(this.openId ? (this.all.find((i) => i.id === this.openId) ?? null) : null);
  selected = $derived(this.selectedId ? (this.all.find((i) => i.id === this.selectedId) ?? null) : null);
  clippingFiles = $derived(this.items.book.filter(isClippingsFile));
  counts = $derived({
    all: this.items.notebook.length + this.items.book.filter((b) => !isClippingsFile(b)).length,
    notebook: this.items.notebook.length,
    book: this.items.book.filter((b) => !isDocument(b) && !isClippingsFile(b)).length,
    document: this.items.book.filter(isDocument).length,
    screenshot: this.items.screenshot.length,
  });

  /** The library items the current place shows, filtered and sorted. */
  visible = $derived.by(() => {
    const p = this.place;
    let list: Item[] =
      p === 'notebook' ? this.items.notebook
      : p === 'book' ? this.items.book.filter((b) => !isDocument(b) && !isClippingsFile(b))
      : p === 'document' ? this.items.book.filter(isDocument)
      : p === 'screenshot' ? this.items.screenshot
      : [...this.items.notebook, ...this.items.book.filter((b) => !isClippingsFile(b))];
    const q = this.filter.trim().toLowerCase();
    if (q) list = list.filter((i) => `${i.title} ${i.rel_path} ${i.authors.join(' ')} ${i.format}`.toLowerCase().includes(q));
    const by = this.sort;
    return [...list].sort((a, b) =>
      by === 'title' ? a.title.localeCompare(b.title)
      : by === 'kind' ? a.kind.localeCompare(b.kind) || a.format.localeCompare(b.format) || b.modified - a.modified
      : b.modified - a.modified,
    );
  });

  runningJobs = $derived(Object.values(this.jobs).filter((j) => j.state === 'running' || j.state === 'queued'));

  async start() {
    onEvent((e) => this.onEvent(e));
    await Promise.all([this.refreshStatus(), this.loadItems(), this.loadSettings()]);
    this.bookMode = this.settings.book_mode === 'true';
    this.layout = this.settings.library_view === 'list' ? 'list' : 'grid';
    const sort = this.settings.library_sort;
    if (sort === 'title' || sort === 'kind' || sort === 'added') this.sort = sort;
    const last = this.settings.last_item;
    if (last && this.all.some((i) => i.id === last)) this.selectedId = last;
    call('device_status').then((d) => (this.device = d)).catch(() => {});
  }

  async refreshStatus() {
    const s = await call<Status>('status');
    this.status = s;
    this.device = s.device;
    for (const j of s.jobs) this.jobs[j.id] = j;
    for (const t of s.transfers) this.transfers[t.id] = t;
  }

  async loadItems() {
    const all = await call<Item[]>('items', {});
    const by: Record<Kind, Item[]> = { notebook: [], book: [], screenshot: [] };
    for (const it of all) by[it.kind].push(it);
    this.items = by;
  }

  async loadSettings() {
    const s = await call<{ values: Record<string, string>; export_dir_default: string }>('settings');
    this.settings = s.values;
    this.exportDirDefault = s.export_dir_default;
  }

  async setSetting(key: string, value: string) {
    this.settings[key] = value;
    await call('set_setting', { key, value });
  }

  onEvent(e: AppEvent) {
    switch (e.type) {
      case 'job': {
        const { type, ...job } = e;
        const before = this.jobs[job.id];
        this.jobs[job.id] = job;
        if (before?.state !== job.state) this.jobFinished(job);
        if (this.exportJob === job.id && job.state !== 'running' && job.state !== 'queued') this.exportJob = null;
        break;
      }
      case 'job_log':
        if (this.syncJobs.has(e.id)) this.addLog(e.line);
        break;
      case 'device': {
        const { type, ...d } = e;
        this.device = d;
        break;
      }
      case 'transfer': {
        const { type, ...t } = e;
        this.transfers[t.id] = t;
        break;
      }
      case 'library':
        this.loadItems();
        this.refreshStatus();
        break;
    }
  }

  addLog(text: string) {
    const kind = text.startsWith('✗') ? 'err' : text.startsWith('↓') || text.startsWith('↑') || text.startsWith('Done') ? 'ok' : 'info';
    this.log.push({ text, kind });
    if (this.log.length > 2000) this.log.splice(0, this.log.length - 2000);
  }

  private jobFinished(job: JobInfo) {
    if (job.state === 'done') {
      if (job.kind === 'export' && job.result?.path) {
        const path = job.result.path as string;
        if (this.exportJob === job.id) this.exportJob = null;
        this.toast(job.result.skipped ? `${job.title}: already up to date` : `${job.title.replace(/^Export /, 'Exported ')}`, 'ok', {
          label: 'Show in Finder',
          run: () => call('reveal', { path }),
        });
      } else if (job.kind === 'sync') {
        this.addLog('Sync finished.');
        this.toast('Sync finished', 'ok');
      } else if (job.kind === 'push') {
        this.toast(`${job.title}: sent`, 'ok');
      } else if (job.kind === 'free') {
        this.toast('The Kindle is free', 'ok');
      } else if (job.kind === 'migrate') {
        this.toast(`Imported ${job.result?.copied ?? 0} files`, 'ok');
      }
    } else if (job.state === 'failed') {
      if (job.kind === 'sync') this.addLog(`✗ ${job.message}`);
      this.toast(`${job.title} failed`, 'error', undefined, job.message);
    }
  }

  toast(text: string, kind: Toast['kind'] = 'info', action?: Toast['action'], details?: string) {
    const id = ++this.toastId;
    this.toasts.push({ id, text, kind, action, details });
    if (this.toasts.length > 4) this.toasts.splice(0, this.toasts.length - 4);
  }

  dismiss(id: number) {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  async run<T>(what: string, f: () => Promise<T>): Promise<T | undefined> {
    try {
      return await f();
    } catch (e) {
      this.toast(`${what} failed`, 'error', undefined, String(e instanceof Error ? e.message : e));
      return undefined;
    }
  }

  go(place: Place) {
    this.place = place;
    this.openId = null;
    this.zen = false;
  }

  /** Opens an item: documents in their own view, screenshots full size. */
  open(it: Item) {
    this.selectedId = it.id;
    if (it.kind === 'screenshot') {
      this.shotId = it.id;
      return;
    }
    if (isClippingsFile(it)) return this.go('clippings');
    this.openId = it.id;
  }

  close() {
    this.openId = null;
    this.zen = false;
  }

  async sync(opts: { notebooks?: boolean; books?: boolean; screenshots?: boolean } = {}) {
    this.addLog('Starting sync…');
    const id = await this.run('Sync', () => call<number>('sync', opts));
    if (id != null) this.syncJobs.add(id);
  }

  async push(paths: string[], to = 'documents') {
    if (!paths.length) return;
    await this.run('Sending', () => call('push', { paths, to }));
  }

  async toggleBookMode() {
    this.bookMode = !this.bookMode;
    await call('set_setting', { key: 'book_mode', value: String(this.bookMode) }).catch(() => {});
  }
}

export const app = new AppState();
