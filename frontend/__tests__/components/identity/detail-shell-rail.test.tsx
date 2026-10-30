import { render, screen } from '@/__tests__/utils/test-utils';
import { AssetDetailShell } from '@/components/asset/detail-shell';
vi.mock('@/components/layout/header', () => ({ Header: () => null }));

// F2 identities (round 2), ID-C11: on a 390 px screen the facts rail came after all of tier 1.
describe('AssetDetailShell rail placement', () => {
  const shell = (railAfterHeader?: boolean) => (
    <AssetDetailShell
      title="valar.bit"
      kind="identities"
      breadcrumb={[{ label: 'Inventory', href: '/inventory/tokens' }]}
      facts={[{ label: 'Expires', value: '2027-02-19' }]}
      railAfterHeader={railAfterHeader}
    >
      <section aria-label="Held until">term</section>
    </AssetDetailShell>
  );
  const follows = (a: Element, b: Element) =>
    Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);

  it('reads header, facts, then tier 1 in one column, with the rail beside both from lg up', () => {
    render(shell(true));
    const header = screen.getByRole('heading', { level: 1 }).closest('header')!;
    const facts = screen.getByRole('complementary', { name: 'Facts' });
    const body = screen.getByRole('region', { name: 'Held until' });
    expect(follows(header, facts)).toBe(true);
    expect(follows(facts, body)).toBe(true);
    expect(facts).toHaveClass('lg:col-start-2', 'lg:row-span-2', 'lg:row-start-1');
    expect(body.closest('[data-tier1-body]')).toHaveClass('lg:col-start-1', 'lg:row-start-2');
  });

  it('keeps the existing order when not asked', () => {
    render(shell());
    const facts = screen.getByRole('complementary', { name: 'Facts' });
    const body = screen.getByRole('region', { name: 'Held until' });
    expect(follows(body, facts)).toBe(true);
  });
});
