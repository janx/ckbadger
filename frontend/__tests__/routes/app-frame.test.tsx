// Visual finish V6 (plan 2026-11-04): the header is part of the route frame. AppFrame renders it
// once, above the page, so a cold load shows it from the first paint — before the page's lazy chunk
// arrives — and no page mounts its own. The not-found route and a page's route error render inside
// the frame, under its header; only when the frame itself fails does the error page draw its own.
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { createMemoryRouter, RouterProvider } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen } from '@/__tests__/utils/test-utils';
import { createAppRouter } from '@/src/routes/router';

const gates = vi.hoisted(() => {
  let open!: () => void;
  const transactions = new Promise<void>((done) => {
    open = done;
  });
  return {
    /** A chunk that never arrives. */
    never: new Promise<never>(() => {}),
    /** A chunk that arrives when the test says so. */
    transactions,
    openTransactions: () => open(),
    frameFails: { on: false },
  };
});

// The real router hooks (setup.ts stubs this module for page tests).
vi.mock('@/src/navigation', async (importOriginal) => importOriginal());
vi.mock('@/components/layout/header', () => ({
  Header: () => <header>CKBadger header</header>,
}));
vi.mock('@/components/layout/site-footer', () => ({
  SiteFooter: () => <footer>site footer</footer>,
}));
// The frame's first child: a switch to make the frame itself fail.
vi.mock('@/components/layout/atmosphere', () => ({
  Atmosphere: () => {
    if (gates.frameFails.on) throw new Error('atmosphere: the sky failed to paint');
    return null;
  },
}));
vi.mock('@/app/page', () => ({ default: () => <div>home page</div> }));
vi.mock('@/app/dao/page', () => ({ default: () => <div>dao page</div> }));
vi.mock('@/app/charts/page', () => ({
  default: () => {
    throw new Error('charts: the chart model broke');
  },
}));
vi.mock('@/app/blocks/page', async () => {
  await gates.never;
  return { default: () => <div>blocks page</div> };
});
vi.mock('@/app/transactions/page', async () => {
  await gates.transactions;
  return { default: () => <div>transactions page</div> };
});

const STALE_FORK =
  '/mainnet/forks/20894012/0xc121e093d0cdd348db70be248efd25ee756af7fce2f24b92741a1084945a5bd4';
const LAZY = { timeout: 5000 };

function mount(path: string) {
  const router = createMemoryRouter(createAppRouter(), { initialEntries: [path] });
  render(<RouterProvider router={router} />);
  return router;
}

beforeEach(() => {
  window.__CKBADGER_RUNTIME_CONFIG__ = {
    networks: [{ name: 'mainnet' }],
    defaultNetwork: 'mainnet',
  };
  window.scrollTo = vi.fn();
  vi.spyOn(console, 'error').mockImplementation(() => {});
  vi.spyOn(console, 'warn').mockImplementation(() => {});
});
afterEach(() => {
  delete window.__CKBADGER_RUNTIME_CONFIG__;
  gates.frameFails.on = false;
  vi.restoreAllMocks();
});

describe('the header is part of the frame (visual finish V6)', () => {
  it('is there while the page chunk is still on its way; the fallback holds the page’s place', async () => {
    mount('/mainnet/blocks');
    expect(await screen.findAllByRole('banner')).toHaveLength(1);
    expect(screen.queryByText('blocks page')).toBeNull();
    expect(document.querySelector('.min-h-\\[60vh\\]')).not.toBeNull();
    expect(document.querySelector('.min-h-\\[240px\\]')).toBeNull();
  });

  it('stays the same one header when the page arrives', async () => {
    mount('/mainnet/transactions');
    const [header] = await screen.findAllByRole('banner');
    gates.openTransactions();
    expect(await screen.findByText('transactions page', {}, LAZY)).toBeInTheDocument();
    expect(screen.getAllByRole('banner')).toEqual([header]);
  });

  it('renders a resolved page under exactly one header and one footer', async () => {
    mount('/mainnet/dao');
    expect(await screen.findByText('dao page', {}, LAZY)).toBeInTheDocument();
    expect(screen.getAllByRole('banner')).toHaveLength(1);
    expect(screen.getAllByRole('contentinfo')).toHaveLength(1);
    // the header comes before the page in reading order
    expect(screen.getByRole('banner').compareDocumentPosition(screen.getByText('dao page'))).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING
    );
  });

  it('renders the not-found route under the frame’s header, once', async () => {
    mount('/mainnet/no-such-page');
    expect(
      await screen.findByRole('heading', {
        level: 1,
        name: 'all that is was worked for. this was not.',
      })
    ).toBeInTheDocument();
    expect(screen.getAllByRole('banner')).toHaveLength(1);
    expect(screen.getAllByRole('contentinfo')).toHaveLength(1);
  });

  it('renders a page’s notFound() in the frame: the 404 under one header, one footer', async () => {
    mount(STALE_FORK);
    expect(
      await screen.findByRole(
        'heading',
        { level: 1, name: 'all that is was worked for. this was not.' },
        LAZY
      )
    ).toBeInTheDocument();
    expect(screen.getAllByRole('banner')).toHaveLength(1);
    expect(screen.getAllByRole('contentinfo')).toHaveLength(1);
  });

  it('renders a page that throws as the crash page in the frame: one header, one footer', async () => {
    mount('/mainnet/charts');
    expect(await screen.findByText('Error · not a 404', {}, LAZY)).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('charts: the chart model broke');
    expect(screen.getAllByRole('banner')).toHaveLength(1);
    expect(screen.getAllByRole('contentinfo')).toHaveLength(1);
    // in the frame: the tinted page root holds the crash page
    expect(document.querySelector('[data-tint]')).toContainElement(screen.getByRole('main'));
  });

  it('when the frame itself fails, the error page draws its own header and footer, once each', async () => {
    gates.frameFails.on = true;
    mount('/mainnet/dao');
    expect(await screen.findByText('Error · not a 404')).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('atmosphere: the sky failed to paint');
    expect(document.querySelector('[data-tint]')).toBeNull();
    expect(screen.getAllByRole('banner')).toHaveLength(1);
    expect(screen.getAllByRole('contentinfo')).toHaveLength(1);
  });
});

// Visual finish V7: the element holding the page is keyed by the path, so each route change mounts
// a new one and the page fades in; the header, outside it, stays. A search-only change (a tab) is
// the same page and does not fade again.
describe('the page arrives (visual finish V7)', () => {
  it('wraps the page in .arrive and replaces the wrapper on navigation; the header stays', async () => {
    const router = mount('/mainnet/dao');
    const wrapper = (await screen.findByText('dao page', {}, LAZY)).parentElement!;
    expect(wrapper).toHaveClass('arrive');
    const header = screen.getByRole('banner');
    expect(header).not.toContainElement(wrapper);

    await act(() => router.navigate('/mainnet?tab=blocks'));
    expect(await screen.findByText('home page', {}, LAZY)).toBeInTheDocument();
    const next = screen.getByText('home page').parentElement!;
    expect(next).toHaveClass('arrive');
    expect(next).not.toBe(wrapper);
    expect(wrapper.isConnected).toBe(false);
    expect(screen.getByRole('banner')).toBe(header);

    // the same path with another search: the same page, the same wrapper, no second fade
    await act(() => router.navigate('/mainnet?tab=transactions'));
    expect(screen.getByText('home page').parentElement).toBe(next);
  });

  it('wraps the chunk fallback in it too', async () => {
    mount('/mainnet/blocks');
    await screen.findAllByRole('banner');
    expect(document.querySelector('.min-h-\\[60vh\\]')!.parentElement).toHaveClass('arrive');
  });

  it('lets the not-found night fill what the header leaves: a flex column down to the shell', async () => {
    mount('/mainnet/no-such-page');
    await screen.findByRole('heading', {
      level: 1,
      name: 'all that is was worked for. this was not.',
    });
    const shell = screen.getByRole('main').parentElement!;
    expect(shell).toHaveClass('flex-1');
    const wrapper = shell.parentElement!;
    expect(wrapper).toHaveClass('arrive', 'flex', 'flex-1', 'flex-col');
    expect(wrapper.parentElement).toHaveClass('flex', 'min-h-screen', 'flex-col');
    expect(wrapper.parentElement).toContainElement(screen.getByRole('banner'));
  });
});

describe('no page mounts the header (visual finish V6)', () => {
  const root = resolve(__dirname, '../..');
  function tsx(dir: string): string[] {
    return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
      e.isDirectory() ? tsx(join(dir, e.name)) : e.name.endsWith('.tsx') ? [join(dir, e.name)] : []
    );
  }

  /** The source without its comments (a comment may name the header). */
  function code(file: string): string {
    return readFileSync(file, 'utf-8')
      .replace(/\/\*[\s\S]*?\*\//g, '')
      .replace(/^\s*\/\/.*$/gm, '');
  }

  it('renders <Header /> only in the frame and in the error shell drawn in place of the frame', () => {
    const mounts = ['app', 'components', 'src']
      .flatMap((dir) => tsx(join(root, dir)))
      .filter((file) => /<Header\s*\/>/.test(code(file)))
      .map((file) => relative(root, file))
      .sort();
    expect(mounts).toEqual(['components/error-page-shell.tsx', 'src/routes/router.tsx']);
  });
});
