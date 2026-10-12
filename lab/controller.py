"""Local JSON CLI/API. No shell execution, host devices, or network endpoints."""
from __future__ import annotations

import argparse
import concurrent.futures
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import subprocess
import sys
import tarfile
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
STATE = ROOT / '.lab'
ARTIFACTS = ROOT / 'artifacts'
TARGET = 'x86_64-unknown-uefi'
BASE_VARIANTS = ('normal', 'panic', 'timeout', 'crash')
VARIANTS = BASE_VARIANTS + ('graphics', 'interrupts', 'invalid-opcode', 'page-fault', 'general-protection', 'double-fault', 'timer-masked', 'memory', 'memory-empty', 'memory-invalid', 'paging', 'guard-low', 'guard-high', 'emergency-low', 'emergency-high', 'stack-overflow')
EVENTS = ['uefi_entry', 'boot_services_exited', 'memory_map', 'boot_success']
LOCKS = threading.local()


def run(args, *, env=None, cwd=ROOT, timeout=600):
    p = subprocess.run([str(x) for x in args], cwd=cwd, env=env,
                       text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
    if p.returncode:
        raise RuntimeError(f'{args[0]} failed ({p.returncode}): {p.stderr[-6000:]} {p.stdout[-1000:]}')
    return p.stdout.strip()


def sha(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def write_json(path, obj):
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + '.' + uuid.uuid4().hex + '.tmp')
    tmp.write_text(json.dumps(obj, indent=2) + '\n')
    os.replace(tmp, path)


def read_json(path):
    return json.loads(path.read_text())


def identity(repo):
    def git(*args):
        return run(['git', '-C', repo, *args])
    try:
        commit = git('rev-parse', 'HEAD')
    except RuntimeError:
        commit = None
    files = git('ls-files', '-co', '--exclude-standard', '-z').split('\0')
    h = hashlib.sha256()
    for name in sorted(set(filter(None, files))):
        path = repo / name
        if path.is_file():
            h.update(name.encode() + b'\0' + bytes.fromhex(sha(path)))
    return {'commit': commit, 'dirty': bool(git('status', '--porcelain')),
            'source_sha256': h.hexdigest()}


@contextlib.contextmanager
def lock(name):
    if not re.fullmatch(r'[a-zA-Z0-9_-]{1,40}', name):
        raise ValueError('invalid lock/VM name')
    held = getattr(LOCKS, 'held', set())
    if name in held:
        yield
        return
    directory = STATE / 'locks'
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / (name + '.lock')).open('a') as f:
        fcntl.flock(f, fcntl.LOCK_EX)
        LOCKS.held = held
        held.add(name)
        try:
            yield
        finally:
            held.remove(name)


def rust_env():
    env = os.environ.copy()
    local = ROOT / '.tools/cargo/bin'
    if (local / 'cargo').exists():
        env['CARGO_HOME'] = str(ROOT / '.tools/cargo')
        env['RUSTUP_HOME'] = str(ROOT / '.tools/rustup')
        env['PATH'] = str(local) + os.pathsep + env['PATH']
    return env


def build(eutherboot=None, variant='normal', **_):
    if variant not in VARIANTS:
        raise ValueError('unknown build variant')
    repo = Path(eutherboot or os.environ.get('EUTHERBOOT_SOURCE', ROOT.parent / 'EutherBoot')).resolve()
    if not (repo / 'crates/uefi-app/Cargo.toml').is_file():
        raise ValueError('EutherBoot source required: --eutherboot PATH')
    with lock('build'):
        env = rust_env()
        env['CARGO_TARGET_DIR'] = str(ROOT / 'target')
        source = identity(ROOT)
        boot_source = identity(repo)
        args = ['cargo', 'build', '--locked', '--release', '--target', TARGET, '-p', 'eutherus-kernel']
        if variant != 'normal':
            args += ['--features', variant if variant in ('graphics', 'interrupts', 'memory', 'paging') else 'fault-' + variant]
        run(args, env=env)
        build_id = uuid.uuid4().hex[:16]
        dest = ARTIFACTS / 'builds' / build_id
        dest.mkdir(parents=True)
        shutil.copy2(ROOT / 'target' / TARGET / 'release/eutherus-kernel.efi', dest / 'kernel.efi')
        pdb = ROOT / 'target' / TARGET / 'release/eutherus_kernel.pdb'
        if pdb.exists():
            shutil.copy2(pdb, dest / 'kernel.pdb')
        # Compile a source snapshot: never write to the existing EutherBoot checkout.
        snapshot = ROOT / 'target/eutherboot-source'
        if snapshot.exists():
            shutil.rmtree(snapshot)
        snapshot.mkdir()
        for name in ('Cargo.toml', 'Cargo.lock', 'LICENSE'):
            shutil.copy2(repo / name, snapshot / name)
        shutil.copytree(repo / 'crates', snapshot / 'crates', ignore=shutil.ignore_patterns('target'))
        if identity(repo) != boot_source:
            raise RuntimeError('EutherBoot source changed during snapshot; retry build')
        run(['cargo', 'build', '--locked', '--manifest-path', snapshot / 'Cargo.toml',
             '--release', '--target', TARGET, '-p', 'eutherboot', '--features', 'qemu-debug'], env=env)
        shutil.copy2(ROOT / 'target' / TARGET / 'release/eutherboot.efi', dest / 'eutherboot.efi')
        with tarfile.open(dest / 'eutherboot-source.tar.gz', 'w:gz') as archive:
            for path in sorted(snapshot.rglob('*')):
                if path.is_file():
                    archive.add(path, arcname=str(path.relative_to(snapshot)))
        code = Path(os.environ.get('EUTHERLAB_OVMF_CODE', '/usr/share/edk2/x64/OVMF_CODE.4m.fd'))
        variables = Path(os.environ.get('EUTHERLAB_OVMF_VARS', '/usr/share/edk2/x64/OVMF_VARS.4m.fd'))
        shutil.copy2(code, dest / 'OVMF_CODE.fd')
        shutil.copy2(variables, dest / 'OVMF_VARS.fd')
        cfg = dest / 'eutherboot.toml'
        cfg.write_text('''[boot]
default = "eutherus"
timeout = 1
theme = "text"

[[entry]]
id = "eutherus"
name = "EutherUS"
type = "efi"
path = '\\EFI\\EutherUS\\kernel.efi'
''')
        # Disposable superfloppy ESP: OVMF removable-media path, no host mount.
        raw = dest / 'esp.img'
        with raw.open('wb') as f:
            f.truncate(64 * 1024 * 1024)
        run(['mkfs.fat', '-F', '32', '-n', 'EUTHERUS', raw])
        run(['mmd', '-i', raw, '::/EFI', '::/EFI/BOOT', '::/EFI/EutherBoot', '::/EFI/EutherUS'])
        for src, target in [('eutherboot.efi', 'BOOT/BOOTX64.EFI'),
                            ('kernel.efi', 'EutherUS/kernel.efi'), ('eutherboot.toml', 'EutherBoot/eutherboot.toml')]:
            run(['mcopy', '-i', raw, dest / src, '::/EFI/' + target])
        run(['qemu-img', 'convert', '-f', 'raw', '-O', 'qcow2', raw, dest / 'base.qcow2'])
        raw.unlink()
        if identity(ROOT) != source:
            raise RuntimeError('EutherUS source changed during build; retry for consistent provenance')
        metadata = {
            'schema': 1, 'build_id': build_id, 'variant': variant,
            'source': source, 'eutherboot_source': boot_source,
            'toolchain': run(['rustc', '--version', '--verbose'], env=env),
            'qemu_version': run(['qemu-system-x86_64', '--version']),
            'firmware': {'source_code': str(code), 'source_vars': str(variables),
                         'version': firmware_version(), 'identity': 'SHA-256 in files'},
            'files': {p.name: sha(p) for p in dest.iterdir() if p.is_file()},
            'directory': str(dest), 'target': TARGET,
        }
        write_json(dest / 'manifest.json', metadata)
        for path in dest.iterdir():
            path.chmod(0o444)
        write_json(STATE / ('build-' + variant + '.json'), metadata)
        return metadata


def firmware_version():
    try:
        return run(['pacman', '-Q', 'edk2-ovmf'])
    except (RuntimeError, FileNotFoundError):
        return 'See content SHA-256; distribution package version unavailable'


def vm_dir(vm):
    if not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9_-]{0,23}', vm):
        raise ValueError('VM name must be 1-24 ASCII letters, digits, underscore or hyphen')
    path = STATE / 'vms' / vm
    if path.is_symlink():
        raise ValueError('VM directory cannot be a symlink')
    return path


def proc_identity(pid):
    try:
        stat = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
        if stat[0] == 'Z':
            return None
        return stat[19]  # field 22: process start ticks, after comm field
    except (FileNotFoundError, ProcessLookupError):
        return None


def alive(s):
    return proc_identity(s['pid']) == s['process_start'] and s['process_start'] is not None


class QMP:
    def __init__(self, path):
        self.events = []
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(5)
        try:
            self.sock.connect(str(path))
            self.file = self.sock.makefile('rwb')
            greeting = self._read()
            if 'QMP' not in greeting:
                raise RuntimeError('invalid QMP greeting')
            self.command('qmp_capabilities')
        except Exception:
            self.sock.close()
            raise

    def _read(self):
        line = self.file.readline()
        if not line:
            raise RuntimeError('QMP disconnected')
        return json.loads(line)

    def command(self, command, arguments=None):
        tag = uuid.uuid4().hex
        self.file.write((json.dumps({'execute': command, 'arguments': arguments or {}, 'id': tag}) + '\n').encode())
        self.file.flush()
        while True:
            reply = self._read()
            if 'event' in reply:
                self.events.append(reply)
            if reply.get('id') != tag:
                continue
            if 'error' in reply:
                raise RuntimeError(f'QMP {command}: {reply["error"]}')
            return reply['return']

    def wait_event(self, name):
        while True:
            for index, event in enumerate(self.events):
                if event.get('event') == name:
                    return self.events.pop(index)
            reply = self._read()
            if 'event' in reply:
                self.events.append(reply)

    def close(self):
        self.file.close()
        self.sock.close()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def load_vm(vm):
    return read_json(vm_dir(vm) / 'state.json')


def boot(vm='kernel-test', variant='normal', paused=False, accel='tcg', known_good=False, build_id=None, **_):
    directory = vm_dir(vm)
    if variant not in VARIANTS:
        raise ValueError('unknown build variant')
    if ',' in str(ROOT):
        raise ValueError('workspace path must not contain commas (QEMU option separator)')
    if accel not in ('tcg', 'kvm'):
        raise ValueError('accel must be tcg or kvm')
    with lock('vm-' + vm):
        if directory.exists():
            raise ValueError('VM already allocated; use stop then clean, or reset --fresh')
        if known_good:
            baseline = read_json(STATE / 'known-good.json')
            if sha(baseline['report']) != baseline['report_sha256']:
                raise ValueError('known-good report hash mismatch')
            manifest = baseline['build']
        elif build_id:
            if not re.fullmatch(r'[a-f0-9]{16}', build_id):
                raise ValueError('invalid build ID')
            manifest = read_json(ARTIFACTS / 'builds' / build_id / 'manifest.json')
        else:
            manifest = read_json(STATE / ('build-' + variant + '.json'))
        base = Path(manifest['directory'])
        for name, expected in manifest['files'].items():
            if sha(base / name) != expected:
                raise RuntimeError('build artifact modified: ' + name)
        directory.mkdir(parents=True, mode=0o700)
        qmp = directory / 'qmp.sock'
        gdb = directory / 'gdb.sock'
        if len(os.fsencode(gdb)) >= 104:
            directory.rmdir()
            raise ValueError('workspace path too long for UNIX sockets; use a shorter worktree path')
        process = None
        try:
            shutil.copy2(base / 'OVMF_VARS.fd', directory / 'vars.fd')
            (directory / 'vars.fd').chmod(0o600)
            run(['qemu-img', 'create', '-f', 'qcow2', '-F', 'qcow2', '-b', base / 'base.qcow2', directory / 'disk.qcow2'])
            token = uuid.uuid4().hex
            args = ['qemu-system-x86_64', '-name', 'eutherus-' + token,
                    '-machine', 'q35,accel=' + accel, '-cpu', 'qemu64' if accel == 'tcg' else 'host',
                    '-m', '256M', '-smp', '1', '-nodefaults', '-no-reboot',
                    '-display', 'none', '-device', 'VGA', '-nic', 'none',
                    '-drive', f'if=pflash,format=raw,readonly=on,file={base / "OVMF_CODE.fd"}',
                    '-drive', f'if=pflash,format=raw,file={directory / "vars.fd"}',
                    '-drive', f'if=none,id=osdisk,format=qcow2,file={directory / "disk.qcow2"}',
                    '-device', 'virtio-blk-pci,drive=osdisk',
                    '-serial', 'file:' + str(directory / 'serial.log'),
                    '-debugcon', 'file:' + str(directory / 'eutherboot.log'),
                    '-global', 'isa-debugcon.iobase=0xe9',
                    '-qmp', f'unix:{qmp},server=on,wait=off',
                    '-gdb', f'unix:{gdb},server=on,wait=off',
                    '-sandbox', 'on,obsolete=deny,elevateprivileges=deny,spawn=deny,resourcecontrol=deny']
            if paused:
                args += ['-S']
            with (directory / 'qemu.log').open('wb') as log:
                process = subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                                           start_new_session=True, cwd=directory)
            s = {'schema': 1, 'vm': vm, 'pid': process.pid, 'process_start': proc_identity(process.pid),
                 'token': token, 'qmp': str(qmp), 'gdb': str(gdb), 'directory': str(directory),
                 'config': {'accel': accel, 'memory_mib': 256, 'cpus': 1, 'network': 'none',
                            'machine': 'q35', 'paused': paused, 'disk': 'virtio-blk-pci'},
                 'argv': args, 'build': manifest, 'started_at': time.time(), 'serial_offset': 0}
            write_json(directory / 'state.json', s)
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError('QEMU startup failed: ' + (directory / 'qemu.log').read_text())
                try:
                    with QMP(qmp) as q:
                        s['qemu_status'] = q.command('query-status')
                    return s
                except (OSError, RuntimeError):
                    time.sleep(0.05)
            raise TimeoutError('QMP startup timeout')
        except Exception:
            if process and process.poll() is None:
                process.terminate()
                process.wait(timeout=10)
            raise


def logs(vm='kernel-test', **_):
    s = load_vm(vm)
    directory = vm_dir(vm)
    contents = {}
    for name in ('serial', 'eutherboot', 'qemu'):
        path = directory / (name + '.log')
        with path.open('rb') as f:
            if name == 'serial':
                f.seek(s.get('serial_offset', 0))
            contents[name] = f.read(4 * 1024 * 1024).decode('utf-8', errors='replace')
    return contents


def events(text, protocol='eutherus.boot'):
    found = []
    for line in text.splitlines():
        try:
            item = json.loads(line)
        except ValueError:
            continue
        if isinstance(item, dict) and item.get('protocol') == protocol and item.get('version') == 1:
            found.append(item)
    return found


def status(vm='kernel-test', **_):
    with lock('vm-' + vm):
        s = load_vm(vm)
        running = alive(s)
        result = {'vm': vm, 'alive': running, 'pid': s['pid'], 'config': s['config'],
                  'build_id': s['build']['build_id']}
        if running:
            with QMP(s['qmp']) as q:
                result['qemu'] = q.command('query-status')
        serial = logs(vm)['serial']
        result['events'] = events(serial)
        result['cpu_events'] = events(serial, 'eutherus.cpu')
        result['memory_events'] = events(serial, 'eutherus.memory')
        result['paging_events'] = events(serial, 'eutherus.paging')
        return result


def stop(vm='kernel-test', **_):
    with lock('vm-' + vm):
        s = load_vm(vm)
        was_alive = alive(s)
        if was_alive:
            try:
                with QMP(s['qmp']) as q:
                    q.command('quit')
            except (OSError, RuntimeError):
                pass
            deadline = time.monotonic() + 5
            while alive(s) and time.monotonic() < deadline:
                time.sleep(0.05)
            if alive(s):
                os.kill(s['pid'], signal.SIGTERM)
                deadline = time.monotonic() + 3
                while alive(s) and time.monotonic() < deadline:
                    time.sleep(0.05)
            if alive(s):
                os.kill(s['pid'], signal.SIGKILL)
                deadline = time.monotonic() + 3
                while alive(s) and time.monotonic() < deadline:
                    time.sleep(0.05)
            if alive(s):
                raise RuntimeError('failed to stop VM')
        try:
            os.waitpid(s['pid'], os.WNOHANG)
        except ChildProcessError:
            pass
        s['stopped_at'] = time.time()
        write_json(vm_dir(vm) / 'state.json', s)
        for name in ('qmp.sock', 'gdb.sock'):
            (vm_dir(vm) / name).unlink(missing_ok=True)
        return {'vm': vm, 'stopped': True, 'was_alive': was_alive}


def clean(vm='kernel-test', **_):
    with lock('vm-' + vm):
        directory = vm_dir(vm)
        if not directory.exists():
            return {'vm': vm, 'clean': True}
        if (directory / 'state.json').exists() and alive(load_vm(vm)):
            raise ValueError('refusing to clean a live VM; stop it first')
        shutil.rmtree(directory)
        return {'vm': vm, 'clean': True}


def reset(vm='kernel-test', fresh=False, **_):
    if fresh:
        with lock('vm-' + vm):
            s = load_vm(vm)
            stop(vm)
            clean(vm)
            return boot(vm, build_id=s['build']['build_id'], accel=s['config']['accel'])
    with lock('vm-' + vm):
        s = load_vm(vm)
        if not alive(s):
            raise ValueError('VM is stopped; use reset --fresh')
        # Old success records must never satisfy the new boot observation.
        with QMP(s['qmp']) as q:
            q.command('stop')
            s['serial_offset'] = (vm_dir(vm) / 'serial.log').stat().st_size
            write_json(vm_dir(vm) / 'state.json', s)
            # -no-reboot also applies to a host-requested reset. Temporarily
            # allow reset, wait for completion, then restore crash visibility.
            q.command('set-action', {'reboot': 'reset'})
            q.command('system_reset')
            q.wait_event('RESET')
            q.command('set-action', {'reboot': 'shutdown'})
            q.command('cont')
        return {'vm': vm, 'reset': True}


def screenshot(vm='kernel-test', **_):
    with lock('vm-' + vm):
        s = load_vm(vm)
        path = vm_dir(vm) / ('screen-' + uuid.uuid4().hex[:8] + '.ppm')
        with QMP(s['qmp']) as q:
            q.command('screendump', {'filename': str(path)})
        return {'path': str(path), 'sha256': sha(path)}


def debug(vm='kernel-test', registers=False, probe=False, **_):
    with lock('vm-' + vm):
        s = load_vm(vm)
        result = {'transport': 'unix', 'endpoint': s['gdb'],
                  'command': ['gdb', '-ex', 'target remote ' + s['gdb']],
                  'image': str(Path(s['build']['directory']) / 'kernel.efi')}
        if registers:
            with QMP(s['qmp']) as q:
                previous = q.command('query-status')['running']
                q.command('stop')
                try:
                    result['registers'] = q.command('human-monitor-command', {'command-line': 'info registers'})
                finally:
                    if previous:
                        q.command('cont')
        if probe:
            with QMP(s['qmp']) as q:
                previous = q.command('query-status')['running']
                try:
                    result['gdb_probe'] = run(['gdb', '-nx', '-batch',
                        '-ex', 'set architecture i386:x86-64', '-ex', 'set remotetimeout 5',
                        '-ex', 'target remote ' + s['gdb'], '-ex', 'info registers rip rsp cr3',
                        '-ex', 'x/4i $rip', '-ex', 'detach'], timeout=20)
                finally:
                    q.command('cont' if previous else 'stop')
        return result


def crash(vm='kernel-test', **_):
    s = load_vm(vm)
    capture = {'vm': vm, 'alive': alive(s), 'logs': logs(vm), 'build': s['build'], 'config': s['config']}
    if capture['alive']:
        try:
            capture['debug'] = debug(vm, registers=True)
        except (OSError, RuntimeError) as error:
            capture['debug_error'] = str(error)
    path = vm_dir(vm) / 'crash.json'
    write_json(path, capture)
    return {'path': str(path), **capture}


def observe(vm, timeout=30):
    started = time.monotonic()
    while True:
        s = load_vm(vm)
        output = logs(vm)
        parsed = events(output['serial'])
        cpu_events = events(output['serial'], 'eutherus.cpu')
        memory_events = events(output['serial'], 'eutherus.memory')
        names = [e['event'] for e in parsed if 'event' in e]
        if 'panic' in names:
            outcome = 'panic'
        elif any(e.get('event') == 'exception' for e in cpu_events):
            outcome = 'exception'
        elif any(e.get('event') == 'out_of_memory' for e in memory_events):
            outcome = 'out_of_memory'
        elif not alive(s):
            outcome = 'crash'
        elif 'boot_success' in names:
            outcome = 'pass' if names == EVENTS and 'EutherUS 0.0.1' in output['serial'] else 'invalid_events'
        elif time.monotonic() - started >= timeout:
            outcome = 'timeout'
        else:
            time.sleep(0.05)
            continue
        exit_code = None
        if not alive(s):
            try:
                pid, wait_status = os.waitpid(s['pid'], os.WNOHANG)
                if pid:
                    exit_code = os.waitstatus_to_exitcode(wait_status)
            except ChildProcessError:
                pass
        return {'outcome': outcome, 'events': parsed, 'cpu_events': cpu_events, 'memory_events': memory_events, 'elapsed_seconds': time.monotonic() - started,
                'logs': output, 'build': s['build'], 'config': s['config'],
                'qemu_argv': s['argv'], 'exit_code': exit_code}


def test_case(variant, timeout=30, build_id=None):
    name = 't-' + variant + '-' + uuid.uuid4().hex[:6]
    expected = {'normal': 'pass', 'panic': 'panic', 'timeout': 'timeout', 'crash': 'crash'}[variant]
    result = {'name': variant, 'vm': name, 'expected': expected, 'passed': False}
    try:
        boot(name, variant=variant, build_id=build_id)
        result.update(observe(name, timeout=timeout))
        names = [event.get('event') for event in result['events']]
        expected_events = EVENTS if variant == 'normal' else EVENTS[:3] + (['panic'] if variant == 'panic' else [])
        result['passed'] = result['outcome'] == expected and names == expected_events
        if result['outcome'] != 'pass':
            result['crash_info'] = crash(name)
    except Exception as error:
        result.update({'outcome': 'harness_error', 'error': str(error), 'passed': False})
    finally:
        if (vm_dir(name) / 'state.json').exists():
            try:
                result.setdefault('logs', logs(name))
                stop(name)
                clean(name)
            except Exception as error:
                result['cleanup_error'] = str(error)
    result['cleanup_passed'] = not vm_dir(name).exists()
    result['passed'] &= result['cleanup_passed']
    return result


def test(suite='smoke', timeout=30, promote=False, **_):
    if suite not in ('smoke', 'full'):
        raise ValueError('suite must be smoke or full')
    if not isinstance(timeout, (int, float)) or not 0 < timeout <= 600:
        raise ValueError('timeout must be between 0 and 600 seconds')
    variants = BASE_VARIANTS if suite == 'full' else ('normal',)
    manifests = {v: read_json(STATE / ('build-' + v + '.json')) for v in variants}
    baseline = manifests['normal']
    if any(m['source'] != baseline['source'] for m in manifests.values()):
        raise ValueError('full suite requires all variants built from identical sources')
    for filename in ('eutherboot.efi', 'OVMF_CODE.fd', 'OVMF_VARS.fd'):
        if any(m['files'][filename] != baseline['files'][filename] for m in manifests.values()):
            raise ValueError('full suite requires identical bootloader and firmware: ' + filename)
    report = {'schema': 1, 'suite': suite, 'started_at': time.time(), 'cases': []}
    report['cases'].append(test_case('normal', timeout, baseline['build_id']))
    from lab.validation import api_case
    report['cases'].append(api_case(baseline['build_id'], timeout))
    # Start both before observing either: guarantees actual overlapping lifetime.
    names = ['parallel-' + uuid.uuid4().hex[:8] for _ in range(2)]
    states = []
    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            states = list(pool.map(lambda name: boot(name, build_id=baseline['build_id']), names))
        overlap = all(alive(s) for s in states)
        resources = ('pid', 'directory', 'qmp', 'gdb')
        isolated = all(states[0][key] != states[1][key] for key in resources)
        isolated &= (vm_dir(names[0]) / 'vars.fd').stat().st_ino != (vm_dir(names[1]) / 'vars.fd').stat().st_ino
        isolated &= (vm_dir(names[0]) / 'disk.qcow2').stat().st_ino != (vm_dir(names[1]) / 'disk.qcow2').stat().st_ino
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            observations = list(pool.map(lambda n: observe(n, timeout), names))
        stop(names[0])
        survivor = alive(states[1])
        screen = screenshot(names[1])
        # Keep screenshot after VM cleanup, together with reports.
        saved = ARTIFACTS / 'screens' / Path(screen['path']).name
        saved.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(screen['path'], saved)
        regs = debug(names[1], registers=True, probe=True)
        reset(names[1])
        reboot = observe(names[1], timeout)
        old_pid = load_vm(names[1])['pid']
        reset(names[1], fresh=True)
        fresh_boot = observe(names[1], timeout)
        fresh_changed_pid = old_pid != load_vm(names[1])['pid']
        report['cases'].append({'name': 'concurrent_isolation', 'overlap': overlap,
                                'independent_resources': bool(isolated), 'survivor_alive': survivor,
                                'resources': [{k: state[k] for k in resources} for state in states],
                                'observations': observations, 'screenshot': str(saved),
                                'registers': regs.get('registers'), 'gdb_probe': regs.get('gdb_probe'), 'reset': reboot,
                                'fresh_reset': fresh_boot, 'fresh_changed_pid': fresh_changed_pid,
                                'passed': overlap and isolated and survivor and reboot['outcome'] == 'pass'
                                and fresh_changed_pid and fresh_boot['outcome'] == 'pass'
                                and all(o['outcome'] == 'pass' for o in observations)})
    except Exception as error:
        report['cases'].append({'name': 'concurrent_isolation', 'passed': False,
                                'outcome': 'harness_error', 'error': str(error)})
    finally:
        for name in names:
            if (vm_dir(name) / 'state.json').exists():
                try:
                    stop(name)
                    clean(name)
                except Exception as error:
                    report['cases'].append({'name': 'cleanup-' + name, 'passed': False, 'error': str(error)})
    report['cases'][-1]['cleanup_passed'] = all(not vm_dir(name).exists() for name in names)
    report['cases'][-1]['passed'] &= report['cases'][-1]['cleanup_passed']
    if suite == 'full':
        for variant in ('panic', 'timeout', 'crash'):
            report['cases'].append(test_case(variant, timeout, manifests[variant]['build_id']))
    report['passed'] = all(c['passed'] for c in report['cases'])
    report['finished_at'] = time.time()
    path = ARTIFACTS / 'reports' / (time.strftime('%Y%m%d-%H%M%S') + '-' + uuid.uuid4().hex[:6] + '.json')
    report['path'] = str(path)
    write_json(path, report)
    if promote:
        manifest = baseline
        if identity(ROOT) != manifest['source']:
            raise ValueError('promotion requires the tested source to match the current checkout')
        if not report['passed'] or suite != 'full' or not manifest['source']['commit'] or manifest['source']['dirty']:
            raise ValueError('promotion requires passing full suite and a build from a clean Git commit; report: ' + str(path))
        write_json(STATE / 'known-good.json', {'build': manifest, 'report': str(path), 'report_sha256': sha(path)})
    return report


COMMANDS = {f.__name__: f for f in (build, boot, stop, reset, status, logs, screenshot, debug, test, clean, crash)}


def dispatch(request):
    if not isinstance(request, dict) or type(request.get('version')) is not int or request.get('version') != 1:
        raise ValueError('request must be an object with version=1')
    operation = request.get('method')
    if operation not in COMMANDS:
        raise ValueError('unknown method')
    params = request.get('params', {})
    if not isinstance(params, dict):
        raise ValueError('params must be an object')
    # Explicit allowlists keep future MCP clients from gaining arbitrary execution.
    allowed = {
        'build': {'eutherboot', 'variant'}, 'boot': {'vm', 'variant', 'paused', 'accel', 'known_good', 'build_id'},
        'reset': {'vm', 'fresh'}, 'test': {'suite', 'timeout', 'promote'},
        'debug': {'vm', 'registers', 'probe'},
    }.get(operation, {'vm'})
    if set(params) - allowed:
        raise ValueError('unknown parameters: ' + str(sorted(set(params) - allowed)))
    for key, value in params.items():
        if key in {'paused', 'fresh', 'registers', 'probe', 'promote', 'known_good'}:
            if type(value) is not bool:
                raise ValueError(key + ' must be boolean')
        elif key == 'timeout':
            if type(value) not in (int, float) or not 0 < value <= 600:
                raise ValueError('timeout must be between 0 and 600 seconds')
        elif not isinstance(value, str):
            raise ValueError(key + ' must be a string')
    return COMMANDS[operation](**params)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    for name in COMMANDS:
        p = sub.add_parser(name)
        if name not in ('build', 'test'):
            p.add_argument('--vm', default='kernel-test')
        if name in ('build', 'boot'):
            p.add_argument('--variant', choices=VARIANTS, default='normal')
        if name == 'build':
            p.add_argument('--eutherboot')
        if name == 'boot':
            p.add_argument('--paused', action='store_true')
            p.add_argument('--known-good', action='store_true')
            p.add_argument('--build-id')
            p.add_argument('--accel', choices=('tcg', 'kvm'), default='tcg')
        if name == 'reset':
            p.add_argument('--fresh', action='store_true')
        if name == 'debug':
            p.add_argument('--registers', action='store_true')
            p.add_argument('--probe', action='store_true')
        if name == 'test':
            p.add_argument('--suite', choices=('smoke', 'full'), default='smoke')
            p.add_argument('--timeout', type=float, default=30)
            p.add_argument('--promote', action='store_true')
    sub.add_parser('api', help='versioned JSON lines on stdin/stdout; no network listener')
    args = vars(parser.parse_args())
    command = args.pop('command')
    if command == 'api':
        for line in sys.stdin:
            req = {}
            try:
                req = json.loads(line)
                result = dispatch(req)
                reply = {'version': 1, 'id': req.get('id'), 'ok': True, 'result': result}
            except Exception as error:
                reply = {'version': 1, 'id': req.get('id') if isinstance(req, dict) else None,
                         'ok': False, 'error': {'type': type(error).__name__, 'message': str(error)}}
            print(json.dumps(reply), flush=True)
        return
    try:
        result = COMMANDS[command](**args)
        print(json.dumps({'version': 1, 'ok': True, 'result': result}, indent=2))
        if command == 'test' and not result['passed']:
            sys.exit(1)
    except Exception as error:
        print(json.dumps({'version': 1, 'ok': False, 'error': {'type': type(error).__name__, 'message': str(error)}}))
        sys.exit(1)
