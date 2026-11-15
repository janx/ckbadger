import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { blockEvidence } from '@/__tests__/msw/fixtures/block-evidence';
import { render } from '../utils/test-utils';
import { atContainerWidth } from '@/__tests__/utils/phone-row';
import BlocksPage from '@/app/blocks/page';
import { api } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';

vi.mock('@/lib/api', () => ({
  api: { getBlocks: vi.fn() },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

// as the blocks board's own test writes a row (blocks-page.test.tsx)
const BLOCK = {
  ...blockEvidence,
  number: 8_775_639,
  hash: `0x${'6b'.repeat(32)}`,
  parentHash: `0x${'11'.repeat(32)}`,
  timestamp: '2026-02-22T00:00:10Z',
  transactionsCount: 2,
  proposalsCount: 0,
  unclesCount: 0,
  difficulty: '0',
  epoch: '5414/1800',
  epochNumber: 5414,
  epochIndex: 8,
  epochLength: 1800,
  nonce: '0x0',
  transactionsRoot: `0x${'33'.repeat(32)}`,
  minerAddress: null,
  minerPool: null,
  minerMessage: null,
  miningReward: null,
  miningRewardTxHash: null,
  hardforkActivation: null,
  compactTarget: '0x0',
  version: 0,
};

/**
 * A4 review 2: the Height cell of the /blocks table is two lines, the block number's link and,
 * under it, the hash at the tight size (`block`). NoBreak's inline `display: inline` beat that
 * class, so the hash ran on after the number with no space ("#8,775,6390x6b6b…") and the second
 * line hung on an emergency wrap. The hash keeps its class; no display is written inline.
 */
describe('the /blocks Height cell', () => {
  beforeEach(() => {
    vi.mocked(api.getBlocks).mockResolvedValue({
      data: [BLOCK],
      total: 1,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    } as unknown as Awaited<ReturnType<typeof api.getBlocks>>);
  });

  it('puts the hash on its own block line under the number in the 608 px table', async () => {
    atContainerWidth(608);
    render(<BlocksPage />);
    const table = await screen.findByRole('table', { name: 'Latest blocks' });
    const number = [...table.querySelectorAll('a')].find(
      (a) => a.textContent === formatBlockNumber(BLOCK.number)
    );
    expect(number).toBeDefined();
    const hash = number!.nextElementSibling as HTMLElement;
    expect(hash).toHaveAttribute('data-nobreak');
    expect(hash.textContent).toMatch(/^0x6b/);
    expect(hash).toHaveClass('block', 'whitespace-nowrap');
    expect(hash.style.display).toBe('');
  });
});
