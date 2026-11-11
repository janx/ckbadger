// Consistency plan C2 (review finding M): a not-found never reaches the crash page. The stale fork
// URL below — the fork route before D7 took an 8-digit orphan prefix — rendered "Error · not a 404"
// with the detail row "error notFound": the page's route parser refuses the 66-character hash
// without asking the API, calls `notFound()`, and that signal reached the route's error boundary as
// a plain `Error('notFound')`, which it read as a crash. (For the record, the API answers this path
// 400 `bad_request` "Invalid orphan hash prefix (8 lowercase hex digits)", and a missing fork with
// a well-formed prefix 404 `not_found` — the page never asks it here.)
import { createMemoryRouter, RouterProvider } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { api } from '@/lib/api';
import { createAppRouter } from '@/src/routes/router';

// The real router hooks and the real `notFound()` (setup.ts stubs this module for page tests).
vi.mock('@/src/navigation', async (importOriginal) => importOriginal());
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
vi.mock('@/components/layout/site-footer', () => ({ SiteFooter: () => null }));
vi.mock('@/components/error-vessels', () => ({
  ErrorVessels: () => <div data-testid="error-vessels" />,
  VesselsCaption: () => <span>Bounded live cell sample</span>,
}));

const STALE_FORK =
  '/mainnet/forks/20894012/0xc121e093d0cdd348db70be248efd25ee756af7fce2f24b92741a1084945a5bd4';

function mount(path: string) {
  const router = createMemoryRouter(createAppRouter(), { initialEntries: [path] });
  return render(<RouterProvider router={router} />);
}

describe('a route that names no page renders the 404 design (C2)', () => {
  beforeEach(() => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }],
      defaultNetwork: 'mainnet',
    };
    vi.spyOn(console, 'error').mockImplementation(() => {});
    vi.spyOn(console, 'warn').mockImplementation(() => {});
  });
  afterEach(() => {
    delete window.__CKBADGER_RUNTIME_CONFIG__;
    vi.restoreAllMocks();
  });

  it('the stale fork URL with a full orphan hash is a 404, not "the page fell"', async () => {
    const getForkAt = vi.spyOn(api, 'getForkAt');
    mount(STALE_FORK);
    expect(
      await screen.findByRole(
        'heading',
        {
          level: 1,
          name: "no page at this path. the chain was not asked; nothing on it is lost. honey badger don't care.",
        },
        // the route's page is a lazy chunk; under the full suite it can take over a second
        { timeout: 5000 }
      )
    ).toBeInTheDocument();
    expect(screen.getByText('Not found · 404')).toBeInTheDocument();
    expect(screen.queryByText(/the chain keeps/)).toBeNull();
    expect(screen.queryByText('notFound')).toBeNull();
    // The address is refused by its shape: no API request is made for it.
    expect(getForkAt).not.toHaveBeenCalled();
    // C1: the tab says so too.
    expect(document.title).toBe('Not found · CKBadger');
  });

  it('an uncle index that is not a number is a 404 too (the same notFound() signal)', async () => {
    mount('/mainnet/blocks/20894834/uncles/first');
    expect(
      await screen.findByRole(
        'heading',
        {
          level: 1,
          name: "no page at this path. the chain was not asked; nothing on it is lost. honey badger don't care.",
        },
        // the route's page is a lazy chunk; under the full suite it can take over a second
        { timeout: 5000 }
      )
    ).toBeInTheDocument();
    expect(screen.queryByText(/the chain keeps/)).toBeNull();
  });
});
