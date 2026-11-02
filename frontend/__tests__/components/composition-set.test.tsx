import { fireEvent, render, screen, within } from '@/__tests__/utils/test-utils';
import {
  CompositionSet,
  compositionCountsFromAddress,
  compositionCountsFromCollection,
  type CompositionCounts,
} from '@/components/ui/composition-set';

// Consistency round C8: a set's composition is first-class information — "N of M fully
// on-chain", the 8 px set bar, and the five-tier legend with every tier's count, zeros kept dim.
const allBtc: CompositionCounts = {
  pure_ckb: 0,
  btc_ckb: 1298,
  decentralized_mixture: 0,
  centralized_mixture: 0,
  unknown: 0,
  decode_pending: 0,
};

const legendCounts = (group: HTMLElement) =>
  within(group)
    .getAllByRole('listitem')
    .map((li) => [li.getAttribute('data-tier'), li.querySelector('[data-count]')!.textContent]);

describe('CompositionSet', () => {
  it('says how many of the set are fully on-chain and lists all five tiers, zeros kept', () => {
    render(<CompositionSet counts={allBtc} total={1298} />);
    const group = screen.getByRole('group', { name: 'Object composition' });
    expect(group).toHaveTextContent('1,298 of 1,298 fully on-chain');
    expect(legendCounts(group)).toEqual([
      ['pure_ckb', '0'],
      ['btc_ckb', '1,298'],
      ['decentralized_mixture', '0'],
      ['centralized_mixture', '0'],
      ['unknown', '0'],
    ]);
    // Zero tiers stay in the legend, in the ghost ink; the held tier is bright.
    const items = within(group).getAllByRole('listitem');
    expect(items[0]).toHaveAttribute('data-zero', 'true');
    expect(items[0].querySelector('[data-count]')).toHaveClass('text-text-ghost');
    expect(items[1]).not.toHaveAttribute('data-zero');
    expect(items[1].querySelector('[data-count]')).toHaveClass('text-text-bright');
  });

  it('draws one 8 px bar whose segments are the non-zero tiers in proportion', () => {
    render(
      <CompositionSet
        counts={{ ...allBtc, btc_ckb: 600, pure_ckb: 200, centralized_mixture: 200 }}
        total={1000}
      />
    );
    const bar = screen.getByRole('img', { name: /Composition of the 1,000 objects/ });
    expect(bar).toHaveClass('h-2');
    expect(bar).toHaveAccessibleName(
      'Composition of the 1,000 objects: BTC+CKB 600, Pure CKB 200, Centralized mix 200'
    );
    const parts = [...bar.querySelectorAll<HTMLElement>('[data-tier-share]')];
    expect(parts.map((p) => [p.dataset.tierShare, p.style.width])).toEqual([
      ['btc_ckb', '60%'],
      ['pure_ckb', '20%'],
      ['centralized_mixture', '20%'],
    ]);
    expect(screen.getByRole('group')).toHaveTextContent('800 of 1,000 fully on-chain');
  });

  it('adds up: the legend counts sum to the set, and a set that does not add up fails', () => {
    const counts = { ...allBtc, btc_ckb: 5, unknown: 2, decode_pending: 1 };
    render(<CompositionSet counts={counts} total={8} />);
    const group = screen.getByRole('group');
    const shown = legendCounts(group).map(([, n]) => Number(n));
    expect(shown.reduce((a, b) => a + b, 0)).toBe(8);
    // Decode pending is listed only while some object awaits decoding.
    expect(legendCounts(group).map(([t]) => t)).toContain('decode_pending');
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<CompositionSet counts={counts} total={9} />)).toThrow(
      'composition counts 8 objects, the set has 9'
    );
    log.mockRestore();
  });

  it('names the set: collections on the inventory, with its own caption', () => {
    render(
      <CompositionSet
        counts={{ ...allBtc, btc_ckb: 0, pure_ckb: 90, centralized_mixture: 110 }}
        total={200}
        noun="collections"
        caption="so are 4,200 live objects"
      />
    );
    expect(screen.getByRole('group')).toHaveTextContent(
      '90 of 200 collections are fully on-chain · so are 4,200 live objects'
    );
  });

  it('turns the legend into filter buttons when asked, the empty tiers disabled', () => {
    const onSelect = vi.fn();
    render(
      <CompositionSet
        counts={{ ...allBtc, btc_ckb: 0, pure_ckb: 90, centralized_mixture: 110 }}
        total={200}
        noun="collections"
        filter={{ selected: 'pure_ckb', onSelect, label: 'Filter by composition' }}
      />
    );
    const chips = screen.getByRole('group', { name: 'Filter by composition' });
    const buttons = within(chips).getAllByRole('button');
    expect(buttons.map((b) => b.textContent)).toEqual([
      'All 200',
      'Pure CKB 90',
      'BTC+CKB 0',
      'Decentralized mix 0',
      'Centralized mix 110',
      'Unknown 0',
    ]);
    expect(buttons[1]).toHaveAttribute('aria-pressed', 'true');
    expect(buttons[2]).toBeDisabled();
    fireEvent.click(buttons[4]);
    expect(onSelect).toHaveBeenCalledWith('centralized_mixture');
    fireEvent.click(buttons[0]);
    expect(onSelect).toHaveBeenCalledWith('');
  });

  it('reads the address and collection payloads into one count shape', () => {
    expect(
      compositionCountsFromAddress({
        pureCkb: 1,
        btcCkb: 2,
        decentralizedMixture: 3,
        centralizedMixture: 4,
        unknown: 5,
        decodePending: 6,
      })
    ).toEqual({
      pure_ckb: 1,
      btc_ckb: 2,
      decentralized_mixture: 3,
      centralized_mixture: 4,
      unknown: 5,
      decode_pending: 6,
    });
    // A collection states its on-chain count (Pure CKB + BTC+CKB) and each other tier.
    expect(
      compositionCountsFromCollection({
        tier: 'btc_ckb',
        onchainCount: 10,
        pureCkbCount: 3,
        decentralizedMixtureCount: 0,
        centralizedMixtureCount: 1,
        unknownCount: 2,
        onchainRatio: '0.7692',
      })
    ).toEqual({
      pure_ckb: 3,
      btc_ckb: 7,
      decentralized_mixture: 0,
      centralized_mixture: 1,
      unknown: 2,
      decode_pending: 0,
    });
    expect(() =>
      compositionCountsFromCollection({
        tier: 'pure_ckb',
        onchainCount: 1,
        pureCkbCount: 3,
        decentralizedMixtureCount: 0,
        centralizedMixtureCount: 0,
        unknownCount: 0,
        onchainRatio: null,
      })
    ).toThrow('on-chain count 1 is below its Pure CKB count 3');
  });
});
