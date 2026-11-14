import { afterEach, describe, expect, it } from 'vitest';
import { onlineManager, QueryClient, QueryObserver } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';

// The one answer to "is this read still on its way?" (A1 review fix, round 2), against a real
// QueryClient in each of the four states a read can be in.

function observe<T>(
  client: QueryClient,
  options: ConstructorParameters<typeof QueryObserver<T>>[1]
) {
  const observer = new QueryObserver<T>(client, options);
  const unsubscribe = observer.subscribe(() => {});
  return { observer, unsubscribe };
}

afterEach(() => {
  onlineManager.setOnline(true);
});

describe('isAwaited', () => {
  it('a read that is fetching its first answer is awaited', () => {
    const client = new QueryClient();
    const { observer, unsubscribe } = observe(client, {
      queryKey: ['fetching'],
      queryFn: () => new Promise<number>(() => {}),
    });
    const result = observer.getCurrentResult();
    expect([result.status, result.fetchStatus]).toEqual(['pending', 'fetching']);
    expect(isAwaited(result)).toBe(true);
    unsubscribe();
  });

  it('a read paused before its first answer (offline, or between retries) is awaited', () => {
    onlineManager.setOnline(false);
    const client = new QueryClient();
    const { observer, unsubscribe } = observe(client, {
      queryKey: ['paused'],
      queryFn: async () => 1,
    });
    const result = observer.getCurrentResult();
    expect([result.status, result.fetchStatus]).toEqual(['pending', 'paused']);
    // isLoading (pending AND fetching) misses exactly this state
    expect(result.isLoading).toBe(false);
    expect(isAwaited(result)).toBe(true);
    unsubscribe();
  });

  it('a disabled read is not awaited: nothing will answer it until it is enabled', () => {
    const client = new QueryClient();
    const { observer, unsubscribe } = observe(client, {
      queryKey: ['disabled'],
      queryFn: async () => 1,
      enabled: false,
    });
    const result = observer.getCurrentResult();
    expect([result.status, result.fetchStatus]).toEqual(['pending', 'idle']);
    expect(isAwaited(result)).toBe(false);
    unsubscribe();
  });

  it('a completed read is not awaited, whether it succeeded or failed, even while it refetches', async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const done = observe(client, { queryKey: ['done'], queryFn: async () => 1 });
    const failed = observe(client, {
      queryKey: ['failed'],
      queryFn: async (): Promise<number> => {
        throw new Error('no');
      },
    });
    await client.getQueryCache().find({ queryKey: ['done'] })!.promise;
    await expect(client.getQueryCache().find({ queryKey: ['failed'] })!.promise).rejects.toThrow();
    expect(done.observer.getCurrentResult().status).toBe('success');
    expect(failed.observer.getCurrentResult().status).toBe('error');
    expect(isAwaited(done.observer.getCurrentResult())).toBe(false);
    expect(isAwaited(failed.observer.getCurrentResult())).toBe(false);
    // a refetch of an answered read, even a paused one, is not a wait for its first answer
    onlineManager.setOnline(false);
    void done.observer.refetch();
    const refetching = done.observer.getCurrentResult();
    expect([refetching.status, refetching.fetchStatus]).toEqual(['success', 'paused']);
    expect(isAwaited(refetching)).toBe(false);
    done.unsubscribe();
    failed.unsubscribe();
  });
});
