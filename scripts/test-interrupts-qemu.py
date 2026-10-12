#!/usr/bin/env python3
"""Owned CPU state, real interrupt wakeups and injected CPU faults in QEMU TCG."""
import argparse
import json
from pathlib import Path
import re
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from lab import controller as lab
VARIANTS = ('interrupts', 'invalid-opcode', 'general-protection', 'page-fault', 'double-fault', 'timer-masked')


def cpu_events(vm):
    return lab.events(lab.logs(vm)['serial'], 'eutherus.cpu')


def wait_event(vm, name, after_ticks=-1):
    deadline = time.monotonic()+30
    while time.monotonic()<deadline:
        assert lab.alive(lab.load_vm(vm)), 'unexpected VM termination'
        events = cpu_events(vm)
        for event in events:
            if event.get('event') == name and event.get('ticks',0)>after_ticks:
                return event
        assert not any(e.get('event') == 'exception' for e in events), 'unexpected exception'
        time.sleep(0.05)
    raise TimeoutError(name)


def halted_registers(vm):
    for _ in range(30):
        result = lab.debug(vm, registers=True)['registers']
        if 'HLT=1' in result:
            return result
        time.sleep(0.02)
    raise AssertionError('guest never observed halted')


def case(variant, manifest):
    vm = 'cpu-'+uuid.uuid4().hex[:8]
    result = {'name': variant, 'passed': False, 'build': manifest}
    try:
        state = lab.boot(vm, variant=variant, build_id=manifest['build_id'])
        result.update(config=state['config'], qemu_argv=state['argv'])
        tables = wait_event(vm, 'tables_ready')
        assert tables['owned_stack'] and tables['breakpoint_returned']
        if variant == 'interrupts':
            first = wait_event(vm, 'idle')
            second = wait_event(vm, 'idle', first['ticks'])
            assert second['ticks'] >= first['ticks']+100
            assert second['halt_waits'] > first['halt_waits']
            regs = halted_registers(vm)
            result['registers'] = regs
            for label,expected in [('GDT',tables['gdt']),('IDT',tables['idt'])]:
                match = re.search(label+r'=\s*([0-9a-fA-F]+)',regs)
                assert match and int(match.group(1),16)==expected, label+' not installed'
            rsp = int(re.search(r'RSP=([0-9a-fA-F]+)',regs).group(1),16)
            assert tables['stack_base']<=rsp<tables['stack_base']+tables['stack_bytes']
            assert lab.observe(vm)['outcome']=='pass'
            # Warm reset must recreate owned tables and resume real IRQ delivery.
            lab.reset(vm)
            wait_event(vm,'timer_ready')
            wait_event(vm,'idle')
            assert lab.observe(vm)['outcome']=='pass'
            result['reset_passed'] = True
        elif variant == 'timer-masked':
            # Same wait code, but IRQ0 masked: no timer readiness or boot success.
            observation = lab.observe(vm,timeout=1.5)
            assert observation['outcome']=='timeout'
            assert [e['event'] for e in cpu_events(vm)]==['tables_ready']
            result['registers']=halted_registers(vm)
        else:
            fault = wait_event(vm,'exception')
            vector={'invalid-opcode':6,'general-protection':13,'page-fault':14,'double-fault':8}[variant]
            assert fault['vector']==vector and fault['rip']!=0
            assert fault['emergency_stack']==(variant=='double-fault')
            if variant=='general-protection':
                assert fault['error_code']==0xfff8
            if variant=='page-fault':
                assert fault['cr2']==0x400000000000 and fault['error_code']==0
            if variant=='double-fault':
                assert fault['error_code']==0
                assert tables['emergency_base']<=fault['handler_rsp']<tables['emergency_base']+tables['stack_bytes']
            assert lab.observe(vm)['outcome']=='exception'
            assert lab.status(vm)['cpu_events'][-1]==fault
            result['registers']=halted_registers(vm)
            result['fault']=fault
        result['passed']=True
    except Exception as error:
        result['error']=repr(error)
    finally:
        if (lab.vm_dir(vm)/'state.json').exists():
            result['logs']=lab.logs(vm)
            result['cpu_events']=cpu_events(vm)
            lab.stop(vm)
            lab.clean(vm)
        result['cleanup_passed']=not lab.vm_dir(vm).exists()
        result['passed'] &= result['cleanup_passed']
    return result


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--build',action='store_true')
    args=parser.parse_args()
    if args.build:
        for variant in VARIANTS:
            print('Building '+variant,file=sys.stderr,flush=True)
            lab.build(variant=variant)
    manifests={v:lab.read_json(lab.STATE/('build-'+v+'.json')) for v in VARIANTS}
    baseline=manifests['interrupts']
    assert all(m['source']==baseline['source'] for m in manifests.values()), 'mixed source revisions'
    for filename in ('eutherboot.efi','OVMF_CODE.fd','OVMF_VARS.fd'):
        assert all(m['files'][filename]==baseline['files'][filename] for m in manifests.values()), 'mixed firmware/bootloader'
    report={'schema':1,'suite':'interrupts','cases':[]}
    for variant in VARIANTS:
        result=case(variant,manifests[variant])
        report['cases'].append(result)
        print(variant+': '+('PASS' if result['passed'] else str(result.get('error'))),file=sys.stderr,flush=True)
    report['passed']=all(c['passed'] for c in report['cases'])
    path=lab.ARTIFACTS/'reports'/('interrupts-'+time.strftime('%Y%m%d-%H%M%S')+'-'+uuid.uuid4().hex[:6]+'.json')
    lab.write_json(path,report)
    lab.write_json(lab.ARTIFACTS/'interrupts-latest.json',{'passed':report['passed'],'report':str(path)})
    print(json.dumps({'passed':report['passed'],'report':str(path)},indent=2))
    return 0 if report['passed'] else 1


if __name__=='__main__':
    sys.exit(main())
