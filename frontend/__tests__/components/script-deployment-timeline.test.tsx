import { fireEvent, render, screen, within } from '@/__tests__/utils/test-utils';
import { describe, expect, it } from 'vitest';
import { ScriptDeploymentTimeline } from '@/components/hardforks/script-deployment-timeline';
import producer from '@/__tests__/fixtures/network-api-producer/hardforks-deployments.json';

describe('ScriptDeploymentTimeline', () => {
  it('orders actual code-cell marks and updates evidence on keyboard focus', () => {
    render(<ScriptDeploymentTimeline deployments={[...producer.scriptDeployments].reverse()} />);

    const timeline = screen.getByRole('list', { name: 'Known script-version deployment timeline' });
    const marks = within(timeline).getAllByRole('button');
    expect(marks[0]).toHaveAccessibleName(/Known Lock v2.*block #10/i);
    expect(marks[1]).toHaveAccessibleName(/Known Lock v3.*block #10/i);
    fireEvent.focus(marks[1]);
    const evidence = screen.getByRole('complementary', {
      name: 'Selected script deployment evidence',
    });
    expect(within(evidence).getByRole('link', { name: 'Block #10' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/10'
    );
    expect(within(evidence).getByRole('link', { name: 'Code cell 0x44444444…:0' })).toHaveAttribute(
      'href',
      '/mainnet/cell/0x4444444444444444444444444444444444444444444444444444444444444444-0'
    );
    expect(
      within(evidence).getByText(/not proof that a script instance used a particular VM/i)
    ).toBeInTheDocument();
  });

  it('shows an explicit empty state when no named deployments exist', () => {
    render(<ScriptDeploymentTimeline deployments={[]} />);
    expect(screen.getByText(/No named script-version code-cell deployments/)).toBeInTheDocument();
  });
});
