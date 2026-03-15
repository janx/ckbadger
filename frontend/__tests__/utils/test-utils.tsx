import { render, RenderOptions } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { ReactNode, ReactElement } from 'react';
import { WarmupPendingBanner } from '@/components/ui/warmup-pending-banner';
import { createAppQueryClient } from '@/lib/query-client';

function createTestQueryClient() {
  return createAppQueryClient({
    nonWarmupRetry: false,
    gcTime: Infinity,
    staleTime: Infinity,
    warmupRetryLimit: 2,
    warmupRetryDelayMs: 10,
  });
}

interface AllProvidersProps {
  children: ReactNode;
}

function AllProviders({ children }: AllProvidersProps) {
  const queryClient = createTestQueryClient();
  return (
    <QueryClientProvider client={queryClient}>
      <WarmupPendingBanner />
      {children}
    </QueryClientProvider>
  );
}

const customRender = (ui: ReactElement, options?: Omit<RenderOptions, 'wrapper'>) =>
  render(ui, { wrapper: AllProviders, ...options });

export * from '@testing-library/react';
export { customRender as render };
export { createTestQueryClient };
