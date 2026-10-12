#!/usr/bin/env python3
"""Real post-UEFI physical allocator acceptance, failure paths and isolation."""
import argparse
import json
from pathlib import Path
import sys
import time
import uuid
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from lab import controller as lab
VARIANTS=('memory','memory-empty','memory-invalid')


def successful(vm):
    result=lab.observe(vm,timeout=40)
    assert result['outcome']=='pass', result['outcome']
    assert result['events'][-1]['handoff']==3
    events=result['memory_events']
    ready=next(e for e in events if e['event']=='allocator_ready')
    test=next(e for e in events if e['event']=='self_test_pass')
    assert ready['free_pages']>16 and ready['page_table_frames']>0
    assert ready['reclaimed_firmware_pages']==0
    assert test['pattern_pages']==5 and test['invalid_free_rejected']
    assert ready['free_pages']==test['exhausted_pages']==test['restored_pages']
    names={e['name'] for e in events if e['event']=='reserved'}
    assert names=={'low_memory','kernel_image_stacks_tables','firmware_stack','memory_map_allocation'}
    assert lab.status(vm)['memory_events']==events
    return result


def case(variant,manifest):
    names=['mem-'+uuid.uuid4().hex[:8] for _ in range(2 if variant=='memory' else 1)]
    result={'name':variant,'passed':False,'build':manifest,'vms':[]}
    try:
        states=[lab.boot(name,variant=variant,build_id=manifest['build_id']) for name in names]
        for state in states:
            result['vms'].append({'vm':state['vm'],'config':state['config'],'qemu_argv':state['argv']})
        if variant=='memory':
            assert all(lab.alive(s) for s in states)
            for field in ('pid','qmp','gdb','directory'):
                assert len({s[field] for s in states})==2
            result['observations']=[successful(name) for name in names]
            lab.reset(names[0])
            result['after_reset']=successful(names[0])
            # Other VM's completed memory test and timer must survive its peer's reset.
            assert successful(names[1])['outcome']=='pass'
            result['checks']=['real_RAM_patterns','exact_exhaustion_and_restore','invalid_free_rejection',
                              'explicit_reservations','two_concurrent_pools','warm_reset','peer_survives_reset']
        else:
            observed=lab.observe(names[0],timeout=40)
            result['observation']=observed
            events=observed['memory_events']
            assert 'boot_success' not in [e['event'] for e in observed['events']]
            if variant=='memory-empty':
                assert observed['outcome']=='out_of_memory'
                assert events[-1]['event']=='out_of_memory'
                assert events[-1]['requested_pages']==1 and events[-1]['free_pages']==0
                assert next(e for e in events if e['event']=='allocator_ready')['free_pages']==0
            else:
                assert observed['outcome']=='panic'
                assert events==[{'protocol':'eutherus.memory','version':1,'event':'map_rejected'}]
                assert 'invalid memory map' in observed['logs']['serial']
            assert lab.alive(states[0]), 'failure should be controlled halt'
        result['passed']=True
    except Exception as error:
        result['error']=repr(error)
    finally:
        for name in names:
            if (lab.vm_dir(name)/'state.json').exists():
                result.setdefault('logs',{})[name]=lab.logs(name)
                lab.stop(name);lab.clean(name)
        result['cleanup_passed']=all(not lab.vm_dir(name).exists() for name in names)
        result['passed'] &= result['cleanup_passed']
    return result


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--build',action='store_true');args=parser.parse_args()
    if args.build:
        for variant in VARIANTS:
            print('Building '+variant,file=sys.stderr,flush=True);lab.build(variant=variant)
    manifests={v:lab.read_json(lab.STATE/('build-'+v+'.json')) for v in VARIANTS}
    baseline=manifests['memory']
    assert all(m['source']==baseline['source'] for m in manifests.values()), 'mixed sources'
    for filename in ('eutherboot.efi','OVMF_CODE.fd','OVMF_VARS.fd'):
        assert all(m['files'][filename]==baseline['files'][filename] for m in manifests.values()), 'mixed firmware'
    report={'schema':1,'suite':'memory','cases':[]}
    for variant in VARIANTS:
        result=case(variant,manifests[variant]);report['cases'].append(result)
        print(variant+': '+('PASS' if result['passed'] else str(result.get('error'))),file=sys.stderr,flush=True)
    report['passed']=all(c['passed'] for c in report['cases'])
    path=lab.ARTIFACTS/'reports'/('memory-'+time.strftime('%Y%m%d-%H%M%S')+'-'+uuid.uuid4().hex[:6]+'.json')
    lab.write_json(path,report)
    summary={'passed':report['passed'],'report':str(path)}
    lab.write_json(lab.ARTIFACTS/'memory-latest.json',summary)
    print(json.dumps(summary,indent=2));return 0 if report['passed'] else 1


if __name__=='__main__':sys.exit(main())
