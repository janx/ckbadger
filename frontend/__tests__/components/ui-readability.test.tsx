import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { TerminalPanelHeader } from '@/components/ui/terminal-panel';

describe('UI text and affordances', () => {
  it('renders terminal helper copy', () => {
    render(
      <div>
        <TerminalPanelHeader actions={<button type="button">Filter</button>}>
          Panel
        </TerminalPanelHeader>
      </div>
    );

    expect(screen.getByText('Panel')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Filter' })).toBeInTheDocument();
  });
});
