import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import FiberChannelDetailPage from '@/app/fiber/channels/[id]/client-page';
import fixture from '@/__tests__/fixtures/fiber-channel-c07e.json';

const id = fixture.channel.channelId;
vi.mock('@/src/navigation', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/src/navigation')>()),
  useParams: () => ({ id }),
  usePathname: () => `/fiber/channels/${id}`,
}));
vi.mock('@/components/layout/header', () => ({ Header: () => null }));

const NAMES: Record<string, string> = {
  '0xe45b1f8f21bff23137035a3ab751d75b36a981deec3e7820194b9c042967f4f1': 'Fiber Funding Lock',
  '0x2d45c4d3ed3e942f1945386ee82a5d1b7e4bb16d7fe1ab015421174ab747406c': 'Fiber Commitment Lock',
  '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8': 'Default Lock',
};

describe('Fiber channel page (FiberChannel board V66)', () => {
  it('heads the page like the board and tells the three on-chain steps with named locks', async () => {
    const txs = fixture.transactions as Record<string, Record<string, unknown>>;
    server.use(
      http.get('/api/:network/v1/fiber/channels/:id', () => HttpResponse.json(fixture.channel)),
      http.get('/api/:network/v1/fiber/channels/:id/evidence', () =>
        HttpResponse.json(fixture.evidence)
      ),
      http.get('/api/:network/v1/transactions/:hash/detail', ({ params }) =>
        HttpResponse.json(txs[String(params.hash)])
      ),
      http.post('/api/:network/v1/scripts/lookup', async ({ request }) => {
        const { codeHashes } = (await request.json()) as { codeHashes: string[] };
        return HttpResponse.json(
          Object.fromEntries(codeHashes.map((h) => [h, { codeHash: h, name: NAMES[h] }]))
        );
      })
    );
    render(<FiberChannelDetailPage />);
    const nav = screen.getByRole('navigation', { name: 'Breadcrumb' });
    expect(within(nav).getByRole('link', { name: 'Fiber' })).toHaveAttribute(
      'href',
      '/mainnet/fiber/channels'
    );
    expect(screen.getByRole('heading', { level: 1, name: 'Fiber channel' })).toHaveClass(
      'text-[26px]'
    );
    expect(await screen.findByText('force-closed · settled')).toBeInTheDocument();
    // no jade uppercase eyebrow (FF-C6)
    expect(screen.queryByText(/on-chain channel/i)).toBeNull();
    expect(
      await screen.findAllByText('Fiber Funding Lock', {}, { timeout: 3000 })
    ).not.toHaveLength(0);
    expect(screen.getAllByText('Fiber Commitment Lock').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Default Lock').length).toBeGreaterThan(0);
    expect(screen.getByRole('heading', { name: /^1 · Open/ })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: /^2 · Force close/ })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: /^3 · Settle/ })).toBeInTheDocument();
    // the tier-2 witnesses row reads the step's own transaction (sizes and fee)
    expect(
      await screen.findByText('2 witnesses, 85, 85 B · fee 920 shannons, from outside inputs')
    ).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: /^2 · Force close/ })).toHaveTextContent(
      'epoch 11,639 · 836 / 1,434'
    );
  });
});
