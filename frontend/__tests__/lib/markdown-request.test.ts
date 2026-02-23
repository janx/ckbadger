import { resolveMarkdownRewrite } from '@/lib/ai/markdown-request';

function params(query: string = ''): URLSearchParams {
  return new URLSearchParams(query);
}

describe('resolveMarkdownRewrite', () => {
  it('rewrites .md suffix to markdown source path', () => {
    const decision = resolveMarkdownRewrite({
      method: 'GET',
      pathname: '/blocks/123.md',
      searchParams: params(),
      acceptHeader: 'text/html',
    });

    expect(decision).toEqual({ rewrite: true, sourcePath: '/blocks/123' });
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
      pathname: '/ckbadger-logo11.png',
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
