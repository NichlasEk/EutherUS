#!/usr/bin/env python3
"""Build all real kernel variants and run the full emulator regression suite."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from lab.controller import build, test

p = argparse.ArgumentParser()
p.add_argument('--eutherboot')
p.add_argument('--promote', action='store_true')
args = p.parse_args()
subprocess.run([sys.executable, '-m', 'unittest', 'discover', '-s', 'tests', '-v'], cwd=ROOT, check=True)
for variant in ('normal', 'panic', 'timeout', 'crash'):
    print('Building ' + variant, file=sys.stderr, flush=True)
    build(eutherboot=args.eutherboot, variant=variant)
report = test(suite='full', promote=args.promote)
print(json.dumps({'passed': report['passed'], 'report': report['path'],
                  'cases': [{'name': c['name'], 'passed': c['passed']} for c in report['cases']]}, indent=2))
sys.exit(0 if report['passed'] else 1)
