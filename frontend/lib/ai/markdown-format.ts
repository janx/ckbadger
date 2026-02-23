export interface MarkdownDocMeta {
  title: string;
  path: string;
  canonical: string;
  pageType: string;
  generatedAt: string;
}

export function formatValue(value: unknown): string {
  if (value === null || value === undefined) return '-';
  if (typeof value === 'string') return value.trim().length > 0 ? value : '-';
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  return JSON.stringify(value);
}

function escapeYaml(value: string): string {
  return JSON.stringify(value);
}

function escapeCell(value: unknown): string {
  return formatValue(value).replace(/\|/g, '\\|').replace(/\r?\n/g, ' ');
}

export function markdownTable(headers: string[], rows: unknown[][]): string {
  if (headers.length === 0) {
    return '_No columns_';
  }
  if (rows.length === 0) {
    return '_No rows_';
  }

  const head = `| ${headers.map(escapeCell).join(' | ')} |`;
  const separator = `| ${headers.map(() => '---').join(' | ')} |`;
  const body = rows.map((row) => `| ${row.map(escapeCell).join(' | ')} |`);
  return [head, separator, ...body].join('\n');
}

export function markdownList(items: string[]): string {
  if (items.length === 0) return '_No items_';
  return items.map((item) => `- ${item}`).join('\n');
}

export function markdownCodeBlock(language: string, content: string): string {
  return `\`\`\`${language}\n${content}\n\`\`\``;
}

export function buildMarkdownDocument(meta: MarkdownDocMeta, sections: string[]): string {
  const frontmatter = [
    '---',
    `title: ${escapeYaml(meta.title)}`,
    `path: ${escapeYaml(meta.path)}`,
    `canonical: ${escapeYaml(meta.canonical)}`,
    `pageType: ${escapeYaml(meta.pageType)}`,
    `generatedAt: ${escapeYaml(meta.generatedAt)}`,
    'formatVersion: 1',
    '---',
  ].join('\n');

  return [frontmatter, '', ...sections].join('\n');
}
