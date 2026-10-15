#!/usr/bin/env python3
"""TCG inspector acceptance: real keyboard, task state, pixels and isolation."""
import argparse
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


def events(vm):
    return lab.events(lab.logs(vm)['serial'], 'eutherus.graphics')


def wait(vm, event, predicate=lambda e: True):
    deadline = time.monotonic() + 40
    while time.monotonic() < deadline:
        assert lab.alive(lab.load_vm(vm)), 'VM terminated'
        serial = lab.logs(vm)['serial']
        assert not any(e['event'] in ('panic', 'exception') for e in lab.events(serial) + lab.events(serial, 'eutherus.cpu')), serial[-1500:]
        for e in lab.events(serial, 'eutherus.graphics'):
            if e['event'] == event and predicate(e):
                return e
        time.sleep(0.05)
    raise TimeoutError(event)


def frame(vm, after=-1, revision=None, visible=None, paused=None):
    return wait(vm, 'frame_presented', lambda e: e['frame'] > after
                and (revision is None or e['selection_revision'] == revision)
                and (visible is None or e['inspector_visible'] == visible)
                and (paused is None or e['paused'] == paused))


def tap(vm, key):
    with lab.QMP(lab.load_vm(vm)['qmp']) as q:
        q.command('send-key', {'keys': [{'type': 'qcode', 'data': key}], 'hold-time': 80})
    time.sleep(0.12)  # Ensure break before another make of this same key.


def edge(vm, key, down):
    with lab.QMP(lab.load_vm(vm)['qmp']) as q:
        q.command('input-send-event', {'events': [{'type': 'key', 'data': {
            'down': down, 'key': {'type': 'qcode', 'data': key}}}]})


def selection(vm, key, revision, selected, visible=True):
    tap(vm, key)
    value = frame(vm, revision=revision, visible=visible)
    assert value['selected_task'] == selected
    changed = wait(vm, 'inspector_changed', lambda e: e['revision'] == revision)
    assert changed['selected'] == selected and changed['visible'] == visible
    return value


def snapshot(vm, revision, id_, after=-1):
    value = wait(vm, 'inspector_presented', lambda e: e['revision'] == revision and e['id'] == id_ and e['frame'] > after)
    ready = next(e for e in lab.status(vm)['task_events'] if e['event'] == 'task_ready' and e['id'] == id_)
    assert value['name'] == ready['name'] == ('graphics' if id_ == 0 else 'input')
    assert value['stack_base'] == ready['stack_base']
    assert value['stack_end'] == ready['guard_high']
    assert value['guard_low'] == ready['guard_low'] == value['stack_base'] - 4096
    assert value['guard_high'] == ready['guard_high']
    assert value['stack_end'] - value['stack_base'] == ready['stack_bytes'] == 128 * 1024
    sample = wait(vm, 'frame_presented', lambda e: e['frame'] == value['frame'])
    assert sample['selection_revision'] == revision and sample['inspector_visible'] and sample['selected_task'] == id_
    prefix = 'gfx' if id_ == 0 else 'input'
    assert value['dispatches'] == sample[prefix + '_dispatches']
    assert value['yields'] == sample[prefix + '_yields']
    assert value['work'] == sample['gfx_frames' if id_ == 0 else 'input_polls']
    assert value['snapshot_ticks'] <= sample['timer_ticks']
    if id_ == 0:
        assert value['state'] == 'running' and value['wait'] == 'none' and value['deadline'] is None
    else:
        assert value['state'] == 'sleeping' and value['wait'] == 'timer'
        assert isinstance(value['deadline'], int) and value['deadline'] <= value['snapshot_ticks'] + 1
    return value


def capture(vm, dest):
    shutil.copy2(lab.screenshot(vm)['path'], dest)
    data = dest.read_bytes()
    header, pixels = data.split(b'\n255\n', 1)
    assert header.split() == [b'P6', b'1024', b'768'] and len(pixels) == 1024 * 768 * 3
    def region(x0, x1, y0, y1):
        return b''.join(pixels[(y*1024+x0)*3:(y*1024+x1)*3] for y in range(y0, y1))
    panel = region(696, 984, 130, 380)
    name = region(704, 976, 158, 174)
    # Exclude branch connectors/highlights when comparing rotation under selection.
    tower = region(420, 620, 188, 610)
    return {'path': str(dest), 'sha256': hashlib.sha256(data).hexdigest(),
            'panel_sha256': hashlib.sha256(panel).hexdigest(), 'name_sha256': hashlib.sha256(name).hexdigest(),
            'tower_sha256': hashlib.sha256(tower).hexdigest(),
            'panel_lit_pixels': sum(panel[i:i+3] != bytes.fromhex('08141d') for i in range(0, len(panel), 3))}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--build', action='store_true')
    args = parser.parse_args()
    if args.build:
        lab.build(variant='graphics')
    manifest = lab.read_json(lab.STATE / 'build-graphics.json')
    out = lab.ARTIFACTS / 'regis/inspector' / (time.strftime('%Y%m%d-%H%M%S') + '-' + uuid.uuid4().hex[:6])
    out.mkdir(parents=True)
    names = ['ins-' + uuid.uuid4().hex[:8] for _ in range(2)]
    vm, other = names
    report = {'schema': 1, 'passed': False, 'build': manifest, 'vms': [], 'captures': [], 'snapshots': []}
    try:
        for name in names:
            state = lab.boot(name, variant='graphics', build_id=manifest['build_id'])
            report['vms'].append({'name': name, 'config': state['config'], 'qemu_argv': state['argv']})
            assert state['config']['accel'] == 'tcg'
        for name in names:
            initial = frame(name, after=0, revision=0, visible=False, paused=False)
            assert initial['selected_task'] == initial['toggles'] == 0
        hidden = capture(vm, out/'hidden.ppm'); report['captures'].append(hidden)
        assert hidden['panel_lit_pixels'] == 0
        selection(vm, 'f2', 1, 0)
        report['snapshots'].append(snapshot(vm, 1, 0))
        graphics = capture(vm, out/'graphics.ppm'); report['captures'].append(graphics)
        assert graphics['panel_lit_pixels'] > 1000
        selection(vm, 'right', 2, 1)
        report['snapshots'].append(snapshot(vm, 2, 1))
        tap(vm, 'spc')
        paused = frame(vm, revision=2, visible=True, paused=True)
        first = snapshot(vm, 2, 1, after=paused['frame']-1)
        input_a = capture(vm, out/'input-paused-a.ppm'); report['captures'].append(input_a)
        later = snapshot(vm, 2, 1, after=first['frame'])
        input_b = capture(vm, out/'input-paused-b.ppm'); report['captures'].append(input_b)
        report['snapshots'] += [first, later]
        assert later['dispatches'] > first['dispatches'] and later['work'] > first['work']
        assert input_a['tower_sha256'] == input_b['tower_sha256']
        assert input_a['panel_sha256'] != input_b['panel_sha256']
        assert input_b['name_sha256'] != graphics['name_sha256']
        left = selection(vm, 'left', 3, 0)
        assert left['paused'] and left['angle_milliradians'] == paused['angle_milliradians']
        report['snapshots'].append(snapshot(vm, 3, 0))
        selection(vm, 'tab', 4, 1)
        selection(vm, 'tab', 5, 0)
        # Unknown and prefix-bearing keys must not become navigation or Space.
        for key in ('a', 'kp_4', 'home', 'pause'):
            tap(vm, key)
        stable = frame(vm, after=frame(vm, revision=5)['frame'], revision=5, visible=True, paused=True)
        assert stable['selected_task'] == 0 and stable['toggles'] == 1
        assert len([e for e in events(vm) if e['event'] == 'inspector_changed']) == 5
        # Explicit duplicate makes + real held-key time must select only once.
        edge(vm, 'tab', True)
        held = frame(vm, revision=6, visible=True)
        assert held['selected_task'] == 1
        for _ in range(3):
            edge(vm, 'tab', True)
        time.sleep(0.8)
        held_later = frame(vm, after=held['frame'], revision=6)
        assert held_later['input_polls'] > held['input_polls']
        assert len([e for e in events(vm) if e['event'] == 'inspector_changed']) == 6
        edge(vm, 'tab', False)
        selection(vm, 'tab', 7, 0)
        selection(vm, 'f2', 8, 0, visible=False)
        closed = capture(vm, out/'closed.ppm'); report['captures'].append(closed)
        assert closed['panel_sha256'] == hidden['panel_sha256']
        selection(vm, 'f2', 9, 0)
        selection(vm, 'esc', 10, 0, visible=False)
        tap(vm, 'spc')
        resumed = frame(vm, revision=10, visible=False, paused=False)
        assert resumed['toggles'] == 2 and resumed['angle_milliradians'] != paused['angle_milliradians']
        peer = frame(other, after=0)
        assert peer['selection_revision'] == peer['toggles'] == peer['selected_task'] == 0 and not peer['inspector_visible']
        report['interaction_events'] = events(vm)
        report['peer_events'] = events(other)
        assert lab.observe(vm)['outcome'] == lab.observe(other)['outcome'] == 'pass'
        # Reset from a changed, open inspector and paused rotation each time.
        for fresh in (False, True):
            revision = 11 if not fresh else 1
            selection(vm, 'right', revision, 1)
            tap(vm, 'spc')
            frame(vm, revision=revision, paused=True)
            lab.reset(vm, fresh=fresh)
            reset = frame(vm, after=0, revision=0, visible=False, paused=False)
            assert reset['selected_task'] == reset['toggles'] == 0
            assert lab.alive(lab.load_vm(other))
            report.setdefault('resets', []).append({'fresh': fresh, 'frame': reset})
        report['checks'] = ['real_task_snapshot_and_stack_bounds', 'running_self_and_sleeping_peer',
                            'timer_wait_deadline', 'live_panel_during_rotation_pause', 'visible_selection_and_panel',
                            'tab_wrap', 'extended_arrows', 'make_break_repeat_suppression', 'unrelated_keys_ignored',
                            'f2_toggle_and_escape_close', 'selection_does_not_pause_tasks', 'two_vm_input_isolation',
                            'warm_and_fresh_reset_defaults']
        report['passed'] = True
    except Exception as error:
        report['error'] = repr(error)
    finally:
        for name in names:
            if (lab.vm_dir(name)/'state.json').exists():
                report.setdefault('logs', {})[name] = lab.logs(name)
                lab.stop(name)
                lab.clean(name)
        report['cleanup_passed'] = all(not lab.vm_dir(name).exists() for name in names)
        report['passed'] &= report['cleanup_passed']
        lab.write_json(out/'report.json', report)
        summary = {'passed': report['passed'], 'report': str(out/'report.json'), 'error': report.get('error')}
        lab.write_json(lab.ARTIFACTS/'inspector-latest.json', summary)
        print(json.dumps(summary, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
