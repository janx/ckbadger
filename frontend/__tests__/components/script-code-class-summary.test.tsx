import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { ScriptCodeClassTotalApi } from '@/lib/api';
import {
  ScriptCodeClassSummary,
  scriptClassLabel,
  scriptShare,
} from '@/components/scripts/script-code-evidence';
import { CODE_CLASS_COLORS } from '@/lib/design-tokens';

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
    const { container } = render(<ScriptCodeClassSummary classes={classes} lockScriptCount={36} />);
    const segment = (cls: string) =>
      container.querySelector<HTMLElement>(`[data-code-class-segment="${cls}"]`)!;
    expect(segment('fixed').style.backgroundColor).toBe(hexToRgb(CODE_CLASS_COLORS.fixed.fill));
    expect(hasEdge(segment('multisig'), CODE_CLASS_COLORS.multisig.edge)).toBe(true);
    expect(hasEdge(segment('one_key'), CODE_CLASS_COLORS.one_key.edge)).toBe(true);
    // Bar order follows the class order immutable → multisig → one key, whatever the API order.
    const order = [...container.querySelectorAll('[data-code-class-segment]')].map((node) =>
      node.getAttribute('data-code-class-segment')
    );
    expect(order).toEqual(['fixed', 'multisig', 'one_key']);
  });

  it('states the split in one sentence and keeps a legend with exact CKB (SC-L3)', () => {
    render(<ScriptCodeClassSummary classes={classes} lockScriptCount={36} />);
    const sentence = screen.getByTestId('code-class-sentence');
    expect(sentence.textContent).toMatch(
      /^Of the 49,794,300,000(\.0+)? CKB under these 36 lock scripts, /
    );
    expect(sentence.textContent).toContain('96.5% runs code no one can change');
    expect(sentence.textContent).toContain('3.47% runs code a multisig can replace');
    expect(sentence.textContent).toContain('0.075% runs code a single key can replace');
    const legend = screen.getByRole('list', { name: 'Code classes' });
    expect(within(legend).getAllByRole('listitem')).toHaveLength(3);
    expect(within(legend).getByText(/a multisig can replace/i)).toBeInTheDocument();
    expect(legend.textContent).toContain('1,727,000,000');
    expect(legend.textContent).toContain('18 scripts');
  });
});

describe('scriptShare (SC-L6)', () => {
  it('rounds half-up and keeps two significant digits below 0.1 %, never a false 0.00%', () => {
    // JoyID: 19,096,283 of 456,002,179.57 CKB = 4.1878…% (the page truncated it to 4.18%).
    expect(scriptShare(BigInt('1909628300000000'), BigInt('45600217957000000'))).toBe('4.19%');
    expect(scriptShare(BigInt(965), BigInt(1000))).toBe('96.5%');
    expect(scriptShare(BigInt(75), BigInt(100000))).toBe('0.075%');
    expect(scriptShare(BigInt(1), BigInt(10000000))).toBe('0.00001%');
    expect(scriptShare(BigInt(0), BigInt(10))).toBe('0%');
  });
});

function hasEdge(node: HTMLElement, hex: string): boolean {
  const shadow = node.style.boxShadow;
  return shadow.includes(hex) || shadow.includes(hexToRgb(hex));
}

function hexToRgb(hex: string): string {
  const h = hex.replace('#', '');
  return `rgb(${parseInt(h.slice(0, 2), 16)}, ${parseInt(h.slice(2, 4), 16)}, ${parseInt(h.slice(4, 6), 16)})`;
}

describe('ScriptCodeClassSummary · visual pass (R2b-b familyCount, D6, 390 px)', () => {
  it('counts scripts per class from familyCount, in the sentence and the legend (SC-L3)', () => {
    render(<ScriptCodeClassSummary classes={classes} lockScriptCount={36} />);
    const sentence = screen.getByTestId('code-class-sentence').textContent!;
    expect(sentence).toContain('3.47% runs code a multisig can replace (18 scripts)');
    expect(sentence).toContain('0.075% runs code a single key can replace (7 scripts)');
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
