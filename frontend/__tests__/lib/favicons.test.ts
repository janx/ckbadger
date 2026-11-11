import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { PALETTE } from '@/lib/palette';

// Rubbing × Song print, phase 2: the favicon is the wordmark's square — the cinnabar `mark`, inset
// in a 16-unit tile (12/16). public/favicon.svg is the one source; scripts/favicons.mjs renders the
// ICO (16 + 32) and the PNGs from it. The webp badge is gone.

const pub = (name: string) => resolve(__dirname, '../../public', name);

/** A PNG's width and height, from its IHDR chunk. */
function pngSize(png: Buffer): [number, number] {
  expect(png.subarray(1, 4).toString('ascii')).toBe('PNG');
  expect(png.subarray(12, 16).toString('ascii')).toBe('IHDR');
  return [png.readUInt32BE(16), png.readUInt32BE(20)];
}

describe('the favicons', () => {
  it('draws the night cinnabar square inset two units in a 16-unit tile', () => {
    const svg = readFileSync(pub('favicon.svg'), 'utf8');
    expect(svg).toContain('viewBox="0 0 16 16"');
    expect(svg).toMatch(
      new RegExp(`<rect x="2" y="2" width="12" height="12" fill="${PALETTE.dark.mark}"`)
    );
    // one shape, nothing drawn but the square
    expect(svg.match(/<(rect|path|circle|text|image)\b/g)).toEqual(['<rect']);
  });

  it('renders the PNGs at their sizes', () => {
    expect(pngSize(readFileSync(pub('favicon-16x16.png')))).toEqual([16, 16]);
    expect(pngSize(readFileSync(pub('favicon-32x32.png')))).toEqual([32, 32]);
    expect(pngSize(readFileSync(pub('apple-touch-icon.png')))).toEqual([180, 180]);
  });

  it('packs a 16 and a 32 px PNG into favicon.ico', () => {
    const ico = readFileSync(pub('favicon.ico'));
    expect(ico.readUInt16LE(0)).toBe(0);
    expect(ico.readUInt16LE(2)).toBe(1);
    expect(ico.readUInt16LE(4)).toBe(2);
    const entries = [0, 1].map((i) => {
      const at = 6 + 16 * i;
      const size = ico.readUInt32LE(at + 8);
      const offset = ico.readUInt32LE(at + 12);
      return {
        w: ico.readUInt8(at),
        h: ico.readUInt8(at + 1),
        png: ico.subarray(offset, offset + size),
      };
    });
    expect(entries.map((e) => [e.w, e.h])).toEqual([
      [16, 16],
      [32, 32],
    ]);
    for (const e of entries) expect(pngSize(e.png)).toEqual([e.w, e.h]);
  });

  it('is linked from index.html, the SVG beside the raster icons', () => {
    const html = readFileSync(resolve(__dirname, '../../index.html'), 'utf8');
    expect(html).toContain('<link rel="icon" type="image/svg+xml" href="/favicon.svg" />');
    expect(html).toContain('<link rel="icon" type="image/x-icon" href="/favicon.ico" />');
    expect(html).toContain(
      '<link rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png" />'
    );
  });

  it('has no badge image left', () => {
    expect(existsSync(pub('ckbadger-logo.webp'))).toBe(false);
  });
});
