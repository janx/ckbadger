import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { TxStructure } from '@/components/transaction/tx-structure';
import type { CellTileData } from '@/components/ui/cell-tile';

const tile = (id: string, state: string): CellTileData => ({
  id,
  href: `/cell/0xabc:${id}`,
  label: id,
  owner: 'Alice',
  lockKind: 'Fiber Commitment Lock',
  kind: 'ckb',
  content: 'CKB',
  capacity: '106200000000',
  occupied: '6100000000',
  state,
});

describe('TxStructure onward / me_out variant (txviz)', () => {
  it('carries the focused output onward with a dashed bridge and a legend', () => {
    const { container } = render(
      <TxStructure
        txHash="0xfunding"
        inputs={[tile('in:0', 'consumed')]}
        outputs={[tile('out:0', 'live'), tile('out:1', 'live')]}
        fee="1000"
        meOut="out:0"
        onward="the channel, onward"
      />
    );
    const grid = container.querySelector('[data-tx-canvas]')!;
    expect(grid.className).toContain('lg:grid-cols-[minmax(0,1fr)_112px_minmax(0,1fr)_96px]');
    const bridge = container.querySelector('[data-onward-bridge]')!;
    expect(bridge.querySelector('path')).toHaveAttribute('stroke-dasharray', '3 3');
    expect(bridge).toHaveAttribute('data-onward-from', 'out:0');
    expect(screen.getByText('the channel, onward')).toBeInTheDocument();
    // the focused output's tile stays highlighted
    expect(container.querySelector('[data-cell-id="out:0"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    expect(container.querySelector('[data-cell-id="out:1"]')).toHaveAttribute(
      'data-highlighted',
      'false'
    );
  });

  it('defaults the legend to "this cell, onward" and draws no bridge without a focused output', () => {
    const { container, rerender } = render(
      <TxStructure
        txHash="0xcell"
        inputs={[tile('in:0', 'consumed')]}
        outputs={[tile('out:0', 'live')]}
        fee="1000"
        meOut="out:0"
      />
    );
    expect(screen.getByText('this cell, onward')).toBeInTheDocument();
    rerender(
      <TxStructure
        txHash="0xcell"
        inputs={[tile('in:0', 'consumed')]}
        outputs={[tile('out:0', 'live')]}
        fee="1000"
      />
    );
    expect(container.querySelector('[data-onward-bridge]')).toBeNull();
    expect(container.querySelector('[data-tx-canvas]')!.className).not.toContain('_96px]');
  });

  it('refuses a focused output that is not one of the outputs', () => {
    expect(() =>
      render(
        <TxStructure
          txHash="0xbad"
          inputs={[]}
          outputs={[tile('out:0', 'live')]}
          fee="0"
          meOut="in:9"
        />
      )
    ).toThrow('0xbad: onward cell in:9 is not an output');
  });
});
