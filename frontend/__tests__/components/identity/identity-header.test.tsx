import { render, screen } from '@/__tests__/utils/test-utils';
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

  it('gives every identity standard the Pure CKB badge and one positioning sentence', () => {
    const { rerender } = render(<IdentityPositioning standard="dotbit" />);
    expect(screen.getByText('Pure CKB').closest('[data-composition]')).toHaveAttribute(
      'data-composition',
      'pure_ckb'
    );
    expect(
      screen.getByText('The name and everything about it live in one CKB cell.')
    ).toBeInTheDocument();
    rerender(<IdentityPositioning standard="dotcell" />);
    expect(
      screen.getByText('The name lives in a CKB cell; its records ride in the witness beside it.')
    ).toBeInTheDocument();
    rerender(<IdentityPositioning standard="did_ckb" />);
    expect(screen.getByText('Its DID document lives in this CKB cell.')).toBeInTheDocument();
    rerender(<IdentityPositioning standard="bit_cell" />);
    expect(
      screen.getByText('A .bit name made into a cell of its own, held in an ordinary wallet.')
    ).toBeInTheDocument();
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
    rerender(<IdentityStatusBadge state="recycled" />);
    expect(screen.getByText('Recycled')).toHaveClass('text-text-dim');
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
