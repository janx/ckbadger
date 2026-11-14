import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { LockKind } from '@/components/ui/lock-kind';
import { rowTagClass } from '@/components/ui/row-tag';

const CODE = `0x${'9b'.repeat(32)}`;

describe('LockKind (R-2)', () => {
  it('shows the registry name with the code hash in the title', () => {
    render(<LockKind lockKind="JoyID" lockCodeHash={CODE} />);
    expect(screen.getByText('JoyID')).toHaveAttribute('title', CODE);
  });

  it('shows the code hash when the registry names none', () => {
    render(<LockKind lockKind={null} lockCodeHash={CODE} />);
    expect(screen.getByText('0x9b9b9b...9b9b')).toBeInTheDocument();
  });

  // U-15 (UI review 2026-11-14): "Lock kind unavailable" told the reader an internal state. A lock
  // the registry does not name is named by what is known: the word "lock" and its code hash,
  // shortened, linking to the script page of that code hash; one unit that never breaks inside.
  it('names an unregistered lock "lock <code hash>", the hash a link to its script page', () => {
    const { container } = render(<LockKind lockKind={null} lockCodeHash={CODE} />);
    const kind = container.querySelector('[data-lock-kind]') as HTMLElement;
    expect(kind).toHaveTextContent(/^lock 0x9b9b9b\.\.\.9b9b$/);
    const link = screen.getByRole('link', { name: '0x9b9b9b...9b9b' });
    expect(link).toHaveAttribute('href', expect.stringMatching(new RegExp(`/script/${CODE}$`)));
    expect(link).toHaveAttribute('title', CODE);
    expect(kind).toHaveClass('whitespace-nowrap');
  });

  it('keeps its dim one-line form by default and takes a row chip’s classes (table-rows L2-4)', () => {
    const { rerender } = render(<LockKind lockKind="JoyID" lockCodeHash={CODE} />);
    expect(screen.getByText('JoyID')).toHaveClass('text-text-dim', 'whitespace-nowrap');
    rerender(<LockKind lockKind="JoyID" lockCodeHash={CODE} className={rowTagClass()} />);
    expect(screen.getByText('JoyID')).toHaveClass('text-meta', 'border', 'rounded-sm', 'leading-4');
    expect(screen.getByText('JoyID')).toHaveAttribute('title', CODE);
  });

  it('shows nothing for a lock that is not indexed', () => {
    const { container } = render(<LockKind lockKind={null} lockCodeHash={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('refuses a kind without its code hash (the contract never sends one)', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<LockKind lockKind="JoyID" lockCodeHash={null} />)).toThrow(
      'lock kind JoyID without its lock code hash'
    );
    log.mockRestore();
  });
});
