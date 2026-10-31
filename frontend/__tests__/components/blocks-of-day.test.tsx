// Round 3 lane F (contracts R-B · R-10, CH-27): "Blocks of <date>" on a block chart's evidence.
import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { BlocksOfDay } from '@/components/charts/blocks-of-day';

describe('BlocksOfDay', () => {
  it('names the first and last block of the UTC+8 day, exactly, linked by hash', async () => {
    render(<BlocksOfDay date="2024-01-15" />);
    const line = await screen.findByTestId('blocks-of-day');
    expect(line).toHaveTextContent(
      'Blocks of 2024-01-15 (UTC+8): #999,000 – #1,000,000 · 1,001 blocks'
    );
    expect(screen.getByRole('link', { name: '#999,000' })).toHaveAttribute(
      'href',
      `/mainnet/blocks/0x${'0d'.repeat(32)}`
    );
    expect(screen.getByRole('link', { name: '#1,000,000' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/0xabc123def456789012345678901234567890123456789012345678901234abcd'
    );
  });

  it('reports a day the API cannot serve in place', async () => {
    render(<BlocksOfDay date="2001-01-01" />);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Blocks of 2001-01-01 (UTC+8): no indexed block falls on 2001-01-01 (UTC+8)'
    );
  });
});
