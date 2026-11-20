import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '../utils/test-utils';
import { StatusStrip, type StripFact } from '@/components/ui/status-strip';

// Lane D2 (the UI review's PA-13, the transaction page's board at gate D0′): one status strip under
// the id line of the five entity pages. A `dl` of at most four facts between two row rules; from sm
// the facts stand in one line (a fact a 12 px quiet label over its 14 px bright value, its note 13
// px quiet, 44 px between facts, nothing wrapped inside a fact); below sm each fact is one labelled
// line (the label column, then the value at 13 px), wrapping only between its unbroken parts and
// never starting a line with a separator. It refuses a fifth fact and an empty value, and states
// nothing its caller did not give.

const FACTS: StripFact[] = [
  { label: 'status', value: 'committed', note: '36\u00a0confirmations', dot: 'jade' },
  { label: 'time', value: '2026-11-14 12:54:57 UTC+8', note: '4m ago' },
  { label: 'block', value: '#21,008,952' },
  { label: 'fee', value: '0.0015724 CKB', note: 'paid by ckb1…sqylvfrs' },
];

const strip = () => screen.getByRole('group', { name: 'Transaction facts' });
const fact = (label: string) =>
  strip().querySelector<HTMLElement>(`[data-strip-fact="${label}"]`) as HTMLElement;

describe('StatusStrip', () => {
  it('is a dl of its facts between two row rules, named, each a label over its value', () => {
    render(<StatusStrip label="Transaction facts" facts={FACTS} />);
    const dl = strip();
    expect(dl.tagName).toBe('DL');
    expect(dl).toHaveAttribute('data-status-strip');
    // the two row rules
    expect(dl).toHaveClass('border-y', 'border-row-rule');
    expect(
      within(dl)
        .getAllByRole('term')
        .map((t) => t.textContent)
    ).toEqual(['status', 'time', 'block', 'fee']);
    // each label is followed by its own value, in order
    for (const { label } of FACTS) {
      const [dt, dd] = Array.from(fact(label).children);
      expect(dt.tagName).toBe('DT');
      expect(dd.tagName).toBe('DD');
      expect(dt).toHaveTextContent(label);
    }
  });

  it('inks by role: a label 12 px quiet, a value bright (13 px below sm, 14 from sm), a note 13 px quiet', () => {
    render(<StatusStrip label="Transaction facts" facts={FACTS} />);
    const [dt, dd] = Array.from(fact('status').children) as HTMLElement[];
    expect(dt).toHaveClass('text-meta', 'text-text-dim');
    expect(dd).toHaveClass('text-text-bright', 'text-small', 'sm:text-body');
    const note = within(dd).getByText('36 confirmations');
    expect(note).toHaveClass('text-small', 'text-text-dim');
    // the value itself carries no ink of its own: the dd's bright
    expect(within(dd).getByText('committed').className).not.toMatch(/text-text/);
  });

  it('from sm sets the facts in one line, 44 px apart, each a column; below sm a labelled line each', () => {
    render(<StatusStrip label="Transaction facts" facts={FACTS} />);
    // below sm: a grid of two columns, the label then the value, 12 px between, 4 px between lines
    expect(strip()).toHaveClass('grid', 'grid-cols-[auto_minmax(0,1fr)]', 'gap-x-3', 'gap-y-1');
    // from sm: a row of facts 44 px apart (a fact moves to a second row whole where the row
    // cannot hold it: nothing is cut and nothing wraps inside a fact)
    expect(strip()).toHaveClass('sm:flex', 'sm:flex-wrap', 'sm:gap-x-11');
    for (const { label } of FACTS) {
      // the fact is no box below sm (its label and value are the grid's two cells) and a column
      // from sm, its label 4 px over its value
      expect(fact(label)).toHaveClass('contents', 'sm:flex', 'sm:flex-col', 'sm:gap-1');
    }
  });

  it('joins a value and its note with the one separator list: never wrapped from sm, never a line opened by a separator', () => {
    render(<StatusStrip label="Transaction facts" facts={FACTS} />);
    const dd = fact('time').querySelector('dd') as HTMLElement;
    const list = dd.querySelector('[data-dotted]') as HTMLElement;
    expect(list).not.toBeNull();
    // the parts are the value and its note, in order; the separator is the list's (hidden from a
    // screen reader, never drawn at a line's start)
    const items = Array.from(list.querySelectorAll('[data-dotted-item]'));
    expect(items.map((i) => i.textContent)).toEqual(['2026-11-14 12:54:57 UTC+8', ' · 4m ago']);
    expect(list.querySelectorAll('[data-dotted-separator]')).toHaveLength(1);
    // below sm it wraps between its parts; from sm it does not wrap at all
    expect(list).toHaveClass('flex-wrap', 'sm:flex-nowrap');
    // a fact with no note is its value alone, with no separator
    const block = fact('block').querySelector('dd') as HTMLElement;
    expect(block.querySelectorAll('[data-dotted-separator]')).toHaveLength(0);
    expect(block).toHaveTextContent(/^#21,008,952$/);
  });

  it('takes several note parts, each joined by the separator', () => {
    render(
      <StatusStrip
        label="Block facts"
        facts={[
          {
            label: 'time',
            value: '2026-11-14 12:54:57 UTC+8',
            note: ['4m ago', '10 s after its parent'],
          },
        ]}
      />
    );
    const dd = screen.getByRole('definition');
    expect(dd).toHaveTextContent('2026-11-14 12:54:57 UTC+8 · 4m ago · 10 s after its parent');
    expect(dd.querySelectorAll('[data-dotted-separator]')).toHaveLength(2);
  });

  it('draws a state’s dot in the ink it is given, a 6 px round mark before the value', () => {
    render(<StatusStrip label="Transaction facts" facts={FACTS} />);
    const dot = fact('status').querySelector('[data-strip-dot]') as HTMLElement;
    expect(dot).toHaveAttribute('aria-hidden', 'true');
    expect(dot).toHaveClass('h-1.5', 'w-1.5', 'rounded-full');
    expect(dot.style.backgroundColor).toBe('var(--c-jade)');
    // only the fact given a dot has one
    expect(strip().querySelectorAll('[data-strip-dot]')).toHaveLength(1);
  });

  it('states what it is given and nothing else', () => {
    render(<StatusStrip label="Transaction facts" facts={FACTS} />);
    expect(strip()).toHaveTextContent(
      'statuscommitted · 36 confirmations' +
        'time2026-11-14 12:54:57 UTC+8 · 4m ago' +
        'block#21,008,952' +
        'fee0.0015724 CKB · paid by ckb1…sqylvfrs'
    );
  });

  it('holds at most four facts: a fifth is refused, by name', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <StatusStrip
          label="Transaction facts"
          facts={[...FACTS, { label: 'size', value: '1,024 bytes' }]}
        />
      )
    ).toThrow('status strip "Transaction facts": 5 facts; a strip holds at most four');
  });

  it('refuses an empty value: a fact the page does not know is not passed', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    for (const value of [null, undefined, false, ''])
      expect(() =>
        render(<StatusStrip label="Block facts" facts={[{ label: 'miner', value }]} />)
      ).toThrow(
        'status strip "Block facts": the fact "miner" has no value; a fact the page does not know is not passed'
      );
  });

  it('refuses a strip of no facts, and two facts of one name', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<StatusStrip label="Cell facts" facts={[]} />)).toThrow(
      'status strip "Cell facts": no facts'
    );
    expect(() =>
      render(
        <StatusStrip
          label="Cell facts"
          facts={[
            { label: 'block', value: '#1' },
            { label: 'block', value: '#2' },
          ]}
        />
      )
    ).toThrow('status strip "Cell facts": two facts named "block"');
  });
});
