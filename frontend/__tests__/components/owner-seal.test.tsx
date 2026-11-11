import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { OWNER_SEAL, OwnerSeal } from '@/components/ui/owner-seal';
import { RowSeal } from '@/components/ui/row-seal';
import { drawSeal } from '@/lib/seal/stones';
import {
  expectBefore,
  expectOneRowSeal,
  expectOwnerSealOf,
  SEAL_IDS,
  SEAL_INKS,
  sealIdsAt,
} from '@/__tests__/utils/seals';

const LOCK = SEAL_IDS.cell.lock;

describe('OwnerSeal (the seals by kind)', () => {
  it('stands the 20 px stone of the lock before an address’s text, an image and not a link', () => {
    render(
      <p data-testid="who">
        <OwnerSeal lockHash={LOCK} />
        <a href="/address/ckb1qyq">ckb1qyq…3cda6</a>
      </p>
    );
    const who = screen.getByTestId('who');
    const owner = expectOwnerSealOf(who, LOCK);
    const link = screen.getByRole('link', { name: 'ckb1qyq…3cda6' });
    expectBefore(owner, link);
    expect(link).not.toContainElement(owner);
    // one image, named as the address's own stone is
    expect(screen.getByRole('img')).toHaveAttribute(
      'aria-label',
      `Script seal ${LOCK.slice(0, 10)}`
    );
    expect(owner).toHaveClass('inline-flex', 'align-middle');
  });

  it('is a script’s row stone 20 px high and 15 wide for every lock, 5 px before the address', () => {
    expect(OWNER_SEAL).toEqual({ height: 20, width: 15, gap: 5 });
    for (let i = 0; i < 24; i++) {
      const lockHash = sealIdsAt(i).cell.lock;
      const { svg } = drawSeal(
        { kind: 'script', hash: lockHash },
        { form: 'row', height: OWNER_SEAL.height, state: 'pressed', ...SEAL_INKS }
      );
      expect(svg).toMatch(/^<svg width="15" height="20"/);
      const { container, unmount } = render(<OwnerSeal lockHash={lockHash} />);
      expectOwnerSealOf(container, lockHash);
      unmount();
    }
  });

  // A cell row on the transaction page: the cell's own stone first, then the owner's stone before
  // the owner's address. The two are told apart: the row has one seal, and one owner's stone.
  it('stands beside a cell’s row seal without being taken for it', () => {
    const cell = SEAL_IDS.cell;
    render(
      <div data-testid="row">
        <RowSeal id={cell} />
        <span>{cell.tx.slice(0, 10)}…:1</span>
        <span>
          <OwnerSeal lockHash={cell.lock} />
          ckb1qyq…3cda6
        </span>
      </div>
    );
    const row = screen.getByTestId('row');
    const seal = expectOneRowSeal(row, cell);
    const owner = expectOwnerSealOf(row, cell.lock);
    expectBefore(seal, owner);
    expect(screen.getAllByRole('img')).toHaveLength(2);
  });

  it('draws once per lock: a re-render keeps the same element', () => {
    const { container, rerender } = render(<OwnerSeal lockHash={LOCK} />);
    const first = container.querySelector('svg');
    rerender(<OwnerSeal lockHash={LOCK} />);
    expect(container.querySelector('svg')).toBe(first);
    rerender(<OwnerSeal lockHash={SEAL_IDS.script.hash} />);
    expectOwnerSealOf(container, SEAL_IDS.script.hash);
  });

  it('refuses a lock hash that is not 32 lowercase bytes, with no fallback', () => {
    for (const bad of [LOCK.slice(0, 65), LOCK.toUpperCase().replace('0X', '0x'), LOCK.slice(2)])
      expect(() => render(<OwnerSeal lockHash={bad} />)).toThrow(
        'the script hash is not a lowercase 32-byte hash'
      );
  });

  it('carries the caller’s classes', () => {
    const { container } = render(<OwnerSeal lockHash={LOCK} className="-my-px" />);
    expect(container.firstElementChild).toHaveClass('-my-px', 'mr-[5px]');
  });
});
