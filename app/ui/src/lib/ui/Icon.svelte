<script lang="ts" module>
  // 16-unit stroke icons from the Folio design boards.
  export const ICONS = {
    all: '<rect x="2" y="2" width="5" height="5" rx="1.2"/><rect x="9" y="2" width="5" height="5" rx="1.2"/><rect x="2" y="9" width="5" height="5" rx="1.2"/><rect x="9" y="9" width="5" height="5" rx="1.2"/>',
    notebook: '<rect x="3" y="1.75" width="10.5" height="12.5" rx="1.6"/><path d="M5.75 1.75v12.5M8.25 5.25h3M8.25 7.75h3"/>',
    book: '<path d="M1.75 3.5c2.1-.9 4.2-.8 6.25.6 2.05-1.4 4.15-1.5 6.25-.6v9.25c-2.1-.9-4.2-.8-6.25.6-2.05-1.4-4.15-1.5-6.25-.6z"/><path d="M8 4.1v9.25"/>',
    document: '<path d="M3.75 1.75h5.5l3 3v9.5h-8.5z"/><path d="M9.25 1.75v3h3"/>',
    clippings: '<path d="M2.5 13V9.5h3.25V13zM2.5 9.5c0-3 1.4-4.8 3.6-5.5M9.25 13V9.5h3.25V13zM9.25 9.5c0-3 1.4-4.8 3.6-5.5"/>',
    kindle: '<rect x="3.25" y="1.5" width="9.5" height="13" rx="1.6"/><path d="M6.5 12.25h3"/>',
    folder: '<path d="M1.75 4.25a1 1 0 0 1 1-1h3.4l1.5 1.5h5.6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1H2.75a1 1 0 0 1-1-1z"/>',
    transfers: '<path d="M5 13.5V2.5M2.5 5 5 2.5 7.5 5M11 2.5v11M8.5 11 11 13.5 13.5 11"/>',
    sync: '<path d="M13.25 6.25A5.5 5.5 0 0 0 3.4 4.4M2.75 9.75a5.5 5.5 0 0 0 9.85 1.85M3.25 1.75v2.9h2.9M12.75 14.25v-2.9h-2.9"/>',
    export: '<path d="M8 10V1.75M5.25 4.5 8 1.75l2.75 2.75M5 6.5H3.5v7.75h9V6.5H11"/>',
    upload: '<path d="M8 13.5V2.5M4.5 6 8 2.5 11.5 6"/>',
    search: '<circle cx="7" cy="7" r="4.75"/><path d="M10.5 10.5 14 14"/>',
    warning: '<path d="M8 2 14.5 13.5h-13z"/><path d="M8 6.5v3.25M8 11.5v.1"/>',
    info: '<circle cx="8" cy="8" r="6.25"/><path d="M8 7.25v4M8 4.9v.1"/>',
    close: '<path d="M4 4l8 8M12 4l-8 8"/>',
    retry: '<path d="M12.8 7.2A5.1 5.1 0 1 0 13.1 9.6M13.3 2.9v4.5H8.8"/>',
    check: '<path d="M3.2 8.3 6.7 11.8 12.8 4.5"/>',
    chevron: '<path d="M4.5 6.25 8 9.75l3.5-3.5"/>',
    back: '<path d="M10 3.5 5.5 8l4.5 4.5"/>',
    forward: '<path d="M6 3.5 10.5 8 6 12.5"/>',
    sidebar: '<rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2"/><path d="M6.25 2.75v10.5"/>',
    grid: '<rect x="2" y="2" width="5" height="5" rx="1"/><rect x="9" y="2" width="5" height="5" rx="1"/><rect x="2" y="9" width="5" height="5" rx="1"/><rect x="9" y="9" width="5" height="5" rx="1"/>',
    list: '<path d="M5.5 4h8.5M5.5 8h8.5M5.5 12h8.5M2.4 4h.2M2.4 8h.2M2.4 12h.2"/>',
    trash: '<path d="M2.75 4.25h10.5M6.25 4.25V2.75h3.5v1.5M4 4.25l.75 9h6.5l.75-9M6.75 6.75v4.25M9.25 6.75v4.25"/>',
    note: '<path d="M3 13.5V11l7.5-7.5 2.5 2.5L5.5 13.5zM9 5l2.5 2.5"/>',
    bookmark: '<path d="M4 1.75h8v12.5L8 11l-4 3.25z"/>',
    gear: '<circle cx="8" cy="8" r="2.4"/><path d="M8 1.75v1.8M8 12.45v1.8M1.75 8h1.8M12.45 8h1.8M3.6 3.6l1.25 1.25M11.15 11.15l1.25 1.25M3.6 12.4l1.25-1.25M11.15 4.85l1.25-1.25"/>',
    brush: '<path d="M3 13.5l1-3.6L12 1.9l2.1 2.1-8 8z"/><path d="M10.5 3.4l2.1 2.1"/>',
    folderPlus: '<path d="M1.75 4.25a1 1 0 0 1 1-1h3.4l1.5 1.5h5.6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1H2.75a1 1 0 0 1-1-1z"/><path d="M8 7v4M6 9h4"/>',
    download: '<path d="M8 2.5v9M4.5 8 8 11.5 11.5 8M3 13.75h10"/>',
    more: '<path d="M3.5 8h.1M8 8h.1M12.5 8h.1" stroke-width="2.2"/>',
    screenshot: '<rect x="1.75" y="3.25" width="12.5" height="9.5" rx="1.6"/><circle cx="8" cy="8" r="2.2"/>',
    pages: '<rect x="2" y="2" width="12" height="12" rx="2"/><path d="M6 2v12M3.5 5h1M3.5 8h1M3.5 11h1"/>',
    pen: '<path d="M2.5 13.5 3.3 10.4 11 2.7l2.3 2.3-7.7 7.7zM9.8 3.9l2.3 2.3"/>',
    eye: '<path d="M1.5 8s2.4-4.5 6.5-4.5S14.5 8 14.5 8 12.1 12.5 8 12.5 1.5 8 1.5 8z"/><circle cx="8" cy="8" r="2"/>',
    log: '<path d="M3.5 1.75h6l3 3v9.5h-9zM5.5 7.5h5M5.5 10h5M5.5 5h2"/>',
  } as const;
  export type IconName = keyof typeof ICONS;
</script>

<script lang="ts">
  let {
    name,
    size = 16,
    color = 'currentColor',
    weight = 1.4,
    label,
  }: { name: IconName; size?: number; color?: string; weight?: number; label?: string } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill="none"
  stroke={color}
  stroke-width={weight}
  stroke-linecap="round"
  stroke-linejoin="round"
  role={label ? 'img' : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : 'true'}
  style="flex: none"
>
  {@html ICONS[name]}
</svg>
