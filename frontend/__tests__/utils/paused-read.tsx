import type { ReactElement } from 'react';
import { onTestFinished } from 'vitest';
import { act, render } from '@testing-library/react';
import { onlineManager, type QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createTestQueryClient } from '@/__tests__/utils/test-utils';

/**
 * Renders `ui` with its reads paused, the browser offline — the state a hidden tab's read sits in
 * between retries. `seed` puts in the reads that have already answered. `resume()` comes back
 * online, and every paused read goes on. The test leaves the browser online whatever happens.
 */
export function renderPaused(ui: ReactElement, seed?: (client: QueryClient) => void) {
  const client = createTestQueryClient();
  seed?.(client);
  onlineManager.setOnline(false);
  onTestFinished(() => onlineManager.setOnline(true));
  const view = render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>);
  return { ...view, client, resume: () => act(() => onlineManager.setOnline(true)) };
}
