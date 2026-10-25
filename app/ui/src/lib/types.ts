export type Kind = 'notebook' | 'book' | 'screenshot';

export interface Item {
  id: string;
  kind: Kind;
  rel_path: string;
  device_path: string | null;
  title: string;
  authors: string[];
  class: 'purchased' | 'sideloaded' | null;
  format: string;
  size: number;
  modified: number;
  content_hash: string;
  page_count: number | null;
  thumbnail: string | null;
  /** Books: why it can't be opened here. */
  blocked: string | null;
  has_cover: boolean;
}

export interface AnnotatedPage {
  pdf_page: number;
  notebook_page: number;
  /** The Kindle canvas on the PDF page, as fractions of its width and height. */
  placement: { left: number; top: number; width: number; height: number };
  strokes: number;
}

export interface TextHit {
  id: string;
  /** `section:<n>` (EPUB), `fraction:<f>` (MOBI/AZW3), `page:<n>` (PDF), or empty (text). */
  loc: string;
  label: string;
  /** Matches are wrapped in \u0002 … \u0003. */
  snippet: string;
}

export type JobState = 'queued' | 'running' | 'done' | 'failed' | 'cancelled';

export interface JobInfo {
  id: number;
  kind: string;
  title: string;
  state: JobState;
  done: number;
  total: number;
  message: string;
  result: any;
}

export interface DeviceStatus {
  connected: boolean;
  name: string | null;
  busy: boolean;
  holder: string | null;
  free_bytes: number | null;
  total_bytes: number | null;
  error: string | null;
}

export interface Transfer {
  id: number;
  job: number;
  direction: 'down' | 'up';
  name: string;
  size: number;
  done: number;
  state: JobState;
  bytes_per_sec: number;
  error: string | null;
}

export interface Status {
  library_root: string;
  cache_root: string;
  counts: Partial<Record<Kind, number>>;
  device: DeviceStatus;
  cache_bytes: number;
  jobs: JobInfo[];
  transfers: Transfer[];
}

export interface NotebookInfo {
  item: Item;
  pages: [number, number][];
  position: number | null;
}

export type AppEvent =
  | ({ type: 'job' } & JobInfo)
  | { type: 'job_log'; id: number; line: string }
  | ({ type: 'device' } & DeviceStatus)
  | ({ type: 'transfer' } & Transfer)
  | { type: 'library' };
