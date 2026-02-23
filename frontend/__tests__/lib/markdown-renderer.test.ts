import { beforeEach, describe, expect, it, vi } from 'vitest';
import { parseMarkdownSourcePath } from '@/lib/ai/markdown-route';
import { MarkdownRenderError, renderMarkdownPage } from '@/lib/ai/markdown-renderer';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getBlocks: vi.fn(),
    getMinerAddressDistributionChart: vi.fn(),
  },
}));

describe('renderMarkdownPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders blocks list markdown', async () => {
    vi.mocked(api.getBlocks).mockResolvedValue({
      data: [
        {
          number: 123,
          hash: `0x${'a'.repeat(64)}`,
          parentHash: `0x${'b'.repeat(64)}`,
          timestamp: '2026-02-23T00:00:00Z',
          transactionsCount: 3,
          proposalsCount: 1,
          unclesCount: 0,
          difficulty: '1',
          epoch: '1/10',
          epochNumber: 1,
          epochIndex: 0,
          epochLength: 10,
          nonce: '0x0',
          transactionsRoot: `0x${'c'.repeat(64)}`,
          minerAddress: null,
          minerMessage: null,
          miningReward: null,
          miningRewardTxHash: null,
          hardforkActivation: null,
          compactTarget: '0x0',
          version: 0,
        },
      ],
      total: 1,
      limit: 1,
      hasMore: false,
      nextCursor: null,
    });

    const result = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/blocks'),
      searchParams: new URLSearchParams('limit=1'),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body).toContain('# Blocks');
    expect(result.body).toContain('| number | hash | txs | proposals | timestamp |');
    expect(result.body).toContain('| 123 |');
  });

  it('fails fast on invalid limit query param', async () => {
    await expect(
      renderMarkdownPage({
        page: parseMarkdownSourcePath('/blocks'),
        searchParams: new URLSearchParams('limit=0'),
        origin: 'http://localhost:3000',
      })
    ).rejects.toEqual(expect.objectContaining<Partial<MarkdownRenderError>>({ status: 400 }));
  });

  it('renders miner distribution chart markdown', async () => {
    vi.mocked(api.getMinerAddressDistributionChart).mockResolvedValue({
      title: 'Miner Distribution',
      totalBlocks: 100,
      data: [
        {
          address: `0x${'d'.repeat(64)}`,
          minerName: 'ExampleMiner',
          blocksMined: 42,
          percentage: '42%',
        },
      ],
    });

    const result = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/charts/miner-address-distribution'),
      searchParams: new URLSearchParams(),
      origin: 'http://localhost:3000',
    });

    expect(result.status).toBe(200);
    expect(result.body).toContain('# Chart miner-address-distribution');
    expect(result.body).toContain('ExampleMiner');
    expect(result.body).toContain('Total blocks: 100');
  });

  it('returns 404 for unknown chart slug', async () => {
    await expect(
      renderMarkdownPage({
        page: parseMarkdownSourcePath('/charts/not-exists'),
        searchParams: new URLSearchParams(),
        origin: 'http://localhost:3000',
      })
    ).rejects.toEqual(expect.objectContaining<Partial<MarkdownRenderError>>({ status: 404 }));
  });
});
