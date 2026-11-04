import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { MemoryRouter, useRoutes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { Atmosphere } from '@/components/layout/atmosphere';
import { ActivityRow, type ActivityRowData } from '@/components/activity/activity-row';
import { ChartFigure } from '@/components/home/mini-chart';
import { ParticipantRefView } from '@/components/participant-ref';
import { DetailLayout, DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { LockKind } from '@/components/ui/lock-kind';
import { rowTagClass } from '@/components/ui/row-tag';
import { TerminalPanel, TerminalPanelHeader } from '@/components/ui/terminal-panel';
import { TintedJson } from '@/components/ui/tinted-json';
import { TopicHero } from '@/components/ui/topic-hero';
import type { ParticipantInfo } from '@/lib/api';
import { createAppRouter } from '@/src/routes/router';
import tailwindConfig from '@/tailwind.config';

// Material pass v2, citypop edition (review 2026-11-03): six moves on shared pieces. Where a move
// is CSS only, the test pins the rule in globals.css and the class a component renders for it;
// where it changes markup, the markup. The boards: Material.dc.html, TxMaterial.dc.html.

vi.mock('@/components/layout/site-footer', () => ({
  SiteFooter: () => <div data-testid="site-footer">SiteFooter</div>,
}));
vi.mock('@/app/page', () => ({
  default: () => <div>home page</div>,
}));
vi.mock('@/app/dao/page', async () => {
  const { TapOrHoverHint } = await import('@/components/ui/tap-or-hover-hint');
  return {
    default: () => (
      <div>
        dao page · <TapOrHoverHint what="a point" /> · <TapOrHoverHint what="a bar" />
      </div>
    ),
  };
});

const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');

describe('the rules in globals.css', () => {
  it('move 1: the night sky — the ground gradient, the pools from the tint variables, the grain', () => {
    const sky = css.match(/\.ambient-glow\s*\{([^}]*)\}/)!;
    expect(sky[1]).toContain(
      'background: linear-gradient(180deg, #0b0d17 0%, #08090e 55%, #09090d 100%)'
    );
    expect(sky[1]).toContain('z-index: -1');
    const pools = css.match(/\.ambient-glow::before\s*\{([^}]*)\}/)!;
    expect(pools[1]).toContain('var(--pool-1) 0%, transparent 70%');
    expect(pools[1]).toContain('var(--pool-2) 0%, transparent 70%');
    expect(pools[1]).toContain('var(--pool-3) 0%, transparent 70%');
    // jade by default (the approved .10 / .07 / .07), on :root
    expect(css).toMatch(
      /--pool-1: rgba\(46, 219, 163, 0\.1\);\s*--pool-2: rgba\(104, 204, 240, 0\.07\);\s*--pool-3: rgba\(242, 197, 92, 0\.07\);/
    );
    // the class hues, from the class colour tokens already declared: no new colour token
    expect(css).toMatch(
      /\[data-tint='gold'\]\s*\{\s*--pool-1: color-mix\(in srgb, var\(--color-class-dao\) 10%, transparent\);/
    );
    expect(css).toMatch(
      /\[data-tint='azure'\]\s*\{\s*--pool-1: color-mix\(in srgb, var\(--color-class-token\) 10%/
    );
    expect(css).toMatch(
      /\[data-tint='violet'\]\s*\{\s*--pool-1: color-mix\(in srgb, var\(--color-class-object\) 10%/
    );
    expect(css).toMatch(
      /\[data-tint='rose'\]\s*\{\s*--pool-1: color-mix\(in srgb, var\(--color-class-identity\) 10%/
    );
    const grain = css.match(/\.crt-grain\s*\{([^}]*)\}/)!;
    expect(grain[1]).toContain("background-image: url('/noise.png')");
    // visual finish V3: a 128 px high-frequency tile at 3 %, drawn at its own size
    expect(grain[1]).toContain('opacity: 0.03;');
    expect(grain[1]).toContain('background-size: 128px 128px');
    expect(grain[1]).toContain('mix-blend-mode: screen');
    expect(grain[1]).toContain('pointer-events: none');
    // under the scanlines (9999), over the content
    expect(grain[1]).toContain('z-index: 9997');
  });

  // Visual finish V1 (plan 2026-11-04): the vignette darkens the sky toward the corners, never the
  // ink — it sits under the content (z −1, after the sky in the DOM), lighter and wider.
  it('move 1 (visual finish V1): the vignette under the content', () => {
    const vignette = css.match(/\.crt-vignette\s*\{([^}]*)\}/)!;
    expect(vignette[1]).toContain('z-index: -1');
    expect(vignette[1]).not.toContain('z-index: 9998');
    expect(vignette[1]).toContain(
      'background: radial-gradient(ellipse at center, transparent 45%, rgba(0, 0, 0, 0.35) 100%)'
    );
    expect(vignette[1]).toContain('position: fixed');
    expect(vignette[1]).toContain('inset: 0');
    expect(vignette[1]).toContain('pointer-events: none');
  });

  // Visual finish V3: the grain tile is regenerated as 128 × 128 Gaussian white noise (grain, not
  // the old 256 px fractal clouds); the PNG's IHDR carries the size (bytes 16–24, big-endian).
  it('move 1 (visual finish V3): the noise tile is 128 × 128', () => {
    const png = readFileSync(resolve(__dirname, '../../public/noise.png'));
    expect(png.subarray(1, 4).toString('ascii')).toBe('PNG');
    expect(png.subarray(12, 16).toString('ascii')).toBe('IHDR');
    expect(png.readUInt32BE(16)).toBe(128);
    expect(png.readUInt32BE(20)).toBe(128);
  });

  // Visual finish V2: the scanlines stay over the content but draw 1 px in 3, in a blue-black at
  // 6 % — glass, not dirt.
  it('move 1 (visual finish V2): the scanlines, 1 px in 3 at 6 %', () => {
    const scan = css.match(/\.crt-scanlines\s*\{([^}]*)\}/)!;
    expect(scan[1]).toMatch(
      /background: repeating-linear-gradient\(\s*0deg,\s*transparent 0px,\s*transparent 2px,\s*rgba\(10, 14, 28, 0\.06\) 2px,\s*rgba\(10, 14, 28, 0\.06\) 3px\s*\);/
    );
    expect(scan[1]).not.toContain('rgba(0, 0, 0, 0.09)');
    expect(scan[1]).toContain('z-index: 9999');
    expect(scan[1]).toContain('pointer-events: none');
  });

  it('move 2: glass, the well and the tier-2 ground', () => {
    const glass = css.match(/\.glass\s*\{([^}]*)\}/)!;
    // visual finish V4, the bezel: a 3 % white sheen over the top 56 px of the 72 % glass, a lit
    // top edge, a dark bottom edge, and a 1 px dark line just outside the border — no drop shadow
    expect(glass[1]).toMatch(
      /background:\s*linear-gradient\(180deg, rgba\(255, 255, 255, 0\.03\), transparent 56px\),\s*rgba\(16, 19, 28, 0\.72\);/
    );
    // exactly these three, split at the commas outside the colours' parentheses
    const shadows = glass[1]
      .match(/box-shadow:([^;]*);/)![1]
      .split(/,(?![^(]*\))/)
      .map((v) => v.trim());
    expect(shadows).toEqual([
      'inset 0 1px 0 rgba(255, 255, 255, 0.1)',
      'inset 0 -1px 0 rgba(0, 0, 0, 0.55)',
      '0 0 0 1px rgba(0, 0, 0, 0.45)',
    ]);
    // no backdrop-filter: nothing behind a frame has detail to blur (see the comment)
    expect(glass[1]).not.toContain('backdrop-filter');
    const well = css.match(/\.well\s*\{([^}]*)\}/)!;
    expect(well[1]).toContain('background: #0a0c12');
    expect(well[1]).toContain('border: 1px solid #1a1f30');
    expect(well[1]).toContain('border-radius: var(--well-radius, 3px)');
    // visual finish V4: the well keeps its inset and gains a 4 % lower lip
    expect(well[1]).toMatch(
      /box-shadow:\s*inset 0 1px 3px rgba\(0, 0, 0, 0\.65\),\s*0 1px 0 rgba\(255, 255, 255, 0\.04\);/
    );
    const ground = css.match(/\.tier2-ground\s*\{([^}]*)\}/)!;
    expect(ground[1]).toContain('background: rgba(6, 7, 11, 0.9)');
    expect(ground[1]).toMatch(
      /margin-left: -1rem;\s*margin-right: -1rem;\s*padding-left: 1rem;\s*padding-right: 1rem;/
    );
    expect(css).toMatch(
      /@media \(min-width: 1024px\)\s*\{\s*\.tier2-ground\s*\{\s*margin-left: -1\.5rem;\s*margin-right: -1\.5rem;\s*padding-left: 1\.5rem;\s*padding-right: 1\.5rem;/
    );
  });

  it('move 3: the three more faces of the one family and the weights', () => {
    for (const [weight, style, file] of [
      ['300', 'normal', 'light'],
      ['600', 'normal', 'semibold'],
      ['400', 'italic', 'italic'],
    ])
      expect(css).toMatch(
        new RegExp(
          `@font-face \\{\\s*font-family: 'JetBrains Mono';\\s*font-style: ${style};\\s*font-weight: ${weight};\\s*font-display: swap;\\s*src: url\\('/fonts/jetbrains-mono-${file}\\.woff2'\\) format\\('woff2'\\);`
        )
      );
    // no second family: every @font-face is JetBrains Mono (the error pages' serif left with the vessels)
    const families = [...css.matchAll(/@font-face \{\s*font-family: '([^']+)'/g)].map((m) => m[1]);
    expect(families.filter((f) => f !== 'JetBrains Mono')).toEqual([]);
    // visual finish V12: the display figure sits tighter (−0.03 em), an entity / index h1 a
    // little (−0.02 em); size and weight unchanged, a tier-1 h2 untouched
    expect(css).toMatch(
      /\n\.text-display\s*\{\s*font-weight: 300;\s*letter-spacing: -0\.03em;\s*\}/
    );
    expect(css).toMatch(
      /@media \(min-width: 640px\)\s*\{\s*\.sm\\:text-display\s*\{\s*font-weight: 300;\s*letter-spacing: -0\.03em;\s*\}\s*\}/
    );
    expect(css).toMatch(
      /h1\.text-entity,\s*h1\.text-index,\s*h2\.text-section\s*\{\s*font-weight: 600;\s*\}/
    );
    expect(css).toMatch(
      /\nh1\.text-entity,\s*h1\.text-index\s*\{\s*letter-spacing: -0\.02em;\s*\}/
    );
    expect(css).not.toMatch(/h2\.text-section\s*\{[^}]*letter-spacing/);
    expect(css).toMatch(
      /\.eyebrow\s*\{\s*text-transform: uppercase;\s*letter-spacing: 0\.12em;\s*font-weight: 500;\s*\}/
    );
  });

  it('move 4: the flare keyframes — only `from` is written, so the end state is the element’s own', () => {
    const flare = css.match(/@keyframes phosphor-flare\s*\{([\s\S]*?)\n\}/)!;
    expect(flare[1]).toMatch(/from\s*\{\s*color: #e9fff6;/);
    expect(flare[1]).toContain('0 0 2px rgba(233, 255, 246, 0.9)');
    expect(flare[1]).toContain('0 0 10px rgba(46, 219, 163, 0.55)');
    expect(flare[1]).toContain('0 0 28px rgba(46, 219, 163, 0.3)');
    expect(flare[1]).not.toMatch(/\bto\b|100%/);
    expect(css).toMatch(/\.phosphor-flare\s*\{\s*animation: phosphor-flare 1\.2s ease-out;\s*\}/);
    expect(css).toMatch(
      /@media \(prefers-reduced-motion: reduce\)\s*\{\s*\.phosphor-flare\s*\{\s*animation: none;/
    );
  });

  // Visual finish V7 (plan 2026-11-04): a page arrives — it fades in over 160 ms from 2 px below
  // (only `from` is written, so it ends on the element's own); under reduced motion it just is.
  it('visual finish V7: the arrival fade, and none under reduced motion', () => {
    expect(css).toMatch(/\n\.arrive\s*\{\s*animation: arrive 160ms ease-out backwards;\s*\}/);
    const arrive = css.match(/@keyframes arrive\s*\{([\s\S]*?)\n\}/)!;
    expect(arrive[1]).toMatch(/from\s*\{\s*opacity: 0;\s*transform: translateY\(2px\);\s*\}/);
    expect(arrive[1]).not.toMatch(/\bto\b|100%/);
    expect(css).toMatch(
      /@media \(prefers-reduced-motion: reduce\)\s*\{\s*\.arrive\s*\{\s*animation: none;\s*\}\s*\}/
    );
  });

  // A finished fade leaves nothing behind: a forward fill would hold the end of
  // `translateY(2px) → none` as an identity matrix (not `none`), so the page wrapper and every
  // DetailLayout root would stay the containing block of their fixed children and a stacking
  // context under the sticky header. Only a backwards fill (the first frame before it starts).
  it('visual finish V7: the arrival fill is backwards only, never forwards', () => {
    const rules = [...css.matchAll(/\.arrive\s*\{([^}]*)\}/g)].map((m) => m[1]);
    expect(rules.length).toBeGreaterThan(0);
    for (const rule of rules) {
      expect(rule).not.toMatch(/\b(both|forwards)\b/);
      expect(rule).not.toMatch(/animation-fill-mode/);
    }
    expect(css).not.toMatch(/animation-name:\s*arrive/);
  });

  it('move 5: small caps and the jade underline', () => {
    expect(css).toMatch(
      /\.small-caps\s*\{\s*font-variant: small-caps;\s*letter-spacing: 0\.04em;\s*\}/
    );
    expect(css).toMatch(
      /\.this-address\s*\{\s*border-bottom: 1px solid rgba\(46, 219, 163, 0\.6\);\s*\}/
    );
  });

  it('move 6: the container gutter is 16 px, 24 px from lg, from the config', () => {
    expect(tailwindConfig.theme.container.padding).toEqual({ DEFAULT: '1rem', lg: '1.5rem' });
  });
});

describe('move 1 on the page root', () => {
  beforeEach(() => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }],
      defaultNetwork: 'mainnet',
    };
    window.scrollTo = vi.fn();
  });
  afterEach(() => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  function Harness() {
    return useRoutes(createAppRouter());
  }

  it('mounts the four atmosphere layers: sky, vignette, grain, scanlines', () => {
    const { container } = render(<Atmosphere />);
    const layers = [...container.children].map((e) => e.className);
    // visual finish V1: the vignette right after the sky, so at the same z −1 it paints over the
    // sky and its pools and under every page element
    expect(layers).toEqual(['ambient-glow', 'crt-vignette', 'crt-grain', 'crt-scanlines']);
    for (const layer of container.children) expect(layer).toHaveAttribute('aria-hidden', 'true');
  });

  it('tints the page root from the route and paints the atmosphere once', async () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/dao']}>
        <Harness />
      </MemoryRouter>
    );
    await screen.findByText(/dao page/);
    const root = document.querySelector('[data-tint]') as HTMLElement;
    expect(root).toHaveAttribute('data-tint', 'gold');
    expect(root).toContainElement(screen.getByTestId('atmosphere-sky'));
    expect(root).toContainElement(screen.getByTestId('atmosphere-grain'));
    expect(document.querySelectorAll('.crt-scanlines')).toHaveLength(1);
    expect(document.querySelectorAll('.ambient-glow')).toHaveLength(1);
  });

  it('is jade on the home page, and lets one tap-or-hover hint per page through', async () => {
    render(
      <MemoryRouter initialEntries={['/mainnet']}>
        <Harness />
      </MemoryRouter>
    );
    await screen.findByText('home page');
    expect(document.querySelector('[data-tint]')).toHaveAttribute('data-tint', 'jade');
  });

  it('renders the first hint of the DAO page only (move 5: once per page)', async () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/dao']}>
        <Harness />
      </MemoryRouter>
    );
    await screen.findByText(/dao page/);
    const hints = [...document.querySelectorAll('[data-hint="tap-or-hover"]')];
    expect(hints.map((h) => h.textContent)).toEqual(['tap or hover a point']);
  });
});

describe('move 2 on the frame and the tier boundary', () => {
  it('sets the frame on glass, with the eyebrow at 500 / .12 em', () => {
    render(
      <TerminalPanel data-testid="frame">
        <TerminalPanelHeader>Cells</TerminalPanelHeader>
      </TerminalPanel>
    );
    const frame = screen.getByTestId('frame');
    expect(frame).toHaveClass('glass', 'border-base-border', 'neon-edge-top', 'rounded');
    expect(frame).not.toHaveClass('bg-base-surface');
    const eyebrow = screen.getByText('Cells');
    expect(eyebrow).toHaveClass('eyebrow', 'text-small', 'text-text-dim');
    expect(eyebrow).not.toHaveClass('uppercase', 'tracking-[0.08em]');
  });

  it('grounds tier 2 one step deeper, through the gutters, under its tube', () => {
    const { container } = render(
      <DetailTier2>
        <DetailSection title="Script groups">evidence</DetailSection>
      </DetailTier2>
    );
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(tier2).toHaveClass('tier2-ground', 'hairline-tube-border', 'border-t', 'pb-10');
    const eyebrow = tier2.firstElementChild as HTMLElement;
    expect(eyebrow).toHaveTextContent('Evidence · for developers');
    expect(eyebrow).toHaveClass('eyebrow', 'text-meta', 'text-text-dim');
  });

  it('puts raw JSON in a well', () => {
    const { container } = render(<TintedJson text='{"a": 1}' />);
    const pre = container.querySelector('pre')!;
    expect(pre).toHaveClass('well');
    expect(pre).not.toHaveClass('bg-base-surface', 'rounded');
  });
});

describe('move 3 on the headings and the display figure', () => {
  it('leaves the weight of an h1, a tier-1 h2 and the display figure to the rules', () => {
    render(
      <DetailLayout title="Transaction" tier2={<DetailSection title="Groups">x</DetailSection>}>
        <DetailSection title="Balance changes">y</DetailSection>
      </DetailLayout>
    );
    expect(screen.getByRole('heading', { level: 1 })).not.toHaveClass('font-medium');
    expect(screen.getByRole('heading', { name: 'Balance changes' })).toHaveClass('text-section');
    expect(screen.getByRole('heading', { name: 'Balance changes' })).not.toHaveClass('font-medium');
    // a tier-2 h2 stays the quiet 500
    expect(screen.getByRole('heading', { name: 'Groups' })).toHaveClass('text-body', 'font-medium');
  });

  it('sets a topic page’s figure label as an eyebrow and its figure without a weight class', () => {
    render(
      <TopicHero
        title="Fiber"
        lede="One sentence."
        layout="figure"
        figures={[]}
        figure={{ label: 'channels open', value: '1,234', unit: 'channels' }}
      />
    );
    expect(screen.getByRole('heading', { level: 1 })).not.toHaveClass('font-medium');
    expect(screen.getByText('channels open')).toHaveClass('eyebrow', 'text-meta', 'text-text-dim');
    expect(screen.getByText('channels open')).not.toHaveClass('uppercase', 'tracking-[0.08em]');
    const figure = screen.getByText('1,234');
    expect(figure).toHaveClass('text-index', 'sm:text-display');
    expect(figure).not.toHaveClass('font-medium');
  });
});

describe('move 4 on a home figure', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('flares the headline when its served value changes, not on mount, not for a readout', () => {
    const body = { state: 'ready', chart: <svg /> } as const;
    const { rerender } = render(
      <ChartFigure testId="fig" title="Block time" subtitle="daily" headline="8.96" body={body} />
    );
    const headline = () => screen.getByTestId('fig-headline');
    expect(headline()).not.toHaveClass('phosphor-flare');
    rerender(
      <ChartFigure testId="fig" title="Block time" subtitle="daily" headline="9.01" body={body} />
    );
    expect(headline()).toHaveClass('phosphor-flare');
    vi.advanceTimersByTime(1200);
    rerender(
      <ChartFigure testId="fig" title="Block time" subtitle="daily" headline="9.01" body={body} />
    );
    expect(headline()).not.toHaveClass('phosphor-flare');
  });
});

describe('move 5 on the activity row', () => {
  const TX = '0xa46efcd8' + 'ef'.repeat(26) + 'dac2e0';
  const party = (tail: string, over: Partial<ParticipantInfo> = {}): ParticipantInfo => ({
    address: `ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq${tail}`,
    lockHash: `0x${tail.padStart(64, '0')}`,
    lockHashPrefix: null,
    lockKind: 'Default Lock',
    roles: [],
    tags: 0,
    ckbDelta: '0',
    usedDelta: '0',
    itemDeltas: [],
    ...over,
  });
  const sender = party('sendr1', { ckbDelta: '-326700000000', usedDelta: '-323100000000' });
  const receiver = party('hprdnn', { ckbDelta: '326699990000', usedDelta: '323100000000' });
  const transfer: ActivityRowData = {
    txHash: TX,
    blockNumber: 17627981,
    timestamp: '1761174000000',
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    participants: [sender, receiver],
  };
  const lines = (c: HTMLElement) => [
    ...c.querySelectorAll<HTMLElement>('[data-role="participant"]'),
  ];

  it('writes the lock kind as dim small caps, never a chip', () => {
    const { container } = render(<ActivityRow activity={transfer} />);
    for (const line of lines(container)) {
      const kind = line.querySelector('[data-role="lock-kind"]') as HTMLElement;
      expect(kind).toHaveTextContent('Default Lock');
      expect(kind).toHaveClass('small-caps', 'lowercase', 'text-text-dim', 'whitespace-nowrap');
      expect(kind).not.toHaveClass('border', 'rounded-sm', 'text-meta');
    }
    expect(container.querySelector('[data-row-tag]')).toBeNull();
  });

  it('underlines the party a page is about in jade, and says "this address" only to a reader', () => {
    const { container } = render(
      <ActivityRow activity={transfer} focusLockHash={receiver.lockHash!} />
    );
    const [focus, other] = lines(container);
    const party = focus.querySelector('[data-role="party"]') as HTMLElement;
    expect(party).toHaveAttribute('data-this-address', 'true');
    expect(within(party).getByTitle(receiver.address!)).toHaveClass('this-address');
    const words = within(party).getByText('this address');
    expect(words).toHaveClass('sr-only');
    expect(container.querySelector('[data-role="this-address"]')).toBeNull();
    expect(other.querySelector('[data-role="party"]')).not.toHaveAttribute('data-this-address');
    expect(within(other).queryByText('this address')).toBeNull();
    expect(within(other).getByRole('link')).not.toHaveClass('this-address');
  });

  it('is the not-linked compact party that carries the underline (ParticipantRefView)', () => {
    render(<ParticipantRefView participant={sender} compact quiet linked={false} />);
    expect(screen.getByTitle(sender.address!)).toHaveClass('this-address', 'text-text-dim');
  });

  it('LockKind: dim small caps by default; a row chip’s classes replace them', () => {
    const code = `0x${'9b'.repeat(32)}`;
    const { rerender } = render(<LockKind lockKind="JoyID" lockCodeHash={code} />);
    expect(screen.getByText('JoyID')).toHaveClass('small-caps', 'lowercase', 'text-text-dim');
    rerender(<LockKind lockKind="JoyID" lockCodeHash={code} className={rowTagClass()} />);
    expect(screen.getByText('JoyID')).not.toHaveClass('small-caps', 'lowercase');
    expect(screen.getByText('JoyID')).toHaveClass('border', 'rounded-sm');
  });
});
