import { describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '../utils/test-utils';
import { HelpPopover } from '@/components/ui/help-popover';

function TestSubject() {
  return (
    <div>
      <HelpPopover label="Explain example" title="Example Help">
        <div>Popover content</div>
      </HelpPopover>
      <button type="button">Outside</button>
    </div>
  );
}

describe('HelpPopover', () => {
  it('opens on hover and focus', () => {
    vi.useFakeTimers();
    render(<TestSubject />);

    const trigger = screen.getByRole('button', { name: 'Explain example' });

    expect(screen.queryByText('Popover content')).toBeNull();

    fireEvent.mouseEnter(trigger);
    expect(screen.getByText('Popover content')).toBeInTheDocument();

    fireEvent.mouseLeave(trigger);

    act(() => {
      vi.advanceTimersByTime(200);
    });

    expect(screen.queryByText('Popover content')).toBeNull();

    fireEvent.focus(trigger);
    expect(screen.getByText('Popover content')).toBeInTheDocument();

    fireEvent.blur(trigger);
    expect(screen.queryByText('Popover content')).toBeNull();
    vi.useRealTimers();
  });

  it('toggles on click and closes on outside click and escape', () => {
    render(<TestSubject />);

    const trigger = screen.getByRole('button', { name: 'Explain example' });

    fireEvent.click(trigger);
    expect(screen.getByText('Popover content')).toBeInTheDocument();

    fireEvent.mouseDown(screen.getByRole('button', { name: 'Outside' }));
    expect(screen.queryByText('Popover content')).toBeNull();

    fireEvent.click(trigger);
    expect(screen.getByText('Popover content')).toBeInTheDocument();

    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByText('Popover content')).toBeNull();
  });

  it('renders the popup through a portal so overflow-hidden containers cannot clip it', () => {
    const { container } = render(
      <div className="overflow-hidden">
        <HelpPopover label="Explain portal" title="Portal Help">
          <div>Portaled content</div>
        </HelpPopover>
      </div>
    );

    const trigger = screen.getByRole('button', { name: 'Explain portal' });
    fireEvent.mouseEnter(trigger);

    const dialog = screen.getByRole('dialog', { name: 'Portal Help' });
    expect(dialog).toBeInTheDocument();
    expect(dialog.parentElement).toBe(document.body);
    expect(container).not.toContainElement(dialog);
  });

  it('stays open while moving the pointer from the trigger into the portaled popup', () => {
    vi.useFakeTimers();
    render(<TestSubject />);

    const trigger = screen.getByRole('button', { name: 'Explain example' });

    fireEvent.mouseEnter(trigger);
    const dialog = screen.getByRole('dialog', { name: 'Example Help' });

    fireEvent.mouseLeave(trigger);
    fireEvent.mouseEnter(dialog);

    act(() => {
      vi.advanceTimersByTime(200);
    });

    expect(screen.getByText('Popover content')).toBeInTheDocument();

    fireEvent.mouseLeave(dialog);

    act(() => {
      vi.advanceTimersByTime(200);
    });

    expect(screen.queryByText('Popover content')).toBeNull();
    vi.useRealTimers();
  });
});
