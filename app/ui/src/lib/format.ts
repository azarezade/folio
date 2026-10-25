export function bytes(n: number | null | undefined): string {
  if (n == null) return '—';
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1e9) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1e9).toFixed(1)} GB`;
}

export function date(unix: number): string {
  return new Date(unix * 1000).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
}

export function dateTime(unix: number): string {
  return new Date(unix * 1000).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
}

export function speed(bps: number): string {
  return bps > 0 ? `${bytes(bps)}/s` : '';
}

/** "Today, 09:12", "Oct 3, 2026". */
export function shortDate(unix: number): string {
  const d = new Date(unix * 1000);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return `Today, ${d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })}`;
  return date(unix);
}

/** Library section a date falls in: Today, Yesterday, Earlier this week, … */
export function dateBucket(unix: number, now = new Date()): string {
  const d = new Date(unix * 1000);
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((day(now) - day(d)) / 86400000);
  if (diff <= 0) return 'Today';
  if (diff === 1) return 'Yesterday';
  if (diff < 7) return 'Earlier this week';
  if (d.getFullYear() === now.getFullYear() && d.getMonth() === now.getMonth()) return 'Earlier this month';
  return d.toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
}

export const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;

/** A path as macOS shows it: the home folder as `~`. */
export const tildify = (path: string | null | undefined) => (path ?? '').replace(/^\/Users\/[^/]+(?=\/|$)/, '~');
