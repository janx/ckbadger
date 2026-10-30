import { describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import { api } from '@/lib/api';
import { parseMarkdownSourcePath } from '@/lib/ai/markdown-route';
import { MarkdownRenderError, renderMarkdownPage } from '@/lib/ai/markdown-renderer';
import uncleFixture from '@/__tests__/fixtures/fork-api-producer/fork-uncle-detail.json';

describe('D7 fork address /forks/<fork_point_height>/<orphan_hash_prefix8>', () => {
  it('fetches the fork detail by fork point and orphan prefix', async () => {
    let requested = '';
    server.use(
      http.get('/api/:network/v1/forks/:height/:orphan', ({ request }) => {
        requested = new URL(request.url).pathname;
        return HttpResponse.json(uncleFixture);
      })
    );
    const detail = await api.getForkAt(10, '17404795');
    expect(requested).toBe('/api/mainnet/v1/forks/10/17404795');
    expect(detail.event.forkPointNumber).toBe(10);
  });

  it('parses the markdown route with both parts and drops the millisecond form', () => {
    expect(parseMarkdownSourcePath('/forks/20617704/a9bc1b75')).toEqual({
      kind: 'fork_detail',
      pathname: '/forks/20617704/a9bc1b75',
      height: '20617704',
      orphan: 'a9bc1b75',
    });
    expect(parseMarkdownSourcePath('/forks/1793278270779').kind).toBe('unknown');
  });

  it('keeps no client method for the removed millisecond route (D7: removed, not redirected)', () => {
    expect(api).not.toHaveProperty('getForkDetail');
  });

  it('renders the fork markdown titled by its fork point, and refuses a malformed address', async () => {
    const getForkAt = vi.spyOn(api, 'getForkAt').mockResolvedValue(uncleFixture as never);
    const page = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/forks/10/17404795'),
      searchParams: new URLSearchParams(),
      origin: 'https://explorer.example',
    });
    expect(getForkAt).toHaveBeenCalledWith(10, '17404795');
    expect(page.body).toContain('# Fork at #10');
    await expect(
      renderMarkdownPage({
        page: parseMarkdownSourcePath('/forks/10/XYZ'),
        searchParams: new URLSearchParams(),
        origin: 'https://explorer.example',
      })
    ).rejects.toBeInstanceOf(MarkdownRenderError);
    getForkAt.mockRestore();
  });

  // contracts W5-2: without the node reader the route answers the stored event, no evidence.
  it('says in the markdown when the branch evidence needs the node store', async () => {
    const getForkAt = vi.spyOn(api, 'getForkAt').mockResolvedValue({
      ...uncleFixture,
      evidenceStatus: 'node_reader_unavailable',
      orphanedBlocks: [],
      orphanedTransactions: [],
      adoptedBranch: [],
      canonicalBranch: [],
      canonicalTipNumber: null,
    } as never);
    const page = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/forks/10/17404795'),
      searchParams: new URLSearchParams(),
      origin: 'https://explorer.example',
    });
    expect(page.body).toContain('| evidenceStatus | node_reader_unavailable |');
    expect(page.body).toContain(`| firstOrphanHash | ${uncleFixture.event.firstOrphanHash} |`);
    getForkAt.mockRestore();
  });
});

describe('markdown forks list (R2b-b)', () => {
  it('lists each switch by its D7 page, never by the millisecond id', async () => {
    const list = (await import('@/__tests__/fixtures/fork-api-producer/fork-list.json')).default;
    const getForks = vi.spyOn(api, 'getForks').mockResolvedValue(list as never);
    const getRecent = vi.spyOn(api, 'getRecentReorg').mockResolvedValue({
      hasRecentReorg: true,
      reorg: list.data[0],
      deepFork: {
        detected: false,
        detectedAt: null,
        depth: null,
        dbTip: null,
        chainTip: null,
        forkPoint: null,
      },
    } as never);
    const page = await renderMarkdownPage({
      page: parseMarkdownSourcePath('/forks'),
      searchParams: new URLSearchParams(),
      origin: 'https://explorer.example',
    });
    expect(page.body).toContain('/forks/10/17404795');
    expect(page.body).toContain('recorded');
    expect(page.body).not.toContain('1754000000000');
    getForks.mockRestore();
    getRecent.mockRestore();
  });
});
