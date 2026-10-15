#!/usr/bin/env python3
"""TCG acceptance: real cooperative switches, lifecycle and task-stack faults."""
import argparse
import json
from pathlib import Path
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from lab import controller as lab

VARIANTS = ('tasks', 'task0-low', 'task0-high', 'task1-low', 'task1-high', 'task-overflow')


def stream(vm):
    result = []
    for line in lab.logs(vm)['serial'].splitlines():
        try:
            value = json.loads(line)
        except ValueError:
            continue
        if isinstance(value, dict) and value.get('protocol', '').startswith('eutherus.'):
            result.append(value)
    return result


def inspect(vm, variant):
    observation = lab.observe(vm, timeout=40)
    assert observation['outcome'] == ('pass' if variant == 'tasks' else 'exception'), observation
    events = stream(vm)
    task = [e for e in events if e['protocol'] == 'eutherus.tasks']
    assert task == lab.status(vm)['task_events']
    ready = [e for e in task if e['event'] == 'task_ready']
    assert [e['id'] for e in ready] == [0, 1]
    assert len({e[k] for e in ready for k in ('guard_low', 'guard_high')}) == 4
    for e in ready:
        assert e['stack_bytes'] == 128 * 1024
        assert e['guard_low'] % 4096 == e['guard_high'] % 4096 == 0
        assert e['guard_low'] + 4096 == e['stack_base']
        assert e['stack_base'] + e['stack_bytes'] == e['guard_high']
    assert ready[0]['guard_high'] + 4096 <= ready[1]['guard_low']
    names = [e['event'] for e in events]
    assert names.index('paging_ready') < names.index('timer_after_switch') < names.index('task_ready') < names.index('scheduler_ready')
    assert [e['event'] for e in task[:3]] == ['task_ready', 'task_ready', 'scheduler_ready']
    assert task[2]['policy'] == 'cooperative_round_robin' and task[2]['tasks'] == 2
    progress = [e for e in task if e['event'] == 'task_progress']
    assert progress, 'fault or completion before real task progress'
    for e in progress:
        owner = ready[e['id']]
        assert owner['stack_base'] <= e['stack_address']
        assert e['stack_address'] + 256 <= owner['guard_high']
        assert all(e[k] is True for k in ('registers_preserved', 'stack_preserved', 'controls_preserved', 'if_preserved'))
    assert all(a['ticks'] < b['ticks'] for a, b in zip(progress, progress[1:]))
    if variant == 'tasks':
        assert [(e['id'], e['round']) for e in progress] == [(i, r) for r in range(1, 6) for i in (0, 1)] + [(0, r) for r in range(6, 9)]
        expected = ['task_ready', 'task_ready', 'scheduler_ready']
        expected += ['task_progress'] * 10 + ['task_finished']
        expected += ['task_progress'] * 3 + ['task_finished', 'tasks_complete']
        assert [e['event'] for e in task] == expected
        assert [e['id'] for e in task if e['event'] == 'task_finished'] == [1, 0]
        assert task[-1]['dispatches'] == 15 and task[-1]['finished'] == 2
        assert names.index('tasks_complete') < names.index('boot_success')
        assert not any(e['event'] in ('panic', 'exception') for e in events)
    else:
        assert 'tasks_complete' not in names and 'boot_success' not in names
        fault = next(e for e in events if e['event'] == 'exception')
        id_ = 0 if variant.startswith('task0') else 1
        # Both tasks actually ran; the chosen task has survived two yields.
        assert progress[-1]['id'] == id_ and progress[-1]['round'] == 2
        assert {e['id'] for e in progress} == {0, 1}
        owner = ready[id_]
        if variant == 'task-overflow':
            assert fault['vector'] == 8 and fault['error_code'] == 0 and fault['emergency_stack']
            assert owner['guard_low'] <= fault['cr2'] < owner['stack_base']
            cpu = next(e for e in events if e['event'] == 'tables_ready')
            assert cpu['emergency_base'] <= fault['handler_rsp'] < cpu['emergency_base'] + cpu['stack_bytes']
        else:
            assert fault['vector'] == 14 and fault['error_code'] == 2 and not fault['emergency_stack']
            assert fault['cr2'] == owner['guard_low' if variant.endswith('low') else 'guard_high']
            assert owner['stack_base'] <= fault['handler_rsp'] < owner['guard_high']
        assert lab.alive(lab.load_vm(vm)), 'captured exception must not triple fault'
    return {'events': events, 'observation': observation}


def case(variant, manifest):
    names = ['tsk-' + uuid.uuid4().hex[:8] for _ in range(2 if variant == 'tasks' else 1)]
    result = {'name': variant, 'build': manifest, 'passed': False, 'runs': []}
    try:
        states = [lab.boot(n, variant=variant, build_id=manifest['build_id']) for n in names]
        assert all(s['config']['accel'] == 'tcg' for s in states)
        for n in names:
            result['runs'].append(inspect(n, variant))
        if variant == 'tasks':
            assert states[0]['pid'] != states[1]['pid']
            assert states[0]['qmp'] != states[1]['qmp']
            for fresh in (False, True):
                lab.reset(names[0], fresh=fresh)
                result['runs'].append(inspect(names[0], variant))
                assert lab.alive(lab.load_vm(names[1]))
                assert lab.observe(names[1])['outcome'] == 'pass'
            lab.stop(names[0])
            assert lab.alive(lab.load_vm(names[1]))
            result['isolation_warm_and_fresh_reset'] = True
        result['passed'] = True
    except Exception as error:
        result['error'] = repr(error)
    finally:
        for n in names:
            if (lab.vm_dir(n) / 'state.json').exists():
                result.setdefault('logs', {})[n] = lab.logs(n)
                lab.stop(n)
                lab.clean(n)
        result['cleanup_passed'] = all(not lab.vm_dir(n).exists() for n in names)
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
    baseline = manifests['tasks']
    assert all(m['source'] == baseline['source'] for m in manifests.values())
    for filename in ('eutherboot.efi', 'OVMF_CODE.fd', 'OVMF_VARS.fd'):
        assert all(m['files'][filename] == baseline['files'][filename] for m in manifests.values())
    report = {'schema': 1, 'suite': 'tasks', 'cases': []}
    for v in VARIANTS:
        result = case(v, manifests[v])
        report['cases'].append(result)
        print(v + ': ' + ('PASS' if result['passed'] else result.get('error', 'FAIL')), file=sys.stderr, flush=True)
    report['passed'] = all(c['passed'] for c in report['cases'])
    path = lab.ARTIFACTS / 'reports' / ('tasks-' + time.strftime('%Y%m%d-%H%M%S') + '-' + uuid.uuid4().hex[:6] + '.json')
    lab.write_json(path, report)
    summary = {'passed': report['passed'], 'report': str(path)}
    lab.write_json(lab.ARTIFACTS / 'tasks-latest.json', summary)
    print(json.dumps(summary, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
