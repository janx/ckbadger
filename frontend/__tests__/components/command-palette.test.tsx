import { fireEvent, render, screen } from '../utils/test-utils';
import { describe, expect, it, vi, beforeEach } from 'vitest';

const pushMock = vi.hoisted(() => vi.fn());

vi.mock('@/src/navigation', () => ({
  useRouter: () => ({
    push: pushMock,
    replace: vi.fn(),
    back: vi.fn(),
    forward: vi.fn(),
    refresh: vi.fn(),
    prefetch: vi.fn(),
  }),
  useSearchParams: () => new URLSearchParams(),
  usePathname: () => '/',
  useParams: () => ({}),
}));

import { CommandPalette } from '@/components/command-palette';

describe('CommandPalette', () => {
  beforeEach(() => {
    pushMock.mockReset();
  });

  it('opens with Ctrl+K and executes a matching command', () => {
    render(<CommandPalette />);

    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });

    const input = screen.getByLabelText('Command palette input');
    fireEvent.change(input, { target: { value: 'blocks' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(pushMock).toHaveBeenCalledWith('/blocks');
  });

  it('executes highlighted go-to command when query is empty', () => {
    render(<CommandPalette />);

    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });

    const input = screen.getByLabelText('Command palette input');
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(pushMock).toHaveBeenCalledWith('/blocks');
  });

  it('falls back to search-bar routing when no command matches', () => {
    render(<CommandPalette />);

    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });

    const input = screen.getByLabelText('Command palette input');
    const txHash = `0x${'a'.repeat(64)}`;
    fireEvent.change(input, { target: { value: `tx:${txHash}` } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(pushMock).toHaveBeenCalledWith(`/tx/${txHash}`);
  });

  it('focuses global search input on slash', () => {
    render(
      <>
        <input data-ckbadger-global-search="true" aria-label="global search input" />
        <CommandPalette />
      </>
    );

    const searchInput = screen.getByLabelText('global search input');
    fireEvent.keyDown(window, { key: '/' });

    expect(searchInput).toHaveFocus();
  });

  it('navigates to blocks with g b chord', () => {
    render(<CommandPalette />);

    fireEvent.keyDown(window, { key: 'g' });
    fireEvent.keyDown(window, { key: 'b' });

    expect(pushMock).toHaveBeenCalledWith('/blocks');
  });

  it('navigates to assets with g a chord', () => {
    render(<CommandPalette />);

    fireEvent.keyDown(window, { key: 'g' });
    fireEvent.keyDown(window, { key: 'a' });

    expect(pushMock).toHaveBeenCalledWith('/assets');
  });

  it('navigates to scripts with g s chord', () => {
    render(<CommandPalette />);

    fireEvent.keyDown(window, { key: 'g' });
    fireEvent.keyDown(window, { key: 's' });

    expect(pushMock).toHaveBeenCalledWith('/scripts');
  });

  it('navigates to home with g h chord', () => {
    render(<CommandPalette />);

    fireEvent.keyDown(window, { key: 'g' });
    fireEvent.keyDown(window, { key: 'h' });

    expect(pushMock).toHaveBeenCalledWith('/');
  });

  it('navigates to charts with g c chord', () => {
    render(<CommandPalette />);

    fireEvent.keyDown(window, { key: 'g' });
    fireEvent.keyDown(window, { key: 'c' });

    expect(pushMock).toHaveBeenCalledWith('/charts');
  });

  it('does not navigate with g f chord', () => {
    render(<CommandPalette />);

    fireEvent.keyDown(window, { key: 'g' });
    fireEvent.keyDown(window, { key: 'f' });

    expect(pushMock).not.toHaveBeenCalled();
  });

  it('opens shortcut help panel with question mark', () => {
    render(<CommandPalette />);

    fireEvent.keyDown(window, { key: '?' });

    expect(screen.getByText('Keyboard Shortcuts')).toBeInTheDocument();
    expect(screen.getByText('Open command palette')).toBeInTheDocument();
    expect(screen.getByText('Focus global search bar')).toBeInTheDocument();
  });
});
