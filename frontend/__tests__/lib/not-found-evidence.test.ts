import { describe, expect, it, vi } from 'vitest';
import { act, renderHook, waitFor } from '@testing-library/react';
import { api } from '@/lib/api';
import { resolveNotFoundEvidence, useNotFoundEvidence } from '@/lib/not-found-evidence';

const hash = `0x${'a'.repeat(64)}`;

describe('not-found identifier evidence', () => {
  it('checks a transaction-shaped block identifier through the typed search route', async () => {
    const search = vi.fn().mockResolvedValue({
      query: `tx:${hash}`,
      results: [
        {
          resultType: 'transaction',
          id: hash,
          label: 'Transaction in Block #12',
          url: `/tx/${hash}`,
          matchKind: 'exact_hash',
        },
      ],
    });
    const evidence = await resolveNotFoundEvidence(
      { kind: 'block', identifier: hash, checkedEndpoint: `/blocks/${hash}` },
      search
    );
    expect(search).toHaveBeenCalledTimes(1);
    expect(search).toHaveBeenCalledWith(`tx:${hash}`);
    expect(evidence.checkedEndpoint).toBe(`/blocks/${hash}`);
    expect(evidence.alternative).toEqual({
      href: `/tx/${hash}`,
      label: 'Transaction in Block #12',
      resultType: 'transaction',
      matchKind: 'exact_hash',
    });
  });

  it('checks a block-shaped transaction identifier without treating an unrelated hit as proof', async () => {
    const search = vi.fn().mockResolvedValue({
      query: `block:${hash}`,
      results: [
        {
          resultType: 'block',
          id: '12',
          label: 'Block #12',
          url: `/blocks/${hash}`,
          matchKind: 'exact_hash',
        },
        {
          resultType: 'transaction',
          id: hash,
          label: 'Wrong route',
          url: `/tx/${hash}`,
          matchKind: 'exact_hash',
        },
      ],
    });
    const evidence = await resolveNotFoundEvidence(
      { kind: 'transaction', identifier: hash, checkedEndpoint: `/transactions/${hash}/detail` },
      search
    );
    expect(search).toHaveBeenCalledTimes(1);
    expect(search).toHaveBeenCalledWith(`block:${hash}`);
    expect(evidence.alternative?.href).toBe(`/blocks/${hash}`);
  });

  it('quotes search-spent outpoint evidence as a conflict, not as an independent diagnosis', async () => {
    const outpoint = `${hash}-2`;
    const search = vi.fn().mockResolvedValue({
      query: `cell:${outpoint}`,
      results: [
        {
          resultType: 'cell',
          id: outpoint,
          label: 'Cell (Dead, 100 CKB)',
          url: `/cell/${outpoint}`,
          matchKind: 'outpoint',
        },
      ],
    });
    const evidence = await resolveNotFoundEvidence(
      { kind: 'cell', identifier: outpoint, checkedEndpoint: `/cells/${hash}/2` },
      search
    );
    expect(evidence.finding).toContain('“Cell (Dead, 100 CKB)”');
    expect(evidence.finding).toContain('returned HTTP 404');
    expect(evidence.alternative).toBeUndefined();
  });

  it('does not turn no search result into all-chain or transaction-pool proof', async () => {
    const search = vi.fn().mockResolvedValue({ query: `tx:${hash}`, results: [] });
    const evidence = await resolveNotFoundEvidence(
      { kind: 'block', identifier: hash, checkedEndpoint: `/blocks/${hash}` },
      search
    );
    expect(evidence.alternative).toBeUndefined();
    expect(evidence.finding).toMatch(/search returned no typed match/i);
    expect(evidence.finding).not.toMatch(/never existed|entire chain|pool/);
  });

  it('rejects a mislabeled or off-route search hit', async () => {
    const search = vi.fn().mockResolvedValue({
      query: `tx:${hash}`,
      results: [
        {
          resultType: 'transaction',
          id: hash,
          label: 'Bad',
          url: 'https://example.com/tx',
          matchKind: 'exact_hash',
        },
      ],
    });
    const evidence = await resolveNotFoundEvidence(
      { kind: 'block', identifier: hash, checkedEndpoint: `/blocks/${hash}` },
      search
    );
    expect(evidence.alternative).toBeUndefined();
  });

  it('does not search an unsupported identifier namespace', async () => {
    const search = vi.fn();
    const evidence = await resolveNotFoundEvidence(
      { kind: 'fork', identifier: '7/a9bc1b75', checkedEndpoint: '/forks/7/a9bc1b75' },
      search
    );
    expect(search).not.toHaveBeenCalled();
    expect(evidence.query).toBeNull();
    expect(evidence.checkedEndpoint).toBe('/forks/7/a9bc1b75');
  });

  it('works outside QueryClientProvider and exposes search failure separately from the entity 404', async () => {
    const spy = vi
      .spyOn(api, 'search')
      .mockRejectedValueOnce(new Error('search transport unavailable'));
    const { result } = renderHook(() =>
      useNotFoundEvidence({ kind: 'block', identifier: hash, checkedEndpoint: `/blocks/${hash}` })
    );
    await waitFor(() => expect(result.current.status).toBe('error'));
    expect(result.current.error).toContain('search transport unavailable');
    spy.mockRestore();
  });

  it('rejects a nonlocal checked endpoint before asking search', async () => {
    const search = vi.fn();
    await expect(
      resolveNotFoundEvidence(
        { kind: 'block', identifier: hash, checkedEndpoint: 'https://elsewhere/blocks/x' },
        search
      )
    ).rejects.toThrow('Invalid entity evidence endpoint');
    expect(search).not.toHaveBeenCalled();
  });

  it('reports the exact search cell label without deriving live or spent from its wording', async () => {
    const outpoint = `${hash}-2`;
    const evidence = await resolveNotFoundEvidence(
      { kind: 'cell', identifier: outpoint, checkedEndpoint: `/cells/${hash}/2` },
      async () => ({
        query: `cell:${outpoint}`,
        results: [
          {
            resultType: 'cell',
            id: outpoint,
            label: 'Cell status unavailable',
            url: `/cell/${outpoint}`,
            matchKind: 'outpoint',
          },
        ],
      })
    );
    expect(evidence.finding).toContain('“Cell status unavailable”');
    expect(evidence.finding).not.toMatch(/reports a live cell|reports a spent cell/);
  });

  it('discards in-flight evidence when the entity or active network changes', async () => {
    const previousConfig = window.__CKBADGER_RUNTIME_CONFIG__;
    const previousPath = window.location.pathname;
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
    window.history.pushState(null, '', `/mainnet/blocks/${hash}`);
    const pending: Array<(value: { query: string; results: [] }) => void> = [];
    const spy = vi
      .spyOn(api, 'search')
      .mockImplementation(
        (query) => new Promise((resolve) => pending.push((value) => resolve(value)))
      );
    const entity = { kind: 'block' as const, identifier: hash, checkedEndpoint: `/blocks/${hash}` };
    try {
      const { result, rerender } = renderHook(({ current }) => useNotFoundEvidence(current), {
        initialProps: { current: entity },
      });
      await waitFor(() => expect(spy).toHaveBeenCalledTimes(1));
      window.history.pushState(null, '', `/testnet/blocks/${hash}`);
      rerender({ current: entity });
      expect(result.current.status).toBe('loading');
      await waitFor(() => expect(spy).toHaveBeenCalledTimes(2));
      await act(async () => pending[0]({ query: `tx:${hash}`, results: [] }));
      expect(result.current.status).toBe('loading');
      await act(async () => pending[1]({ query: `tx:${hash}`, results: [] }));
      expect(result.current.status).toBe('checked');
    } finally {
      spy.mockRestore();
      window.__CKBADGER_RUNTIME_CONFIG__ = previousConfig;
      window.history.pushState(null, '', previousPath);
    }
  });
});
