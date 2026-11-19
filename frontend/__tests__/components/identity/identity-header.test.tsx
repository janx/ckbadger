import { render, screen } from '@/__tests__/utils/test-utils';
import { numberStrings } from '@/__tests__/utils/lede-rule';
import {
  IdentityName,
  IdentityPositioning,
  IdentityStatusBadge,
  identityNameParts,
} from '@/components/identity/identity-header';

describe('identity header (ID-C5 / ID-C10 / ID-BC1)', () => {
  it('sets the standard suffix in the identity rose, the name in bright', () => {
    render(
      <h1>
        <IdentityName name="valar" suffix=".bit" />
      </h1>
    );
    const suffix = screen.getByText('.bit');
    expect(suffix).toHaveClass('text-identity');
    expect(screen.getByRole('heading')).toHaveTextContent('valar.bit');
  });

  it('splits a name at its standard suffix and nowhere else', () => {
    expect(identityNameParts('valar.bit', '.bit')).toEqual({ name: 'valar', suffix: '.bit' });
    expect(identityNameParts('apt.cell', '.cell')).toEqual({ name: 'apt', suffix: '.cell' });
    expect(identityNameParts('sub.apt.cell', '.cell')).toEqual({
      name: 'sub.apt',
      suffix: '.cell',
    });
    expect(identityNameParts('odd', '.bit')).toEqual({ name: 'odd', suffix: '' });
  });

  // Re-pinned on purpose (readability R8, lane p-assets; renamed, it was "gives every identity
  // standard the Pure CKB badge and one positioning sentence"): an identity's lede is the sentence
  // that says what it is and whether it is live, by its standard and its state (the review: the
  // generic "The name and everything about it live in one CKB cell." stood beside "0 CKB · no live
  // cell" on a recycled .bit name). It states no figure (the lede rule, C7).
  it('gives every identity its state in one sentence, the Pure CKB badge after it', () => {
    const { rerender } = render(<IdentityPositioning standard="dotbit" state="live" />);
    expect(screen.getByText('Pure CKB').closest('[data-composition]')).toHaveAttribute(
      'data-composition',
      'pure_ckb'
    );
    const said = () => screen.getByTestId('identity-state-sentence');
    expect(said()).toHaveTextContent(
      'A live .bit name: it and everything about it live in one CKB cell.'
    );
    for (const [standard, state, words] of [
      ['dotbit', 'expired', 'A .bit name whose term has ended; its CKB cell still holds it.'],
      [
        'dotbit',
        'recycled',
        'A .bit name that is no longer a live account: no CKB cell holds it now.',
      ],
      [
        'bit_cell',
        'live',
        'A live .bit Cell: a .bit name made into a cell of its own, held in an ordinary wallet.',
      ],
      ['bit_cell', 'expired', 'A .bit Cell whose name’s term has ended; the cell is still live.'],
      ['bit_cell', 'inactive', 'A .bit Cell that is no longer live: no cell holds the name now.'],
      [
        'did_ckb',
        'live',
        'A live Web5 identity: its DID document lives in this CKB cell, with no expiry.',
      ],
      [
        'did_ckb',
        'inactive',
        'A Web5 identity that is no longer live: no CKB cell holds its DID document now.',
      ],
      [
        'dotcell',
        'active',
        'A live .cell name: it lives in a CKB cell, its records in the witness beside it.',
      ],
      [
        'dotcell',
        'grace',
        'A .cell name whose term has ended, in its grace period; its cell is still live.',
      ],
      [
        'dotcell',
        'free',
        'A .cell name past its term and its grace period, free to register; its cell is still live.',
      ],
      ['dotcell', 'recycled', 'A .cell name that is no longer live: no CKB cell holds it now.'],
    ] as const) {
      rerender(<IdentityPositioning standard={standard} state={state} />);
      expect(said()).toHaveTextContent(words);
      expect(said().textContent!.length).toBeLessThanOrEqual(160);
      // no figure of the metrics row: no number of two characters or more (the lede rule, C7)
      expect(numberStrings(said().textContent!)).toEqual([]);
    }
    // the review's plainest sentence, said where a .bit Cell outlives its account
    rerender(<IdentityPositioning standard="bit_cell" state="expired" accountGone />);
    expect(said()).toHaveTextContent(
      'A .bit Cell whose name’s term has ended; the .bit account is no longer live, but this cell remains.'
    );
  });

  it('reads its state before the composition badge, in the reading ink at the body step (R8)', () => {
    render(<IdentityPositioning standard="dotbit" state="recycled" />);
    const said = screen.getByTestId('identity-state-sentence');
    const badge = screen.getByText('Pure CKB').closest('[data-composition]')!;
    expect(said.compareDocumentPosition(badge) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(said.tagName).toBe('P');
    expect(said).toHaveClass('text-text', 'text-body');
    // the badge beside it is the small one, quieter than the sentence
    expect(badge).toHaveClass('h-[18px]');
  });

  it('refuses a state its standard does not have', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<IdentityPositioning standard="did_ckb" state="grace" />)).toThrow(
      /did_ckb.*grace/
    );
    error.mockRestore();
  });

  it('shows an expired identity in neutral grey with a dashed border, a live one in jade', () => {
    const { rerender } = render(<IdentityStatusBadge state="expired" />);
    const expired = screen.getByText('Expired');
    expect(expired).toHaveClass('border-dashed', 'border-text-dim', 'text-text');
    expect(expired.className).not.toMatch(/gold|jade/);
    rerender(<IdentityStatusBadge state="live" />);
    expect(screen.getByText('Live')).toHaveClass('text-jade', 'border-jade/40');
    rerender(<IdentityStatusBadge state="active" />);
    expect(screen.getByText('Active')).toHaveClass('text-jade');
    rerender(<IdentityStatusBadge state="grace" />);
    expect(screen.getByText('In grace period')).toHaveClass('border-dashed');
    // Re-pinned on purpose (readability R8, lane p-assets): the state reads in the reading ink
    // with a border that shows (it was the quiet ink on a 1.15 : 1 border, the head's weakest mark,
    // under the Pure CKB badge every identity page carries)
    for (const [state, words] of [
      ['recycled', 'Recycled'],
      ['inactive', 'Inactive'],
      ['free', 'Free to register'],
    ] as const) {
      rerender(<IdentityStatusBadge state={state} />);
      expect(screen.getByText(words)).toHaveClass('text-text', 'border-text-dim');
      expect(screen.getByText(words).className).not.toMatch(/text-text-dim|border-base-border/);
    }
  });
});

describe('IdentityPositioning for a whole collection (ID-COL-5)', () => {
  it('speaks of every name in the standard, not one cell', () => {
    const { rerender } = render(<IdentityPositioning standard="did_ckb" scope="collection" />);
    expect(
      screen.getByText('Each did:ckb identity keeps its DID document in a CKB cell.')
    ).toBeInTheDocument();
    rerender(<IdentityPositioning standard="dotbit" scope="collection" />);
    expect(
      screen.getByText('Each .bit name and everything about it live in one CKB cell.')
    ).toBeInTheDocument();
  });
});
