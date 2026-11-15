import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import FiberChannelDetailPage from '@/app/fiber/channels/[id]/client-page';

const id = `0x${'a'.repeat(64)}`;
vi.mock('@/src/navigation', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/src/navigation')>()),
  useParams: () => ({ id }),
  usePathname: () => `/fiber/channels/${id}`,
}));

describe('Fiber channel entity error routing', () => {
  it('shows exact channel endpoint evidence for an HTTP 404', async () => {
    server.use(
      http.get('/api/:network/v1/fiber/channels/:id', () =>
        HttpResponse.json({ error: 'not_found', message: 'channel absent' }, { status: 404 })
      )
    );
    render(<FiberChannelDetailPage />);
    expect(await screen.findByText('no fiber channel with this id.')).toBeInTheDocument();
    // U-18: the checked endpoint stands in the facts, the id shortened, once
    expect(document.querySelector('.error-facts [data-endpoint]')).toHaveTextContent(
      `/fiber/channels/${id.slice(0, 10)}…${id.slice(-6)}`
    );
  });

  it('keeps a source invariant failure distinct from a missing channel', async () => {
    server.use(
      http.get('/api/:network/v1/fiber/channels/:id', () =>
        HttpResponse.json(
          { error: 'internal_error', message: 'channel index mismatch' },
          { status: 500 }
        )
      )
    );
    render(<FiberChannelDetailPage />);
    expect(await screen.findByRole('alert')).toHaveTextContent('channel index mismatch');
    expect(screen.queryByText('no fiber channel with this id.')).not.toBeInTheDocument();
  });
});
