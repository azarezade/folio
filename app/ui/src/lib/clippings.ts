// The Kindle's "My Clippings.txt": every highlight, note and bookmark, across
// books, separated by "==========" lines.

export type ClipKind = 'Highlight' | 'Note' | 'Bookmark';
export interface Clip {
  kind: ClipKind | string;
  /** "page 3 · Location 40-41" */
  where: string;
  added: string;
  text: string;
}
export interface ClipBook {
  title: string;
  author: string;
  clips: Clip[];
}

/** "Title (Author)" / "- Your Highlight on page 3 | Location 40-41 | Added on …" / blank / text. */
export function parseClippings(t: string): ClipBook[] {
  const books = new Map<string, ClipBook>();
  const seen = new Set<string>();
  for (const raw of t.replace(/^﻿/, '').replace(/\r\n?/g, '\n').split(/\n==========\n?/)) {
    const lines = raw.replace(/^﻿/, '').split('\n');
    while (lines.length && !lines[0].trim()) lines.shift();
    if (lines.length < 2) continue;
    const head = lines[0].trim();
    const m = head.match(/^(.*)\(([^()]*)\)\s*$/);
    const title = (m ? m[1] : head).trim() || 'Untitled';
    const author = m ? m[2].trim() : '';
    const meta = lines[1].replace(/^-\s*/, '').split('|').map((s) => s.trim());
    const kind = meta[0].match(/Your (\w+)/)?.[1] ?? meta[0];
    const added = meta.find((s) => /^Added on/i.test(s))?.replace(/^Added on\s*/i, '') ?? '';
    const where = meta
      .filter((s) => !/^Added on/i.test(s))
      .map((s) => s.replace(/^Your \w+\s*(on\s*)?/i, ''))
      .filter(Boolean)
      .join(' · ');
    const text = lines.slice(2).join('\n').trim();
    // the Kindle appends a new entry when a highlight is extended; keep one
    const key = `${title}\u0000${kind}\u0000${where}\u0000${text}`;
    if (seen.has(key)) continue;
    seen.add(key);
    const bk = `${title}\u0000${author}`;
    if (!books.has(bk)) books.set(bk, { title, author, clips: [] });
    books.get(bk)!.clips.push({ kind, where, added, text });
  }
  return [...books.values()];
}
