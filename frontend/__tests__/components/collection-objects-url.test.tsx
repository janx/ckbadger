// Consistency round C15: the cluster gel's filter is in the URL — `?background=…`, `?view=…` for
// the DOB/1 keys that read as words, the gene's own name otherwise — read on arrival and written
// on every band click (replace, not a history step), the D11 `?filter=` pattern.
import { act, fireEvent, screen, waitFor } from '@testing-library/react';
import { createMemoryRouter, RouterProvider } from 'react-router-dom';
import { render } from '@/__tests__/utils/test-utils';
import { CollectionObjects } from '@/components/asset/collection-objects';
import { api } from '@/lib/api';
import { mockItemReads, emptyPage } from '@/__tests__/utils/item-read-fixtures';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { NERVAPE3X_CLUSTER, nervape3xInsights } from '@/__tests__/fixtures/objects/nervape3x';

vi.mock('@/src/navigation', async (importOriginal) => await importOriginal());

const prevBg = nervape3xInsights.genetics.traits.find((t) => t.name === 'prev.bg')!.values[0].value;

function mount(at: string) {
  const router = createMemoryRouter(
    [
      {
        path: '/mainnet/clusters/:id',
        element: (
          <CollectionObjects
            id={NERVAPE3X_CLUSTER}
            title="Nervape 3x"
            insights={nervape3xInsights}
          />
        ),
      },
    ],
    { initialEntries: [at] }
  );
  render(<RouterProvider router={router} />);
  return router;
}

beforeEach(() => {
  mockItemReads();
  stubChartWidth(1248);
  vi.mocked(api.getCollectionGallery).mockResolvedValue(emptyPage());
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it('reads the filter from the URL on arrival', async () => {
  mount(
    `/mainnet/clusters/${NERVAPE3X_CLUSTER}?Color=Calm%20Blue&background=${encodeURIComponent(prevBg)}`
  );
  await waitFor(() =>
    expect(api.getCollectionGallery).toHaveBeenCalledWith(NERVAPE3X_CLUSTER, {
      limit: 20,
      cursor: undefined,
      traits: { 'prev.bg': prevBg, Color: 'Calm Blue' },
    })
  );
  expect(
    await screen.findByRole('button', { name: 'Clear Color = Calm Blue' })
  ).toBeInTheDocument();
});

it('writes a band click to the URL by its word key, keeps other params, and clears it again', async () => {
  const router = mount(`/mainnet/clusters/${NERVAPE3X_CLUSTER}?tab=objects`);
  await screen.findByRole('table', { name: 'Newest objects' });
  const band = document.querySelector(`[data-lane="prev.bg"] [data-allele="${prevBg}"]`)!;
  act(() => {
    fireEvent.click(band);
  });
  await waitFor(() =>
    expect(new URLSearchParams(router.state.location.search).get('background')).toBe(prevBg)
  );
  expect(new URLSearchParams(router.state.location.search).get('tab')).toBe('objects');
  expect(router.state.historyAction).toBe('REPLACE');
  act(() => {
    fireEvent.click(document.querySelector('[data-lane="Color"] [data-allele="Calm Blue"]')!);
  });
  await waitFor(() =>
    expect(new URLSearchParams(router.state.location.search).get('Color')).toBe('Calm Blue')
  );
  act(() => {
    fireEvent.click(screen.getByRole('button', { name: `Clear prev.bg = ${prevBg}` }));
  });
  await waitFor(() =>
    expect(new URLSearchParams(router.state.location.search).get('background')).toBeNull()
  );
  expect(new URLSearchParams(router.state.location.search).get('Color')).toBe('Calm Blue');
  await waitFor(() =>
    expect(api.getCollectionGallery).toHaveBeenLastCalledWith(NERVAPE3X_CLUSTER, {
      limit: 20,
      cursor: undefined,
      traits: { Color: 'Calm Blue' },
    })
  );
});

it('ignores a URL key for a gene that has no lane', async () => {
  // Note is empty for every object (C9): it gets no lane, so `?Note=` filters nothing.
  mount(`/mainnet/clusters/${NERVAPE3X_CLUSTER}?Note=x`);
  await waitFor(() =>
    expect(api.getCollectionGallery).toHaveBeenCalledWith(NERVAPE3X_CLUSTER, {
      limit: 20,
      cursor: undefined,
      traits: {},
    })
  );
});
