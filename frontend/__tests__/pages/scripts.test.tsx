import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ScriptsPage from '@/app/scripts/page';
import { api, KnownScript } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getScripts: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockScriptsResponse = {
  data: [
    {
      codeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
      name: 'SECP256K1_BLAKE160',
      description: 'Default lock script',
      scriptKind: 'lock',
      rfc: null,
      website: null,
      sourceUrl: null,
      decoderType: null,
      network: 'mainnet',
      hashType: 'type',
      dataHash: null,
      typeHash: null,
      tag: null,
      deprecated: false,
      isSystem: true,
      codeCellTxHash: null,
      codeCellOutputIndex: null,
    } satisfies KnownScript,
  ],
  total: 1,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

describe('ScriptsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders script list', async () => {
    vi.mocked(api.getScripts).mockResolvedValue(mockScriptsResponse);

    render(<ScriptsPage />);

    await waitFor(() => {
      expect(api.getScripts).toHaveBeenCalled();
    });

    await waitFor(() => {
      expect(screen.getByText('Kind')).toBeInTheDocument();
    });

    expect(screen.getByRole('link', { name: 'SECP256K1_BLAKE160' })).toBeInTheDocument();
    expect(screen.getByText('lock')).toBeInTheDocument();
  });
});
