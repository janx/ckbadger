import { buildMarkdownDocument, markdownList, markdownTable } from '@/lib/ai/markdown-format';

describe('markdown-format helpers', () => {
  it('renders markdown table', () => {
    const output = markdownTable(
      ['name', 'value'],
      [
        ['hash', '0xabc'],
        ['count', 10],
      ]
    );
    expect(output).toContain('| name | value |');
    expect(output).toContain('| hash | 0xabc |');
    expect(output).toContain('| count | 10 |');
  });

  it('renders markdown list', () => {
    const output = markdownList(['alpha', 'beta']);
    expect(output).toBe('- alpha\n- beta');
  });

  it('renders markdown document with frontmatter', () => {
    const output = buildMarkdownDocument(
      {
        title: 'doc title',
        path: '/blocks',
        canonical: 'http://localhost:3000/blocks',
        pageType: 'blocks_list',
        generatedAt: '2026-01-01T00:00:00.000Z',
        buildVersion: '0.1.0+feature/foo@abcdef123456',
      },
      ['# Header', '', 'body']
    );
    expect(output).toContain('---');
    expect(output).toContain('title: "doc title"');
    expect(output).toContain('path: "/blocks"');
    expect(output).toContain('canonical: "http://localhost:3000/blocks"');
    expect(output).toContain('buildVersion: "0.1.0+feature/foo@abcdef123456"');
    expect(output).toContain('formatVersion: 1');
    expect(output).toContain('# Header');
  });
});
