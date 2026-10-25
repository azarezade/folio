// Copies PDF.js's font, CMap, colour-profile and WASM data into public/pdfjs so
// the app can render PDFs whose fonts are not embedded, without the network.
import { cpSync, mkdirSync, rmSync } from 'node:fs';

const out = new URL('../public/pdfjs/', import.meta.url);
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
for (const dir of ['standard_fonts', 'cmaps', 'iccs', 'wasm']) {
  cpSync(new URL(`../node_modules/pdfjs-dist/${dir}/`, import.meta.url), new URL(`${dir}/`, out), { recursive: true });
}
