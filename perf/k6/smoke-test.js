import http from 'k6/http';
import { check } from 'k6';

const BASE_URL = __ENV.API_URL || 'http://localhost:3001/api/v1';

export const options = {
  vus: 1,
  iterations: 1,
  thresholds: {
    checks: ['rate==1'],
    http_req_duration: ['p(99)<1000'],
  },
};

export default function () {
  const endpoints = [
    { path: '/blocks?limit=10', name: 'blocks' },
    { path: '/statistics/network', name: 'network' },
    { path: '/statistics/tx-stats', name: 'tx_stats' },
    { path: '/mempool/summary', name: 'mempool' },
    { path: '/charts/transaction-count', name: 'chart' },
  ];

  for (const ep of endpoints) {
    const res = http.get(`${BASE_URL}${ep.path}`);
    check(res, {
      [`${ep.name} returns 200`]: (r) => r.status === 200,
      [`${ep.name} < 1s`]: (r) => r.timings.duration < 1000,
      [`${ep.name} has body`]: (r) => r.body && r.body.length > 0,
    });
  }
}
