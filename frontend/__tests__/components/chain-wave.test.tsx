import { describe, it, expect, afterEach, beforeAll, beforeEach, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { render } from '../utils/test-utils';
import { PackedContainer, TxItem } from '@/components/chain-wave/packed-container';
import { EpochProgress } from '@/components/chain-wave/epoch-progress';

// The shared test setup replaces `@/src/navigation` with a no-op router; the
// navigation test below asserts the real network-prefixing behaviour.
vi.mock('@/src/navigation', async () => await vi.importActual('@/src/navigation'));

// Mock ResizeObserver for PackedContainer tests
beforeAll(() => {
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

function createProposalItem(id: string, overrides: Partial<TxItem> = {}): TxItem {
  return {
    id,
    size: 500,
    fee: undefined,
    feeRate: undefined,
    category: 'normal',
    ...overrides,
  };
}

describe('PackedContainer (proposals type)', () => {
  it('renders empty state with title and subtitle when no proposals', () => {
    render(
      <PackedContainer
        title="Proposed"
        subtitle="Awaiting commit"
        type="proposals"
        items={[]}
        totalCount={0}
        emptyText="No proposed txs"
        globalMaxSize={10000}
      />
    );
    expect(screen.getByText('No proposed txs')).toBeInTheDocument();
    expect(screen.getByText('0')).toBeInTheDocument();
    expect(screen.getByText('Proposed')).toBeInTheDocument();
    expect(screen.getByText('Awaiting commit')).toBeInTheDocument();
  });

  it('renders proposal count and hides empty text when proposals exist', () => {
    const items = [
      createProposalItem('0x1234567890abcdef1234', { size: 1000 }),
      createProposalItem('0xfedcba0987654321fedc', { size: 2000 }),
    ];
    render(
      <PackedContainer
        title="Proposed"
        subtitle="Awaiting commit"
        type="proposals"
        items={items}
        totalCount={2}
        emptyText="No proposed txs"
        globalMaxSize={10000}
      />
    );

    expect(screen.getByText('2')).toBeInTheDocument();
    expect(screen.queryByText('No proposed txs')).not.toBeInTheDocument();
  });
});

describe('PackedContainer (tip type) transaction navigation', () => {
  beforeEach(() => {
    // jsdom reports every element as zero-width, so the packer would lay out no
    // boxes at all; give the container a width so a clickable box is rendered.
    Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
      configurable: true,
      get: () => 400,
    });
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
  });

  afterEach(() => {
    Reflect.deleteProperty(HTMLElement.prototype, 'clientWidth');
    delete window.__CKBADGER_RUNTIME_CONFIG__;
  });

  function TipHarness() {
    const location = useLocation();

    return (
      <>
        <div data-testid="pathname">{location.pathname}</div>
        <PackedContainer
          title="Tip"
          type="tip"
          items={[createProposalItem('0xabc123')]}
          totalCount={1}
          blockNumber={987}
          globalMaxSize={10000}
        />
      </>
    );
  }

  it('navigates to the transaction under the active network prefix', async () => {
    const user = userEvent.setup();

    render(
      <MemoryRouter initialEntries={['/testnet/']}>
        <TipHarness />
      </MemoryRouter>
    );

    await user.click(await screen.findByTestId('tx-box-0xabc123'));

    await waitFor(() => {
      // A bare `/tx/0xabc123` would be resolved against the DEFAULT network by
      // the route guard, 404-ing a testnet-only transaction.
      expect(screen.getByTestId('pathname').textContent).toBe('/testnet/tx/0xabc123');
    });
  });
});

describe('EpochProgress', () => {
  it('renders epoch progress and block range', () => {
    render(
      <EpochProgress epochNumber={100} epochIndex={450} epochLength={1800} latestBlock={10450} />
    );
    expect(screen.getByText('100')).toBeInTheDocument();
    expect(screen.getByText('25.0%')).toBeInTheDocument();
    expect(screen.getByText('450 / 1,800')).toBeInTheDocument();
    expect(screen.getByText('#10,000')).toBeInTheDocument();
    expect(screen.getByText('#11,799')).toBeInTheDocument();
  });

  it('renders estimated time remaining', () => {
    render(
      <EpochProgress
        epochNumber={100}
        epochIndex={900}
        epochLength={1800}
        latestBlock={10900}
        estimatedTimeRemaining="2h 30m"
      />
    );
    expect(screen.getByText('2h 30m')).toBeInTheDocument();
  });

  it('handles zero epoch length gracefully', () => {
    render(<EpochProgress epochNumber={1} epochIndex={0} epochLength={0} latestBlock={0} />);
    expect(screen.getByText('0.0%')).toBeInTheDocument();
  });
});
