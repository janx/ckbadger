import { afterEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { server } from '@/__tests__/msw/server';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { ConcentrationBar } from '@/components/ui/concentration-bar';
import { HostingBar } from '@/app/network/distributions';
import { DaoBoard } from '@/components/dao/dao-board';
import { api } from '@/lib/api';
import { daoStatistics } from '@/__tests__/msw/fixtures/home';
import evidence from '@/__tests__/fixtures/dao-evidence.json';
import {
  PALETTE,
  PALETTE_KEYS,
  contrastRatio,
  hexToRgb,
  type PaletteKey,
  type ThemeName,
} from '@/lib/palette';
import { resetThemeForTests } from '@/lib/theme';
import { withGroupings } from '@/__tests__/utils/network-distributions';

vi.mock('@/components/dao/dao-sources', () => ({
  DaoSources: () => <div data-testid="sources" />,
}));
vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>();
  return {
    ...actual,
    api: { ...actual.api, calculateDaoCompensation: vi.fn(), getDaoArHistory: vi.fn() },
  };
});

// DY-03 (UI review 2026-11-14): the words inside a filled bar segment take the ink the fill's own
// luminance calls for, in each theme, so every in-bar label reads at ≥ 4.5:1 — never an ink chosen
// by the segment's rank, never the dim ink on a fill.

const isKey = (key: string): key is PaletteKey => (PALETTE_KEYS as string[]).includes(key);

/** A palette key at `alpha` over the ground, as the eye sees it (sRGB "over"). */
function over(theme: ThemeName, key: PaletteKey, alpha: number, ground: PaletteKey): string {
  const f = hexToRgb(PALETTE[theme][key]);
  const g = hexToRgb(PALETTE[theme][ground]);
  const c = (a: number, b: number) =>
    Math.round(alpha * a + (1 - alpha) * b)
      .toString(16)
      .padStart(2, '0');
  return `#${c(f.r, g.r)}${c(f.g, g.g)}${c(f.b, g.b)}`;
}

/** The segment's fill: `color-mix(… var(--c-<key>) N%, …)`, a `bg-<key>/N` or a `bg-<key>` class. */
function fillOf(el: HTMLElement): { key: PaletteKey; alpha: number } {
  const mixed = /var\(--c-([a-z0-9-]+)\)\s+([\d.]+)%/.exec(el.getAttribute('style') ?? '');
  if (mixed && isKey(mixed[1])) return { key: mixed[1], alpha: Number(mixed[2]) / 100 };
  for (const cls of Array.from(el.classList)) {
    const m = /^bg-([a-z-]+?)(?:\/(\d+))?$/.exec(cls);
    if (m && isKey(m[1])) return { key: m[1], alpha: m[2] === undefined ? 1 : Number(m[2]) / 100 };
  }
  throw new Error(`no fill on ${el.outerHTML.slice(0, 120)}`);
}

/** Text ink classes as palette keys (`text-text-dim` → text-dim, `text-base-bg` → base-bg). */
const INK_CLASS: Record<string, PaletteKey> = {
  'text-text-bright': 'text-bright',
  'text-text': 'text',
  'text-text-dim': 'text-dim',
  'text-text-ghost': 'text-ghost',
  'text-base-bg': 'base-bg',
  'text-base-void': 'base-void',
};

/** The ink a text node is set in: the nearest element up to `root` that names one. */
function inkOf(node: Node, root: HTMLElement): PaletteKey {
  for (let el = node.parentElement; el; el = el.parentElement) {
    const styled = /^var\(--c-([a-z0-9-]+)\)$/.exec(el.style.color);
    if (styled && isKey(styled[1])) return styled[1];
    const cls = Array.from(el.classList).find((c) => c in INK_CLASS);
    if (cls) return INK_CLASS[cls];
    if (el === root) break;
  }
  throw new Error(`no ink for "${node.textContent}" inside ${root.outerHTML.slice(0, 120)}`);
}

/** Every word inside the segment, with its contrast on the segment's fill over the page ground. */
function inks(segment: HTMLElement, theme: ThemeName) {
  const { key, alpha } = fillOf(segment);
  const fill = over(theme, key, alpha, 'base-bg');
  const walker = document.createTreeWalker(segment, NodeFilter.SHOW_TEXT);
  const out: { text: string; ink: PaletteKey; ratio: number }[] = [];
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (!n.textContent?.trim()) continue;
    const ink = inkOf(n, segment);
    out.push({
      text: n.textContent,
      ink,
      ratio: Math.round(contrastRatio(PALETTE[theme][ink], fill) * 100) / 100,
    });
  }
  return out;
}

function inTheme(theme: ThemeName) {
  localStorage.setItem('ckbadger.theme', theme);
  resetThemeForTests();
  document.documentElement.setAttribute('data-theme', theme);
}

afterEach(() => {
  localStorage.clear();
  resetThemeForTests();
  document.documentElement.setAttribute('data-theme', 'dark');
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

/** The DAO's four groups with a largest one of 11 %, whose words fit its segment at 1,232 px. */
const DAO_GROUPS_INSIDE = [
  { label: 'largest', value: BigInt(1100) },
  { label: 'next 9', value: BigInt(1783) },
  { label: 'next 90', value: BigInt(3091) },
  { label: 'the other 16,495', value: BigInt(4026) },
];

describe.each(['dark', 'light'] as const)('in-bar words in the %s theme (DY-03)', (theme) => {
  it('reads every group of the DAO concentration bar at 4.5:1 or more', () => {
    inTheme(theme);
    // Re-pinned (review-fb-tests finding 3): the bar was drawn 1,480 px wide (1,408 before R4) so
    // that mainnet's largest group, 8.23 %, held its words, a width no page gives it (the DAO
    // page's column is at most 1,232 px; at mainnet's shares the largest group's words stand in the
    // legend row there, dao-board.test.tsx). The bar is drawn at the page's widest, 1,232 px, with
    // a largest group of 11 %: "largest 11.00%" is 14 glyphs at 13 px and 2 × 10 px of padding,
    // 129.2 px of the segment's 135.5, so every group's words are inside and their ink is read.
    stubChartWidth(1232);
    render(
      <ConcentrationBar
        label="Share of deposits"
        assetClass="dao"
        total={BigInt(10000)}
        groups={DAO_GROUPS_INSIDE}
        labelsInside
      />
    );
    const bar = screen.getByRole('img', { name: /^Share of deposits/ });
    const words = Array.from(bar.children).flatMap((s) => inks(s as HTMLElement, theme));
    expect(words.map((w) => w.text)).toEqual([
      'largest 11.00%',
      'next 9 · 17.83%',
      'next 90 · 30.91%',
      'the other 16,495 · 40.26%',
    ]);
    expect(words.filter((w) => w.ratio < 4.5)).toEqual([]);
  });

  it('reads every share inside the network hosting bar at 4.5:1 or more', async () => {
    inTheme(theme);
    stubChartWidth(1232);
    server.use(
      http.get('/api/:network/v1/network/distributions', () =>
        HttpResponse.json(
          withGroupings({
            verifiedRetained: 78,
            sameNetworkReachable: 57,
            verifiedUnavailable: 21,
            versions: [],
            countries: [{ label: 'United States', count: 40 }],
            asns: [
              { label: 'AS16509 Amazon.com, Inc.', count: 48 },
              { label: 'AS24940 Hetzner Online GmbH', count: 5 },
              { label: 'AS14061 DigitalOcean, LLC', count: 5 },
              { label: 'AS16276 OVH SAS', count: 5 },
              { label: 'AS63949 Akamai Connected Cloud', count: 4 },
              { label: 'AS1 a', count: 4 },
              { label: 'AS2 b', count: 4 },
              { label: 'AS3 c', count: 3 },
            ],
            protocols: [],
          })
        )
      )
    );
    render(<HostingBar />);
    const bar = await screen.findByRole('img', { name: 'Verified nodes by hosting network' });
    const words = Array.from(bar.children).flatMap((s) => inks(s as HTMLElement, theme));
    // Re-pinned on purpose (readability P4, "a split bar names every segment in place"): a part
    // whose segment cannot hold its name, share and count writes nothing in it and is named on its
    // own line under the bar, so on this crawl only the leader's words stand on a fill (it was
    // `words.length >= 5`: the shares alone, "6.41%", in segments of 78 px). The fills of the other
    // steps keep their words in the case below, where every segment holds them.
    expect(words.map((w) => w.text)).toEqual(['Amazon.com, Inc. 61.54% · 48 nodes']);
    expect(words.filter((w) => w.ratio < 4.5)).toEqual([]);
  });

  // Every opacity step of the hosting bar (PROVIDER_OPACITY, 0.9 to 0.2) with words on it: five
  // networks of 12 nodes and "other networks" of 20 in 1,232 px (1,222 less the gaps, 15.275 px
  // a node): each of the five, 183.3 px, holds "a 15.00% · 12 nodes" (19 characters at 13 px,
  // 148.2 px, and 20 of padding); the other networks, 305.5 px, hold their 32 (249.6 and 20).
  it('reads the words on every step of the hosting bar at 4.5:1 or more', async () => {
    inTheme(theme);
    stubChartWidth(1232);
    server.use(
      http.get('/api/:network/v1/network/distributions', () =>
        HttpResponse.json(
          withGroupings({
            verifiedRetained: 80,
            sameNetworkReachable: 80,
            verifiedUnavailable: 0,
            versions: [],
            countries: [{ label: 'United States', count: 80 }],
            asns: ['a', 'b', 'c', 'd', 'e', 'f', 'g'].map((org, i) => ({
              label: `AS${i + 1} ${org}`,
              count: i < 5 ? 12 : 10,
            })),
            protocols: [],
          })
        )
      )
    );
    render(<HostingBar />);
    const bar = await screen.findByRole('img', { name: 'Verified nodes by hosting network' });
    const words = Array.from(bar.children).flatMap((s) => inks(s as HTMLElement, theme));
    expect(words).toHaveLength(6);
    expect(words.filter((w) => w.ratio < 4.5)).toEqual([]);
  });

  it('sets the claimable words in one ink that reads at 4.5:1 or more on their gold, no dim part', () => {
    inTheme(theme);
    stubChartWidth(800);
    vi.mocked(api.getDaoArHistory).mockResolvedValue({
      anchor: { blockNumber: 4, blockHash: '0x' + '4'.repeat(64) },
      days: [],
    });
    // The DAO board test's statistics at the evidence tip (#5): parts that partition the whole.
    const statistics = {
      ...daoStatistics,
      tipBlockNumber: 5,
      anchor: { blockNumber: 5, blockHash: '0x' + 'a5'.repeat(32) },
      miningReward: '100000000',
      depositCompensation: '300000000',
      totalCompensationPaid: '100000000',
      unclaimedCompensation: '200000000',
      burnt: '300000000',
      // the whole, the route's: 1 + 3 + 3 CKB (lane exactness, part A, A5 ii)
      secondaryIssuance: '700000000',
      tipSecondaryPool: '500000000',
      totalDeposited: '50000000000',
      totalDepositedCkb: '500',
      daoFreeCapacity: '40000000000',
      depositorAllocationWeight: '45000000000',
      tipTotalIssuance: '1000000000000',
      tipProtocolOccupiedCapacity: '100000000',
    };
    render(<DaoBoard statistics={statistics} evidence={evidence as never} />);
    const windows = screen.getByRole('region', { name: 'When deposits can leave' });
    const bar = within(windows).getByRole('img', { name: /^Withdrawal requests/ });
    const segment = bar.firstElementChild as HTMLElement;
    // the exact form on the fill (readability R5, re-pinned on purpose: was padded to 8 decimals)
    expect(segment).toHaveTextContent(/^claimable now 400 CKB · 1 cell$/);
    const words = inks(segment, theme);
    expect(new Set(words.map((w) => w.ink)).size).toBe(1);
    expect(words.filter((w) => w.ratio < 4.5)).toEqual([]);
  });
});
