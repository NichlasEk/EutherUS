#!/usr/bin/env python3
"""Prove live graphics/input failure paths after boot_success in real TCG."""
import argparse
import json
from pathlib import Path
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from lab import controller as lab
VARIANTS = ('graphics-input-exit', 'graphics-input-guard')


def case(variant, manifest):
    name = 'gft-' + uuid.uuid4().hex[:8]
    result = {'name': variant, 'build': manifest, 'passed': False}
    expected = 'panic' if variant.endswith('exit') else 'exception'
    try:
        state = lab.boot(name, variant=variant, build_id=manifest['build_id'])
        result.update(config=state['config'], qemu_argv=state['argv'])
        assert state['config']['accel'] == 'tcg'
        deadline = time.monotonic() + 40
        while time.monotonic() < deadline:
            assert lab.alive(lab.load_vm(name)), 'unexpected triple fault/termination'
            logs = lab.logs(name)['serial']
            boot = lab.events(logs)
            cpu = lab.events(logs, 'eutherus.cpu')
            if any(e['event'] == expected for e in boot + cpu):
                break
            time.sleep(0.05)
        else:
            raise TimeoutError(expected)
        result['observation'] = lab.observe(name)
        assert result['observation']['outcome'] == expected, 'boot success hid later task failure'
        assert 'boot_success' in [e['event'] for e in boot]
        task = lab.events(logs, 'eutherus.tasks')
        ready = [e for e in task if e['event'] == 'task_ready']
        assert [(e['id'], e['name']) for e in ready] == [(0, 'graphics'), (1, 'input')]
        graphics = lab.events(logs, 'eutherus.graphics')
        live = next(e for e in graphics if e['event'] == 'input_live')
        assert live['polls'] == live['input_dispatches'] == 20 and live['graphics_frames'] >= 2
        assert ready[1]['stack_base'] <= live['rsp'] < ready[1]['guard_high']
        assert any(e['event'] == 'frame_presented' for e in graphics)
        assert not any(e['event'] == 'tasks_complete' for e in task)
        if expected == 'panic':
            assert [e['id'] for e in task if e['event'] == 'task_finished'] == [1]
            assert 'essential graphics task exited' in logs
        else:
            fault = next(e for e in cpu if e['event'] == 'exception')
            assert fault['vector'] == 14 and fault['error_code'] == 2
            assert fault['cr2'] == ready[1]['guard_low'] and not fault['emergency_stack']
            assert ready[1]['stack_base'] <= fault['handler_rsp'] < ready[1]['guard_high']
            assert not any(e['event'] == 'task_finished' for e in task)
        # Fatal task failure must halt the guest, not quietly leave stale graphics running.
        result['registers'] = lab.debug(name, registers=True)['registers']
        assert 'HLT=1' in result['registers']
        before = lab.logs(name)['serial']
        time.sleep(0.15)
        assert lab.logs(name)['serial'] == before
        result['passed'] = True
    except Exception as error:
        result['error'] = repr(error)
    finally:
        if (lab.vm_dir(name) / 'state.json').exists():
            result['logs'] = lab.logs(name)
            lab.stop(name)
            lab.clean(name)
        result['cleanup_passed'] = not lab.vm_dir(name).exists()
        result['passed'] &= result['cleanup_passed']
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--build', action='store_true')
    args = parser.parse_args()
    if args.build:
        for v in VARIANTS:
            print('Building ' + v, file=sys.stderr, flush=True)
            lab.build(variant=v)
    manifests = {v: lab.read_json(lab.STATE / ('build-' + v + '.json')) for v in VARIANTS}
    reference = manifests[VARIANTS[0]]
    assert all(m['source'] == reference['source'] for m in manifests.values())
    for filename in ('eutherboot.efi', 'OVMF_CODE.fd', 'OVMF_VARS.fd'):
        assert all(m['files'][filename] == reference['files'][filename] for m in manifests.values())
    report = {'schema': 1, 'suite': 'graphics-faults', 'cases': []}
    for v in VARIANTS:
        result = case(v, manifests[v])
        report['cases'].append(result)
        print(v + ': ' + ('PASS' if result['passed'] else result.get('error', 'FAIL')), file=sys.stderr, flush=True)
    report['passed'] = all(c['passed'] for c in report['cases'])
    path = lab.ARTIFACTS / 'reports' / ('graphics-faults-' + time.strftime('%Y%m%d-%H%M%S') + '-' + uuid.uuid4().hex[:6] + '.json')
    lab.write_json(path, report)
    summary = {'passed': report['passed'], 'report': str(path)}
    lab.write_json(lab.ARTIFACTS / 'graphics-faults-latest.json', summary)
    print(json.dumps(summary, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
