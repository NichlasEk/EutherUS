import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from lab import controller as lab


class ProtocolTests(unittest.TestCase):
    def test_serial_ignores_firmware_and_wrong_protocol(self):
        text = '\x1b[2JUEFI menu\nnot json\n[]\n' + '\n'.join(map(json.dumps, [
            {'protocol': 'eutherus.boot', 'version': 2, 'event': 'boot_success'},
            {'protocol': 'other', 'version': 1, 'event': 'boot_success'},
            {'protocol': 'eutherus.boot', 'version': 1, 'event': 'panic'},
        ]))
        self.assertEqual([e['event'] for e in lab.events(text)], ['panic'])

    def test_cpu_exception_overrides_prior_boot_success(self):
        boot = [dict(protocol='eutherus.boot', version=1, event=event) for event in lab.EVENTS]
        fault = dict(protocol='eutherus.cpu', version=1, event='exception', vector=14)
        output = 'EutherUS 0.0.1\n' + '\n'.join(map(json.dumps, boot+[fault]))
        state = dict(build={}, config={}, argv=[])
        with patch.object(lab, 'load_vm', return_value=state), \
             patch.object(lab, 'alive', return_value=True), \
             patch.object(lab, 'logs', return_value={'serial': output}):
            observed = lab.observe('exception-after-boot')
        self.assertEqual(observed['outcome'], 'exception')
        self.assertEqual(observed['cpu_events'], [fault])
        self.assertEqual([e['event'] for e in observed['events']], lab.EVENTS)

    def test_memory_failure_is_not_hidden_by_boot_success(self):
        boot = [dict(protocol='eutherus.boot', version=1, event=e) for e in lab.EVENTS]
        oom = dict(protocol='eutherus.memory', version=1, event='out_of_memory', requested_pages=1)
        output = 'EutherUS 0.0.1\n' + '\n'.join(map(json.dumps, boot+[oom]))
        with patch.object(lab, 'load_vm', return_value=dict(build={}, config={}, argv=[])), \
             patch.object(lab, 'alive', return_value=True), \
             patch.object(lab, 'logs', return_value={'serial': output}):
            observed = lab.observe('oom-after-boot')
        self.assertEqual(observed['outcome'], 'out_of_memory')
        self.assertEqual(observed['memory_events'], [oom])

    def test_traversal_and_invalid_names(self):
        for name in ('../outside', '/tmp/host', '', 'a/b', 'x' * 25, 'a\n'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                lab.vm_dir(name)
        for name in ('vm-../outside', '/tmp/host', '', 'vm-a/b', 'x' * 41, 'vm-a\n'):
            with self.subTest(lock=name), self.assertRaises(ValueError):
                with lab.lock(name):
                    self.fail('invalid lock accepted')

    def test_clean_refuses_live_vm(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(lab, 'STATE', Path(tmp)):
            path = lab.vm_dir('live')
            path.mkdir(parents=True)
            lab.write_json(path / 'state.json', {'pid': 1, 'process_start': '123'})
            with patch.object(lab, 'alive', return_value=True), self.assertRaises(ValueError):
                lab.clean('live')
            self.assertTrue(path.exists())

    def test_clean_rejects_symlink(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(lab, 'STATE', Path(tmp) / 'state'):
            outside = Path(tmp) / 'outside'
            outside.mkdir()
            (outside / 'keep').write_text('untouched')
            (lab.STATE / 'vms').mkdir(parents=True)
            (lab.STATE / 'vms/evil').symlink_to(outside, target_is_directory=True)
            with self.assertRaises(ValueError):
                lab.clean('evil')
            self.assertEqual((outside / 'keep').read_text(), 'untouched')

    def test_pid_reuse_is_not_ours(self):
        with patch.object(lab, 'proc_identity', return_value='new'):
            self.assertFalse(lab.alive({'pid': 123, 'process_start': 'old'}))

    def test_api_rejects_unknown_methods_parameters_and_types(self):
        for request in ([], {}, {'version': True, 'method': 'status'}, {'version': 1, 'method': 'shell'},
                        {'version': 1, 'method': 'boot', 'params': {'disk': '/dev/sda'}},
                        {'version': 1, 'method': 'boot', 'params': {'paused': 'false'}},
                        {'version': 1, 'method': 'test', 'params': {'timeout': -1}}):
            with self.subTest(request=request), self.assertRaises(ValueError):
                lab.dispatch(request)

    def test_jsonlines_error_does_not_break_next_request(self):
        p = subprocess.run([sys.executable, str(lab.ROOT / 'eutherlab'), 'api'],
                           input='invalid\n[]\n{"version":1,"id":"x","method":"shell"}\n',
                           text=True, capture_output=True, check=True)
        replies = [json.loads(line) for line in p.stdout.splitlines()]
        self.assertEqual(len(replies), 3)
        self.assertTrue(all(not r['ok'] for r in replies))
        self.assertEqual(replies[-1]['id'], 'x')


if __name__ == '__main__':
    unittest.main()
