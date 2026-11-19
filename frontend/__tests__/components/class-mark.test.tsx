import { describe, expect, it } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { ClassMark } from '@/components/ui/class-mark';
import { CompositionBadge, CompositionMark } from '@/components/ui/composition-mark';
import { CompositionSet } from '@/components/ui/composition-set';
import { CLASS_COLORS, COMPOSITION_BADGES, COMPOSITION_COLORS } from '@/lib/design-tokens';

const mark = (name: string) => screen.getByRole('img', { name }) as unknown as SVGSVGElement;

describe('ClassMark (IV-22 / IV-23)', () => {
  it('draws the approved marks: vault ring, coin stack, cube, person', () => {
    render(
      <>
        {(['dao', 'tokens', 'objects', 'identities'] as const).map((kind) => (
          <ClassMark key={kind} kind={kind} />
        ))}
      </>
    );
    const dao = mark('dao class');
    const ring = dao.querySelector('circle[r="6"]')!;
    expect(ring).toHaveAttribute('stroke', CLASS_COLORS.dao);
    expect(ring).toHaveAttribute('fill', 'none');
    expect(dao.querySelector('circle[r="2.4"]')).toHaveAttribute('fill', CLASS_COLORS.dao);
    // The retired temple shape (roof + three columns) is gone.
    expect(dao.innerHTML).not.toContain('M3 12V6l5-3 5 3v6');
    expect(mark('tokens class').querySelector('ellipse')).toHaveAttribute(
      'fill',
      CLASS_COLORS.tokens
    );
    const cube = mark('objects class').querySelector('path')!;
    expect(cube).toHaveAttribute('fill', CLASS_COLORS.objects);
    expect(cube).toHaveAttribute('fill-opacity', '0.2');
    expect(mark('identities class').querySelector('circle')).toHaveAttribute(
      'fill',
      CLASS_COLORS.identities
    );
  });

  it('is 16 px by default and only comes in the 12 / 16 / 24 sizes', () => {
    render(
      <>
        <ClassMark kind="dao" />
        <ClassMark kind="tokens" size={12} />
        <ClassMark kind="objects" size={24} />
      </>
    );
    expect(mark('dao class')).toHaveAttribute('width', '16');
    expect(mark('tokens class')).toHaveAttribute('width', '12');
    expect(mark('objects class')).toHaveAttribute('width', '24');
  });
});

describe('CompositionMark (IV-21 / BA-33)', () => {
  it('draws Pure CKB as the crystal and BTC+CKB as the crystal in a copper mount', () => {
    render(
      <>
        <CompositionMark tier="pure_ckb" />
        <CompositionMark tier="btc_ckb" />
        <CompositionMark tier="btc_ckb" />
      </>
    );
    const crystal = mark('Pure CKB');
    expect(crystal).toHaveAttribute('width', '16');
    expect(crystal.querySelector(`path[fill="${COMPOSITION_COLORS.crystal.left}"]`)).not.toBeNull();
    expect(crystal.querySelector('linearGradient')).toBeNull();
    const [a, b] = screen.getAllByRole('img', { name: 'BTC+CKB' });
    const ids = [a, b].map((m) => m.querySelector('linearGradient')!.id);
    // Two mounts on one page must not share gradient ids.
    expect(new Set(ids).size).toBe(2);
    expect(a.querySelector(`path[fill="url(#${ids[0]})"]`)).not.toBeNull();
  });

  it('draws the off-chain tiers as plain grey outline icons', () => {
    render(
      <>
        <CompositionMark tier="decentralized_mixture" />
        <CompositionMark tier="centralized_mixture" />
        <CompositionMark tier="unknown" />
      </>
    );
    for (const name of ['Decentralized mix', 'Centralized mix', 'Unknown composition']) {
      const icon = mark(name);
      const strokes = Array.from(icon.querySelectorAll('[stroke]')).map((e) =>
        e.getAttribute('stroke')
      );
      expect(strokes.length).toBeGreaterThan(0);
      expect(new Set(strokes)).toEqual(new Set([COMPOSITION_COLORS.offchain]));
    }
  });
});

describe('CompositionBadge (IV-24)', () => {
  it('is one drawing everywhere: gradient frame for the on-chain tiers, plain for off-chain', () => {
    render(
      <>
        <CompositionBadge tier="btc_ckb" />
        <CompositionBadge tier="pure_ckb" />
        <CompositionBadge tier="centralized_mixture" />
        <CompositionBadge tier="decode_pending" />
      </>
    );
    const btc = screen.getByText('BTC+CKB').closest('[data-composition]') as HTMLElement;
    expect(btc).toHaveAttribute('data-composition', 'btc_ckb');
    expect(btc.style.backgroundImage).toBe(COMPOSITION_BADGES.btc_ckb.frame);
    expect(btc.style.backgroundClip).toBe('padding-box, border-box');
    expect(btc.querySelector('svg[aria-label="BTC+CKB"]')).not.toBeNull();
    const pure = screen.getByText('Pure CKB').closest('[data-composition]') as HTMLElement;
    expect(pure.style.backgroundImage).toBe(COMPOSITION_BADGES.pure_ckb.frame);
    const central = screen
      .getByText('Centralized mix')
      .closest('[data-composition]') as HTMLElement;
    expect(central.style.backgroundImage).toBe('');
    // Re-pinned on purpose (readability P2, lane p-assets: "the composition badge on every row
    // stays; only its colour is made to agree with the filter's"): an off-chain tier reads as the
    // composition legend, the inventory's filter, draws it, its words in the reading ink and its
    // mark as it was (it was the quiet ink)
    expect(central).toHaveClass('text-text');
    expect(central).not.toHaveClass('text-text-dim');
    expect(screen.getByText('Decode pending')).toBeInTheDocument();
  });

  it('draws a tier as the composition legend draws it: one colour for the filter and the rows', () => {
    const { container } = render(
      <>
        <CompositionBadge tier="decentralized_mixture" />
        <CompositionSet
          counts={{
            pure_ckb: 1,
            btc_ckb: 1,
            decentralized_mixture: 1,
            centralized_mixture: 0,
            unknown: 0,
            decode_pending: 0,
          }}
          total={3}
          filter={{ selected: undefined, onSelect: () => {}, label: 'composition' }}
        />
      </>
    );
    const badge = container.querySelector('[data-composition="decentralized_mixture"]')!;
    const legend = within(screen.getByRole('group', { name: 'composition' }))
      .getByText('Decentralized mix')
      .closest('button')!;
    const words = (el: Element) =>
      Array.from(el.querySelectorAll('span')).find((s) => s.textContent === 'Decentralized mix') ??
      el;
    expect(words(badge).className.match(/text-text(-dim)?\b/g)?.[0] ?? 'text-text').toBe(
      words(legend).className.match(/text-text(-dim)?\b/g)?.[0] ?? 'text-text'
    );
  });
});
