import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '../utils/test-utils';
import { TerminalDivider, TerminalPanelHeader } from '@/components/ui/terminal-panel';
import { MiniStat, StatBlock } from '@/components/ui/stat-block';
import { SparkChart } from '@/components/ui/spark-chart';
import { PageHeader } from '@/components/ui/page-header';
import { DataField } from '@/components/ui/data-field';

describe('UI text and affordances', () => {
  beforeEach(() => {
    Object.assign(navigator, {
      clipboard: {
        writeText: vi.fn().mockResolvedValue(undefined),
      },
    });
  });

  it('renders terminal, stat, and spark-chart helper copy', () => {
    render(
      <div>
        <TerminalDivider label="network" />
        <TerminalPanelHeader actions={<button type="button">Filter</button>}>
          Panel
        </TerminalPanelHeader>
        <StatBlock
          label="TPS"
          value={12.5}
          trend={{ direction: 'neutral', value: '0.0%', label: '24h' }}
          subtext="stable"
        />
        <MiniStat label="delta" value="1.23" />
        <SparkChart data={[]} />
      </div>
    );

    expect(screen.getByText('network')).toBeInTheDocument();
    expect(screen.getByText('Panel')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Filter' })).toBeInTheDocument();
    expect(screen.getByText('24h')).toBeInTheDocument();
    expect(screen.getByText('stable')).toBeInTheDocument();
    expect(screen.getByText('delta')).toBeInTheDocument();
    expect(screen.getByText('No data')).toBeInTheDocument();
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

  it('renders data-field labels, help text, and copies the value on click', async () => {
    render(
      <DataField label="Hash" helpText="Cell hash" copyValue="0x1234">
        0x1234
      </DataField>
    );

    expect(screen.getByText('Hash')).toBeInTheDocument();
    expect(screen.getByTitle('Cell hash')).toBeInTheDocument();

    fireEvent.click(screen.getByText('0x1234'));

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('0x1234');
    await waitFor(() => {
      expect(screen.getByText('Copied!')).toBeInTheDocument();
    });
  });
});
