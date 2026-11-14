import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { DeepForkAlert } from '@/components/deep-fork-alert';
import { DeepForkStatus } from '@/lib/api';
import { color, inkOnFill, type PaletteKey } from '@/lib/palette';
import { resetThemeForTests } from '@/lib/theme';

describe('DeepForkAlert', () => {
  const activeDeepFork: DeepForkStatus = {
    detected: true,
    detectedAt: '2024-01-15T10:30:00Z',
    depth: 50,
    dbTip: 1000,
    chainTip: 1050,
    forkPoint: 950,
  };

  const noDeepFork: DeepForkStatus = {
    detected: false,
    detectedAt: null,
    depth: null,
    dbTip: null,
    chainTip: null,
    forkPoint: null,
  };

  it('renders nothing when not detected', () => {
    const { container } = render(<DeepForkAlert status={noDeepFork} />);
    expect(container.firstChild).toBeNull();
  });

  it('renders alert details and details link when a deep fork is detected', () => {
    render(<DeepForkAlert status={activeDeepFork} />);

    expect(screen.getByText('Chain Fork Detected - Sync Paused')).toBeInTheDocument();
    expect(screen.getByText('50 blocks')).toBeInTheDocument();
    expect(screen.getByText('#1,000')).toBeInTheDocument();
    expect(screen.getByText('#1,050')).toBeInTheDocument();
    expect(screen.getByText('#950')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /View Details/i })).toHaveAttribute(
      'href',
      '/mainnet/forks'
    );
  });

  // UI review 2026-11-14 (U-08, the fill-ink rule): the banner is a fill. Its words take the one
  // ink its own luminance calls for (inkOnFill), in each theme — the button's words the ink of the
  // button's fill — never white at 70–80 % (white on the night cinnabar is about 3:1).
  it.each(['dark', 'light'] as const)(
    'sets every word on its cinnabar in the ink the fill calls for, in the %s theme',
    (theme) => {
      localStorage.setItem('ckbadger.theme', theme);
      resetThemeForTests();
      render(<DeepForkAlert status={activeDeepFork} />);
      const banner = screen.getByRole('heading', { level: 2 }).closest('.bg-negative')!;
      expect(banner).not.toBeNull();
      expect(banner.outerHTML).not.toMatch(/text-white|text-[a-z-]+\/\d/);
      const link = screen.getByRole('link', { name: /View Details/i });
      const walker = document.createTreeWalker(banner, NodeFilter.SHOW_TEXT);
      const short: string[] = [];
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        const text = n.textContent?.trim();
        if (!text || text === '⚠️') continue;
        const onButton = link.contains(n);
        const fill: PaletteKey = onButton ? 'rouge-bright' : 'rouge';
        const want = inkOnFill(theme, fill, 1);
        let ink = '';
        for (let el = n.parentElement; el && !ink; el = el.parentElement) ink = el.style.color;
        expect(ink, text).toBe(color(want.ink));
        if (want.contrast < 4.5) short.push(`${text}: ${want.contrast.toFixed(2)}`);
      }
      expect(short).toEqual([]);
      localStorage.clear();
      resetThemeForTests();
    }
  );

  // Rubbing × Song print, phase 5 (H6): the alert's h2 is not a tier-1 section, so it is not
  // `text-section` — whose CSS (h2.text-section) draws the cinnabar prompt square on the red
  // banner and sets the words lowercase — but a plain bold h2 on the type scale's body step.
  it('heads the alert with a plain bold h2: no section square, its words in their own case', () => {
    render(<DeepForkAlert status={activeDeepFork} />);
    const heading = screen.getByRole('heading', {
      level: 2,
      name: 'Chain Fork Detected - Sync Paused',
    });
    expect(heading).not.toHaveClass('text-section');
    expect(heading).toHaveClass('text-body', 'font-bold');
    expect(heading).not.toHaveClass('lowercase');
    expect(heading.className).not.toMatch(/prompt|section-mark/);
    expect(heading.querySelector('.section-mark, .prompt-square')).toBeNull();
  });
});
