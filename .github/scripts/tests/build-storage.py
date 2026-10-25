#!/usr/bin/env python3
"""Real-filesystem fixtures for scoped hosted OA-12 scratch reclamation."""
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import assurance_build_storage as storage

ACTUAL_MANIFEST = storage.ROOT / '.github/negative-controls.json'
BOUNDARY_SPEC = importlib.util.spec_from_file_location(
    'storage_boundary', Path(__file__).resolve().parents[1] / 'operational-assurance.py')
boundary = importlib.util.module_from_spec(BOUNDARY_SPEC)
BOUNDARY_SPEC.loader.exec_module(boundary)


class BuildStorage(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.root = self.base / 'checkout'
        (self.root / '.github').mkdir(parents=True)
        (self.root / '.github/negative-controls.json').write_bytes(ACTUAL_MANIFEST.read_bytes())
        self.case = json.loads(ACTUAL_MANIFEST.read_bytes())['cases'][0]['id']
        self.scratch = self.root / 'target/oa12' / self.case
        self.output = self.base / 'raw-evidence'
        self.output.mkdir()
        self.raw = self.output / 'normal.stdout.log'
        self.raw.write_bytes(b'original oracle raw proof\n')
        self.addCleanup(patch.stopall)
        patch.object(storage, 'ROOT', self.root).start()
        patch.dict(os.environ, {'GITHUB_ACTIONS': 'true',
                               'BOOTSTRAP_RUNNER_ENVIRONMENT': 'github-hosted',
                               'GITHUB_RUN_ID': '40174959655',
                               'GITHUB_WORKSPACE': str(self.root)}).start()

    def begin(self):
        return storage.begin_case(self.case, '8a8adf90677498342bdf106c2b6be20c1019817b', self.output)

    def build_files(self):
        for mode in ('normal', 'mutant'):
            folder = self.scratch / mode / 'debug'
            folder.mkdir(parents=True)
            (folder / 'artifact').write_bytes(b'build artifact' * 1024)

    def test_exact_owned_case_cleanup_preserves_siblings_and_raw_proof(self):
        lease = self.begin()
        self.build_files()
        sibling = self.scratch.parent / 'another-case'
        sibling.mkdir()
        (sibling / 'artifact').write_bytes(b'keep sibling')
        shared = self.root / 'target/debug'
        shared.mkdir()
        (shared / 'artifact').write_bytes(b'keep shared')
        receipt = storage.finish_case(lease, 0, row_passed=True)
        self.assertEqual(receipt['status'], 'RECLAIMED')
        self.assertGreater(receipt['reclaimed_bytes'], 0)
        self.assertIn('free_bytes_before', receipt)
        self.assertIn('free_bytes_after', receipt)
        self.assertFalse(self.scratch.exists())
        self.assertEqual((sibling / 'artifact').read_bytes(), b'keep sibling')
        self.assertEqual((shared / 'artifact').read_bytes(), b'keep shared')
        self.assertEqual(self.raw.read_bytes(), b'original oracle raw proof\n')
        self.assertEqual(receipt['owner']['output'], str(self.output))

    def test_failed_or_blocked_outer_exits_retain_even_with_pass_flag(self):
        lease = self.begin()
        self.build_files()
        for code in (1, 2):
            with self.subTest(exit_code=code):
                receipt = storage.finish_case(lease, code, row_passed=True)
                self.assertEqual(receipt['status'], 'NOT_RECLAIMED')
                self.assertEqual(receipt['exit_code'], code)
                self.assertTrue(receipt['row_passed'])
                self.assertTrue((self.scratch / 'normal/debug/artifact').exists())

    def test_zero_exit_without_explicit_validated_pass_retains(self):
        lease = self.begin()
        self.build_files()
        self.assertEqual(storage.finish_case(lease, 0)['status'], 'NOT_RECLAIMED')
        for passed in (False, None, 1, 'PASS'):
            with self.subTest(row_passed=passed):
                receipt = storage.finish_case(lease, 0, row_passed=passed)
                self.assertEqual(receipt['status'], 'NOT_RECLAIMED')
                self.assertEqual(receipt['row_passed'], passed)
        self.assertTrue(self.scratch.exists())

    def test_real_inner_watchdog_failure_cannot_reclaim_scratch(self):
        lease = self.begin()
        self.build_files()
        watchdog_output = self.output / 'actual-watchdog'
        runner_path = Path(__file__).resolve().with_name('negative-controls.py')
        # Exercise actual Runner.command and main's Failure -> exit 1 handler.
        # Substitute only run_case to avoid invoking unrelated Cargo operations.
        probe = """
import importlib.util
from pathlib import Path
import sys
spec = importlib.util.spec_from_file_location('negative_controls', sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
output = Path(sys.argv[2])
def watchdog_row(args, manifest):
    module.Runner(args.output).command(
        'watchdog', [sys.executable, '-c',
                     "import time; print('watchdog raw witness', flush=True); time.sleep(30)"],
        timeout=0.5)
module.run_case = watchdog_row
sys.argv = [sys.argv[1], 'run', '--case', sys.argv[3], '--output', str(output)]
sys.exit(module.main())
"""
        process = subprocess.run([sys.executable, '-B', '-c', probe, str(runner_path),
                                  str(watchdog_output), self.case],
                                 capture_output=True, text=True, timeout=15)
        (self.output / 'outer.stdout.log').write_text(process.stdout)
        (self.output / 'outer.stderr.log').write_text(process.stderr)
        self.assertEqual(process.returncode, 1)
        self.assertIn('FAIL: watchdog: watchdog is not a designated assertion', process.stderr)
        commands = json.loads((watchdog_output / 'commands.json').read_bytes())
        self.assertEqual(commands[0]['exit'], 'TIMEOUT')
        raw = watchdog_output / commands[0]['stdout']
        self.assertEqual(raw.read_text(), 'watchdog raw witness\n')
        proof = raw.read_bytes()
        self.assertEqual(hashlib.sha256(proof).hexdigest(), commands[0]['stdout_sha256'])
        receipt = storage.finish_case(lease, process.returncode, row_passed=False)
        self.assertEqual(receipt['status'], 'NOT_RECLAIMED')
        self.assertFalse(receipt['row_passed'])
        self.assertTrue((self.scratch / 'mutant/debug/artifact').exists())
        self.assertEqual(raw.read_bytes(), proof)
        self.assertEqual(json.loads((watchdog_output / 'commands.json').read_bytes()), commands)
        self.assertEqual(self.raw.read_bytes(), b'original oracle raw proof\n')

    def run_parent_validation_failure(self, malformed_receipt):
        # Component integration only: bypass freeze/model/source preflight, while
        # running the real parent row loop, execute(), helper lease and cleanup.
        candidate = '8a8adf90677498342bdf106c2b6be20c1019817b'
        binding = '2' * 64
        output = self.base / 'parent-boundary'
        child_receipt = output / 'child-receipt.json'
        payload = [] if malformed_receipt else {
            'status': 'PASS', 'candidate': candidate, 'contract_digest': binding}
        child_script = """
import json
from pathlib import Path
import sys
scratch = Path(sys.argv[1])
for mode in ('normal', 'mutant'):
    folder = scratch / mode
    folder.mkdir()
    (folder / 'artifact').write_bytes(b'parent integration scratch')
Path(sys.argv[2]).write_text(sys.argv[3])
print('parent child raw proof', flush=True)
"""
        argv = [sys.executable, '-B', '-c', child_script, str(self.scratch),
                str(child_receipt), json.dumps(payload)]
        row_id = 'oa12-' + self.case
        row = {'id': row_id, 'scenarios': ['OA-12'], 'tools': [], 'argv': argv,
               'cwd': str(self.root), 'timeout': 10, 'receipt': str(child_receipt)}
        model = {'profiles': {'linux': 'native'},
                 'budgets': {'active_minutes': 1, 'commands': 1}}
        manifest = {'commands': [row], 'inputs': {}, 'tools': {}, 'images': {},
                    'source_sha256': {}, 'contract_digest': binding}
        args = SimpleNamespace(out=output, profile='linux', candidate=candidate,
                               freeze_only=False)
        cleanup_receipts = []

        def freeze(*_):
            boundary.write(output / 'checked-manifest.json', manifest)
            return manifest

        def finish(lease, exit_code, *, row_passed=False):
            receipt = storage.finish_case(lease, exit_code, row_passed=row_passed)
            cleanup_receipts.append(receipt)
            return receipt

        final_git_failure = subprocess.CalledProcessError(
            128, ['git', 'status', '--porcelain'], stderr='injected final Git failure')
        clean_results = [None] if malformed_receipt else [None, final_git_failure]
        expected_exception = AttributeError if malformed_receipt else subprocess.CalledProcessError
        with patch.object(boundary, 'ROOT', self.root), \
                patch.object(boundary, 'definition', return_value=model), \
                patch.object(boundary, 'make_freeze', side_effect=freeze), \
                patch.object(boundary, 'sources', return_value={}), \
                patch.object(boundary, 'clean', side_effect=clean_results), \
                patch.object(boundary, 'finish_case', side_effect=finish), \
                patch.dict(os.environ, {'CARGO_TARGET_DIR': str(self.root / 'target')}):
            with self.assertRaises(expected_exception):
                boundary.run(args)
        self.assertEqual(len(cleanup_receipts), 1)
        receipt = cleanup_receipts[0]
        self.assertEqual(receipt['status'], 'NOT_RECLAIMED')
        self.assertEqual(receipt['exit_code'], 0)
        self.assertIs(receipt['row_passed'], False)
        self.assertEqual(json.loads((self.scratch / storage.MARKER).read_bytes()), receipt['owner'])
        for mode in ('normal', 'mutant'):
            self.assertEqual((self.scratch / mode / 'artifact').read_bytes(),
                             b'parent integration scratch')
        self.assertEqual(json.loads(child_receipt.read_bytes()), payload)
        self.assertEqual((output / (row_id + '.stdout.txt')).read_text(),
                         'parent child raw proof\n')
        self.assertEqual((output / (row_id + '.stderr.txt')).read_bytes(), b'')
        frozen = json.loads((output / 'checked-manifest.json').read_bytes())
        self.assertEqual(frozen['commands'][0]['argv'], argv)
        lock = self.root / 'target/.oa18-writer-lock'
        self.assertTrue(lock.is_dir())
        owner = json.loads((lock / 'owner.json').read_bytes())
        self.assertEqual(owner['candidate'], candidate)
        self.assertEqual(owner['out'], str(output))
        self.assertEqual(self.raw.read_bytes(), b'original oracle raw proof\n')

    def test_parent_malformed_child_receipt_retains_scratch_proof_and_lock(self):
        self.run_parent_validation_failure(malformed_receipt=True)

    def test_parent_final_clean_failure_retains_scratch_proof_and_lock(self):
        self.run_parent_validation_failure(malformed_receipt=False)

    def test_local_and_self_hosted_are_noops(self):
        for setting in ({'GITHUB_ACTIONS': 'false'},
                        {'BOOTSTRAP_RUNNER_ENVIRONMENT': 'self-hosted'},
                        {'BOOTSTRAP_RUNNER_ENVIRONMENT': ''}):
            with self.subTest(setting=setting), patch.dict(os.environ, setting):
                self.assertIsNone(self.begin())
                self.assertEqual(storage.finish_case(None, 0)['status'], 'DISABLED')
                self.assertFalse((self.root / 'target').exists())

    def test_preexisting_unowned_case_is_refused(self):
        self.scratch.mkdir(parents=True)
        (self.scratch / 'artifact').write_bytes(b'keep')
        with self.assertRaises(FileExistsError):
            self.begin()
        self.assertEqual((self.scratch / 'artifact').read_bytes(), b'keep')

    def test_parent_symlinks_are_refused(self):
        outside = self.base / 'outside'
        outside.mkdir()
        for component in ('target', 'oa12'):
            with self.subTest(component=component):
                parent = self.root if component == 'target' else self.root / 'target'
                parent.mkdir(exist_ok=True)
                link = parent / component
                link.symlink_to(outside, target_is_directory=True)
                with self.assertRaises(OSError):
                    self.begin()
                self.assertEqual(list(outside.iterdir()), [])
                link.unlink()

    def test_redirected_case_is_refused(self):
        self.scratch.parent.mkdir(parents=True)
        self.scratch.symlink_to(self.output, target_is_directory=True)
        with self.assertRaises(FileExistsError):
            self.begin()
        self.assertTrue(self.raw.exists())

    def test_redirected_descendant_retains_entire_case(self):
        lease = self.begin()
        self.build_files()
        (self.scratch / 'normal/redirect').symlink_to(self.output, target_is_directory=True)
        receipt = storage.finish_case(lease, 0, row_passed=True)
        self.assertEqual(receipt['status'], 'NOT_RECLAIMED')
        self.assertTrue((self.scratch / 'mutant/debug/artifact').exists())
        self.assertTrue(self.raw.exists())

    def test_unknown_timeout_signal_exits_retain(self):
        lease = self.begin()
        self.build_files()
        for code in (None, 'TIMEOUT', -9, 124, 137, 3, True):
            with self.subTest(exit_code=code):
                receipt = storage.finish_case(lease, code)
                self.assertEqual(receipt['status'], 'NOT_RECLAIMED')
                self.assertTrue((self.scratch / 'normal/debug/artifact').exists())

    def test_hosted_context_mismatch_refuses_claim(self):
        for setting in ({'GITHUB_RUN_ID': 'bad-run'}, {'GITHUB_RUN_ID': ''},
                        {'GITHUB_WORKSPACE': str(self.root) + '/.'},
                        {'GITHUB_WORKSPACE': str(self.base)}):
            with self.subTest(setting=setting), patch.dict(os.environ, setting):
                with self.assertRaises(ValueError):
                    self.begin()
                self.assertFalse(self.scratch.exists())

    def test_changed_context_retains_claim(self):
        lease = self.begin()
        with patch.dict(os.environ, {'GITHUB_RUN_ID': '999'}):
            self.assertEqual(storage.finish_case(lease, 0, row_passed=True)['status'], 'NOT_RECLAIMED')
        self.assertTrue(self.scratch.exists())

    def test_unknown_or_traversing_case_refused(self):
        for case in ('../escape', 'not-in-manifest', '.', 's-1/child'):
            with self.subTest(case=case), self.assertRaises(ValueError):
                storage.begin_case(case, 'candidate', self.output)
        self.assertFalse((self.root / 'target').exists())

    def test_evidence_inside_checkout_refused(self):
        with self.assertRaises(ValueError):
            storage.begin_case(self.case, 'candidate', self.scratch / 'logs')
        self.assertFalse(self.scratch.exists())

    def test_changed_marker_retains_case(self):
        lease = self.begin()
        (self.scratch / storage.MARKER).write_text('{}')
        self.assertEqual(storage.finish_case(lease, 0, row_passed=True)['status'], 'NOT_RECLAIMED')
        self.assertTrue(self.scratch.exists())

    def test_redirected_parent_after_claim_retains_case(self):
        lease = self.begin()
        original = self.root / 'target-original'
        (self.root / 'target').rename(original)
        (self.root / 'target').symlink_to(original, target_is_directory=True)
        self.assertEqual(storage.finish_case(lease, 0, row_passed=True)['status'], 'NOT_RECLAIMED')
        self.assertTrue((original / 'oa12' / self.case).exists())

    def test_marker_fifo_does_not_block_cleanup_check(self):
        lease = self.begin()
        marker = self.scratch / storage.MARKER
        marker.unlink()
        os.mkfifo(marker)
        self.assertEqual(storage.finish_case(lease, 0, row_passed=True)['status'], 'NOT_RECLAIMED')
        self.assertTrue(self.scratch.exists())

    def test_replaced_directory_retains_replacement(self):
        lease = self.begin()
        original = self.scratch.with_name(self.case + '-original')
        self.scratch.rename(original)
        self.scratch.mkdir()
        (self.scratch / storage.MARKER).write_bytes((original / storage.MARKER).read_bytes())
        self.assertEqual(storage.finish_case(lease, 0, row_passed=True)['status'], 'NOT_RECLAIMED')
        self.assertTrue(self.scratch.exists())
        self.assertTrue(original.exists())


if __name__ == '__main__':
    unittest.main()
