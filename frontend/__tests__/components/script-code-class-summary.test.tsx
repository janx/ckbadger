import { render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ScriptCodeClassTotalApi } from '@/lib/api';
import {
  ScriptCodeClassBar,
  ScriptCodeClassLegend,
  ScriptCodeClassSummary,
  scriptClassLabel,
} from '@/components/scripts/script-code-evidence';
import { CODE_CLASS_COLORS, CODE_CLASS_KEYS } from '@/lib/design-tokens';
import { percentOf } from '@/lib/format-unit';
import { color, inkOnFill, PALETTE_KEYS, type ThemeName } from '@/lib/palette';
import { resetThemeForTests } from '@/lib/theme';
import { stubChartWidth } from '@/__tests__/utils/chart-width';

// Scripts board (Version 50): of 49.77B CKB under the lock scripts, 96.5% immutable, 3.47% a
// multisig can replace, 0.075% one key can. Numbers below are shaped like /scripts classSummary.
const classes: ScriptCodeClassTotalApi[] = [
  // API order is not the display order: the colour must follow the class, never the index.
  {
    class: 'multisig',
    lockCellsCount: 161049,
    lockOwnedCapacitySum: '172700000000000000',
    referenceCount: 18,
    familyCount: 18,
  },
  {
    class: 'fixed',
    lockCellsCount: 9000000,
    lockOwnedCapacitySum: '4803000000000000000',
    referenceCount: 40,
    familyCount: 36,
  },
  {
    class: 'one_key',
    lockCellsCount: 92,
    lockOwnedCapacitySum: '3730000000000000',
    referenceCount: 7,
    familyCount: 7,
  },
];

describe('ScriptCodeClassSummary (Scripts board "Who can change the code")', () => {
  it('names the fixed class "Immutable", never "Fixed bytecode" (SC-L2)', () => {
    expect(scriptClassLabel('fixed')).toMatch(/^Immutable/);
    expect(scriptClassLabel('unspendable')).toMatch(/immutable/i);
    render(<ScriptCodeClassSummary classes={classes} lockScriptCount={36} />);
    expect(document.body.textContent).not.toMatch(/Fixed bytecode/);
    expect(screen.getAllByText(/^Immutable/).length).toBeGreaterThan(0);
  });

  it('colours each class by a fixed palette, not by position (SC-L3)', () => {
    // Hierarchy v3: the bar is the index hero's picture (ScriptCodeClassBar), not the summary's.
    const { container } = render(<ScriptCodeClassBar classes={classes} />);
    const segment = (cls: string) =>
      container.querySelector<HTMLElement>(`[data-code-class-segment="${cls}"]`)!;
    expect(segment('fixed').style.backgroundColor).toBe(CODE_CLASS_COLORS.fixed.fill);
    expect(hasEdge(segment('multisig'), CODE_CLASS_COLORS.multisig.edge)).toBe(true);
    expect(hasEdge(segment('one_key'), CODE_CLASS_COLORS.one_key.edge)).toBe(true);
    // Bar order follows the class order immutable → multisig → one key, whatever the API order.
    const order = [...container.querySelectorAll('[data-code-class-segment]')].map((node) =>
      node.getAttribute('data-code-class-segment')
    );
    expect(order).toEqual(['fixed', 'multisig', 'one_key']);
  });

  it('states the split in one sentence and keeps a legend with exact CKB (SC-L3)', () => {
    const { container } = render(<ScriptCodeClassSummary classes={classes} lockScriptCount={36} />);
    // Hierarchy v3: the summary is tier-2 evidence; the bar is the hero's (ScriptCodeClassBar).
    expect(container.querySelector('[data-code-class-segment]')).toBeNull();
    const sentence = screen.getByTestId('code-class-sentence');
    expect(sentence.textContent).toMatch(
      /^Of the 49,794,300,000(\.0+)? CKB under these 36\slock scripts, /
    );
    expect(sentence.textContent).toContain('96.46% runs code no one can change');
    expect(sentence.textContent).toContain('3.47% runs code a multisig can replace');
    expect(sentence.textContent).toContain('0.07% runs code a single key can replace');
    const legend = screen.getByRole('list', { name: 'Code classes' });
    expect(within(legend).getAllByRole('listitem')).toHaveLength(3);
    expect(within(legend).getByText(/a multisig can replace/i)).toBeInTheDocument();
    expect(legend.textContent).toContain('1,727,000,000');
    // re-pinned (A4 round 2): the count a unit, number and noun joined by a plain space
    expect(legend.textContent).toContain('18 scripts');
  });
});

// CH-14 and the review's item I (re-pinned on purpose): the scripts' shares are the one
// percentage, percentOf, two decimals and never a false 0.00% (scriptShare wrote one decimal from
// 10 %, two from 0.1 % and two significant digits below that; the page gave no reason in words)
describe('the scripts’ shares (SC-L6)', () => {
  it('round half up at two decimals and never print a false 0.00%', () => {
    // JoyID: 19,096,283 of 456,002,179.57 CKB = 4.1878…% (the page truncated it to 4.18%).
    expect(percentOf(BigInt('1909628300000000'), BigInt('45600217957000000'))).toBe('4.19%');
    expect(percentOf(BigInt(965), BigInt(1000))).toBe('96.50%');
    expect(percentOf(BigInt(75), BigInt(100000))).toBe('0.08%');
    expect(percentOf(BigInt(1), BigInt(10000000))).toBe('<0.01%');
    expect(percentOf(BigInt(0), BigInt(10))).toBe('0.00%');
  });
});

/** The segment's edge is drawn as a box-shadow in the class's edge colour (a theme reference). */
function hasEdge(node: HTMLElement, color: string): boolean {
  return node.style.boxShadow.includes(color);
}

/** A Tailwind text-colour class of a palette key (`text-text-bright`, `text-base-bg`), not a size. */
function isInkClass(name: string): boolean {
  const m = /^text-(.+?)(?:\/\d+)?$/.exec(name);
  return m !== null && (PALETTE_KEYS as string[]).includes(m[1]);
}

function inTheme(theme: ThemeName) {
  localStorage.setItem('ckbadger.theme', theme);
  resetThemeForTests();
  document.documentElement.setAttribute('data-theme', theme);
}

describe('ScriptCodeClassSummary · visual pass (R2b-b familyCount, D6, 390 px)', () => {
  it('counts scripts per class from familyCount, in the sentence and the legend (SC-L3)', () => {
    render(<ScriptCodeClassSummary classes={classes} lockScriptCount={36} />);
    const sentence = screen.getByTestId('code-class-sentence').textContent!;
    expect(sentence).toContain('3.47% runs code a multisig can replace (18\u00a0scripts)');
    expect(sentence).toContain('0.07% runs code a single key can replace (7\u00a0scripts)');
    const legend = screen.getByRole('list', { name: 'Code classes' });
    expect(legend).toHaveTextContent('· 18 scripts');
    expect(legend).toHaveTextContent('· 36 scripts');
    expect(legend.textContent).not.toContain('references');
  });

  it('fails loudly when a class summary row has no familyCount', () => {
    expect(() =>
      render(
        <ScriptCodeClassSummary
          classes={[
            { class: 'fixed', lockCellsCount: 1, lockOwnedCapacitySum: '1', referenceCount: 1 },
          ]}
          lockScriptCount={1}
        />
      )
    ).toThrow(/familyCount/);
  });

  it('says why an all-zero-args secp256k1 lock counts as unspendable (D6, SC-L9)', () => {
    render(
      <ScriptCodeClassSummary
        classes={[
          {
            class: 'unspendable',
            lockCellsCount: 2,
            lockOwnedCapacitySum: '200',
            referenceCount: 5,
            familyCount: 4,
          },
        ]}
        lockScriptCount={4}
      />
    );
    expect(screen.getByTestId('unspendable-note')).toHaveTextContent(
      'secp256k1 lock over 20 zero bytes'
    );
  });

  it('lets legend items wrap so a long exact amount never widens a phone screen', () => {
    render(<ScriptCodeClassSummary classes={classes} lockScriptCount={36} />);
    for (const item of within(screen.getByRole('list', { name: 'Code classes' })).getAllByRole(
      'listitem'
    ))
      expect(item).toHaveClass('flex-wrap');
  });
});

// Hierarchy v3 (C1, R12, R15): the bar is 28 px with each class's name and share inside the
// segments that can hold them; one quiet row keys every segment; exact amounts stay in tier 2.
describe('ScriptCodeClassBar and its legend row (hierarchy v3, C1)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    localStorage.clear();
    resetThemeForTests();
    document.documentElement.setAttribute('data-theme', 'dark');
  });

  it('writes the name and share inside a segment that fits, dark on the light fill, and pads only a labelled segment', () => {
    stubChartWidth(1376);
    const { container } = render(<ScriptCodeClassBar classes={classes} />);
    const bar = screen.getByRole('img', {
      name: 'CKB under lock scripts by who can change their code',
    });
    expect(bar).toHaveClass('h-[28px]', 'text-small');
    const segments = [...container.querySelectorAll<HTMLElement>('[data-code-class-segment]')];
    expect(segments.map((s) => s.textContent)).toEqual([
      'Immutable · pinned by data hash 96.46%',
      '',
      '',
    ]);
    // re-pinned on purpose (readability F1 follow-up): the dark ink is the fill-ink rule's, the
    // well, set as the segment's colour; it was the class text-base-bg, named by hand
    expect(segments[0]).toHaveClass('px-2.5');
    expect(segments[0].style.color).toBe('var(--c-base-void)');
    // An unlabelled sliver is not widened by padding: 0.075 % stays a sliver.
    expect(segments[2]).not.toHaveClass('px-2.5');
    expect(segments[2]).toHaveClass('min-w-[3px]');
    expect(segments[1].title).toBe('A multisig can replace: 3.47%');
  });

  it('writes light words on the dark outlined segments where they fit', () => {
    stubChartWidth(1376);
    const { container } = render(
      <ScriptCodeClassBar
        classes={[
          { ...classes[0], lockOwnedCapacitySum: '600' },
          { ...classes[1], class: 'unspendable', lockOwnedCapacitySum: '400' },
        ]}
      />
    );
    const segments = [...container.querySelectorAll<HTMLElement>('[data-code-class-segment]')];
    expect(segments.map((s) => s.dataset.codeClassSegment)).toEqual(['unspendable', 'multisig']);
    expect(segments.map((s) => s.textContent)).toEqual([
      'Immutable · code cell unspendable 40.00%',
      'A multisig can replace 60.00%',
    ]);
    // re-pinned on purpose (readability F1 follow-up): the bright ink comes from the fill-ink rule
    // as the segment's colour; it was the class text-text-bright, named by hand
    for (const s of segments) expect(s.style.color).toBe('var(--c-text-bright)');
  });

  it('keeps the words of the wide segment at 358 px and holds none at 240 px', () => {
    stubChartWidth(358);
    const { container: phone, unmount } = render(<ScriptCodeClassBar classes={classes} />);
    expect(
      [...phone.querySelectorAll('[data-code-class-segment]')].map((s) => s.textContent)
    ).toEqual(['Immutable · pinned by data hash 96.46%', '', '']);
    unmount();
    vi.restoreAllMocks();
    stubChartWidth(240);
    const { container } = render(<ScriptCodeClassBar classes={classes} />);
    for (const s of container.querySelectorAll('[data-code-class-segment]'))
      expect(s.textContent).toBe('');
  });

  it('draws nothing when no lock script holds CKB', () => {
    const { container } = render(
      <ScriptCodeClassBar classes={[{ ...classes[0], lockOwnedCapacitySum: '0' }]} />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('keys every drawn segment with its swatch, name and share in one quiet row, no exact amounts', () => {
    render(<ScriptCodeClassLegend classes={classes} />);
    const legend = screen.getByRole('list', { name: 'Who can change the code, in shares' });
    expect(legend).toHaveClass('text-small', 'text-text-dim', 'flex', 'flex-wrap');
    expect(
      within(legend)
        .getAllByRole('listitem')
        .map((li) => li.textContent)
    ).toEqual([
      'Immutable · pinned by data hash 96.46%',
      'A multisig can replace 3.47%',
      'One key can replace 0.07%',
    ]);
    expect(legend.textContent).not.toMatch(/\d\.\d{8}|scripts/);
  });

  // Readability F1 follow-up (DY-03's rule): the words inside a segment take the ink the segment's
  // own fill calls for in the theme in force (inkOnFill), never an ink the component names per
  // class; on the unspendable grey the hand-picked bright ink fell to 3.11:1 by night.
  it.each(['dark', 'light'] as const)(
    'writes every segment’s words in the ink the fill-ink rule gives for its fill: %s',
    (theme) => {
      inTheme(theme);
      stubChartWidth(4000);
      const drawn = ['fixed', 'unspendable', 'multisig', 'retired'] as const;
      const { container } = render(
        <ScriptCodeClassBar
          classes={drawn.map((cls) => ({
            class: cls,
            lockCellsCount: 1,
            lockOwnedCapacitySum: '250',
            referenceCount: 1,
            familyCount: 1,
          }))}
        />
      );
      const segments = [...container.querySelectorAll<HTMLElement>('[data-code-class-segment]')];
      expect(segments.map((s) => s.dataset.codeClassSegment)).toEqual(drawn);
      for (const s of segments) {
        const cls = s.dataset.codeClassSegment as (typeof drawn)[number];
        expect(s.textContent, cls).toMatch(/ 25\.00%$/);
        const rule = inkOnFill(theme, CODE_CLASS_KEYS[cls].fill, 1).ink;
        expect(s.style.color, `${theme} ${cls}`).toBe(color(rule));
        // no ink class on the segment or on its words: the rule's ink is the only one
        for (const el of [s, ...s.querySelectorAll<HTMLElement>('*')])
          expect([...el.classList].filter(isInkClass), `${theme} ${cls}`).toEqual([]);
      }
    }
  );

  it('fails loudly on an unknown class, as the summary does', () => {
    expect(() =>
      render(
        <ScriptCodeClassBar
          classes={[{ ...classes[0], class: 'nonsense' as never, lockOwnedCapacitySum: '1' }]}
        />
      )
    ).toThrow(/Unknown script code class nonsense/);
  });
});
