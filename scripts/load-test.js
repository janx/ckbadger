import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

const errorRate = new Rate('errors');
const blocksTrend = new Trend('blocks_duration');
const txsTrend = new Trend('transactions_duration');
const statsTrend = new Trend('statistics_duration');

const BASE_URL = __ENV.API_URL || 'http://localhost:3001';

export const options = {
  scenarios: {
    smoke: {
      executor: 'constant-vus',
      vus: 1,
      duration: '30s',
      tags: { test_type: 'smoke' },
      exec: 'smokeTest',
    },
    load: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '30s', target: 10 },
        { duration: '1m', target: 50 },
        { duration: '30s', target: 100 },
        { duration: '1m', target: 100 },
        { duration: '30s', target: 0 },
      ],
      tags: { test_type: 'load' },
      exec: 'loadTest',
      startTime: '35s',
    },
    stress: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '30s', target: 100 },
        { duration: '1m', target: 200 },
        { duration: '30s', target: 300 },
        { duration: '1m', target: 300 },
        { duration: '30s', target: 0 },
      ],
      tags: { test_type: 'stress' },
      exec: 'stressTest',
      startTime: '5m',
    },
  },
  thresholds: {
    http_req_duration: ['p(95)<200', 'p(99)<500'],
    http_req_failed: ['rate<0.01'],
    errors: ['rate<0.01'],
    blocks_duration: ['p(95)<150'],
    transactions_duration: ['p(95)<150'],
    statistics_duration: ['p(95)<100'],
  },
};

export function smokeTest() {
  const endpoints = [
    '/api/v1/blocks?limit=10',
    '/api/v1/transactions?limit=10',
    '/api/v1/statistics/network',
  ];

  for (const endpoint of endpoints) {
    const res = http.get(`${BASE_URL}${endpoint}`);
    check(res, {
      'status is 200': (r) => r.status === 200,
      'response has body': (r) => r.body.length > 0,
    });
    errorRate.add(res.status !== 200);
  }

  sleep(1);
}

export function loadTest() {
  const scenario = Math.random();

  if (scenario < 0.4) {
    const res = http.get(`${BASE_URL}/api/v1/blocks?limit=20`);
    blocksTrend.add(res.timings.duration);
    check(res, { 'blocks status 200': (r) => r.status === 200 });
    errorRate.add(res.status !== 200);
  } else if (scenario < 0.7) {
    const res = http.get(`${BASE_URL}/api/v1/transactions?limit=20`);
    txsTrend.add(res.timings.duration);
    check(res, { 'transactions status 200': (r) => r.status === 200 });
    errorRate.add(res.status !== 200);
  } else if (scenario < 0.9) {
    const res = http.get(`${BASE_URL}/api/v1/statistics/network`);
    statsTrend.add(res.timings.duration);
    check(res, { 'statistics status 200': (r) => r.status === 200 });
    errorRate.add(res.status !== 200);
  } else {
    const blockNum = Math.floor(Math.random() * 1000) + 1;
    const res = http.get(`${BASE_URL}/api/v1/blocks/${blockNum}`);
    check(res, { 'block detail status ok': (r) => r.status === 200 || r.status === 404 });
    errorRate.add(res.status >= 500);
  }

  sleep(0.1);
}

export function stressTest() {
  const endpoints = [
    '/api/v1/blocks?limit=50',
    '/api/v1/transactions?limit=50',
    '/api/v1/statistics/network',
    '/api/v1/cells/live?limit=20',
  ];

  const endpoint = endpoints[Math.floor(Math.random() * endpoints.length)];
  const res = http.get(`${BASE_URL}${endpoint}`);

  check(res, {
    'stress test status ok': (r) => r.status === 200 || r.status === 429,
  });

  errorRate.add(res.status >= 500);
  sleep(0.05);
}

export function handleSummary(data) {
  return {
    stdout: textSummary(data, { indent: ' ', enableColors: true }),
    'scripts/load-test-results.json': JSON.stringify(data),
  };
}

function textSummary(data, opts) {
  const indent = opts.indent || '  ';
  let summary = '\n========== LOAD TEST SUMMARY ==========\n\n';

  summary += `${indent}Total Requests: ${data.metrics.http_reqs.values.count}\n`;
  summary += `${indent}Failed Requests: ${data.metrics.http_req_failed.values.passes}\n`;
  summary += `${indent}Error Rate: ${(data.metrics.errors.values.rate * 100).toFixed(2)}%\n\n`;

  summary += `${indent}Response Times:\n`;
  summary += `${indent}${indent}Avg: ${data.metrics.http_req_duration.values.avg.toFixed(2)}ms\n`;
  summary += `${indent}${indent}P50: ${data.metrics.http_req_duration.values['p(50)'].toFixed(2)}ms\n`;
  summary += `${indent}${indent}P95: ${data.metrics.http_req_duration.values['p(95)'].toFixed(2)}ms\n`;
  summary += `${indent}${indent}P99: ${data.metrics.http_req_duration.values['p(99)'].toFixed(2)}ms\n`;
  summary += `${indent}${indent}Max: ${data.metrics.http_req_duration.values.max.toFixed(2)}ms\n\n`;

  summary += `${indent}Throughput: ${data.metrics.http_reqs.values.rate.toFixed(2)} req/s\n`;

  summary += '\n========================================\n';

  return summary;
}
