#!/usr/bin/env python3
"""Prompt-driven camera, fixed text, live data and release behavior in actual TCG."""
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

def nav(vm,predicate=lambda e:True):
    return ins.wait(vm,'navigation_presented',predicate)

def capture(vm,path):
    result=ins.capture(vm,path)
    data=path.read_bytes().split(b'\n255\n',1)[1]
    import hashlib
    def region(x0,x1,y0,y1):
        return hashlib.sha256(b''.join(data[(y*1024+x0)*3:(y*1024+x1)*3] for y in range(y0,y1))).hexdigest()
    result['title']=region(704,976,136,153)
    result['selected_label']=region(704,976,400,414)
    result['visible_tower']=region(390,650,188,490)
    return result

def main():
    if '--build' in sys.argv:lab.build(variant='graphics')
    build=lab.read_json(lab.STATE/'build-graphics.json')
    out=lab.ARTIFACTS/'regis/navigation'/uuid.uuid4().hex[:12];out.mkdir(parents=True)
    names=['nav-'+uuid.uuid4().hex[:8] for _ in range(2)];vm,peer=names
    report={'passed':False,'build':build,'captures':[],'commands':[]}
    sequence=0
    def command(value,status='ok'):
        nonlocal sequence
        p.text(vm,value);ins.tap(vm,'ret');sequence+=1
        r=p.wait(vm,'command_result',lambda e:e['sequence']==sequence)
        assert r['status']==status,r
        shown=p.wait(vm,'prompt_presented',lambda e:e['completed']==sequence and e['input_len']==0)
        report['commands'].append(r)
        return shown
    try:
        for name in names:
            state=lab.boot(name,variant='graphics',build_id=build['build_id']);assert state['config']['accel']=='tcg'
            nav(name,lambda e:e['revision']==0 and not e['focused'])
        ins.tap(vm,'f1');command('select input')
        first=nav(vm,lambda e:e['revision']==1)
        assert first['selected']==1 and first['focused'] and not first['inspector_visible']
        moving=capture(vm,out/'turning.ppm');report['captures'].append(moving)
        report['animation']=[{'path':moving['path'],'time':time.monotonic()}]
        seen=first['frame']
        for i in range(8):
            step=nav(vm,lambda e:e['revision']==1 and e['frame']>seen+3)
            seen=step['frame']
            image=capture(vm,out/('turn-%02d.ppm'%i))
            report['animation'].append({'path':image['path'],'time':time.monotonic()})
            if step['settled']: break
        settled=nav(vm,lambda e:e['revision']==1 and e['settled'])
        assert settled['angle_milliradians']==settled['target_milliradians']==3141
        frames=[e for e in ins.events(vm) if e['event']=='navigation_presented' and e['revision']==1]
        assert len(frames)>=10 and any(not e['settled'] for e in frames)
        for a,b in zip(frames,frames[1:]):
            diff=(b['angle_milliradians']-a['angle_milliradians']+3142)%6283-3142
            assert abs(diff)<=92*(b['frame']-a['frame']), (a,b)
        held=capture(vm,out/'selected.ppm');report['captures'].append(held)
        assert moving['visible_tower']!=held['visible_tower']
        assert moving['selected_label']==held['selected_label']
        later=nav(vm,lambda e:e['revision']==1 and e['frame']>settled['frame']+10)
        held2=capture(vm,out/'selected-held.ppm');report['captures'].append(held2)
        assert later['angle_milliradians']==settled['angle_milliradians']
        assert held['visible_tower']==held2['visible_tower']
        activity=ins.frame(vm,after=later['frame'])
        assert activity['input_polls']>first['frame'] and activity['gfx_frames']>first['frame']
        command('inspect')
        detail=nav(vm,lambda e:e['revision']==2 and e['settled'])
        assert detail['selected']==1 and detail['inspector_visible']
        inspected=capture(vm,out/'inspect.ppm');report['captures'].append(inspected)
        assert inspected['title']!=held['title']
        command('memory')
        memory=nav(vm,lambda e:e['revision']==3 and e['settled'])
        assert memory['mode']=='memory' and not memory['inspector_visible']
        allocation=next(e for e in lab.events(lab.logs(vm)['serial'],'eutherus.memory') if e['event']=='graphics_allocated')
        assert memory['free_pages']==allocation['free_pages'] and memory['arena_bytes']==allocation['pages']*4096
        mem=capture(vm,out/'memory.ppm');report['captures'].append(mem)
        assert mem['title']!=inspected['title']
        # Invalid commands must not move the camera or change the panel.
        for cmd,status in [('select nope','unknown_task'),('select','invalid_command'),('select input extra','invalid_command'),('tree nope','invalid_command'),('inspect 2','unknown_task')]:
            last_error=command(cmd,status)
        unchanged=nav(vm,lambda e:e['frame']>last_error['frame'])
        assert unchanged['revision']==3 and unchanged['mode']=='memory'
        command('tree')
        overview=nav(vm,lambda e:e['revision']==4 and not e['focused'])
        running=nav(vm,lambda e:e['revision']==4 and e['frame']>overview['frame'])
        assert running['angle_milliradians']!=overview['angle_milliradians']
        tree=capture(vm,out/'tree.ppm');report['captures'].append(tree)
        assert tree['title']==held['title']
        command('select graphics');nav(vm,lambda e:e['revision']==5 and e['settled'] and e['selected']==0)
        p.text(vm,'draft');ins.tap(vm,'esc')
        released=nav(vm,lambda e:e['revision']==6 and not e['focused'])
        p.wait(vm,'prompt_presented',lambda e:not e['active'] and e['input_hex']==b'draft'.hex())
        ins.tap(vm,'f1');p.wait(vm,'prompt_presented',lambda e:e['active'] and e['input_hex']==b'draft'.hex())
        for _ in 'draft':ins.tap(vm,'backspace')
        # Explicit manual pause survives entering and leaving a camera focus.
        ins.tap(vm,'esc');ins.tap(vm,'spc');paused=ins.frame(vm,paused=True)
        ins.tap(vm,'f1');command('select input');nav(vm,lambda e:e['revision']==7 and e['settled'])
        ins.tap(vm,'f1');released=nav(vm,lambda e:e['revision']==8 and not e['focused'])
        later=nav(vm,lambda e:e['revision']==8 and e['frame']>released['frame'])
        assert later['angle_milliradians']==released['angle_milliradians']
        assert ins.frame(vm,after=later['frame'])['paused']
        peer_state=nav(peer,lambda e:e['frame']>30)
        assert peer_state['revision']==0 and not peer_state['focused']
        report['events']=ins.events(vm)
        for fresh in (False,True):
            ins.tap(vm,'f1');command('memory')
            lab.reset(vm,fresh=fresh);sequence=0
            initial=nav(vm,lambda e:e['revision']==0)
            assert initial['mode']=='overview' and not initial['focused'] and initial['selected']==0
            assert not ins.frame(vm)['paused']
        report['passed']=True
    except Exception as error:report['error']=repr(error)
    finally:
        for name in names:
            if (lab.vm_dir(name)/'state.json').exists():
                report.setdefault('logs',{})[name]=lab.logs(name);lab.stop(name);lab.clean(name)
        report['cleanup_passed']=all(not lab.vm_dir(name).exists() for name in names)
        report['passed'] &= report['cleanup_passed']
        lab.write_json(out/'report.json',report)
        result={'passed':report['passed'],'report':str(out/'report.json'),'error':report.get('error')}
        lab.write_json(lab.ARTIFACTS/'navigation-latest.json',result);print(json.dumps(result,indent=2))
    return 0 if report['passed'] else 1
if __name__=='__main__':sys.exit(main())
