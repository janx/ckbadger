import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { DaoBoard } from '@/components/dao/dao-board';
import { api } from '@/lib/api';
import { daoStatistics } from '@/__tests__/msw/fixtures/home';
import evidence from '@/__tests__/fixtures/dao-evidence.json';
import {
  PALETTE,
  PALETTE_KEYS,
  contrastRatio,
  inkOnFill,
  overGround,
  type PaletteKey,
  type ThemeName,
} from '@/lib/palette';
import { resetThemeForTests } from '@/lib/theme';

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

// UI review 2026-11-14 (lane A5's hand-over to B2): the words inside a box of the DAO issuance
// icicle were inked by hand per node ('dark' / 'bright', the unit in the text ink, the note in the
// dim ink or the dark ink at 70 %). Every word inside a box takes the one ink the box's own fill
// calls for in the theme in force (`inkOnFill`), so each reads at 4.5:1 or more in both themes.

const isKey = (key: string): key is PaletteKey => (PALETTE_KEYS as string[]).includes(key);

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

describe.each(['dark', 'light'] as const)('the DAO issuance icicle in the %s theme', (theme) => {
  it('sets every word inside a box in the ink its fill calls for, at 4.5:1 or more', () => {
    inTheme(theme);
    stubChartWidth(1232);
    vi.mocked(api.getDaoArHistory).mockResolvedValue({
      anchor: { blockNumber: 4, blockHash: '0x' + '4'.repeat(64) },
      days: [],
    });
    render(<DaoBoard statistics={statistics} evidence={evidence as never} />);
    const svg = screen.getByRole('img', { name: /^Secondary issuance since genesis, split/ });
    const labels = [...svg.querySelectorAll<SVGTextElement>('[data-label]')];
    expect(labels.length).toBeGreaterThanOrEqual(4);
    const short: string[] = [];
    for (const label of labels) {
      const node = label.getAttribute('data-label')!;
      const box = svg.querySelector(`[data-node="${node}"]`)!;
      const key = /^var\(--c-([a-z0-9-]+)\)$/.exec(box.getAttribute('fill') ?? '')?.[1] ?? '';
      expect(isKey(key), `${node}: fill ${box.getAttribute('fill')}`).toBe(true);
      const alpha = Number(box.getAttribute('fill-opacity') ?? '1');
      const want = inkOnFill(theme, key as PaletteKey, alpha).ink;
      // every word of the label is in that one ink, at full strength
      for (const el of [label, ...label.querySelectorAll('tspan')]) {
        expect(el.getAttribute('fill-opacity'), `${node} word opacity`).toBeNull();
        const inks = [...el.classList].filter((c) => c.startsWith('fill-'));
        if (el === label) expect(inks, node).toEqual([`fill-${want}`]);
        else expect(inks, `${node} "${el.textContent}"`).toEqual([]);
      }
      const ratio = contrastRatio(
        PALETTE[theme][want],
        overGround(theme, key as PaletteKey, alpha)
      );
      if (ratio < 4.5) short.push(`${node}: ${want} ${ratio.toFixed(2)}`);
    }
    expect(short).toEqual([]);
    // the burnt box's note is one of those words (it was the dim ink on the slate: about 3.5:1)
    expect(
      within(svg as unknown as HTMLElement).getByText(/no cell can ever claim it/)
    ).toBeTruthy();
  });
});
