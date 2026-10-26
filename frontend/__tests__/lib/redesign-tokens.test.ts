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
