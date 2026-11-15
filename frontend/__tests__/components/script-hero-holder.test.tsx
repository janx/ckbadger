import { describe, expect, it } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import fixture from '@/__tests__/fixtures/scripts/joyid-family-detail.json';
import type { ScriptFamilyDetail, ScriptHistoryMarkerApi } from '@/lib/api';
import { ScriptHeroMetrics } from '@/components/scripts/script-family-board';
import { typeLineage } from '@/lib/script-lineage';

const family = fixture.family as unknown as ScriptFamilyDetail;
const lineage = typeLineage(family.history as ScriptHistoryMarkerApi[])!;

/** The hero's "Code cell held by" value: the link to the code cell. */
const heldBy = () =>
  within(screen.getByText('Code cell held by').parentElement!).getByRole('link');

/**
 * A4 Chrome F2: the hero wrapped the holder's whole name in one NoBreak, so "an unspendable lock"
 * (194 px at 17 px, the default lock and the DAO) ran 15 px off a 390 page. A phrase is not a
 * unit; a kind with its id ("multisig 0x6598…d696") is (a label with its id). The name's parts
 * say which, where the custody's class is known.
 */
describe('the hero names the code cell holder (A4 Chrome F2)', () => {
  it('keeps a kind with its id whole', () => {
    render(<ScriptHeroMetrics family={family} lineage={lineage} />);
    const unit = heldBy().querySelector('[data-nobreak]');
    expect(unit).not.toBeNull();
    expect(unit!.textContent).toBe('multisig 0x6598…d696');
  });

  it('lets a phrase break between its words', () => {
    const last = lineage.at(-1)!;
    const unspendable = [
      ...lineage.slice(0, -1),
      { ...last, custody: { ...last.custody, class: 'unspendable' } },
    ] as ScriptHistoryMarkerApi[];
    render(<ScriptHeroMetrics family={family} lineage={unspendable} />);
    const link = heldBy();
    expect(link).toHaveTextContent(/^an unspendable lock$/);
    expect(link.querySelector('[data-nobreak]')).toBeNull();
    expect(link.closest('.whitespace-nowrap')).toBeNull();
  });
});
