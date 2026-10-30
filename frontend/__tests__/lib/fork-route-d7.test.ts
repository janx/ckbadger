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
});
