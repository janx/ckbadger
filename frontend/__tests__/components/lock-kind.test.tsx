import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { LockKind } from '@/components/ui/lock-kind';
import { rowTagClass } from '@/components/ui/row-tag';
import { JOYID_CODE_HASH } from '@/__tests__/utils/ckb-address';
import { DEFAULT_LOCK_CODE_HASH } from '@/lib/lock-script';

/** A lock the registry does not name (made up). */
const CODE = `0x${'9b'.repeat(32)}`;
/** JoyID's lock, a real other lock (B4 fix round: fixtures say what they are). */
const JOYID = JOYID_CODE_HASH;

describe('LockKind (R-2)', () => {
  it('shows the registry name with the code hash in the title', () => {
    render(<LockKind address={null} size="row" lockKind="JoyID" lockCodeHash={JOYID} />);
    expect(screen.getByText('JoyID')).toHaveAttribute('title', JOYID);
  });

  // U-09 (re-pinned): the code hash at the size the caller names, the one ellipsis
  it('shows the code hash when the registry names none', () => {
    render(<LockKind address={null} size="row" lockKind={null} lockCodeHash={CODE} />);
    expect(screen.getByText('0x9b9b9b9b…9b9b9b')).toBeInTheDocument();
  });

  it('shows the code hash at the tight size where the caller names it', () => {
    render(<LockKind address={null} size="tight" lockKind={null} lockCodeHash={CODE} />);
    expect(screen.getByText('0x9b9b…9b9b')).toBeInTheDocument();
  });

  // U-15 (UI review 2026-11-14): "Lock kind unavailable" told the reader an internal state. A lock
  // the registry does not name is named by what is known: the word "lock" and its code hash,
  // shortened, linking to the script page of that code hash; one unit that never breaks inside.
  it('names an unregistered lock "lock <code hash>", the hash a link to its script page', () => {
    const { container } = render(
      <LockKind address={null} size="row" lockKind={null} lockCodeHash={CODE} />
    );
    const kind = container.querySelector('[data-lock-kind]') as HTMLElement;
    expect(kind).toHaveTextContent(/^lock 0x9b9b9b9b…9b9b9b$/);
    const link = screen.getByRole('link', { name: '0x9b9b9b9b…9b9b9b' });
    expect(link).toHaveAttribute('href', expect.stringMatching(new RegExp(`/script/${CODE}$`)));
    expect(link).toHaveAttribute('title', CODE);
    expect(kind).toHaveClass('whitespace-nowrap');
  });

  // A4 Chrome sweep (re-pinned): a registered name is words ("Nervape Shadow Lock"), not a unit, so
  // LockKind makes it nowrap in no form; only the unregistered "lock <code hash>" (a label with its
  // id) is. A row chip is a box whose own class keeps its words on one line (rowTagClass).
  it('lets a registered name break between its words (A4 Chrome sweep)', () => {
    render(
      <LockKind address={null} size="row" lockKind="Nervape Shadow Lock" lockCodeHash={JOYID} />
    );
    expect(screen.getByText('Nervape Shadow Lock')).not.toHaveClass('whitespace-nowrap');
  });

  it('keeps its dim form by default and takes a row chip’s classes (table-rows L2-4)', () => {
    const { rerender } = render(
      <LockKind address={null} size="row" lockKind="JoyID" lockCodeHash={JOYID} />
    );
    expect(screen.getByText('JoyID')).toHaveClass('text-text-dim');
    rerender(
      <LockKind
        address={null}
        size="row"
        lockKind="JoyID"
        lockCodeHash={JOYID}
        className={rowTagClass()}
      />
    );
    expect(screen.getByText('JoyID')).toHaveClass('text-meta', 'border', 'leading-4');
    expect(screen.getByText('JoyID')).toHaveAttribute('title', JOYID);
  });

  it('shows nothing for a lock that is not indexed', () => {
    const { container } = render(
      <LockKind address={null} size="row" lockKind={null} lockCodeHash={null} />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('refuses a kind without its code hash (the contract never sends one)', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(<LockKind size="row" lockKind="JoyID" lockCodeHash={null} address={null} />)
    ).toThrow('lock kind JoyID without its lock code hash');
    log.mockRestore();
  });

  // B4, the lead's ruling: silent for the default lock beside a party, named in a "Lock" column.
  // B4 fix round (re-pinned on purpose): the default lock is the pair (code hash, type), read off
  // the row's address; a bare code hash cannot tell it from another lock on that code hash.
  it('names the default lock only in a column, in quiet words without the column’s tag', () => {
    const address =
      'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4';
    const tag = rowTagClass('default', 'min-w-0 truncate');
    const lock = { lockKind: 'Default Lock', lockCodeHash: DEFAULT_LOCK_CODE_HASH, address };
    const { container, rerender } = render(<LockKind size="row" {...lock} className={tag} />);
    expect(container).toBeEmptyDOMElement();
    rerender(<LockKind size="row" {...lock} className={tag} place="column" />);
    const plain = screen.getByText('Default Lock');
    expect(plain).toHaveClass('text-text-dim', 'text-meta', 'truncate');
    expect(plain).not.toHaveClass('border');
    rerender(
      <LockKind
        size="row"
        lockKind="JoyID"
        lockCodeHash={JOYID}
        address={null}
        className={tag}
        place="column"
      />
    );
    expect(screen.getByText('JoyID')).toHaveClass('border');
    // the code hash alone (no address): it cannot be told from a lock on it by data, so it is named
    rerender(<LockKind size="row" {...lock} address={null} className={tag} />);
    expect(screen.getByText('Default Lock')).toHaveClass('border');
  });

  it('refuses a row whose address spells another code hash than the row says', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    const address =
      'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4';
    expect(() =>
      render(<LockKind size="row" lockKind="JoyID" lockCodeHash={JOYID} address={address} />)
    ).toThrow(/spells the lock code hash 0x9bd7e06f.*the row says 0xd00c84f0/);
    log.mockRestore();
  });
});
