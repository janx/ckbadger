import { readFileSync, statSync } from 'node:fs';
import { resolve } from 'node:path';
import { hexBytes, validateByteRanges } from '@/lib/hex-bytes';
import { HUE_RAMP, CLASS_COLORS, BITCOIN_COLORS } from '@/lib/design-tokens';

it('strictly decodes chain hex and validates byte ownership', () => {
  expect(hexBytes('0x00aAFF', 'test')).toEqual(['00', 'aa', 'ff']);
  expect(() => hexBytes('0x0g', 'test')).toThrow('test: expected byte-aligned');
  expect(() =>
    validateByteRanges([{ id: 'a', label: 'A', offset: -1, length: 1, description: '' }], 1, 'cell')
  ).toThrow('cell: field a');
});
it('keeps class/copper encodings separate and bundles fonts without runtime requests', () => {
  expect(new Set(Object.values(CLASS_COLORS)).size).toBe(6);
  expect(BITCOIN_COLORS.copper).toBe('#dc8a5a');
  expect(HUE_RAMP).not.toContain(CLASS_COLORS.dao);
  const html = readFileSync(resolve('index.html'), 'utf8');
  expect(html).not.toMatch(/fonts\.googleapis|fonts\.gstatic/);
  for (const font of ['jetbrains-mono-regular', 'jetbrains-mono-medium', 'noto-serif-display'])
    expect(statSync(resolve(`public/fonts/${font}.woff2`)).size).toBeGreaterThan(1000);
  const css = readFileSync(resolve('app/globals.css'), 'utf8');
  expect(css).toContain('Noto Serif Display');
  expect(css).toContain('.crt-screen::before');
});
it('paints every scrollbar in the palette, thin inside the page, so a region that scrolls inside itself shows no platform-grey bar', () => {
  // The vitals strip and the pipeline scroll inside themselves below md; with classic (non-overlay)
  // scrollbars that bar is the platform's light one unless the page colours it.
  const css = readFileSync(resolve('app/globals.css'), 'utf8');
  expect(css).toMatch(/:root\s*\{[^}]*scrollbar-color:\s*#343c50 transparent;/);
  expect(css).toMatch(/body \*\s*\{\s*scrollbar-width:\s*thin;\s*\}/);
});
