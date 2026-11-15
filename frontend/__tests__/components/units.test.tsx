import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';
import { unitText, unitNode } from '@/components/ui/units';

/**
 * A4 round 2 (the second Chrome pass): dates, shortened ids and counts broke inside — "2019-11-16"
 * at a hyphen, "0x9bd7e06f…a3cce8" after its ellipsis, "78,675,029 cells" inside "cells". The one
 * rendering point for a composed string: each unit in the forms the site's own formatters write
 * is one unit, a count breaks only between its number and its noun, and nothing is added to the
 * text (the strings below are the ones the pass measured).
 */
const units = (text: string) => {
  const { container } = render(<p>{unitText(text)}</p>);
  return {
    container,
    whole: [...container.querySelectorAll('[data-nobreak]')].map((u) => u.textContent),
    counts: [...container.querySelectorAll<HTMLElement>('[data-count]')],
  };
};

describe('unitText: the units a composed string carries', () => {
  it('keeps a date, a date-time, a time, a month and a range whole', () => {
    expect(units('on 2019-11-16').whole).toEqual(['2019-11-16']);
    expect(units(', decoded 2026-11-13 21:19:20 UTC+8').whole).toEqual([
      '2026-11-13 21:19:20 UTC+8',
    ]);
    expect(units('2026-11-15 12:03:29').whole).toEqual(['2026-11-15 12:03:29']);
    expect(units('mined 12:03:29 UTC+8').whole).toEqual(['12:03:29 UTC+8']);
    expect(units('2026-10 →').whole).toEqual(['2026-10']);
    expect(units('2026-11-08 → 2026-11-14').whole).toEqual(['2026-11-08 → 2026-11-14']);
    expect(units('Coverage: 2026-10-27 through 2026-11-14').whole).toEqual([
      '2026-10-27',
      '2026-11-14',
    ]);
  });

  it('keeps a shortened id whole, an out point with its index', () => {
    expect(units('pubkey hash 0x4a33…d637').whole).toEqual(['0x4a33…d637']);
    expect(units('MEMES · 0x5cb3…b4fa').whole).toEqual(['0x5cb3…b4fa']);
    expect(units('by the same miner (ckb1qzda0cr0…4s93hhsj).').whole).toEqual([
      'ckb1qzda0cr0…4s93hhsj',
    ]);
    expect(units('cell 0x673e18…55f9:0').whole).toEqual(['0x673e18…55f9:0']);
  });

  it('sets a count as a unit that breaks only between its number and its noun', () => {
    const { counts, whole, container } = units(
      'one type id, 0x9bd7e06f…a3cce8, 1\u00a0binary · then 30\u00a0days of grace, to 2027-10-21'
    );
    expect(whole).toEqual(['0x9bd7e06f…a3cce8', '2027-10-21']);
    expect(counts.map((c) => c.textContent)).toEqual(['1 binary', '30 days']);
    for (const count of counts)
      expect(count).toHaveClass('inline-block', 'max-w-full', '[overflow-wrap:normal]');
    // a signed count keeps its sign with its number
    expect(units('+3\u00a0participants').counts.map((c) => c.textContent)).toEqual([
      '+3 participants',
    ]);
    // the copy is the text: nothing added, the count's no-break space a plain space
    expect(container.textContent).toBe(
      'one type id, 0x9bd7e06f…a3cce8, 1 binary · then 30 days of grace, to 2027-10-21'
    );
  });

  it('leaves a string with no unit as it is, and a node it is not given as text', () => {
    expect(unitText('free to break')).toBe('free to break');
    const node = <b>x</b>;
    expect(unitNode(node)).toBe(node);
    expect(unitNode(null)).toBeNull();
  });
});
