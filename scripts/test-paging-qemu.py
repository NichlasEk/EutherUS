#!/usr/bin/env python3
"""Real CR3 transition, four stack guards and overflow-to-IST acceptance."""
import argparse
import json
from pathlib import Path
import re
import sys
import time
import uuid
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from lab import controller as lab
VARIANTS=('paging','guard-low','guard-high','emergency-low','emergency-high','stack-overflow')


def event(vm,protocol,name):
    deadline=time.monotonic()+40
    while time.monotonic()<deadline:
        assert lab.alive(lab.load_vm(vm)), 'VM terminated unexpectedly'
        logs=lab.logs(vm)['serial']
        for e in lab.events(logs,protocol):
            if e.get('event')==name:return e
        assert not any(e.get('event')=='panic' for e in lab.events(logs)), logs[-1500:]
        time.sleep(0.05)
    raise TimeoutError(name)


def ready(vm):
    e=event(vm,'eutherus.paging','paging_ready')
    assert e['cr3']!=e['old_cr3'] and e['cr3']%4096==0
    assert e['table_pages']>0 and e['old_guards_still_present']
    assert e['all_tables_owned'] and e['neighbor_mappings_preserved']
    assert e in lab.status(vm)['paging_events']
    assert len({e[k] for k in ('stack_low','stack_high','emergency_low','emergency_high')})==4
    cpu=event(vm,'eutherus.cpu','tables_ready')
    assert e['stack_low']+4096==cpu['stack_base']
    assert e['stack_high']==cpu['stack_base']+cpu['stack_bytes']
    assert e['emergency_low']+4096==cpu['emergency_base']
    assert e['emergency_high']==cpu['emergency_base']+cpu['stack_bytes']
    initial=event(vm,'eutherus.memory','allocator_ready')
    assert initial['free_pages']-e['free_pages']==e['table_pages']
    event(vm,'eutherus.paging','timer_after_switch')
    registers=lab.debug(vm,registers=True)['registers']
    assert int(re.search(r'CR3=([0-9a-fA-F]+)',registers).group(1),16)==e['cr3']
    return e,registers


def case(variant,manifest):
    names=['pg-'+uuid.uuid4().hex[:8] for _ in range(2 if variant=='paging' else 1)]
    result={'name':variant,'build':manifest,'passed':False,'vms':[]}
    try:
        states=[lab.boot(n,variant=variant,build_id=manifest['build_id']) for n in names]
        for s in states:result['vms'].append({'name':s['vm'],'config':s['config'],'qemu_argv':s['argv']})
        page,registers=ready(names[0]);result.update(paging=page,registers=registers)
        if variant=='paging':
            ready(names[1])
            assert states[0]['pid']!=states[1]['pid'] and all(lab.alive(s) for s in states)
            for n in names:assert lab.observe(n)['outcome']=='pass'
            lab.reset(names[0]);ready(names[0]);assert lab.observe(names[0])['outcome']=='pass'
            assert lab.observe(names[1])['outcome']=='pass'
            result['reset_and_isolation_passed']=True
        else:
            fault=event(names[0],'eutherus.cpu','exception');result['fault']=fault
            assert lab.observe(names[0])['outcome']=='exception'
            assert 'boot_success' not in [e['event'] for e in lab.events(lab.logs(names[0])['serial'])]
            if variant=='stack-overflow':
                assert fault['vector']==8 and fault['error_code']==0 and fault['emergency_stack']
                assert page['stack_low']<=fault['cr2']<page['stack_low']+4096
                assert page['emergency_low']+4096<=fault['handler_rsp']<page['emergency_high']
            else:
                target={'guard-low':'stack_low','guard-high':'stack_high','emergency-low':'emergency_low','emergency-high':'emergency_high'}[variant]
                assert fault['vector']==14 and fault['error_code']==2
                assert fault['cr2']==page[target] and not fault['emergency_stack']
            assert lab.alive(states[0]), 'expected captured fault, not triple fault'
        result['passed']=True
    except Exception as error:result['error']=repr(error)
    finally:
        for n in names:
            if (lab.vm_dir(n)/'state.json').exists():
                result.setdefault('logs',{})[n]=lab.logs(n);lab.stop(n);lab.clean(n)
        result['cleanup_passed']=all(not lab.vm_dir(n).exists() for n in names)
        result['passed'] &= result['cleanup_passed']
    return result


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--build',action='store_true');args=parser.parse_args()
    if args.build:
        for v in VARIANTS:print('Building '+v,file=sys.stderr,flush=True);lab.build(variant=v)
    manifests={v:lab.read_json(lab.STATE/('build-'+v+'.json')) for v in VARIANTS}
    baseline=manifests['paging']
    assert all(m['source']==baseline['source'] for m in manifests.values())
    for filename in ('eutherboot.efi','OVMF_CODE.fd','OVMF_VARS.fd'):
        assert all(m['files'][filename]==baseline['files'][filename] for m in manifests.values())
    report={'schema':1,'suite':'paging','cases':[]}
    for v in VARIANTS:
        r=case(v,manifests[v]);report['cases'].append(r)
        print(v+': '+('PASS' if r['passed'] else str(r.get('error'))),file=sys.stderr,flush=True)
    report['passed']=all(c['passed'] for c in report['cases'])
    path=lab.ARTIFACTS/'reports'/('paging-'+time.strftime('%Y%m%d-%H%M%S')+'-'+uuid.uuid4().hex[:6]+'.json')
    lab.write_json(path,report);summary={'passed':report['passed'],'report':str(path)}
    lab.write_json(lab.ARTIFACTS/'paging-latest.json',summary);print(json.dumps(summary,indent=2))
    return 0 if report['passed'] else 1


if __name__=='__main__':sys.exit(main())
