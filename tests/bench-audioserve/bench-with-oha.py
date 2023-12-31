import argparse
import base64
from collections import namedtuple
import hashlib
import json
import os
from pprint import pprint
import socket
import subprocess
import sys

try:
    import requests
except ImportError:
    print('requests not found, please install it', file=sys.stderr)
    exit(1)

try:
    from tabulate import tabulate
except ImportError:
    print('tabulate not found, please install it', file=sys.stderr)
    exit(1)

HOME = os.path.expanduser('~')

TestResult = namedtuple('TestResult', ['success_rate', 'req_per_sec', 'average_time', 'median_time', 'max_time', 'min_time', 'status_codes'])
TestParams = namedtuple('TestParams', ['path', 'https', 'http2', 'compress', 'token'])

TESTS = [
    # Important - each suite must contain same https and compress values, as it requires restart of audioserve
    [
        TestParams(path='/',compress=False, https=False, token=False, http2=False),
        TestParams(path='/',compress=False, https=False, token=False, http2=True),
        TestParams(path='/0/folder/',compress=False, https=False, token=False, http2=False),
        TestParams(path='/0/folder/',compress=False, https=False, token=False, http2=True),
        TestParams(path='/0/folder/',compress=False, https=False, token=True, http2=False),
        TestParams(path='/0/folder/',compress=False, https=False, token=True, http2=True),
    ],
    [
        TestParams(path='/0/folder/',compress=True, https=False, token=True, http2=False),
        TestParams(path='/0/folder/',compress=True, https=False, token=True, http2=True),

    ]
]

def parse_arguments():
    parser = argparse.ArgumentParser()
    parser.add_argument('--test-duration', type=int, default=10, help='Test duration in seconds')
    parser.add_argument('--num-tests', type=int, default=1, help='Number of tests')
    parser.add_argument('--test-address', type=str, default='127.0.0.1', help='Test audioserve address to listen on')
    parser.add_argument('--test-port', type=int, default=3003, help='Test audioserve port')
    parser.add_argument('--audioserve-path', type=str, default=f'{HOME}/workspace/audioserve', help='Path to audioserve')
    parser.add_argument('--audioserve-https-cert', type=str, default=f'{HOME}/.audioserve/ssl/certificate.pem', help='Path to audioserve HTTPS cert')
    parser.add_argument('--audioserve-https-key', type=str, default=f'{HOME}/.audioserve/ssl/key.pem', help='Path to audioserve HTTPS key')
    parser.add_argument('--audioserve-client-dir', type=str, default=f'{HOME}/workspace/audioserve-web/dist', help='Path to audioserve client dir')
    parser.add_argument('--audioserve-token', type=str, default='test', help='Audioserve token')
    parser.add_argument('--audioserve-verbose', action='store_true', help='Audioserve verbose - logs are printed to stderr')
    parser.add_argument('--audio-collection', type=str, default=f'{HOME}/test_audiobooks', help='Path to audio collection')
    return parser.parse_args()

def oha_exists():
        try:
            p = subprocess.run(['oha', '--version'], capture_output=True, check=True)
            output = p.stdout.decode('utf-8')
            return output.startswith('oha')
        except subprocess.CalledProcessError:
            return False

# Test port is listening
def test_port_is_listening(args):
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.settimeout(5)  # Timeout for the operation
    try:
        result = sock.connect_ex((args.test_address, args.test_port))
        sock.close()
        return result == 0  # Returns True if port is open/listening
    except socket.gaierror:
        return False  # Returns False if host is unreachable


def start_audioserve(args, compress=False, https=False):
    cmd = ['cargo', 'run', '--release', '--', '--listen', f'{args.test_address}:{args.test_port}', 
           '--client-dir', args.audioserve_client_dir, 
           '--force-cache-update', args.audio_collection]
    if compress:
        cmd.append('--compress-responses')
    if https:
        cmd.extend(['--ssl-cert', args.audioserve_https_cert, '--ssl-key', args.audioserve_https_key])
    env= os.environ.copy()
    env["RUST_LOG"] = "audioserve=info"
    stderr = subprocess.DEVNULL if not args.audioserve_verbose else None
    proc = subprocess.Popen(cmd, env=env, stderr=stderr)
    return proc

def run_test(args, params: TestParams, token):
    protocol = 'https' if params.https else 'http'
    url = f'{protocol}://{args.test_address}:{args.test_port}'
    url += params.path

    cmd = ['oha', '-z', str(args.test_duration)+'s', '--no-tui', '--json', '--insecure']
    if params.http2:
        cmd.append('--http2')
    if params.token:
        cmd.append('-H')
        cmd.append(f'Cookie: audioserve_token={token}')
    cmd.append(url)
    results = []
    for _ in range(args.num_tests):
        res = run_single_test_iteration(cmd)
        results.append(res)
    return combine_results(results)

def combine_results(results):
    success_rate = sum(r.success_rate for r in results) / len(results)
    req_per_sec = sum(r.req_per_sec for r in results) / len(results)
    average_time = sum(r.average_time for r in results) / len(results)
    median_time = sum(r.median_time for r in results) / len(results)
    max_time = sum(r.max_time for r in results) / len(results)
    min_time = sum(r.min_time for r in results) / len(results)
    status_codes = {}
    for r in results:
        for (code, count) in r.status_codes.items():
            if code in status_codes:
                status_codes[code] += count
            else:
                status_codes[code] = count
    return TestResult(success_rate=success_rate, req_per_sec=req_per_sec, average_time=average_time, 
                median_time=median_time, max_time=max_time, min_time=min_time, status_codes=status_codes)

def run_single_test_iteration(cmd):
    res = subprocess.run(cmd, capture_output=True, check=True)
    output = res.stdout.decode('utf-8')
    data = json.loads(output)
    return extract_results(data)

def check_env(args):
    if not os.path.isdir(args.audioserve_path):
        print(f'audioserve path {args.audioserve_path} is not a directory', file=sys.stderr)
        exit(1)
    if not oha_exists():
        print('oha not found, please install it', file=sys.stderr)
        exit(1)
    if not os.environ.get('AUDIOSERVE_SHARED_SECRET'):
        print('AUDIOSERVE_SHARED_SECRET not set, please set it', file=sys.stderr)
        exit(1)
    if not os.path.isdir(args.audio_collection):
        print('audio collection path is not a directory', file=sys.stderr)
        exit(1)
    if not os.path.isfile(args.audioserve_https_cert):
        print('audioserve HTTPS cert not found, please generate it', file=sys.stderr)
        exit(1)
    if not os.path.isfile(args.audioserve_https_key):
        print('audioserve HTTPS key not found, please generate it', file=sys.stderr)
        exit(1)

def extract_results(data):
    summary = data["summary"]
    result = TestResult(success_rate=summary["successRate"], req_per_sec=summary["requestsPerSec"], 
                average_time=summary["average"], max_time=summary["slowest"], min_time=summary["fastest"],
                median_time=data["latencyPercentiles"]["p50"], status_codes=data["statusCodeDistribution"])
    return result

def print_results(args, results, title=None):
    headers = ["protocol"]
    http1_results = {"protocol": "http/1.1"}
    http2_results = {"protocol": "http/2.0"}
    for (result, test) in results:
        name = test.path + (' 401' if not test.token and test.path != "/" else '') + (' no gzip' if not test.compress and test.token else '')
        if not name in headers:
            headers.append(name)
        if test.http2:
            http2_results[name] = result.req_per_sec
        else:
            http1_results[name] = result.req_per_sec
    print()
    if title:
        print(title)   
    print(tabulate([http1_results, http2_results], headers="keys", tablefmt="github"))

def run_test_suite(args, suite, token):
    first_test = suite[0]
    proc = start_audioserve(args, compress=first_test.compress, https=first_test.https)
    results = []
    while not test_port_is_listening(args):
        pass
    try:
        for (n,test) in enumerate(suite):
            print(f'\tRunning test {n+1}/{len(suite)} - {test}')
            res = run_test(args, test, token)
            results.append((res, test))
    finally:
        proc.terminate()
    proc.wait()
    return results

def encode_secret(secret):
    secret_bytes = secret.encode('utf-8')
    random_bytes = os.urandom(32)
    concated_bytes = secret_bytes + random_bytes
    digest = hashlib.sha256(concated_bytes).digest()
    final_secret = base64.b64encode(random_bytes).decode() + "|" + base64.b64encode(digest).decode()
    return final_secret

def retrieve_token(args):
    proc = start_audioserve(args)
    while not test_port_is_listening(args):
        pass
    secret = None
    try:
        secret = os.environ.get('AUDIOSERVE_SHARED_SECRET')
        secret = encode_secret(secret)
        resp = requests.post(f'http://{args.test_address}:{args.test_port}/authenticate', data={'secret': secret})
        resp.raise_for_status()
        secret = resp.text
    finally:
        proc.terminate()
    proc.wait()
    return secret

def run_all(args, token, tests=TESTS, name='Default'):
    results = []
    for (n,suite) in enumerate(tests):
        print(f'Running {name} test suite {n+1}/{len(tests)}')
        if suite:
            res = run_test_suite(args, suite, token)
            results.extend(res)
    return results

def main():
    args = parse_arguments()
    check_env(args)
    os.chdir(args.audioserve_path)
    token = retrieve_token(args)
    if not token:
        print('Failed to retrieve token', file=sys.stderr)
        exit(1)
    res_http = run_all(args, token, name='HTTP')
    https_tests = [[test._replace(https=True) for test in suite] for suite in TESTS]
    res_https = run_all(args, token, https_tests, name='HTTPS')
    print_results(args, res_http, "HTTP")
    print_results(args, res_https, "HTTPS")

if __name__ == "__main__":
    main()
