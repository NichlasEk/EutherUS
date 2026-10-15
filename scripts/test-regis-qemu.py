#!/usr/bin/env python3
"""Real TCG graphics/input tasks, live telemetry, pixels and lifecycle acceptance."""
import hashlib
import json
from pathlib import Path
import shutil
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from lab import controller as lab


def frames(vm):
    result = []
    for line in lab.logs(vm)['serial'].splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if isinstance(event, dict) and event.get('protocol') == 'eutherus.graphics' and event.get('event') == 'frame_presented':
            result.append(event)
    return result


def wait_frame(vm, after=-1, paused=None):
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        assert lab.alive(lab.load_vm(vm)), 'VM exited'
        assert not any(e['event'] == 'panic' for e in lab.events(lab.logs(vm)['serial'])), 'kernel panic'
        assert not any(e.get('event') == 'exception' for e in lab.events(lab.logs(vm)['serial'], 'eutherus.cpu')), 'CPU exception'
        for event in frames(vm):
            if event['frame'] > after and (paused is None or event['paused'] == paused):
                return event
        time.sleep(0.1)
    raise TimeoutError('graphics frame: ' + vm)


def space(vm):
    with lab.QMP(lab.load_vm(vm)['qmp']) as q:
        q.command('send-key', {'keys': [{'type': 'qcode', 'data': 'spc'}], 'hold-time': 150})


def capture(vm, dest):
    # Screenshots may be between presents, so rotation tests compare a large
    # tower region rather than exact expected pixels. Paused tower pixels match;
    # live telemetry must keep changing.
    source = Path(lab.screenshot(vm)['path'])
    shutil.copy2(source, dest)
    data = dest.read_bytes()
    header, pixels = data.split(b'\n255\n', 1)
    assert header.split() == [b'P6', b'1024', b'768']
    assert len(pixels) == 1024*768*3
    tower = b''.join(pixels[(y*1024+380)*3:(y*1024+660)*3] for y in range(188,610))
    branches = b''.join(pixels[(y*1024+x)*3:(y*1024+x+232)*3] for x, start in ((64, 276), (704, 408)) for y in range(start, start+96))
    background = bytes.fromhex('08141d')
    lit = sum(tower[i:i+3] != background for i in range(0,len(tower),3))
    assert lit > 2000, 'missing tower pixels'
    return {'path': str(dest), 'sha256': hashlib.sha256(data).hexdigest(),
            'tower_sha256': hashlib.sha256(tower).hexdigest(), 'tower_lit_pixels': lit,
            'branches_sha256': hashlib.sha256(branches).hexdigest(),
            'branches_lit_pixels': sum(branches[i:i+3] != background for i in range(0, len(branches), 3))}


def task_evidence(vm):
    status = lab.status(vm)
    task = status['task_events']
    ready = [e for e in task if e['event'] == 'task_ready']
    assert [(e['id'], e['name']) for e in ready] == [(0, 'graphics'), (1, 'input')]
    assert ready[0]['guard_high'] + 4096 <= ready[1]['guard_low']
    for owner in ready:
        assert owner['stack_bytes'] == 128 * 1024
        assert owner['stack_base'] == owner['guard_low'] + 4096
        assert owner['guard_high'] == owner['stack_base'] + owner['stack_bytes']
    ordered = []
    for line in lab.logs(vm)['serial'].splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if isinstance(event, dict) and event.get('protocol', '').startswith('eutherus.'):
            ordered.append(event.get('event'))
    sequence = ['allocator_ready', 'paging_ready', 'graphics_allocated', 'boot_success',
                'task_ready', 'scheduler_ready', 'renderer_ready', 'frame_presented', 'input_ready']
    assert [ordered.index(event) for event in sequence] == sorted(ordered.index(event) for event in sequence)
    graphics = lab.events(lab.logs(vm)['serial'], 'eutherus.graphics')
    for name, id_ in [('renderer_ready', 0), ('input_ready', 1)]:
        e = next(e for e in graphics if e['event'] == name)
        assert e['task_id'] == id_
        assert ready[id_]['stack_base'] <= e['rsp'] < ready[id_]['guard_high']
    assert any(e['event'] == 'scheduler_idle' for e in task)
    woke = [e for e in task if e['event'] == 'task_woke']
    assert {e['id'] for e in woke} == {0, 1}
    assert all(e['ticks'] >= e['deadline'] for e in woke)
    assert not any(e['event'] in ('task_finished', 'tasks_complete') for e in task)
    samples = frames(vm)
    assert samples[0]['frame'] == 0
    assert samples[0]['input_polls'] == samples[0]['toggles'] == 0
    for e in samples:
        assert e['gfx_dispatches'] == e['frame'] + 1
        assert e['gfx_frames'] == e['gfx_yields'] == e['frame']
        assert e['input_dispatches'] == e['input_polls'] == e['input_yields']
    for a, b in zip(samples, samples[1:]):
        assert b['input_polls'] > a['input_polls']
        assert b['halt_waits'] > a['halt_waits']
        assert b['timer_ticks'] - a['timer_ticks'] >= (b['frame'] - a['frame']) * 5
    assert len(samples) >= 2 and samples[-1]['input_polls'] > samples[-1]['gfx_dispatches']
    return {'task_events': task, 'graphics_events': graphics}


def main():
    out = ROOT / 'artifacts/regis/qemu' / (time.strftime('%Y%m%d-%H%M%S')+'-'+uuid.uuid4().hex[:6])
    out.mkdir(parents=True)
    names = ['gfx-'+uuid.uuid4().hex[:8] for _ in range(2)]
    manifest = lab.read_json(lab.STATE / 'build-graphics.json')
    report = {'schema': 1, 'passed': False, 'build': manifest, 'vms': [], 'captures': []}
    try:
        for name in names:
            state = lab.boot(name, variant='graphics', build_id=manifest['build_id'])
            report['vms'].append({'name': name, 'config': state['config'], 'qemu_argv': state['argv']})
        first = wait_frame(names[0])
        wait_frame(names[1])
        report['captures'].append(capture(names[0], out/'running-a.ppm'))
        second = wait_frame(names[0], first['frame'])
        report['captures'].append(capture(names[0], out/'running-b.ppm'))
        assert first['angle_milliradians'] != second['angle_milliradians']
        assert second['timer_ticks'] > first['timer_ticks']
        assert second['halt_waits'] > first['halt_waits']
        assert report['captures'][0]['tower_sha256'] != report['captures'][1]['tower_sha256']
        assert all(c['branches_lit_pixels'] > 500 for c in report['captures'])
        space(names[0])
        paused = wait_frame(names[0], second['frame'], True)
        report['captures'].append(capture(names[0], out/'paused-a.ppm'))
        held = wait_frame(names[0], paused['frame'], True)
        report['captures'].append(capture(names[0], out/'paused-b.ppm'))
        assert paused['angle_milliradians'] == held['angle_milliradians']
        assert report['captures'][2]['tower_sha256'] == report['captures'][3]['tower_sha256']
        assert report['captures'][2]['branches_sha256'] != report['captures'][3]['branches_sha256']
        assert held['gfx_dispatches'] > paused['gfx_dispatches'] and held['input_polls'] > paused['input_polls']
        assert held['toggles'] == paused['toggles'] == 1
        other = frames(names[1])[-1]
        other_next = wait_frame(names[1], other['frame'], False)
        assert other_next['angle_milliradians'] != other['angle_milliradians'], 'input leaked across VMs'
        space(names[0])
        resumed = wait_frame(names[0], held['frame'], False)
        assert resumed['angle_milliradians'] != held['angle_milliradians']
        assert resumed['toggles'] == 2
        assert other_next['toggles'] == 0
        report['captures'].append(capture(names[0], out/'resumed.ppm'))
        assert report['captures'][-1]['tower_sha256'] != report['captures'][-2]['tower_sha256']
        for name in names:
            report.setdefault('task_evidence', {})[name] = task_evidence(name)
            result = lab.observe(name)
            assert result['outcome'] == 'pass'
            assert result['events'][-1]['handoff'] == 3
        for name in names:
            memory = lab.status(name)['memory_events']
            ready = next(e for e in memory if e['event'] == 'allocator_ready')
            passed = next(e for e in memory if e['event'] == 'self_test_pass')
            arena = next(e for e in memory if e['event'] == 'graphics_allocated')
            assert arena['pages'] == 1536 and arena['address'] % 4096 == 0
            paging = next(e for e in lab.events(lab.logs(name)['serial'], 'eutherus.paging') if e['event']=='paging_ready')
            assert arena['free_pages'] + arena['pages'] + paging['table_pages'] == ready['free_pages']
            assert passed['restored_pages'] == ready['free_pages']
            for r in memory:
                if r['event'] == 'reserved':
                    assert arena['address']+arena['pages']*4096 <= r['start'] or r['start']+r['bytes'] <= arena['address']
        for fresh in (False, True):
            lab.reset(names[0], fresh=fresh)
            reset_frame = wait_frame(names[0], after=0, paused=False)
            assert reset_frame['toggles'] == 0
            report.setdefault('reset_evidence', []).append(task_evidence(names[0]))
            assert lab.alive(lab.load_vm(names[1]))
            assert wait_frame(names[1], frames(names[1])[-1]['frame'])['toggles'] == 0
        lab.stop(names[0])
        assert lab.alive(lab.load_vm(names[1]))
        report['checks'] = ['boot_handoff_v3', 'allocator_owned_graphics_arena', 'interrupt_timer_progress', 'halt_wait_progress', 'visible_tower', 'rotation', 'keyboard_pause',
                            'stable_paused_tower', 'live_task_branches_while_paused', 'guarded_graphics_input_stacks',
                            'timed_task_sleep_and_scheduler_hlt', 'real_dispatch_work_yield_counters', 'keyboard_resume',
                            'two_concurrent_graphics_vms', 'input_isolation', 'warm_and_fresh_reset', 'isolated_stop']
        report['passed'] = True
    except Exception as error:
        report['error'] = repr(error)
    finally:
        for name in names:
            if (lab.vm_dir(name)/'state.json').exists():
                report.setdefault('logs', {})[name] = lab.logs(name)
                lab.stop(name)
                lab.clean(name)
        report['cleanup_passed'] = all(not lab.vm_dir(n).exists() for n in names)
        report['passed'] &= report['cleanup_passed']
        lab.write_json(out/'report.json', report)
        lab.write_json(ROOT/'artifacts/regis/qemu-latest.json', {'passed': report['passed'], 'report': str(out/'report.json')})
    print(json.dumps({'passed': report['passed'], 'report': str(out/'report.json'), 'error': report.get('error')}, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
