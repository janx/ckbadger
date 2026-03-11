import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { AssetEcosystem } from '@/components/asset-ecosystem';
import { api, type AssetEcosystemResponse } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getAssetEcosystem: vi.fn(),
  },
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
    capacityBreakdown: [
      { category: 'dao', capacityCkb: '11200000000.00', percentage: '57.1' },
      { category: 'tokens', capacityCkb: '1500000000.00', percentage: '7.6' },
      { category: 'objects', capacityCkb: '500000000.00', percentage: '2.5' },
      { category: 'other', capacityCkb: '6300000000.00', percentage: '32.8' },
    ],
    totalKnowledgeSizeCkb: '19500000000.00',
    ...overrides,
  };
}

describe('AssetEcosystem', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders top tokens after loading', async () => {
    vi.mocked(api.getAssetEcosystem).mockResolvedValue(mockAssetEcosystem());

    render(<AssetEcosystem />);

    await waitFor(() => {
      expect(screen.getByText('USDT')).toBeInTheDocument();
    });

    expect(screen.getByText('SEAL')).toBeInTheDocument();
    expect(screen.getByText('CKBull')).toBeInTheDocument();
    expect(screen.getByText('JoyID')).toBeInTheDocument();
    expect(screen.getByText('RUSD')).toBeInTheDocument();

    // Holder counts
    expect(screen.getByText('1,500 holders')).toBeInTheDocument();
    expect(screen.getByText('800 holders')).toBeInTheDocument();
  });

  it('renders capacity breakdown categories in the legend', async () => {
    vi.mocked(api.getAssetEcosystem).mockResolvedValue(mockAssetEcosystem());

    render(<AssetEcosystem />);

    await waitFor(() => {
      expect(screen.getByText('DAO')).toBeInTheDocument();
    });

    expect(screen.getByText('Tokens')).toBeInTheDocument();
    expect(screen.getByText('Objects')).toBeInTheDocument();
    expect(screen.getByText('Other')).toBeInTheDocument();

    // Percentages
    expect(screen.getByText('57.1%')).toBeInTheDocument();
    expect(screen.getByText('7.6%')).toBeInTheDocument();
    expect(screen.getByText('2.5%')).toBeInTheDocument();
    expect(screen.getByText('32.8%')).toBeInTheDocument();
  });

  it('renders token links to correct pages', async () => {
    vi.mocked(api.getAssetEcosystem).mockResolvedValue(mockAssetEcosystem());

    render(<AssetEcosystem />);

    await waitFor(() => {
      expect(screen.getByText('USDT')).toBeInTheDocument();
    });

    const usdtLink = screen.getByText('USDT').closest('a');
    expect(usdtLink).toHaveAttribute('href', '/tokens/0xaaa111');

    const sealLink = screen.getByText('SEAL').closest('a');
    expect(sealLink).toHaveAttribute('href', '/tokens/0xbbb222');
  });

  it('shows loading skeleton initially', () => {
    vi.mocked(api.getAssetEcosystem).mockReturnValue(new Promise(() => {}));

    const { container } = render(<AssetEcosystem />);

    const pulseElements = container.querySelectorAll('.animate-pulse');
    expect(pulseElements.length).toBeGreaterThanOrEqual(3);
  });

  it('renders the header', async () => {
    vi.mocked(api.getAssetEcosystem).mockResolvedValue(mockAssetEcosystem());

    render(<AssetEcosystem />);

    expect(screen.getByText('Asset Ecosystem')).toBeInTheDocument();
  });
});
