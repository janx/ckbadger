import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

const errorRate = new Rate('errors');
const blockListLatency = new Trend('block_list_latency');
const blockDetailLatency = new Trend('block_detail_latency');
const networkStatsLatency = new Trend('network_stats_latency');
const chartLatency = new Trend('chart_latency');

const BASE_URL = __ENV.API_URL || 'http://localhost:3001/api/v1';

export const options = {
  scenarios: {
    smoke: {
      executor: 'constant-vus',
      vus: 1,
      duration: '30s',
      exec: 'smokeTest',
    },
    load: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '1m', target: 25 },
        { duration: '3m', target: 50 },
        { duration: '1m', target: 0 },
      ],
      exec: 'loadTest',
      startTime: '35s',
    },
  },
  thresholds: {
    errors: ['rate<0.01'],
    http_req_duration: ['p(95)<500', 'p(99)<1000'],
    block_list_latency: ['p(95)<100', 'p(99)<200'],
    block_detail_latency: ['p(95)<50', 'p(99)<100'],
    network_stats_latency: ['p(95)<100', 'p(99)<200'],
    chart_latency: ['p(95)<500', 'p(99)<1000'],
  },
};

export function smokeTest() {
  const endpoints = [
    { name: 'blocks', path: '/blocks?limit=10' },
    { name: 'network_stats', path: '/statistics/network' },
    { name: 'tx_stats', path: '/statistics/tx-stats' },
    { name: 'recent_blocks', path: '/statistics/recent-blocks' },
    { name: 'chart_tx', path: '/charts/transaction-count' },
    { name: 'mempool', path: '/mempool/summary' },
  ];

  for (const ep of endpoints) {
    const res = http.get(`${BASE_URL}${ep.path}`);
    const success = check(res, {
      [`${ep.name} status 200`]: (r) => r.status === 200,
      [`${ep.name} < 500ms`]: (r) => r.timings.duration < 500,
    });
    errorRate.add(!success);
    sleep(0.5);
  }
}

export function loadTest() {
  const rand = Math.random();

  if (rand < 0.35) {
    const res = http.get(`${BASE_URL}/blocks?limit=10`);
    blockListLatency.add(res.timings.duration);
    errorRate.add(res.status !== 200);
  } else if (rand < 0.5) {
    const blockNum = Math.floor(Math.random() * 1000000) + 1;
    const res = http.get(`${BASE_URL}/blocks/${blockNum}`);
    blockDetailLatency.add(res.timings.duration);
    errorRate.add(res.status !== 200 && res.status !== 404);
  } else if (rand < 0.7) {
    const res = http.get(`${BASE_URL}/statistics/network`);
    networkStatsLatency.add(res.timings.duration);
    errorRate.add(res.status !== 200);
  } else if (rand < 0.85) {
    const res = http.get(`${BASE_URL}/mempool/summary`);
    errorRate.add(res.status !== 200);
  } else {
    const charts = ['transaction-count', 'cell-count', 'hash-rate', 'average-block-time'];
    const chart = charts[Math.floor(Math.random() * charts.length)];
    const res = http.get(`${BASE_URL}/charts/${chart}`);
    chartLatency.add(res.timings.duration);
    errorRate.add(res.status !== 200);
  }

  sleep(Math.random() * 2);
}

export function handleSummary(data) {
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
  return {
    [`perf/results/api-load-test-${timestamp}.json`]: JSON.stringify(data, null, 2),
    stdout: textSummary(data, { indent: '  ', enableColors: true }),
  };
}

function textSummary(data, options) {
  const { metrics } = data;
  let output = '\n=== API Load Test Results ===\n\n';

  const keyMetrics = [
    'http_req_duration',
    'block_list_latency',
    'block_detail_latency',
    'network_stats_latency',
    'chart_latency',
    'errors',
  ];

  for (const name of keyMetrics) {
    if (metrics[name]) {
      const m = metrics[name];
      if (m.type === 'trend') {
        output += `${name}:\n`;
        output += `  avg: ${m.values.avg?.toFixed(2) || 'N/A'}ms\n`;
        output += `  p95: ${m.values['p(95)']?.toFixed(2) || 'N/A'}ms\n`;
        output += `  p99: ${m.values['p(99)']?.toFixed(2) || 'N/A'}ms\n`;
      } else if (m.type === 'rate') {
        output += `${name}: ${(m.values.rate * 100).toFixed(2)}%\n`;
      }
    }
  }

  return output;
}
