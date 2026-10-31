import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { LockKind } from '@/components/ui/lock-kind';

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
