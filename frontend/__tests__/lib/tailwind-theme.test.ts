import { describe, expect, it, vi } from 'vitest';
import config from '../../tailwind.config';
import { themeBaseStyles, twColor } from '@/lib/palette';

type Colors = Record<string, string | Record<string, string>>;

describe('tailwind.config.ts draws every colour through the theme variables', () => {
  const colors = (config.theme?.extend?.colors ?? {}) as Colors;

  it('resolves each colour token to its palette channels, with the opacity modifier kept', () => {
    const flat: string[] = [];
    for (const value of Object.values(colors))
      if (typeof value === 'string') flat.push(value);
      else flat.push(...Object.values(value));
    expect(flat.length).toBeGreaterThan(50);
    for (const value of flat) expect(value).toMatch(/^rgb\(var\(--rgb-[a-z0-9-]+\) \/ /);
    expect((colors.jade as Record<string, string>).DEFAULT).toBe(twColor('jade'));
    expect((colors.base as Record<string, string>).bg).toBe(twColor('base-bg'));
    expect((colors.text as Record<string, string>).dim).toBe(twColor('text-dim'));
    expect(colors.copper).toBe(twColor('copper'));
    expect((colors.interactive as Record<string, string>).hover).toBe(twColor('jade'));
    expect((colors.emphasis as Record<string, string>).glow).toBe('rgb(var(--rgb-jade) / 0.25)');
  });

  it('scales every glow shadow by the theme’s glow factor', () => {
    const shadows = config.theme?.extend?.boxShadow as Record<string, string>;
    for (const value of Object.values(shadows)) {
      expect(value).toContain('var(--m-glow)');
      expect(value).not.toMatch(/#[0-9a-f]{6}/i);
    }
  });

  it('keys dark: variants on the data-theme attribute, never the OS', () => {
    expect(config.darkMode).toEqual(['selector', '[data-theme="dark"]']);
  });

  it('adds both themes’ variables as base styles from lib/palette.ts', () => {
    const [themePlugin] = config.plugins as { handler: (api: unknown) => void }[];
    const addBase = vi.fn();
    themePlugin.handler({ addBase });
    expect(addBase).toHaveBeenCalledTimes(1);
    expect(addBase).toHaveBeenCalledWith(themeBaseStyles());
  });
});
