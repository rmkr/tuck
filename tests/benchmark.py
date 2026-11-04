"""Run the real HTTP/DB benchmark. Usage: python3 tests/benchmark.py BINARY."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import math
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import time
from process import BINARY, Server

parser = argparse.ArgumentParser()
parser.add_argument('--samples', type=int, default=100)
parser.add_argument('--cold-trials', type=int, default=5)
parser.add_argument('--sizes', type=int, nargs='+', default=[1000, 10000])
args = parser.parse_args()
if args.cold_trials < 1 or args.samples < 20 or any(size < 1 for size in args.sizes):
    parser.error('use at least 20 samples and positive dataset sizes')


def summary(values):
    return {'count':len(values), 'median_ms':statistics.median(values),
            'p95_ms':sorted(values)[math.ceil(len(values)*.95)-1]}


QUERIES = ['Parser::decode17', 'cache_key42', 'module_9.rs', 'namespace.component3', 'decision']


def query(server, token, index):
    start = time.perf_counter()
    result = server.call('memory_search', {'query':QUERIES[index % len(QUERIES)], 'scopes':['project:a']}, token)
    duration = (time.perf_counter()-start)*1000
    assert isinstance(result, list) and result and len(result) <= 5, result
    return duration


report = {'engine':'turso 0.8.3', 'durability':'WAL, synchronous=FULL, one serialized connection',
          'binary_sha256':hashlib.sha256(Path(BINARY).read_bytes()).hexdigest(),
          'platform':platform.platform(), 'cpu':subprocess.check_output(['sysctl','-n','machdep.cpu.brand_string'], text=True).strip(),
          'dataset':'Short sourced decisions in 100 groups; four identifier queries and one broad keyword query; all seeding via MCP.',
          'cold_definition':'Five process restarts by default, first wave of queries after each. OS file cache is retained.', 'results':[]}
for size in args.sizes:
    server = Server()
    try:
        server.init()
        credentials = [server.grant(f'bench{i}', ['project:a']) for i in range(4)]
        server.start()
        for index in range(size):
            content = f'decision {index}: Parser::decode{index%100} uses cache_key{index%100} in module_{index%100}.rs and namespace.component{index%100}.'
            saved = server.save(f'bench-{index}', content)
            assert 'error' not in saved, saved
        print(f'Seeded {size} memories', file=sys.stderr)
        for clients in [1,4]:
            cold = []
            for trial in range(args.cold_trials):
                server.stop()
                server.start()
                with ThreadPoolExecutor(clients) as pool:
                    cold.extend(pool.map(lambda i:query(server, credentials[i%clients], i+trial), range(clients)))
            with ThreadPoolExecutor(clients) as pool:
                list(pool.map(lambda i:query(server, credentials[i%clients], i), range(20)))
                warm = list(pool.map(lambda i:query(server, credentials[i%clients], i), range(args.samples)))
            report['results'].append({'memories':size,'clients':clients,'cold':summary(cold),'warm':summary(warm),'warm_by_query':{query:summary(warm[index::len(QUERIES)]) for index,query in enumerate(QUERIES)}})
    finally:
        server.close()
print(json.dumps(report, indent=2))
