#!/usr/bin/env node
// The favicons, from one source: public/favicon.svg (the 16-unit tile with the cinnabar prompt
// square inset 2 units, the wordmark's square). Renders the PNGs with rsvg-convert and writes
// favicon.ico (16 + 32, PNG entries) itself, so the output depends on rsvg-convert alone.
//
//   node scripts/favicons.mjs        (from frontend/; the files are committed)
//
// The apple-touch icon gets the night ground behind the square (iOS fills a transparent icon with
// black); the colours are lib/palette.ts `mark` and `base-bg`, night, as the SVG writes them.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const PUBLIC = resolve(import.meta.dirname, '..', 'public');
const SVG = join(PUBLIC, 'favicon.svg');
const NIGHT_GROUND = '#15120f';

function render(size, out, background) {
  const args = ['-w', String(size), '-h', String(size), '-o', out];
  if (background) args.push('--background-color', background);
  execFileSync('rsvg-convert', [...args, SVG]);
}

/** An ICO holding PNG images: the 6-byte header, a 16-byte entry per image, then the images. */
function ico(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // 1 = icon
  header.writeUInt16LE(images.length, 4);
  let offset = 6 + 16 * images.length;
  const entries = images.map(({ size, png }) => {
    if (size > 255) throw new Error(`favicons: an ICO entry of ${size} px needs the 256 rule`);
    const entry = Buffer.alloc(16);
    entry.writeUInt8(size, 0); // width
    entry.writeUInt8(size, 1); // height
    entry.writeUInt8(0, 2); // no palette
    entry.writeUInt8(0, 3); // reserved
    entry.writeUInt16LE(1, 4); // colour planes
    entry.writeUInt16LE(32, 6); // bits per pixel
    entry.writeUInt32LE(png.length, 8);
    entry.writeUInt32LE(offset, 12);
    offset += png.length;
    return entry;
  });
  return Buffer.concat([header, ...entries, ...images.map((i) => i.png)]);
}

render(16, join(PUBLIC, 'favicon-16x16.png'));
render(32, join(PUBLIC, 'favicon-32x32.png'));
render(180, join(PUBLIC, 'apple-touch-icon.png'), NIGHT_GROUND);

const scratch = mkdtempSync(join(tmpdir(), 'favicons-'));
try {
  writeFileSync(
    join(PUBLIC, 'favicon.ico'),
    ico(
      [16, 32].map((size) => {
        const out = join(scratch, `${size}.png`);
        render(size, out);
        return { size, png: readFileSync(out) };
      })
    )
  );
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
console.log(
  'favicons: favicon.ico (16, 32), favicon-16x16.png, favicon-32x32.png, apple-touch-icon.png'
);
