import { render } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { alphaOf, declaration, inTheme, ruleOf, stopsOf } from '@/__tests__/utils/css-paint';
import { Atmosphere } from '@/components/layout/atmosphere';
import { hexAlpha, PALETTE, type ThemeName } from '@/lib/palette';
import { resetThemeForTests, THEME_STORAGE_KEY } from '@/lib/theme';

// Readability R1 (Gate F0; the owner: by night the scan is cut into a figure's fills, and no layer
// lies over the text). The window-wide scan (`.crt-scanlines`, fixed at z 9999) was seen by night
// only where it crossed something bright, and that was the text: on the rows it hit the strokes
// lost 41–43 % of their luminance, and every glyph re-striped with each pixel of scroll. By day
// the same layer is ruled paper, the day ink at 4.5 %, landing on the paper and leaving the ink
// whole: the day keeps it as it is.

const THEMES: readonly ThemeName[] = ['dark', 'light'];

/** The layers Atmosphere mounts over the content (z-index above 0) that draw lines. */
function scansOverTheContent(container: HTMLElement): string[] {
  return [...container.children]
    .map((layer) => layer.className)
    .filter((className) => {
      const rule = ruleOf(className);
      const z = Number(declaration(rule, 'z-index') ?? '0');
      return z > 0 && (declaration(rule, 'background') ?? '').includes('repeating-linear-gradient');
    });
}

function renderIn(theme: ThemeName) {
  localStorage.setItem(THEME_STORAGE_KEY, theme);
  resetThemeForTests();
  document.documentElement.setAttribute('data-theme', theme);
  return render(<Atmosphere />);
}

describe('Atmosphere by theme (readability R1)', () => {
  afterEach(() => {
    localStorage.clear();
    resetThemeForTests();
    document.documentElement.setAttribute('data-theme', 'dark');
  });

  it('mounts the same layers in both themes: the theme lives in the materials, not in the tree', () => {
    const layers = THEMES.map((theme) => {
      const { container, unmount } = renderIn(theme);
      const classes = [...container.children].map((e) => e.className);
      unmount();
      return classes;
    });
    expect(layers[0]).toEqual(layers[1]);
    // one layer over the content draws lines: the scan
    const { container } = renderIn('dark');
    expect(scansOverTheContent(container)).toEqual(['crt-scanlines']);
  });

  it('lays no scan over the content by night: every line of the layer over the page is transparent', () => {
    const { container } = renderIn('dark');
    for (const layer of scansOverTheContent(container)) {
      const stops = stopsOf(inTheme('dark', declaration(ruleOf(layer), 'background')!));
      expect(stops.length, layer).toBeGreaterThan(0);
      for (const stop of stops) expect(alphaOf(stop.colour), `${layer} ${stop.colour}`).toBe(0);
    }
  });

  it('keeps the ruled paper by day: the day ink at 4.5 % in 1 px of every 3, over the page', () => {
    const { container } = renderIn('light');
    const [layer] = scansOverTheContent(container);
    const rule = ruleOf(layer);
    expect(declaration(rule, 'position')).toBe('fixed');
    expect(declaration(rule, 'z-index')).toBe('9999');
    const ink = hexAlpha(PALETTE.light['text-bright'], 0.045);
    expect(stopsOf(inTheme('light', declaration(rule, 'background')!))).toEqual([
      { colour: 'transparent', at: 0 },
      { colour: 'transparent', at: 2 },
      { colour: ink, at: 2 },
      { colour: ink, at: 3 },
    ]);
  });
});
