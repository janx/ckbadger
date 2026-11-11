import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { LIGHT_SCHEME_QUERY, THEME_STORAGE_KEY } from '@/lib/theme';

/**
 * index.html carries an inline script that sets `data-theme` before the first paint, from the
 * same storage key and by the same rules as lib/theme.ts. The script is run here against a fake
 * document, storage and matchMedia for every state it must handle.
 */
const html = readFileSync(resolve(__dirname, '../../index.html'), 'utf8');

function bootScript(): string {
  const scripts = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map((m) => m[1]);
  const boot = scripts.find((s) => s.includes(THEME_STORAGE_KEY));
  if (!boot) throw new Error('index.html has no inline theme boot script');
  return boot;
}

function boot(stored: string | null, osLight: boolean, storageThrows = false): string | null {
  const root = { attrs: new Map<string, string>() };
  const document = {
    documentElement: {
      setAttribute: (name: string, value: string) => root.attrs.set(name, value),
    },
  };
  const localStorage = {
    getItem: (key: string) => {
      if (storageThrows) throw new Error('blocked');
      return key === THEME_STORAGE_KEY ? stored : null;
    },
  };
  const matchMedia = (query: string) => {
    if (query !== LIGHT_SCHEME_QUERY) throw new Error(`unexpected query ${query}`);
    return { matches: osLight };
  };
  new Function('localStorage', 'matchMedia', 'document', bootScript())(
    localStorage,
    matchMedia,
    document
  );
  return root.attrs.get('data-theme') ?? null;
}

describe('the theme boot script in index.html', () => {
  it('starts the page dark by default, in its markup and in the script', () => {
    expect(html).toMatch(/<html lang="en" data-theme="dark">/);
    expect(boot(null, false)).toBe('dark');
    expect(boot(null, true)).toBe('dark');
    expect(boot('midnight', true)).toBe('dark');
  });

  it('honours a stored choice', () => {
    expect(boot('light', false)).toBe('light');
    expect(boot('dark', true)).toBe('dark');
  });

  it('follows the OS only when the stored choice is system', () => {
    expect(boot('system', true)).toBe('light');
    expect(boot('system', false)).toBe('dark');
  });

  it('falls back to dark when storage cannot be read', () => {
    expect(boot('light', true, true)).toBe('dark');
  });

  it('runs before any stylesheet or module, in the head', () => {
    const head = html.slice(0, html.indexOf('</head>'));
    expect(head).toContain(THEME_STORAGE_KEY);
    expect(head.indexOf(THEME_STORAGE_KEY)).toBeLessThan(head.indexOf('runtime-config.js'));
  });
});
