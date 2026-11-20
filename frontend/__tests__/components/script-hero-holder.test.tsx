import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import fixture from '@/__tests__/fixtures/scripts/joyid-family-detail.json';
import type { ScriptFamilyDetail, ScriptHistoryMarkerApi } from '@/lib/api';
import { ScriptHeroMetrics } from '@/components/scripts/script-family-board';
import { typeLineage } from '@/lib/script-lineage';
import { atViewportWidth } from '@/__tests__/utils/viewport';

const family = fixture.family as unknown as ScriptFamilyDetail;
const lineage = typeLineage(family.history as ScriptHistoryMarkerApi[])!;

/** The hero's "Code cell held by" value: the link to the code cell. */
const heldBy = () => within(screen.getByText('Code cell held by').parentElement!).getByRole('link');

afterEach(() => vi.unstubAllGlobals());

/**
 * A4 Chrome F2: the hero wrapped the holder's whole name in one NoBreak, so "an unspendable lock"
 * (194 px at 17 px, the default lock and the DAO) ran 15 px off a 390 page. A phrase is not a
 * unit; a kind with its id ("multisig 0x6598…d696") is (a label with its id). The name's parts
 * say which, where the custody's class is known.
 */
describe('the hero names the code cell holder (A4 Chrome F2)', () => {
  // re-pinned with the fix: the id alone is the unit — "multisig 0x6598…d696" whole is 204 px at
  // 17 px, wider than the hero tile's 163 px column at 390
  it('keeps the id whole, the kind a word before it', () => {
    atViewportWidth(390);
    render(<ScriptHeroMetrics family={family} lineage={lineage} />);
    const link = heldBy();
    expect(link).toHaveTextContent(/^multisig 0x6598…d696$/);
    const unit = link.querySelector('[data-nobreak]');
    expect(unit).not.toBeNull();
    expect(unit!.textContent).toBe('0x6598…d696');
  });

  // R9: a tile is under 200 px only on a phone; from sm (220 px at 1024, 288 at 640) the holder
  // takes the row size the lede and the tables write it in, its id 17 characters at 17 px.
  it('names the holder at the row size from sm, its id one unit the 220 px tile holds', () => {
    atViewportWidth(1024);
    render(<ScriptHeroMetrics family={family} lineage={lineage} />);
    const link = heldBy();
    expect(link).toHaveTextContent(/^multisig 0x65983f1d…eed696$/);
    const unit = link.querySelector('[data-nobreak]')!;
    expect(unit.textContent).toBe('0x65983f1d…eed696');
    expect(unit.textContent!.length * 10.2).toBeLessThanOrEqual(220);
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

/**
 * The pieces a line can never break inside: a unit's whole text (NoBreak), else the text between
 * ordinary spaces (a no-break space joins; a count's inline unit breaks only at its own space).
 */
function unbreakablePieces(root: HTMLElement): string[] {
  const pieces: string[] = [];
  const walk = (node: Node) => {
    if (node instanceof HTMLElement && node.hasAttribute('data-nobreak')) {
      pieces.push(node.textContent!);
      return;
    }
    if (node.nodeType === Node.TEXT_NODE)
      pieces.push(...node.textContent!.split(/[ \n\t]+/).filter(Boolean));
    node.childNodes.forEach(walk);
  };
  walk(root);
  return pieces;
}

/**
 * A4 round 2, task 1: "Ever used" read "78,675,029 cell / s" at 390 and "78,675,029 cel / ls" at
 * 360 — the count, joined by a no-break space, is 16 × 10.2 = 163.2 px at 17 px, the tile's value
 * 153 px at 390 and 143 at 360 (a column of (358 − 32) / 2 = 163 and (328 − 32) / 2 = 148, less the
 * value's own box), and the dd's overflow-wrap: anywhere cut the word. A count breaks only between
 * its number and its noun. Every tile value is checked against 143 px: no piece a line cannot break
 * inside may be wider (14 characters at 17 px, 19 at the notes' 12 px).
 */
describe('the hero tiles break no word at a phone width (A4 round 2)', () => {
  it('sets every value and note in pieces the 143 px tile holds', () => {
    atViewportWidth(360);
    render(<ScriptHeroMetrics family={{ ...family, cellsCount: 78_675_029 }} lineage={lineage} />);
    const metrics = screen.getByLabelText('Script figures');
    for (const dd of metrics.querySelectorAll<HTMLElement>('dd')) {
      for (const value of dd.querySelectorAll<HTMLElement>('.text-section'))
        for (const piece of unbreakablePieces(value))
          expect(piece.length * 10.2, `${piece} in ${dd.textContent}`).toBeLessThanOrEqual(143);
      for (const note of dd.querySelectorAll<HTMLElement>('.text-text-dim'))
        for (const piece of unbreakablePieces(note))
          expect(piece.length * 7.2, `${piece} in ${dd.textContent}`).toBeLessThanOrEqual(143);
    }
    // the count: its number and its noun, one unit where the line holds it
    const ever = within(screen.getByText('Ever used').parentElement!).getByText(/78,675,029/);
    // re-pinned (A4 round 3): the unit's own marker, data-count-unit
    expect(ever.closest('[data-count-unit]')).toHaveTextContent(/^78,675,029 cells$/);
  });
});

/** The ink an element reads in: the nearest of the three text inks on it or around it. */
function nearestInk(el: Element): string | null {
  for (let at: Element | null = el; at; at = at.parentElement) {
    const ink = [...at.classList].find((c) =>
      ['text-text', 'text-text-dim', 'text-text-bright'].includes(c)
    );
    if (ink) return ink;
  }
  return null;
}

/**
 * BRIEF-roles-rest part A: a hero metric's note is quiet (R13), its values are not. The deploying
 * transaction under "In use since" (a link), the day the code was last replaced and a holder that
 * has never signed stood in the note's quiet ink with their label words; they are values, in the
 * reading ink, and "deployed in" and "last replaced" stay quiet labels.
 */
describe('the hero metrics read their notes’ values in the reading ink', () => {
  it('reads the deploying tx, the day last replaced and the holder’s state; quiet labels', () => {
    render(<ScriptHeroMetrics family={family} lineage={lineage} />);
    const since = screen.getByText('In use since').parentElement!;
    const tx = within(since).getByRole('link');
    expect(nearestInk(tx)).toBe('text-text');
    expect(nearestInk(within(since).getByText(/deployed in/))).toBe('text-text-dim');
    const code = screen.getByText('Code').parentElement!;
    const replaced = within(code).getByText(/last replaced/);
    expect(nearestInk(replaced)).toBe('text-text-dim');
    const day = replaced.querySelector('time') ?? replaced.lastElementChild!;
    expect(day.textContent).toMatch(/^\d{4}-\d{2}-\d{2}/);
    expect(nearestInk(day)).toBe('text-text');
    const held = screen.getByText('Code cell held by').parentElement!;
    expect(nearestInk(within(held).getByText('has never signed'))).toBe('text-text');
  });
});
