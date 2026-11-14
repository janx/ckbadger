import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '../utils/test-utils';
import { TerminalDivider, TerminalPanelHeader } from '@/components/ui/terminal-panel';
import { PageHeader } from '@/components/ui/page-header';

describe('UI text and affordances', () => {
  beforeEach(() => {
    Object.assign(navigator, {
      clipboard: {
        writeText: vi.fn().mockResolvedValue(undefined),
      },
    });
  });

  it('renders terminal helper copy', () => {
    render(
      <div>
        <TerminalDivider label="network" />
        <TerminalPanelHeader actions={<button type="button">Filter</button>}>
          Panel
        </TerminalPanelHeader>
      </div>
    );

    expect(screen.getByText('network')).toBeInTheDocument();
    expect(screen.getByText('Panel')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Filter' })).toBeInTheDocument();
  });

  it('renders page header actions and copies the hash on click', async () => {
    render(
      <PageHeader title="Cell" hash="0x1234" actions={<button type="button">Action</button>} />
    );

    expect(screen.getByRole('button', { name: 'Action' })).toBeInTheDocument();

    fireEvent.click(screen.getByTitle('Click to copy'));

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('0x1234');
    await waitFor(() => {
      expect(screen.getByText('Cell')).toBeInTheDocument();
    });
  });
});
