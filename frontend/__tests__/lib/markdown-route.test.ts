import fs from 'fs';
import path from 'path';
import { parseMarkdownSourcePath } from '@/lib/ai/markdown-route';

function walk(dir: string): string[] {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  const files: string[] = [];
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...walk(fullPath));
      continue;
    }
    files.push(fullPath);
  }
  return files;
}

function routeFromPageFile(filePath: string): string {
  const appRoot = path.join(process.cwd(), 'app');
  const relativePath = path.relative(appRoot, filePath).replace(/\\/g, '/');
  const routePath = relativePath.replace(/\/page\.tsx$/, '');
  if (routePath.length === 0 || routePath === 'page.tsx') return '/';
  return `/${routePath}`;
}

function sampleDynamicPath(routePath: string): string {
  return routePath
    .replace('[addr]', 'ckt1qyq9sampleaddress')
    .replace('[outpoint]', `0x${'a'.repeat(64)}-0`)
    .replace('[clusterId]', `0x${'b'.repeat(64)}`)
    .replace('[sporeId]', `0x${'c'.repeat(64)}`)
    .replace('[nftId]', `0x${'9'.repeat(56)}`)
    .replace('[codeHash]', `0x${'d'.repeat(64)}`)
    .replace('[typeHash]', `0x${'e'.repeat(64)}`)
    .replace('[hash]', `0x${'f'.repeat(64)}`)
    .replace('[name]', 'secp256k1_blake160_sighash_all')
    .replace('[id]', routePath.startsWith('/forks/') ? '1' : '123');
}

describe('parseMarkdownSourcePath', () => {
  it('parses representative routes', () => {
    expect(parseMarkdownSourcePath('/').kind).toBe('home');
    expect(parseMarkdownSourcePath('/blocks').kind).toBe('blocks_list');
    expect(parseMarkdownSourcePath('/blocks/123').kind).toBe('block_detail');
    expect(parseMarkdownSourcePath('/tx/0x123').kind).toBe('tx_detail');
    expect(parseMarkdownSourcePath('/charts/hash-rate').kind).toBe('chart_detail');
    expect(parseMarkdownSourcePath('/nfts/dotbit/0x123').kind).toBe('dotbit_item_detail');
    expect(parseMarkdownSourcePath('/nfts/mnft/0x123').kind).toBe('mnft_item_detail');
    expect(parseMarkdownSourcePath('/unknown/path').kind).toBe('unknown');
  });

  it('covers every app page route', () => {
    const appDir = path.join(process.cwd(), 'app');
    const pageFiles = walk(appDir).filter((file) => file.endsWith('/page.tsx'));
    const routes = pageFiles
      .filter((file) => !file.includes('/__md/'))
      .map((file) => routeFromPageFile(file))
      .sort();

    for (const route of routes) {
      const sample = sampleDynamicPath(route);
      const parsed = parseMarkdownSourcePath(sample);
      expect(parsed.kind, `route not parsed: ${route} -> ${sample}`).not.toBe('unknown');
    }
  });
});
