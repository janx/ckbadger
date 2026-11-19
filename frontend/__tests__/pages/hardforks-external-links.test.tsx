import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { stubChartWidth } from '../utils/chart-width';
import producerMainnet from '@/__tests__/fixtures/network-api-producer/hardforks-mainnet.json';

// U-20 (UI review 2026-11-14): the hardforks page marked "CKB2021 overview ↗" as leaving the
// site and not its neighbours "Migration Guide", "RFC49 VM V2" and "RFC50 Spawn", which leave it
// too. One rule: an <a> that leaves the site says so, " ↗" after its words, and opens in a new
// tab without a referrer.

const getHardforksMock = vi.hoisted(() => vi.fn());

vi.mock('@/lib/api', () => ({
  api: {
    getHardforks: getHardforksMock,
    getBlock: vi.fn(() => Promise.reject(new Error('the hardforks page read a block'))),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

/** The lanes and the timeline's ends the captured producer payload predates (R11, R-10). */
const lanes = [
  { hashType: 'type', knownVersions: 59, liveReferences: '1830780' },
  { hashType: 'data1', knownVersions: 19, liveReferences: '98551' },
  { hashType: 'data2', knownVersions: 0, liveReferences: '0' },
  { hashType: 'data', knownVersions: 1, liveReferences: '472' },
];
const ends = {
  genesisTimestampMs: Date.parse('2019-11-15T21:49:50.812Z'),
  tipTimestampMs: Date.parse('2026-10-30T00:00:00.000Z'),
};

describe('the hardforks page’s links off the site (U-20)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    stubChartWidth(1200);
  });

  it('marks every link that leaves the site with " ↗", a new tab and no referrer', async () => {
    const { default: HardforksPage } = await import('@/app/hardforks/page');
    getHardforksMock.mockResolvedValue({ ...producerMainnet, ...ends, hashTypeLanes: lanes });
    render(<HardforksPage />);
    await screen.findByText('CKB Edition Mirana');

    const external = Array.from(document.querySelectorAll<HTMLAnchorElement>('a[href]')).filter(
      (a) => /^https?:\/\//.test(a.getAttribute('href')!)
    );
    // the two editions' overviews and their further resources, from the payload
    // re-pinned (lane c2-finish, rulings round 2): the ↗ is glued to the last word by a no-break
    // space (it was a space), so it never stands alone on a line
    expect(external.map((a) => a.textContent)).toEqual([
      'CKB2021 overview\u00a0↗',
      'Migration Guide\u00a0↗',
      'CKB2023 overview\u00a0↗',
      'RFC49 VM V2\u00a0↗',
      'RFC50 Spawn\u00a0↗',
    ]);
    for (const a of external) {
      expect(a, a.textContent!).toHaveAttribute('target', '_blank');
      expect(a, a.textContent!).toHaveAttribute('rel', 'noreferrer');
    }
    // and no link within the site carries the mark
    const internal = Array.from(document.querySelectorAll<HTMLAnchorElement>('a[href]')).filter(
      (a) => !external.includes(a)
    );
    expect(internal.length).toBeGreaterThan(0);
    for (const a of internal) expect(a, a.getAttribute('href')!).not.toHaveTextContent('↗');
  });
});
