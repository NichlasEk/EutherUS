#!/usr/bin/env python3
"""Real TCG graphics/keyboard/isolation acceptance; no mocked guest results."""
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
    # tower region rather than exact expected pixels. Paused pixels must match.
    source = Path(lab.screenshot(vm)['path'])
    shutil.copy2(source, dest)
    data = dest.read_bytes()
    header, pixels = data.split(b'\n255\n', 1)
    assert header.split() == [b'P6', b'1024', b'768']
    assert len(pixels) == 1024*768*3
    tower = b''.join(pixels[(y*1024+380)*3:(y*1024+660)*3] for y in range(188,610))
    background = bytes.fromhex('08141d')
    lit = sum(tower[i:i+3] != background for i in range(0,len(tower),3))
    assert lit > 2000, 'missing tower pixels'
    return {'path': str(dest), 'sha256': hashlib.sha256(data).hexdigest(),
            'tower_sha256': hashlib.sha256(tower).hexdigest(), 'tower_lit_pixels': lit}


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
        space(names[0])
        paused = wait_frame(names[0], second['frame'], True)
        report['captures'].append(capture(names[0], out/'paused-a.ppm'))
        held = wait_frame(names[0], paused['frame'], True)
        report['captures'].append(capture(names[0], out/'paused-b.ppm'))
        assert paused['angle_milliradians'] == held['angle_milliradians']
        assert report['captures'][2]['sha256'] == report['captures'][3]['sha256']
        other = frames(names[1])[-1]
        other_next = wait_frame(names[1], other['frame'], False)
        assert other_next['angle_milliradians'] != other['angle_milliradians'], 'input leaked across VMs'
        space(names[0])
        resumed = wait_frame(names[0], held['frame'], False)
        assert resumed['angle_milliradians'] != held['angle_milliradians']
        report['captures'].append(capture(names[0], out/'resumed.ppm'))
        assert report['captures'][-1]['tower_sha256'] != report['captures'][-2]['tower_sha256']
        for name in names:
            result = lab.observe(name)
            assert result['outcome'] == 'pass'
            assert result['events'][-1]['handoff'] == 3
        for name in names:
            memory = lab.status(name)['memory_events']
            ready = next(e for e in memory if e['event'] == 'allocator_ready')
            passed = next(e for e in memory if e['event'] == 'self_test_pass')
            arena = next(e for e in memory if e['event'] == 'graphics_allocated')
            assert arena['pages'] == 1536 and arena['address'] % 4096 == 0
            assert arena['free_pages'] + arena['pages'] == ready['free_pages']
            assert passed['restored_pages'] == ready['free_pages']
            for r in memory:
                if r['event'] == 'reserved':
                    assert arena['address']+arena['pages']*4096 <= r['start'] or r['start']+r['bytes'] <= arena['address']
        report['checks'] = ['boot_handoff_v3', 'allocator_owned_graphics_arena', 'interrupt_timer_progress', 'halt_wait_progress', 'visible_tower', 'rotation', 'keyboard_pause',
                            'stable_paused_pixels', 'keyboard_resume', 'two_concurrent_graphics_vms', 'input_isolation']
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
