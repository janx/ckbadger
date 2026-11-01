import { describe, expect, it } from 'vitest';
import { act, fireEvent, render, renderHook, screen } from '@testing-library/react';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';

describe('usePinnedReadout (R6: tap to pin, tap again or Escape to clear)', () => {
  it('shows the hovered target while hovered and nothing after', () => {
    const { result } = renderHook(() => usePinnedReadout<string>());
    expect(result.current.active).toBeNull();
    act(() => result.current.hover('a'));
    expect(result.current.active).toBe('a');
    expect(result.current.pinned).toBeNull();
    act(() => result.current.hover(null));
    expect(result.current.active).toBeNull();
  });

  it('pins a tapped target, keeps it after the pointer leaves, and a second tap clears it', () => {
    const { result } = renderHook(() => usePinnedReadout<string>());
    // a tap on a touch screen: an emulated mouseenter, then the click
    act(() => result.current.hover('a'));
    act(() => result.current.toggle('a'));
    expect(result.current.pinned).toBe('a');
    act(() => result.current.hover(null));
    expect(result.current.active).toBe('a');
    // the second tap: the emulated hover is still on it, yet the readout clears
    act(() => result.current.hover('a'));
    act(() => result.current.toggle('a'));
    expect(result.current.pinned).toBeNull();
    expect(result.current.active).toBeNull();
  });

  it('moves the pin to another target, and a hover shows over a pin', () => {
    const { result } = renderHook(() => usePinnedReadout<string>());
    act(() => result.current.toggle('a'));
    act(() => result.current.toggle('b'));
    expect(result.current.pinned).toBe('b');
    act(() => result.current.hover('c'));
    expect(result.current.active).toBe('c');
    act(() => result.current.hover(null));
    expect(result.current.active).toBe('b');
  });

  it('compares targets by key, so a fresh object for the same target unpins it', () => {
    const { result } = renderHook(() =>
      usePinnedReadout<{ kind: string; id: string }>((t) => `${t.kind}:${t.id}`)
    );
    act(() => result.current.toggle({ kind: 'node', id: 'x' }));
    expect(result.current.pinned).toEqual({ kind: 'node', id: 'x' });
    act(() => result.current.toggle({ kind: 'node', id: 'x' }));
    expect(result.current.pinned).toBeNull();
  });

  it('clears the pin on Escape', () => {
    const { result } = renderHook(() => usePinnedReadout<string>());
    act(() => result.current.toggle('a'));
    act(() => {
      fireEvent.keyDown(window, { key: 'Escape' });
    });
    expect(result.current.pinned).toBeNull();
    expect(result.current.active).toBeNull();
  });

  it('binds a target: hover, focus, click and Enter all drive the readout', () => {
    function Probe() {
      const readout = usePinnedReadout<string>();
      return (
        <div>
          <span {...readout.bind('a')} aria-label="target a" />
          <output data-testid="out">
            {readout.active ?? 'none'}|{readout.pinned ?? 'none'}
          </output>
        </div>
      );
    }
    render(<Probe />);
    const target = screen.getByRole('button', { name: 'target a' });
    const out = screen.getByTestId('out');
    expect(target).toHaveAttribute('tabindex', '0');
    expect(target).toHaveAttribute('aria-pressed', 'false');
    fireEvent.mouseEnter(target);
    expect(out).toHaveTextContent('a|none');
    fireEvent.click(target);
    expect(out).toHaveTextContent('a|a');
    expect(target).toHaveAttribute('aria-pressed', 'true');
    fireEvent.mouseLeave(target);
    expect(out).toHaveTextContent('a|a');
    fireEvent.keyDown(target, { key: 'Enter' });
    expect(out).toHaveTextContent('none|none');
    fireEvent.focus(target);
    expect(out).toHaveTextContent('a|none');
    fireEvent.keyDown(target, { key: ' ' });
    expect(out).toHaveTextContent('a|a');
    fireEvent.blur(target);
    expect(out).toHaveTextContent('a|a');
  });
});
