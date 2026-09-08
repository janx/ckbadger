import { resolveMarkdownRewrite } from '@/lib/ai/markdown-request';

function params(query: string = ''): URLSearchParams {
  return new URLSearchParams(query);
}

describe('resolveMarkdownRewrite', () => {
  it('negotiates registered page parameters containing dots', () => {
    expect(
      resolveMarkdownRewrite({
        method: 'GET',
        pathname: '/mainnet/scripts/deployed.v1',
        searchParams: params(),
        acceptHeader: 'text/markdown',
      }).rewrite
    ).toBe(true);
  });
  it('uses Accept quality weights and keeps browsers on HTML', () => {
    const resolve = (acceptHeader: string) =>
      resolveMarkdownRewrite({
        method: 'GET',
        pathname: '/mainnet/blocks/42',
        searchParams: params(),
        acceptHeader,
      });
    expect(resolve('text/html, text/markdown;q=0.5').rewrite).toBe(false);
    expect(resolve('text/markdown;q=0').rewrite).toBe(false);
    expect(resolve('text/markdown, application/vnd.ckbadger.raw+json;q=0.2').internalPrefix).toBe(
      '/ai-md'
    );
    expect(resolve('TEXT/MARKDOWN').internalPrefix).toBe('/ai-md');
  });

  it('explicit HTML overrides suffix and Accept', () => {
    expect(
      resolveMarkdownRewrite({
        method: 'HEAD',
        pathname: '/testnet/blocks/42.raw',
        searchParams: params('format=html'),
        acceptHeader: 'text/markdown',
      })
    ).toEqual({ rewrite: false, sourcePath: '/testnet/blocks/42', removeFormatParam: true });
  });

  it('rejects invalid and ambiguous format requests', () => {
    for (const query of ['format=json', 'format=', 'format=raw&format=md']) {
      expect(() =>
        resolveMarkdownRewrite({
          method: 'GET',
          pathname: '/mainnet/blocks/42',
          searchParams: params(query),
          acceptHeader: 'text/html',
        })
      ).toThrow('Invalid query format');
    }
  });
  it('rewrites .md suffix to markdown source path', () => {
    const decision = resolveMarkdownRewrite({
      method: 'GET',
      pathname: '/blocks/123.md',
      searchParams: params(),
      acceptHeader: 'text/html',
    });

    expect(decision).toEqual({
      rewrite: true,
      sourcePath: '/blocks/123',
      internalPrefix: '/ai-md',
    });
  });

  it('rewrites format=md query', () => {
    const decision = resolveMarkdownRewrite({
      method: 'GET',
      pathname: '/tx/0xabc',
      searchParams: params('format=md&limit=10'),
      acceptHeader: 'text/html',
    });

    expect(decision).toEqual({
      rewrite: true,
      sourcePath: '/tx/0xabc',
      removeFormatParam: true,
      internalPrefix: '/ai-md',
    });
  });

  it('rewrites Accept: text/markdown', () => {
    const decision = resolveMarkdownRewrite({
      method: 'GET',
      pathname: '/charts/hash-rate',
      searchParams: params(),
      acceptHeader: 'text/markdown, text/plain;q=0.5',
    });

    expect(decision).toEqual({
      rewrite: true,
      sourcePath: '/charts/hash-rate',
      internalPrefix: '/ai-md',
    });
  });

  it('rewrites .raw suffix to raw source path', () => {
    const decision = resolveMarkdownRewrite({
      method: 'GET',
      pathname: '/tx/0xabc.raw',
      searchParams: params(),
      acceptHeader: 'text/html',
    });

    expect(decision).toEqual({
      rewrite: true,
      sourcePath: '/tx/0xabc',
      internalPrefix: '/ai-raw',
    });
  });

  it('rewrites format=raw query', () => {
    const decision = resolveMarkdownRewrite({
      method: 'GET',
      pathname: '/blocks/123',
      searchParams: params('format=raw&profile=default'),
      acceptHeader: 'text/html',
    });

    expect(decision).toEqual({
      rewrite: true,
      sourcePath: '/blocks/123',
      removeFormatParam: true,
      internalPrefix: '/ai-raw',
    });
  });

  it('prioritizes query format over suffix format', () => {
    const decision = resolveMarkdownRewrite({
      method: 'GET',
      pathname: '/tx/0xabc.raw',
      searchParams: params('format=md'),
      acceptHeader: 'application/vnd.ckbadger.raw+json, text/markdown;q=0.5',
    });

    expect(decision).toEqual({
      rewrite: true,
      sourcePath: '/tx/0xabc',
      removeFormatParam: true,
      internalPrefix: '/ai-md',
    });
  });

  it('rewrites Accept: application/vnd.ckbadger.raw+json', () => {
    const decision = resolveMarkdownRewrite({
      method: 'GET',
      pathname: '/cell/0xabc-0',
      searchParams: params(),
      acceptHeader: 'application/vnd.ckbadger.raw+json, application/json;q=0.8',
    });

    expect(decision).toEqual({
      rewrite: true,
      sourcePath: '/cell/0xabc-0',
      internalPrefix: '/ai-raw',
    });
  });

  it('does not rewrite API routes', () => {
    const decision = resolveMarkdownRewrite({
      method: 'GET',
      pathname: '/api/v1/blocks',
      searchParams: params('format=md'),
      acceptHeader: 'text/markdown',
    });

    expect(decision).toEqual({ rewrite: false });
  });

  it('does not rewrite static asset paths', () => {
    const decision = resolveMarkdownRewrite({
      method: 'GET',
      pathname: '/ckbadger-logo.webp',
      searchParams: params('format=md'),
      acceptHeader: 'text/markdown',
    });

    expect(decision).toEqual({ rewrite: false });
  });

  it('does not rewrite non-GET/HEAD methods', () => {
    const decision = resolveMarkdownRewrite({
      method: 'POST',
      pathname: '/blocks',
      searchParams: params('format=md'),
      acceptHeader: 'text/markdown',
    });

    expect(decision).toEqual({ rewrite: false });
  });
});
