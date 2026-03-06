import { describe, expect, it } from 'vitest';
import { matchRoutes } from 'react-router-dom';
import { createAppRouter } from '@/src/routes/router';

describe('explorer routes', () => {
  it('matches the core explorer routes in the SPA router', () => {
    const routes = createAppRouter();

    expect(matchRoutes(routes, '/blocks')).toBeTruthy();
    expect(matchRoutes(routes, '/blocks/123')).toBeTruthy();
    expect(matchRoutes(routes, '/transactions')).toBeTruthy();
    expect(matchRoutes(routes, '/tx/0x1234')).toBeTruthy();
    expect(matchRoutes(routes, '/address/ckb1qyqszqgpqyqszqgpqyqszqgpqyqszqgp9f0v3')).toBeTruthy();
    expect(matchRoutes(routes, '/cell/0xtx-0')).toBeTruthy();
    expect(matchRoutes(routes, '/forks')).toBeTruthy();
    expect(matchRoutes(routes, '/forks/1')).toBeTruthy();
    expect(matchRoutes(routes, '/charts')).toBeTruthy();
    expect(matchRoutes(routes, '/charts/most-utilized-scripts')).toBeTruthy();
    expect(matchRoutes(routes, '/dao')).toBeTruthy();
    expect(matchRoutes(routes, '/hardforks')).toBeTruthy();
  });
});
