"""Integration checks through the public JSON-lines API, using actual QEMU."""
import json
from pathlib import Path
import select
import subprocess
import sys
import time
import uuid


def api_case(build_id, timeout):
    from lab import controller as lab
    name = 'api-' + uuid.uuid4().hex[:8]
    result = {'name': 'api_lifecycle', 'vm': name, 'passed': False}
    server = subprocess.Popen([sys.executable, str(lab.ROOT / 'eutherlab'), 'api'],
                              stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE, text=True, cwd=lab.ROOT)
    request_id = 0

    def request(method, *, expected_ok=True, **params):
        nonlocal request_id
        request_id += 1
        server.stdin.write(json.dumps({'version': 1, 'id': request_id,
                                      'method': method, 'params': {'vm': name, **params}}) + '\n')
        server.stdin.flush()
        if not select.select([server.stdout], [], [], 30)[0]:
            raise TimeoutError('API response timeout: ' + method)
        reply = json.loads(server.stdout.readline())
        if reply.get('id') != request_id or reply.get('ok') is not expected_ok:
            raise RuntimeError('unexpected API response: ' + str(reply))
        return reply.get('result', reply.get('error'))

    try:
        state = request('boot', build_id=build_id, paused=True)
        result['build'] = state['build']
        request('boot', expected_ok=False, build_id=build_id)
        request('clean', expected_ok=False)
        result['duplicate_name_rejected'] = True
        result['live_cleanup_rejected'] = True
        # Attaching GDB to a paused VM must preserve the caller's pause state.
        result['debug'] = request('debug', registers=True, probe=True)
        if request('status')['qemu']['running']:
            raise RuntimeError('debug probe unexpectedly resumed paused VM')
        request('reset')
        deadline = time.monotonic() + timeout
        while True:
            state = request('status')
            if [e.get('event') for e in state['events']] == lab.EVENTS:
                break
            if not state['alive'] or time.monotonic() >= deadline:
                raise RuntimeError('API VM failed to boot')
            time.sleep(0.05)
        result['events'] = state['events']
        result['logs'] = request('logs')
        screen = request('screenshot')
        with Path(screen['path']).open('rb') as f:
            if f.read(2) != b'P6':
                raise RuntimeError('invalid QMP screenshot')
        result['screenshot_sha256'] = screen['sha256']
        capture = request('crash')
        if not capture['alive'] or 'registers' not in capture.get('debug', {}):
            raise RuntimeError('crash inspection did not capture live CPU state')
        request('stop')
        if request('status')['alive']:
            raise RuntimeError('API stop left QEMU alive')
        request('stop')
        request('clean')
        request('clean')
        result['passed'] = True
    except Exception as error:
        result['error'] = str(error)
    finally:
        server.stdin.close()
        try:
            server.wait(timeout=5)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait()
        result['api_stderr'] = server.stderr.read()
        server.stdout.close()
        server.stderr.close()
        if (lab.vm_dir(name) / 'state.json').exists():
            try:
                lab.stop(name)
                lab.clean(name)
            except Exception as error:
                result['cleanup_error'] = str(error)
        result['cleanup_passed'] = not lab.vm_dir(name).exists()
        result['passed'] &= result['cleanup_passed']
    return result
