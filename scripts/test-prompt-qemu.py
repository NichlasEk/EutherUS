#!/usr/bin/env python3
"""Actual TCG keyboard acceptance for the bounded read-only console."""
import importlib.util
import json
import sys
import time
import uuid
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from lab import controller as lab
spec = importlib.util.spec_from_file_location('inspector', ROOT/'scripts/test-inspector-qemu.py')
ins = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ins)
def events(vm):
    return lab.events(lab.logs(vm)['serial'], 'eutherus.prompt')
def wait(vm, event, predicate=lambda e: True):
    deadline = time.monotonic()+40
    while time.monotonic()<deadline:
        assert lab.alive(lab.load_vm(vm))
        serial = lab.logs(vm)['serial']
        assert not any(e['event'] in ('panic','exception') for e in lab.events(serial)+lab.events(serial,'eutherus.cpu')), serial[-2000:]
        for e in events(vm):
            if e['event']==event and predicate(e): return e
        time.sleep(.05)
    raise TimeoutError((event, events(vm)[-3:]))
def latest(vm):
    return [e for e in events(vm) if e['event']=='prompt_presented'][-1]
def text(vm, value):
    for c in value:
        ins.tap(vm, 'spc' if c==' ' else c)
def main():
    if '--build' in sys.argv: lab.build(variant='graphics')
    build=lab.read_json(lab.STATE/'build-graphics.json')
    out=lab.ARTIFACTS/'regis/prompt'/uuid.uuid4().hex[:12]; out.mkdir(parents=True)
    names=['prompt-'+uuid.uuid4().hex[:8] for _ in range(2)]
    vm, peer=names
    report={'passed':False,'build':build,'commands':[]}
    seq=0
    def state(active=None, value=None, after=-1):
        return wait(vm,'prompt_presented',lambda e:e['revision']>after and (active is None or e['active']==active) and (value is None or e['input_hex']==value.encode().hex()))
    def command(value, status='ok'):
        nonlocal seq
        text(vm,value); ins.tap(vm,'ret'); seq+=1
        result=wait(vm,'command_result',lambda e:e['sequence']==seq)
        assert result['status']==status,result
        wait(vm,'prompt_presented',lambda e:e['completed']==seq and e['input_len']==0)
        report['commands'].append(result)
        return result
    try:
        for name in names:
            boot=lab.boot(name,variant='graphics',build_id=build['build_id'])
            assert boot['config']['accel']=='tcg'
            wait(name,'prompt_presented',lambda e:not e['active'] and e['completed']==0)
        text(vm,'help'); ins.tap(vm,'ret'); ins.tap(vm,'f1'); state(True,'')
        command('help')
        text(vm,'memorz'); ins.tap(vm,'backspace'); command('y')
        memory=wait(vm,'command_memory')
        allocated=next(e for e in lab.events(lab.logs(vm)['serial'], 'eutherus.memory') if e['event']=='graphics_allocated')
        assert memory['free_pages']==allocated['free_pages']
        assert memory['arena_bytes']==allocated['pages']*4096
        command('tasks')
        ts=[e for e in events(vm) if e['event']=='command_task' and e['sequence']==seq]
        assert [t['id'] for t in ts]==[0,1] and ts[1]['state']=='running'
        for t in ts:
            ready=next(e for e in lab.status(vm)['task_events'] if e['event']=='task_ready' and e['id']==t['id'])
            assert t['stack_base']==ready['stack_base'] and t['stack_end']==ready['guard_high']
        command('inspect'); command('inspect input'); command('inspect 2','unknown_task')
        command('tasks nope','invalid_command'); command('reboot','invalid_command')
        before=latest(vm)['revision']; text(vm,'help ')
        changes=len([e for e in ins.events(vm) if e['event']=='inspector_changed'])
        for key in ('tab','right','f2'): ins.tap(vm,key)
        draft=state(True,'help ',after=before)
        assert len([e for e in ins.events(vm) if e['event']=='inspector_changed'])==changes
        ins.tap(vm,'esc'); state(False,'help ',after=draft['revision'])
        ins.tap(vm,'f1'); state(True,'help ',after=draft['revision'])
        command('')
        assert report['commands'][-1]['context']==1
        assert not any(e['event']=='rotation_toggled' for e in ins.events(vm))
        before=latest(vm)['revision']; ins.tap(vm,'ret'); ins.tap(vm,'backspace')
        blank=state(True,'',after=before); assert blank['completed']==seq
        command('help'+' '*60+'x','line_too_long')
        command('help')
        ins.edge(vm,'shift',True); text(vm,'help'); ins.edge(vm,'shift',False)
        state(True,'HELP'); command('')
        ins.edge(vm,'a',True); ins.edge(vm,'a',True); time.sleep(.7); ins.edge(vm,'a',False)
        state(True,'a'); ins.tap(vm,'backspace')
        ins.tap(vm,'esc'); ins.tap(vm,'right'); ins.tap(vm,'f1'); command('inspect')
        assert report['commands'][-1]['context']==1
        wait(vm,'command_task',lambda e:e['sequence']==seq and e['id']==1)
        command('memory'); assert latest(vm)['rows']==8
        report['capture']=ins.capture(vm,out/'console.ppm')
        pixels=(out/'console.ppm').read_bytes().split(b'\n255\n',1)[1]
        def pixel(x,y): return pixels[(y*1024+x)*3:(y*1024+x+1)*3]
        assert pixel(32,600)==bytes.fromhex('77cfc6')
        assert pixel(40,600)==bytes.fromhex('0d1b25')
        assert pixel(168,704)==bytes.fromhex('e9ca8c')
        ordered=events(vm)
        for i,e in enumerate(ordered):
            if e['event'] in ('command_task','command_memory'):
                assert any(r['event']=='command_result' and r['sequence']==e['sequence'] for r in ordered[i+1:])
        assert wait(peer,'prompt_presented')['completed']==0
        assert not any(e['event']=='command_result' for e in events(peer))
        report['events']=events(vm)
        for fresh in (False,True):
            text(vm,'draft'); lab.reset(vm,fresh=fresh)
            reset=wait(vm,'prompt_presented',lambda e:e['revision']==0)
            assert not reset['active'] and reset['completed']==reset['input_len']==0
            ins.tap(vm,'f1'); state(True,'')
        report['passed']=True
    except Exception as error:
        report['error']=repr(error)
    finally:
        for name in names:
            if (lab.vm_dir(name)/'state.json').exists():
                report.setdefault('logs',{})[name]=lab.logs(name)
                lab.stop(name); lab.clean(name)
        lab.write_json(out/'report.json',report)
        summary={'passed':report['passed'],'report':str(out/'report.json'),'error':report.get('error')}
        lab.write_json(lab.ARTIFACTS/'prompt-latest.json',summary)
        print(json.dumps(summary,indent=2))
    return 0 if report['passed'] else 1
if __name__=='__main__': sys.exit(main())
