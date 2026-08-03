import { MemoryRouter, useLocation, useRoutes } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { render, screen, waitFor } from '@/__tests__/utils/test-utils';
import { api, type KnownScript } from '@/lib/api';
import { createAppRouter } from '@/src/routes/router';

vi.mock('@/lib/api', () => ({
  api: {
    getScripts: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/app/script/[codeHash]/client-page', () => ({
  default: ({ codeHash }: { codeHash: string }) => <div>script detail {codeHash}</div>,
}));

vi.mock('@/app/scripts/[name]/client-page', () => ({
  default: ({ name }: { name: string }) => <div>named script detail {name}</div>,
}));

vi.mock('@/app/identities/bit-cell/[identityId]/client-page', () => ({
  default: ({ identityId }: { identityId: string }) => (
    <div>.bit Cell identity detail {identityId}</div>
  ),
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
      ownedCapacitySum: '2000000000',
      ownedKnowledgeSum: '1000000000',
    } satisfies KnownScript,
  ],
  total: 1,
  limit: 20,
  hasMore: false,
  nextCursor: null,
};

describe('detail navigation', () => {
  function RouterHarness() {
    const location = useLocation();
    const element = useRoutes(createAppRouter());

    return (
      <>
        <div data-testid="pathname">{location.pathname}</div>
        {element}
      </>
    );
  }

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getScripts).mockResolvedValue(mockScriptsResponse);
  });

  it('navigates from scripts list to the script detail route', async () => {
    const user = userEvent.setup();

    render(
      <MemoryRouter initialEntries={['/mainnet/scripts']}>
        <RouterHarness />
      </MemoryRouter>
    );

    await waitFor(
      () => {
        expect(api.getScripts).toHaveBeenCalled();
      },
      { timeout: 3000 }
    );
    const links = await screen.findAllByRole(
      'link',
      { name: 'SECP256K1_BLAKE160' },
      { timeout: 3000 }
    );
    await user.click(links[0]);

    await waitFor(() => {
      expect(screen.getByTestId('pathname')).toHaveTextContent(
        '/mainnet/scripts/SECP256K1_BLAKE160'
      );
      expect(screen.getByText('named script detail SECP256K1_BLAKE160')).toBeInTheDocument();
    });
  });

  it('routes .bit Cell identity cards to the item detail page', async () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/identities/bit-cell/0xbitcell']}>
        <RouterHarness />
      </MemoryRouter>
    );

    expect(
      await screen.findByText('.bit Cell identity detail 0xbitcell', {}, { timeout: 3000 })
    ).toBeInTheDocument();
  });
});
