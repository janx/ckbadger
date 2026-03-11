import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { TerminalDivider, TerminalPanelHeader } from '@/components/ui/terminal-panel';
import { MiniStat, StatBlock } from '@/components/ui/stat-block';
import { SparkChart } from '@/components/ui/spark-chart';
import { PageHeader } from '@/components/ui/page-header';
import { DataField } from '@/components/ui/data-field';

describe('UI readability classes', () => {
  it('uses readable slate tone for terminal divider label', () => {
    render(<TerminalDivider label="network" />);
    expect(screen.getByText('network')).toHaveClass('text-text-dim');
  });

  it('uses readable slate tone for stat metadata text', () => {
    render(
      <div>
        <StatBlock
          label="TPS"
          value={12.5}
          trend={{ direction: 'neutral', value: '0.0%', label: '24h' }}
          subtext="stable"
        />
        <MiniStat label="delta" value="1.23" />
      </div>
    );

    expect(screen.getByText('24h')).toHaveClass('text-text-dim');
    expect(screen.getByText('stable')).toHaveClass('text-text-dim');
    expect(screen.getByText('delta')).toHaveClass('text-text-dim');
  });

  it('uses readable slate tone for spark chart empty state', () => {
    render(<SparkChart data={[]} />);
    expect(screen.getByText('No data')).toHaveClass('text-text-dim');
  });

  it('uses readable slate tone for page header copy icon', () => {
    render(<PageHeader title="Cell" hash="0x1234" />);
    const copyContainer = screen.getByTitle('Click to copy');
    const copyIcon = copyContainer.querySelector('svg');

    expect(copyIcon).toBeTruthy();
    expect(copyIcon).toHaveClass('text-text-dim');
  });

  it('uses readable slate tone for data-field help and copy icons', () => {
    const { container } = render(
      <DataField label="Hash" helpText="Cell hash" copyValue="0x1234">
        0x1234
      </DataField>
    );

    const helpIconWrapper = screen.getByTitle('Cell hash');
    const copyIcon = container.querySelector('.group .h-3\\.5.w-3\\.5');

    expect(helpIconWrapper).toHaveClass('text-text-dim');
    expect(copyIcon).toBeTruthy();
    expect(copyIcon).toHaveClass('text-text-dim');
  });

  it('uses responsive wrapping layout for page header actions', () => {
    render(<PageHeader title="Cell" actions={<button type="button">Action</button>} />);
    const actionButton = screen.getByRole('button', { name: 'Action' });
    const actionsWrapper = actionButton.parentElement;
    const topRow = actionsWrapper?.parentElement;

    expect(actionsWrapper).toHaveClass('flex-wrap');
    expect(topRow).toHaveClass('flex-wrap');
  });

  it('uses responsive wrapping layout for terminal panel header actions', () => {
    render(
      <TerminalPanelHeader actions={<button type="button">Filter</button>}>
        Panel
      </TerminalPanelHeader>
    );

    const actionButton = screen.getByRole('button', { name: 'Filter' });
    const actionsWrapper = actionButton.parentElement;
    const headerRow = actionsWrapper?.parentElement;

    expect(actionsWrapper).toHaveClass('flex-wrap');
    expect(headerRow).toHaveClass('flex-wrap');
  });

  it('uses responsive stacked layout for horizontal data fields', () => {
    render(<DataField label="Hash">0x1234</DataField>);

    const row = screen.getByText('Hash').closest('div')?.parentElement;
    expect(row).toHaveClass('flex-col');
    expect(row).toHaveClass('sm:flex-row');
  });
});
