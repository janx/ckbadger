import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import pkg from '@/package.json';

describe('tooling config', () => {
  it('uses vite for dev and build scripts', () => {
    expect(pkg.scripts.dev).toMatch(/^vite/);
    expect(pkg.scripts.build).toMatch(/^vite build/);
  });

  it('does not keep next runtime packages in package.json', () => {
    expect(pkg.dependencies).not.toHaveProperty('next');
    expect(pkg.devDependencies).not.toHaveProperty('eslint-config-next');
  });

  it('exposes a canonical local navigation module', async () => {
    const navigation = await vi.importActual<typeof import('@/src/navigation')>('@/src/navigation');

    expect(typeof navigation.useRouter).toBe('function');
    expect(typeof navigation.usePathname).toBe('function');
    expect(typeof navigation.useSearchParams).toBe('function');
    expect(typeof navigation.redirect).toBe('function');
    expect(typeof navigation.notFound).toBe('function');
  });

  it('does not keep runtime next/link imports in app-level code', () => {
    const runtimeFiles = [
      'components/not-found-page.tsx',
      'components/nft/identity-nft-item-detail.tsx',
      'components/nft/nft-activity-card.tsx',
      'components/nft/nft-collection-stat-cards.tsx',
      'components/home-charts.tsx',
      'components/mempool-blocks.tsx',
      'components/latest-transactions.tsx',
      'components/latest-blocks.tsx',
      'components/deep-fork-alert.tsx',
      'components/ui/page-header.tsx',
      'components/ui/address.tsx',
      'components/ui/chart-card.tsx',
      'components/chain-wave/packed-container.tsx',
      'components/chain-wave/index.tsx',
      'components/layout/site-footer.tsx',
      'components/layout/logo.tsx',
      'components/layout/header.tsx',
      'components/charts/chart-page.tsx',
      'app/transactions/page.tsx',
      'app/forks/page.tsx',
      'app/blocks/page.tsx',
      'app/forks/[id]/client-page.tsx',
      'app/blocks/[id]/client-page.tsx',
      'app/address/[addr]/client-page.tsx',
      'app/tx/[hash]/client-page.tsx',
      'app/cell/[outpoint]/client-page.tsx',
      'app/tokens/[typeHash]/client-page.tsx',
      'app/scripts/[name]/client-page.tsx',
      'app/nfts/[sporeId]/client-page.tsx',
      'app/dao/page.tsx',
      'app/nfts/mnft/[nftId]/client-page.tsx',
      'app/clusters/[clusterId]/client-page.tsx',
      'app/script/[codeHash]/client-page.tsx',
      'app/charts/cell-count/page.tsx',
      'app/charts/knowledge-size/page.tsx',
      'app/charts/total-supply/page.tsx',
      'app/hardforks/page.tsx',
      'app/charts/miner-address-distribution/page.tsx',
      'app/charts/hodl-wave/page.tsx',
      'app/charts/common-knowledge-composition/page.tsx',
      'app/charts/cell-age-vs-occupied-capacity/page.tsx',
      'app/charts/secondary-issuance/page.tsx',
      'app/charts/most-utilized-assets/page.tsx',
      'app/charts/most-utilized-scripts/page.tsx',
    ];

    for (const relativePath of runtimeFiles) {
      const source = readFileSync(join(process.cwd(), relativePath), 'utf8');
      expect(source).not.toContain("from 'next/link'");
    }
  });

  it('does not keep runtime next/image imports in app-level code', () => {
    const runtimeFiles = [
      'components/layout/logo.tsx',
      'app/address/[addr]/client-page.tsx',
      'app/assets/assets-page-client.tsx',
      'app/nfts/[sporeId]/client-page.tsx',
    ];

    for (const relativePath of runtimeFiles) {
      const source = readFileSync(join(process.cwd(), relativePath), 'utf8');
      expect(source).not.toContain("from 'next/image'");
    }
  });

  it('does not keep runtime next/dynamic imports in graph components', () => {
    const runtimeFiles = ['components/proposal-graph.tsx', 'components/cell-graph.tsx'];

    for (const relativePath of runtimeFiles) {
      const source = readFileSync(join(process.cwd(), relativePath), 'utf8');
      expect(source).not.toContain("from 'next/dynamic'");
    }
  });

  it('routes graph loading through the local dynamic client boundary', () => {
    const dynamicClientSource = readFileSync(join(process.cwd(), 'lib/dynamic-client.tsx'), 'utf8');
    const cellGraphSource = readFileSync(join(process.cwd(), 'components/cell-graph.tsx'), 'utf8');
    const proposalGraphSource = readFileSync(
      join(process.cwd(), 'components/proposal-graph.tsx'),
      'utf8'
    );

    expect(dynamicClientSource).toContain('lazy');
    expect(dynamicClientSource).toContain('Suspense');
    expect(dynamicClientSource).not.toContain('next-compat/dynamic');
    expect(cellGraphSource).toContain("from '@/lib/dynamic-client'");
    expect(proposalGraphSource).toContain("from '@/lib/dynamic-client'");
  });

  it('keeps heavy graph runtime code out of the wrapper modules', () => {
    const cellGraphSource = readFileSync(join(process.cwd(), 'components/cell-graph.tsx'), 'utf8');
    const proposalGraphSource = readFileSync(
      join(process.cwd(), 'components/proposal-graph.tsx'),
      'utf8'
    );

    expect(cellGraphSource).not.toContain("from 'react-force-graph-2d'");
    expect(proposalGraphSource).not.toContain("from 'react-force-graph-2d'");
    expect(cellGraphSource).toContain("() => import('@/components/cell-graph-renderer')");
    expect(proposalGraphSource).toContain("() => import('@/components/proposal-graph-renderer')");
  });

  it('does not keep next aliases in vite config', () => {
    const source = readFileSync(join(process.cwd(), 'vite.config.ts'), 'utf8');

    expect(source).not.toContain("'next/link'");
    expect(source).not.toContain("'next/image'");
    expect(source).not.toContain("'next/navigation'");
    expect(source).not.toContain("'next/dynamic'");
  });
});
