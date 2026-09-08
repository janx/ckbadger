#!/usr/bin/env python3
"""Black-box contract test for the real embedded frontend in a built CLI binary.

Run after `pnpm --dir frontend build && cargo build --release -p ckbadger`.
The frontend runs alone in a fresh workdir, with two deterministic read-only API
and RPC fixtures. No chain store, running indexer, Node server, or asset override
is involved. Every advertised route/profile must reach its network's API, even
while that API reports the normal pre-sync 503 state.
"""

import argparse
import concurrent.futures
import contextlib
import http.client
import http.server
import json
from pathlib import Path
import re
import socket
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request


TX = '0x' + 'a' * 64
INPUT = '0x' + '1' * 64
GROUP = '0x' + '2' * 64
DEP = '0x' + '3' * 64
HEADER = '0x' + '4' * 64
WITNESS = '0x1b00000010000000160000001600000006000000112205000000aa'
PUBLIC_ORIGIN = 'https://explorer.example'
RAW_TYPE = 'application/vnd.ckbadger.raw+json'


class Fixture(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def send_json(self, payload, status=200):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        path = urllib.parse.urlsplit(self.path).path
        network = self.server.network
        self.server.requests.append(('GET', self.path))
        block = {
            'number': 42, 'hash': HEADER, 'parentHash': INPUT,
            'timestamp': '2026-09-08T00:00:00Z', 'epochNumber': 1, 'epochIndex': 2,
            'epochLength': 1000, 'transactionsCount': 1, 'proposalsCount': 0,
            'unclesCount': 0, 'difficulty': '9007199254740993', 'fixtureNetwork': network,
        }
        transaction = {
            'hash': TX, 'status': 'committed', 'pendingSince': None, 'blockNumber': 42,
            'blockHash': HEADER, 'index': 0, 'inputsCount': 1, 'outputsCount': 1,
            'fee': '1000', 'confirmations': 10, 'isCellbase': False,
            'timestamp': block['timestamp'], 'inputsCapacity': '9007199254740993',
            'outputsCapacity': '9007199254739993', 'inputsCommonKnowledgeSize': '61',
            'outputsCommonKnowledgeSize': '61',
            'inputs': [{'lock': {'codeHash': DEP, 'hashType': 'type', 'args': '0x01'}}],
            'outputs': [], 'witnesses': [WITNESS], 'witnessesAvailable': True,
            'fixtureNetwork': network,
        }
        fixtures = {
            '/api/v1/blocks/42': block,
            '/api/v1/blocks/42/fee-stats': {'totalSize': 500, 'totalCycles': 123,
                'avgFeeRate': '2', 'minFeeRate': '1', 'maxFeeRate': '3'},
            '/api/v1/blocks/42/proposals': [],
            '/api/v1/transactions': {'data': [], 'limit': 20, 'nextCursor': None, 'hasMore': False},
            f'/api/v1/transactions/{TX}/detail': transaction,
            f'/api/v1/transactions/{TX}/cell-deps': [],
            f'/api/v1/transactions/{TX}/lifecycle': {'hash': TX, 'phase': 'committed',
                'proposalId': '0x1234', 'proposedIn': None,
                'committedIn': {'blockNumber': 42, 'blockHash': HEADER, 'timestamp': block['timestamp']},
                'commitmentDistance': 2, 'commitmentWindow': {'close': 2, 'far': 10},
                'isCellbase': False, 'confirmations': 10},
            f'/api/v1/cells/{TX}/0': {'txHash': TX, 'outputIndex': 0,
                'capacity': '9007199254740993', 'fixtureNetwork': network},
        }
        if path in fixtures:
            self.send_json(fixtures[path])
        else:
            self.send_json({'error': 'initializing', 'message': f'{network} fixture API reached {path}'}, 503)

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.server.requests.append(('POST', self.path, request))
        if self.path == '/api/v1/scripts/lookup':
            self.send_json({'error': 'initializing', 'message': f'{self.server.network} fixture API reached scripts/lookup'}, 503)
            return
        method, params = request['method'], request['params']
        output = {'capacity': '0x174876e800', 'lock': {
            'code_hash': DEP, 'hash_type': 'type',
            'args': '0x01' if self.server.network == 'mainnet' else '0x02',
        }, 'type': None}
        if method == 'get_transaction' and len(params) == 2:
            result = {'transaction': None, 'tx_status': {'status': 'committed', 'block_hash': HEADER}}
        elif method == 'get_transaction':
            assert params == [TX], params
            result = {'transaction': {'version': '0x0', 'cell_deps': [
                {'out_point': {'tx_hash': GROUP, 'index': '0x0'}, 'dep_type': 'dep_group'}],
                'header_deps': [HEADER], 'inputs': [{'previous_output': {'tx_hash': INPUT, 'index': '0x0'}, 'since': '0x0'}],
                'outputs': [output], 'outputs_data': ['0x'], 'witnesses': [WITNESS]}}
        elif method == 'get_live_cell':
            data = '0x01000000' + DEP[2:] + '00000000' if params[0]['tx_hash'] == GROUP else '0x'
            result = {'status': 'live', 'cell': {'output': output, 'data': {'content': data}}}
        elif method == 'get_header':
            assert params == [HEADER], params
            result = {'hash': HEADER, 'number': '0x1', 'epoch': '0x1', 'timestamp': '0x1',
                'parent_hash': INPUT, 'nonce': '0x0', 'version': '0x0', 'compact_target': '0x0',
                'dao': '0x' + '0' * 64, 'transactions_root': DEP, 'proposals_hash': DEP, 'extra_hash': DEP}
        else:
            raise AssertionError(f'Unexpected RPC method: {method}')
        self.send_json({'jsonrpc': '2.0', 'id': request['id'], 'result': result})


class FixtureServer(http.server.ThreadingHTTPServer):
    def handle_error(self, request, client_address):
        # Promise.all propagates the first pre-sync 503 immediately. Dropping
        # that render cancels its other in-flight API reads; a disconnected
        # fixture client is expected here, not a fixture assertion failure.
        if isinstance(sys.exc_info()[1], (BrokenPipeError, ConnectionResetError)):
            self.cancelled_requests.append(client_address)
            return
        super().handle_error(request, client_address)


@contextlib.contextmanager
def fixture_server(network):
    server = FixtureServer(('127.0.0.1', 0), Fixture)
    server.network, server.requests = network, []
    server.cancelled_requests = []
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield server
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', default='target/release/ckbadger')
    parser.add_argument('--benchmark-against', help='Optional baseline binary for a local HTML latency comparison')
    args = parser.parse_args()
    binary = Path(args.binary).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='ckbadger-frontend-e2e-') as tmp, \
            fixture_server('mainnet') as mainnet, fixture_server('testnet') as testnet:
        root = Path(tmp)
        with socket.socket() as socket_:
            socket_.bind(('127.0.0.1', 0))
            port = socket_.getsockname()[1]
        servers = {'mainnet': mainnet, 'testnet': testnet}
        for name, server in servers.items():
            (root / name).mkdir()
            (root / name / 'config.toml').write_text(
                f'[ckb]\nnetwork = "{name}"\nrpc_url = "http://127.0.0.1:{server.server_port}/rpc"\n'
                f'[api]\nhost = "127.0.0.1"\nport = {server.server_port}\n')
        (root / 'ckbadger.toml').write_text(
            '[[network]]\nname = "mainnet"\n[[network]]\nname = "testnet"\n'
            f'[frontend]\nhost = "127.0.0.1"\nport = {port}\npublic_origin = "{PUBLIC_ORIGIN}"\n')
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

        def get(path, status=200, accept=None, method='GET'):
            headers = {'Accept': accept or '*/*'}
            request = urllib.request.Request(f'http://127.0.0.1:{port}{path}', headers=headers, method=method)
            try:
                response = opener.open(request, timeout=35)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                body = response.read().decode()
                assert response.status == status, (path, response.status, status, body)
                return response.headers, body

        with (root / 'frontend.log').open('w+') as log:
            process = subprocess.Popen([str(binary), '-C', str(root), 'internal', 'frontend-server'],
                cwd=root, stdout=log, stderr=subprocess.STDOUT)
            try:
                for _ in range(200):
                    if process.poll() is not None:
                        log.seek(0)
                        raise AssertionError(log.read())
                    try:
                        _, body = get('/capabilities')
                        break
                    except urllib.error.URLError:
                        time.sleep(0.05)
                else:
                    raise AssertionError('Frontend did not start')
                caps = json.loads(body)
                assert caps['origin'] == PUBLIC_ORIGIN
                assert caps['site']['networks'] == ['mainnet', 'testnet']
                for discovery in ['/llms.txt', '/llms-full.txt']:
                    _, document = get(discovery)
                    assert '<!-- REGISTERED_PAGE_FORMATS -->' not in document
                    for pattern in caps['routes']['markdown']:
                        assert f'| `{pattern}` | .md |' in document, (discovery, pattern)

                def raw_block(network):
                    for suffix, accept in [('.raw', None), ('?format=raw', None), ('', RAW_TYPE),
                            ('.md?format=raw', 'text/markdown'), ('?%66ormat=raw', None)]:
                        headers, body = get(f'/{network}/blocks/42{suffix}', accept=accept)
                        assert headers['Content-Type'].startswith(RAW_TYPE)
                        assert headers['Vary'] == 'Accept' and headers['Cache-Control'] == 'no-store'
                        assert headers['x-ckbadger-format'] == 'raw'
                        payload = json.loads(body)
                        assert payload['data']['block']['fixtureNetwork'] == network
                        assert payload['data']['block']['difficulty'] == '9007199254740993'
                        assert payload['meta']['network'] == network
                        assert payload['meta']['canonical'] == f'{PUBLIC_ORIGIN}/{network}/blocks/42'
                        assert payload['meta']['buildVersion']
                        assert payload['meta']['schemaVersion'] == headers['x-ckbadger-schema']
                        assert payload['meta']['profile'] == headers['x-ckbadger-profile'] == 'default'

                with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                    list(pool.map(raw_block, servers))
                for network in servers:
                    _, body = get(f'/{network}/scripts/deployed.v1', 503, 'text/markdown')
                    assert f'{network} fixture API reached' in json.loads(body)['error']['message']
                    for suffix, accept in [('.md', None), ('.md/', None), ('?format=md', None),
                            ('', 'TEXT/MARKDOWN'), ('.raw?format=md', RAW_TYPE)]:
                        headers, body = get(f'/{network}/blocks/42{suffix}', accept=accept)
                        assert headers['Content-Type'].startswith('text/markdown'), body
                        assert '# Block 42' in body and '9007199254740993' in body
                        assert f'canonical: "{PUBLIC_ORIGIN}/{network}/blocks/42"' in body
                    for suffix, accept in [('', 'text/html'), ('.raw?format=html', RAW_TYPE),
                            ('', 'text/html, text/markdown;q=0.1'), ('', 'text/markdown;q=0')]:
                        headers, body = get(f'/{network}/blocks/42{suffix}', accept=accept)
                        assert headers['Content-Type'].startswith('text/html') and '<html' in body.lower()
                        assert headers['Vary'] == 'Accept' and headers['Cache-Control'] == 'no-store'
                    headers, body = get(f'/{network}/blocks/42.raw', method='HEAD')
                    assert body == '' and headers['Content-Type'].startswith(RAW_TYPE)
                    _, body = get(f'/{network}/tx/{TX}.raw?profile=debugger')
                    payload = json.loads(body)
                    assert payload['meta']['network'] == network
                    mock = payload['data']['txDebugger']['mockTransaction']
                    assert len(mock['mock_info']['cell_deps']) == 2  # dep_group expansion
                    assert mock['mock_info']['inputs'][0]['header'] == HEADER
                    assert mock['mock_info']['header_deps'][0]['hash'] == HEADER
                    assert mock['tx']['outputs'][0]['lock']['args'] == ('0x01' if network == 'mainnet' else '0x02')
                    assert payload['data']['txWitness']['analyses'][0]['deterministic']['kind'] == 'WitnessArgs'

                for path, status, code in [
                    ('/mainnet/blocks/42.raw?profile=unknown', 400, 'invalid_profile'),
                    ('/mainnet/blocks/42.raw?profile=debugger', 400, 'profile_not_supported'),
                    ('/mainnet/blocks/42?format=json', 400, 'invalid_format'),
                    ('/mainnet/blocks/42?format=raw&format=md', 400, 'invalid_format'),
                    ('/devnet/blocks/42.raw', 404, 'unknown_network'),
                    ('/mainnet/tokens.raw', 404, 'unknown_page'),
                    ('/mainnet/not-a-route.md', 404, 'unknown_page'),
                    ('/mainnet/blocks/..%2F..%2Fws.raw', 400, 'invalid_request'),
                ]:
                    headers, body = get(path, status)
                    assert headers['Content-Type'].startswith('application/json')
                    assert json.loads(body)['error']['code'] == code, (path, body)

                matrix_count = 0
                for network, server in servers.items():
                    for slug in caps['chartSlugs']:
                        _, body = get(f'/{network}/charts/{slug}.md', 503)
                        assert f'{network} fixture API reached' in json.loads(body)['error']['message']
                    for format_, suffix in [('markdown', 'md'), ('raw', 'raw')]:
                        for pattern in caps['routes'][format_]:
                            path = re.sub(r'\{([^}]+)\}', lambda m: {
                                'slug': 'hash-rate', 'outpoint': '0xmissing-0',
                            }.get(m[1], '424242'), pattern)
                            profiles = caps['rawProfiles']['routes'][pattern] if format_ == 'raw' else ['default']
                            for profile in profiles:
                                route = f'/{network}{path}.{suffix}?profile={profile}'
                                # The transactions list has a successful fixture; all other
                                # matrix entries see a pre-sync API and must preserve its 503.
                                expected = 200 if pattern == '/transactions' else 503
                                _, body = get(route, expected)
                                if expected == 503:
                                    assert f'{network} fixture API reached' in json.loads(body)['error']['message'], (route, body)
                                matrix_count += 1
                    assert server.requests
                _, body = get('/assets/missing.js?format=md', 404, 'text/markdown')
                assert body == 'not found'
                _, body = get('/api/mainnet/v1/blocks/42', accept='text/markdown')
                assert json.loads(body)['fixtureNetwork'] == 'mainnet'
                print(f'PASS: embedded binary, {matrix_count} network/route/profile combinations, negotiation, discovery, debugger, HEAD, HTML, errors and HTTPS origin')
                if args.benchmark_against:
                    baseline = Path(args.benchmark_against).resolve(strict=True)
                    baseline_root = root / 'baseline'
                    baseline_root.mkdir()
                    with socket.socket() as socket_:
                        socket_.bind(('127.0.0.1', 0))
                        baseline_port = socket_.getsockname()[1]
                    (baseline_root / 'ckbadger.toml').write_text((root / 'ckbadger.toml').read_text().replace(
                        f'port = {port}', f'port = {baseline_port}'))
                    for network in servers:
                        (baseline_root / network).mkdir()
                        (baseline_root / network / 'config.toml').write_text((root / network / 'config.toml').read_text())

                    def measure_html(server_port):
                        samples = []
                        connection = http.client.HTTPConnection('127.0.0.1', server_port, timeout=5)
                        try:
                            for index in range(220):
                                started = time.perf_counter_ns()
                                connection.request('GET', '/mainnet/blocks/42', headers={'Accept': 'text/html'})
                                reply = connection.getresponse()
                                assert reply.status == 200 and b'<html' in reply.read().lower()
                                if index >= 20:
                                    samples.append((time.perf_counter_ns() - started) / 1_000_000)
                        finally:
                            connection.close()
                        return {'median_ms': round(statistics.median(samples), 3), 'p95_ms': round(sorted(samples)[189], 3)}

                    before = subprocess.Popen([str(baseline), '-C', str(baseline_root), 'internal', 'frontend-server'],
                        cwd=baseline_root, stdout=log, stderr=subprocess.STDOUT)
                    try:
                        for _ in range(200):
                            try:
                                with socket.create_connection(('127.0.0.1', baseline_port), timeout=0.1):
                                    break
                            except OSError:
                                time.sleep(0.05)
                        else:
                            raise AssertionError('Baseline frontend did not start')
                        print('HTML latency (200 requests, keep-alive, loopback):', json.dumps({
                            'before': measure_html(baseline_port), 'after': measure_html(port),
                            'binary_bytes_before': baseline.stat().st_size, 'binary_bytes_after': binary.stat().st_size,
                        }))
                    finally:
                        before.terminate()
                        before.wait(timeout=10)
            finally:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


if __name__ == '__main__':
    main()
