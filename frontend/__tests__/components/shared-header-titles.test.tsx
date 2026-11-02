// Consistency plan C1: the four shared headers name the browser tab, and give it back on unmount;
// the 404 and crash pages name it too.
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from '@/__tests__/utils/test-utils';
import { DetailLayout } from '@/components/ui/detail-layout';
import { AssetDetailShell } from '@/components/asset/detail-shell';
import { TopicHero } from '@/components/ui/topic-hero';
import { PageHeader } from '@/components/ui/page-header';
import { NotFoundPage } from '@/components/not-found-page';
import { CrashPage } from '@/components/crash-page';

vi.mock('@/src/navigation', async () => await vi.importActual('@/src/navigation'));
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
vi.mock('@/components/error-vessels', () => ({
  ErrorVessels: () => null,
  VesselsCaption: () => null,
}));

const SLOGAN = 'CKBadger — Local-first CKB-native Explorer';

describe('shared headers set document.title (C1)', () => {
  beforeEach(() => {
    document.title = SLOGAN;
  });
  afterEach(() => {
    document.title = SLOGAN;
  });

  it('DetailLayout: a string h1 names the tab; documentTitle overrides it', () => {
    const list = render(<DetailLayout title="Blocks">rows</DetailLayout>);
    expect(document.title).toBe('Blocks · CKBadger');
    list.unmount();
    expect(document.title).toBe(SLOGAN);
    render(
      <DetailLayout title="Address" documentTitle={['ckb1qzda…3x8k26', 'address']}>
        rows
      </DetailLayout>
    );
    expect(document.title).toBe('ckb1qzda…3x8k26 · address · CKBadger');
  });

  it('DetailLayout: a markup h1 takes its name from documentTitle', () => {
    render(
      <DetailLayout title={<span>#20,894,843</span>} documentTitle={['#20,894,843', 'block']}>
        rows
      </DetailLayout>
    );
    expect(document.title).toBe('#20,894,843 · block · CKBadger');
  });

  it('AssetDetailShell: [displayName, kind]', () => {
    const view = render(
      <MemoryRouter>
        <AssetDetailShell
          title="RGB++"
          documentTitle={['RGB++', 'token']}
          kind="tokens"
          breadcrumb={[{ label: 'Inventory', href: '/inventory/tokens' }]}
        >
          body
        </AssetDetailShell>
      </MemoryRouter>
    );
    expect(document.title).toBe('RGB++ · token · CKBadger');
    view.unmount();
    expect(document.title).toBe(SLOGAN);
  });

  it('TopicHero: [title]', () => {
    render(<TopicHero title="Forks" lede="Where the chain split." figures={undefined} />);
    expect(document.title).toBe('Forks · CKBadger');
  });

  it('PageHeader: [title]', () => {
    render(
      <MemoryRouter>
        <PageHeader title="Scripts" kind="index" />
      </MemoryRouter>
    );
    expect(document.title).toBe('Scripts · CKBadger');
  });

  it('NotFoundPage is "Not found", CrashPage is "Error"', () => {
    const notFound = render(
      <MemoryRouter initialEntries={['/mainnet/nope']}>
        <NotFoundPage />
      </MemoryRouter>
    );
    expect(document.title).toBe('Not found · CKBadger');
    notFound.unmount();
    render(
      <MemoryRouter initialEntries={['/mainnet/tx/0xabc']}>
        <CrashPage message="index read failed" />
      </MemoryRouter>
    );
    expect(document.title).toBe('Error · CKBadger');
  });
});
