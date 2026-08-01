import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { AssetEcosystem } from '@/components/asset-ecosystem';
import { api, type AssetEcosystemResponse } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getAssetEcosystem: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

function mockAssetEcosystem(
  overrides: Partial<AssetEcosystemResponse> = {}
): AssetEcosystemResponse {
  return {
    topTokens: [
      {
        typeScriptHash: '0xaaa111',
        name: 'USDT',
        symbol: 'USDT',
        holdersCount: 1500,
        totalCapacityCkb: '500000.00',
      },
      {
        typeScriptHash: '0xbbb222',
        name: 'SEAL',
        symbol: 'SEAL',
        holdersCount: 800,
        totalCapacityCkb: '300000.00',
      },
      {
        typeScriptHash: '0xccc333',
        name: 'CKBull',
        symbol: 'CKBULL',
        holdersCount: 450,
        totalCapacityCkb: '150000.00',
      },
      {
        typeScriptHash: '0xddd444',
        name: 'JoyID',
        symbol: 'JOYID',
        holdersCount: 320,
        totalCapacityCkb: '100000.00',
      },
      {
        typeScriptHash: '0xeee555',
        name: 'RUSD',
        symbol: 'RUSD',
        holdersCount: 280,
        totalCapacityCkb: '80000.00',
      },
    ],
    // Percentages are shares of totalLiveCapacityCkb (the four categories
    // partition it exactly); knowledge size is a standalone stat.
    capacityBreakdown: [
      { category: 'dao', capacityCkb: '11200000000.00', percentage: '57.4' },
      { category: 'tokens', capacityCkb: '1500000000.00', percentage: '7.7' },
      { category: 'objects', capacityCkb: '500000000.00', percentage: '2.6' },
      { category: 'other', capacityCkb: '6300000000.00', percentage: '32.3' },
    ],
    totalLiveCapacityCkb: '19500000000.00',
    totalKnowledgeSizeCkb: '7300000000.00',
    ...overrides,
  };
}

describe('AssetEcosystem', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders asset ecosystem links, top tokens, and capacity legend after loading', async () => {
    vi.mocked(api.getAssetEcosystem).mockResolvedValue(mockAssetEcosystem());

    render(<AssetEcosystem />);

    await waitFor(() => {
      expect(screen.getByText('USDT')).toBeInTheDocument();
    });

    expect(screen.getByRole('link', { name: 'Asset Ecosystem' })).toHaveAttribute(
      'href',
      '/mainnet/tokens'
    );
    expect(screen.getByRole('link', { name: /VIEW ALL/i })).toHaveAttribute(
      'href',
      '/mainnet/tokens'
    );
    expect(screen.getByText('SEAL')).toBeInTheDocument();
    expect(screen.getByText('CKBull')).toBeInTheDocument();
    expect(screen.getByText('JoyID')).toBeInTheDocument();
    expect(screen.getByText('RUSD')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'USDT' })).toHaveAttribute(
      'href',
      '/mainnet/tokens/0xaaa111'
    );
    expect(screen.getByRole('link', { name: 'SEAL' })).toHaveAttribute(
      'href',
      '/mainnet/tokens/0xbbb222'
    );
    expect(screen.getByText('1,500 holders')).toBeInTheDocument();
    expect(screen.getByText('800 holders')).toBeInTheDocument();

    expect(screen.getByText('DAO')).toBeInTheDocument();
    expect(screen.getByText('Tokens')).toBeInTheDocument();
    expect(screen.getByText('Objects')).toBeInTheDocument();
    expect(screen.getByText('Other')).toBeInTheDocument();
    expect(screen.getByText('57.4%')).toBeInTheDocument();
    expect(screen.getByText('7.7%')).toBeInTheDocument();
    expect(screen.getByText('2.6%')).toBeInTheDocument();
    expect(screen.getByText('32.3%')).toBeInTheDocument();
  });
});
