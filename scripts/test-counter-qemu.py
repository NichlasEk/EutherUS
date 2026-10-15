#!/usr/bin/env python3
"""Restartable built-in counter: actual TCG lifecycle and both new stack guards."""
import importlib.util
import json
import sys
import time
import uuid
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from lab import controller as lab
spec=importlib.util.spec_from_file_location('prompt',ROOT/'scripts/test-prompt-qemu.py')
p=importlib.util.module_from_spec(spec);spec.loader.exec_module(p)
ins=p.ins

def tasks(vm):return lab.events(lab.logs(vm)['serial'],'eutherus.tasks')
def wait_task(vm,event,predicate=lambda e:True):
    deadline=time.monotonic()+45
    while time.monotonic()<deadline:
        assert lab.alive(lab.load_vm(vm))
        for e in tasks(vm):
            if e['event']==event and predicate(e):return e
        time.sleep(.05)
    raise TimeoutError((event,tasks(vm)[-4:]))

def main():
    variants=['graphics','graphics-counter-low','graphics-counter-high']
    if '--build' in sys.argv:
        for v in variants:lab.build(variant=v)
    builds={v:lab.read_json(lab.STATE/('build-'+v+'.json')) for v in variants}
    out=lab.ARTIFACTS/'regis/counter'/uuid.uuid4().hex[:12];out.mkdir(parents=True)
    report={'passed':False,'builds':builds,'captures':[],'cases':[]}
    names=[]
    def boot(v):
        name='ctr-'+uuid.uuid4().hex[:8];names.append(name)
        state=lab.boot(name,variant=v,build_id=builds[v]['build_id']);assert state['config']['accel']=='tcg'
        ins.frame(name,after=0);ins.tap(name,'f1');return name
    def command(vm,text,status='ok'):
        seq=1+len([e for e in p.events(vm) if e['event']=='command_result'])
        p.text(vm,text);ins.tap(vm,'ret')
        result=p.wait(vm,'command_result',lambda e:e['sequence']==seq)
        assert result['status']==status,result
        return result
    try:
        vm=boot('graphics');peer=boot('graphics')
        command(vm,'inspect counter','unknown_task')
        command(vm,'run nope','invalid_command')
        command(vm,'run counter extra','invalid_command')
        assert not any(e['event']=='counter_started' for e in tasks(vm))
        report['captures'].append(ins.capture(vm,out/'before.ppm'))
        baseline=ins.frame(vm,after=0)
        starts=[]
        for run in (1,2):
            command(vm,'run counter')
            start=wait_task(vm,'counter_started',lambda e:e['run']==run);starts.append(start)
            live=ins.wait(vm,'counter_presented',lambda e:e['run']==run and e['state'] in ('ready','sleeping'))
            assert live['selected']==2 and live['steps']<10
            if run==1:
                report['captures'].append(ins.capture(vm,out/'running.ppm'))
                command(vm,'run counter','task_busy')
            finish=wait_task(vm,'counter_finished',lambda e:e['run']==run)
            shown=ins.wait(vm,'counter_presented',lambda e:e['run']==run and e['state']=='finished')
            progress=[e for e in tasks(vm) if e['event']=='counter_progress' and e['run']==run]
            assert [e['step'] for e in progress]==list(range(1,11))
            assert finish['steps']==shown['steps']==shown['sleeps']==10
            assert shown['dispatches']==11
            assert all(start['stack_base']<=e['rsp']<start['guard_high'] and e['stack_preserved'] for e in progress)
            ticks=[start['ticks']]+[e['ticks'] for e in progress]
            assert all(b-a>=50 for a,b in zip(ticks,ticks[1:]))
            assert shown['gfx_frames']>live['gfx_frames'] and shown['input_polls']>live['input_polls']
            all_events=tasks(vm)
            a=next(i for i,e in enumerate(all_events) if e['event']=='counter_started' and e['run']==run)
            z=next(i for i,e in enumerate(all_events) if e['event']=='counter_finished' and e['run']==run)
            assert a<z and all_events[z+1]['event']=='task_finished' and all_events[z+1]['id']==2
            command(vm,'inspect counter')
            p.wait(vm,'command_task',lambda e:e['id']==2 and e['state']=='finished' and e['work']==10)
            stable=ins.wait(vm,'counter_presented',lambda e:e['run']==run and e['frame']>shown['frame']+20)
            assert stable['dispatches']==11 and stable['steps']==10
            report['cases'].append({'run':run,'start':start,'finish':finish,'shown':shown})
        assert starts[0]['stack_base']==starts[1]['stack_base']
        assert len([e for e in tasks(vm) if e['event']=='counter_started'])==2
        assert not any(e['event']=='counter_started' for e in tasks(peer))
        ready=[e for e in tasks(vm) if e['event']=='task_ready']
        assert starts[0]['guard_low']>=ready[1]['guard_high']+4096
        report['captures'].append(ins.capture(vm,out/'finished.ppm'))
        def branch(path):
            data=Path(path).read_bytes().split(b'\n255\n',1)[1]
            return b''.join(data[(y*1024+64)*3:(y*1024+240)*3] for y in range(400,494))
        regions=[branch(c['path']) for c in report['captures']]
        lit=lambda b:sum(b[i:i+3]!=bytes.fromhex('08141d') for i in range(0,len(b),3))
        assert lit(regions[0])==0 and lit(regions[1])>500 and lit(regions[2])>500
        assert regions[1]!=regions[2]
        command(vm,'tasks');command(vm,'tree');command(vm,'memory')
        allocation=next(e for e in lab.events(lab.logs(vm)['serial'],'eutherus.memory') if e['event']=='graphics_allocated')
        memory=p.wait(vm,'command_memory')
        assert memory['free_pages']==allocation['free_pages'] and memory['task_stack_bytes']==3*128*1024
        # Third branch participates in keyboard cycling; no nonexistent slot selected.
        ins.tap(vm,'esc');ins.tap(vm,'tab')
        latest=ins.wait(vm,'frame_presented',lambda e:e['frame']>stable['frame'] and e['selected_task']==0 and e['inspector_visible'])
        report['lifecycle_events']=tasks(vm)
        for fresh in (False,True):
            ins.tap(vm,'f1');command(vm,'run counter')
            lab.reset(vm,fresh=fresh)
            ins.frame(vm,after=0)
            assert not any(e['event']=='counter_started' for e in tasks(vm))
            ins.tap(vm,'f1');command(vm,'inspect counter','unknown_task')
            ins.tap(vm,'esc')
        for variant in variants[1:]:
            fault_vm=boot(variant);command(fault_vm,'run counter')
            started=wait_task(fault_vm,'counter_started')
            deadline=time.monotonic()+40
            while time.monotonic()<deadline:
                logs=lab.logs(fault_vm)['serial']
                faults=[e for e in lab.events(logs,'eutherus.cpu') if e['event']=='exception']
                if faults:break
                time.sleep(.05)
            assert faults,'missing guard fault'
            fault=faults[0]
            expected=started['guard_low' if variant.endswith('low') else 'guard_high']
            assert fault['vector']==14 and fault['error_code']==2 and fault['cr2']==expected
            assert started['stack_base']<=fault['handler_rsp']<started['guard_high'] and not fault['emergency_stack']
            assert [e['step'] for e in tasks(fault_vm) if e['event']=='counter_progress']==[1,2]
            assert not any(e['event']=='counter_finished' for e in tasks(fault_vm))
            assert lab.observe(fault_vm)['outcome']=='exception'
            assert 'HLT=1' in lab.debug(fault_vm,registers=True)['registers']
            before=lab.logs(fault_vm)['serial'];time.sleep(.15);assert lab.logs(fault_vm)['serial']==before
            report['cases'].append({'variant':variant,'fault':fault,'started':started})
        report['passed']=True
    except Exception as error:report['error']=repr(error)
    finally:
        for name in names:
            if (lab.vm_dir(name)/'state.json').exists():
                report.setdefault('logs',{})[name]=lab.logs(name);lab.stop(name);lab.clean(name)
        report['cleanup_passed']=all(not lab.vm_dir(n).exists() for n in names)
        report['passed'] &= report['cleanup_passed']
        lab.write_json(out/'report.json',report)
        result={'passed':report['passed'],'report':str(out/'report.json'),'error':report.get('error')}
        lab.write_json(lab.ARTIFACTS/'counter-latest.json',result);print(json.dumps(result,indent=2))
    return 0 if report['passed'] else 1
if __name__=='__main__':sys.exit(main())
