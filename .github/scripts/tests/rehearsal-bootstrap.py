#!/usr/bin/env python3
"""Bootstrap function controls; synthetic collaborators are not hosted completion evidence."""
from __future__ import annotations

import copy
import datetime as dt
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
spec = importlib.util.spec_from_file_location('bootstrap_boundary', SCRIPTS / 'operational-assurance.py')
boundary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boundary)
consumer_spec = importlib.util.spec_from_file_location('bootstrap_consumer', SCRIPTS / 'trusted-release.py')
consumer = importlib.util.module_from_spec(consumer_spec)
consumer_spec.loader.exec_module(consumer)


class BootstrapControls(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='oa20-bootstrap-controls-')
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()
        self.root = self.base / 'checkout'
        self.root.mkdir()
        self.temp = self.base / 'runner'
        self.materials = self.temp / 'oa18-materials'
        self.materials.mkdir(parents=True)
        self.candidate = '1' * 40
        self.tag = 'oa20-rehearsal'
        self.scenarios = [f'OA-{n:02}' for n in range(1, 18)]
        contract = {'identity': {}, 'scenarioMatrix': [{'id': s} for s in self.scenarios]}
        self.binding = boundary.sha256(boundary.canonical(contract))
        contract['identity'] = {'contractDigest': self.binding, 'planRevision': 1}
        self.contract = self.root / 'source-contract.json'
        boundary.write(self.contract, contract)
        self.workflow = self.root / boundary.BOUNDARY_WORKFLOW
        self.workflow.parent.mkdir(parents=True)
        self.workflow.write_bytes(b'name: Synthetic bootstrap function control\n')
        self.model = {'contract_digest': self.binding, 'required_scenarios': self.scenarios,
                      'profiles': {p: 'native' for p in ('baseline', 'linux', 'macos', 'windows',
                                                       'android', 'ios', 'macos-gui', 'quiet')},
                      'required_authority': ['G-02', 'G-03', 'G-04', 'review', 'OA15-GUI'],
                      'commands': [], 'budgets': {}}
        self.definition = self.root / '.github/operational-assurance.json'
        boundary.write(self.definition, self.model)
        self.policy = {'schema_version': 1, 'repository': 'Prometheus-AGS/prometheus-parking-lot-rs',
                       'branch': 'main', 'version': '0.4.0', 'contract_digest': self.binding,
                       'boundary_workflow_sha256': boundary.digest(self.workflow),
                       'release_workflow_sha256': '3' * 64, 'max_age_seconds': 3600,
                       'boundary_environment': {'name': 'release-certification', 'id': 7, 'reviewer_ids': [8]},
                       'publish_environment': {'name': 'release-crates-io', 'id': 9, 'reviewer_ids': [8]},
                       'signature': {'public_trust_sha256': '4' * 64}}
        self.policy_path = self.temp / 'release-trust-policy.json'
        boundary.write(self.policy_path, self.policy)
        self.plan = self.materials / 'original-plan.json'
        self.plan_bytes = b'{"candidate":"2222222222222222222222222222222222222222","synthetic_control":true}\n'
        self.plan.write_bytes(self.plan_bytes)
        self.material = {'artifact_id': 42, 'artifact_digest': 'sha256:' + '5' * 64,
                         'member': 'public/original-plan.json', 'sha256': boundary.digest(self.plan),
                         'profiles': ['collector']}
        self.material_manifest = {'schema_version': 1, 'files': {self.plan.name: self.material}}
        boundary.write(self.materials / 'staging-receipt.json',
                       {'schema_version': 1, 'profile': 'collector',
                        'materials': [{'name': self.plan.name, **self.material}]})
        self.raw = self.temp / 'actual-fixture-proof.log'
        self.raw.write_bytes(b'real fixture bytes; not actual operator approval\n')
        self.authority_config = self.temp / 'authority.json'
        authority = {'candidate': self.candidate, 'contract_digest': self.binding,
                     'max_age_seconds': 3600, 'subjects': {}}
        self.reports = {}
        for subject in ('G-02', 'G-03', 'G-04'):
            report = {'candidate': self.candidate, 'contract_digest': self.binding,
                      'status': 'PASS', 'observed_at': dt.datetime.now(dt.timezone.utc).isoformat()}
            if subject == 'G-02':
                report.update({key: '6' * 64 for key in ('approved_rewrite_plan_sha256', 'ref_plan_sha256',
                                                         'cache_removal_evidence_sha256')})
            if subject == 'G-03':
                report.update(tag=self.tag, public_trust_sha256='4' * 64)
            if subject == 'G-04':
                report.update(settings_evidence_sha256='6' * 64, environment_receipt_sha256='7' * 64)
            path = self.temp / (subject + '.json')
            boundary.write(path, report)
            self.reports[subject] = path
            authority['subjects'][subject] = {'report': str(path), 'sha256': boundary.digest(path),
                'source': 'synthetic function fixture',
                'evidence': [{'path': str(self.raw), 'sha256': boundary.digest(self.raw)}]}
        boundary.write(self.authority_config, authority)
        self.inputs = self.temp / 'oa18-inputs.json'
        boundary.write(self.inputs, {'trust_policy': str(self.policy_path),
                                    'preparation_plan': str(self.plan),
                                    'authority_config': str(self.authority_config)})
        self.current = {'id': 40184361135, 'repository': {'full_name': self.policy['repository'], 'id': 11},
                        'head_repository': {'id': 11}, 'head_sha': self.candidate, 'head_branch': 'main',
                        'path': boundary.BOUNDARY_WORKFLOW, 'event': 'workflow_dispatch',
                        'run_attempt': 1, 'status': 'in_progress'}
        self.approvals = [{'environments': [{'id': 7, 'name': 'release-certification'}],
                           'state': 'approved', 'user': {'id': 8}}]
        self.signed = {'status': 'PASS', 'candidate': self.candidate, 'tag': self.tag,
                       'tag_object': '8' * 40, 'format': 'ssh'}
        self.counter = 0
        env = {**os.environ, 'GITHUB_ACTIONS': 'true', 'BOOTSTRAP_RUNNER_ENVIRONMENT': 'github-hosted',
               'GITHUB_SHA': self.candidate, 'GITHUB_REF': 'refs/heads/main', 'GITHUB_REF_PROTECTED': 'true',
               'GITHUB_RUN_ATTEMPT': '1', 'GITHUB_EVENT_NAME': 'workflow_dispatch',
               'GITHUB_REPOSITORY': self.policy['repository'], 'GITHUB_WORKSPACE': str(self.root),
               'GITHUB_WORKFLOW_REF': self.policy['repository'] + '/' + boundary.BOUNDARY_WORKFLOW + '@refs/heads/main',
               'GITHUB_RUN_ID': '40184361135', 'RUNNER_TEMP': str(self.temp),
               'RELEASE_TRUST_POLICY_JSON': json.dumps(self.policy),
               'OA18_INPUTS_JSON': json.dumps({'collector': {'preparation_plan': 'material:' + self.plan.name}}),
               'OA18_MATERIALS_JSON': json.dumps(self.material_manifest),
               'OA18_AUTHORITY_SHA256': boundary.digest(self.authority_config)}
        for key in ('GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_OBJECT_DIRECTORY',
                    'GIT_ALTERNATE_OBJECT_DIRECTORIES'):
            env.pop(key, None)
        env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull)
        self.addCleanup(patch.stopall)
        patch.dict(os.environ, env, clear=True).start()
        subprocess.run(['git', 'init', '-q'], cwd=self.root, check=True, capture_output=True)
        subprocess.run(['git', 'add', '.'], cwd=self.root, check=True, capture_output=True)
        patch.object(boundary, 'ROOT', self.root).start()
        patch.object(boundary, 'DEFINITION', self.definition).start()
        patch.object(boundary, 'definition', return_value=self.model).start()
        self.clean = patch.object(boundary, 'clean').start()
        self.prepare = patch.object(boundary, 'validate_preparation').start()
        self.verify_tag = patch.object(boundary, 'verify_candidate_tag', return_value=(self.signed, '4' * 64)).start()
        api_class = patch.object(boundary, 'GitHubAPI').start()
        self.api = api_class.return_value
        self.api.get.side_effect = lambda path: copy.deepcopy(self.approvals if path.endswith('/approvals') else self.current)
        self.api.workflow_digest.return_value = self.policy['boundary_workflow_sha256']
        for name in ('make_freeze', 'run', 'execute', 'fetch_jobs', 'import_manual_jobs'):
            patch.object(boundary, name, side_effect=AssertionError('production path forbidden: ' + name)).start()

    def args(self):
        self.counter += 1
        return SimpleNamespace(candidate=self.candidate, contract=self.contract, policy=self.policy_path,
                               inputs=self.inputs, tag=self.tag, preparation_plan=self.plan,
                               out=self.base / ('output-' + str(self.counter)))

    def bootstrap(self):
        args = self.args()
        self.assertEqual(boundary.bootstrap_rehearsal(args), 0)
        return args, json.loads((args.out / 'compact/boundary-receipt.json').read_bytes())

    def reject(self):
        args = self.args()
        with self.assertRaises((boundary.ReleaseError, OSError, ValueError, KeyError, TypeError)):
            boundary.bootstrap_rehearsal(args)
        self.assertFalse((args.out / 'compact').exists())

    def test_exact_shared_protocol_has_no_executed_or_passing_oracles(self):
        args, receipt = self.bootstrap()
        boundary.validate_receipt(receipt, self.candidate, self.binding, self.tag, '0.4.0', publication=False)
        self.assertEqual({p.name for p in (args.out / 'compact').iterdir()},
                         {'boundary-receipt.json', 'release-plan.json'})
        self.assertEqual((args.out / 'compact/release-plan.json').read_bytes(), self.plan_bytes)
        self.assertEqual(receipt['purpose'], 'rehearsal')
        self.assertEqual(receipt['status'], 'NOT_EXERCISED')
        self.assertEqual({r['id'] for r in receipt['scenarios']}, set(self.scenarios))
        for oracle in [*receipt['scenarios'], receipt['baseline'], receipt['review'],
                       *receipt['operator_gates'].values()]:
            self.assertEqual(oracle['status'], 'NOT_EXERCISED')
            self.assertNotIn('receipt_sha256', oracle)
        inspection_path = args.out / 'inspection-manifest.json'
        inspection = json.loads(inspection_path.read_bytes())
        self.assertEqual(inspection['production_profiles_executed'], [])
        self.assertEqual(receipt['manifest_sha256'], boundary.digest(inspection_path))
        self.assertTrue(inspection['source_sha256'])
        self.assertEqual(json.loads((args.out / 'workflow-run.json').read_bytes()), self.current)
        self.assertEqual(json.loads((args.out / 'environment-approvals.json').read_bytes()), self.approvals)
        for subject in ('G-02', 'G-03', 'G-04'):
            retained = inspection['authority'][subject]['retained']
            self.assertTrue(retained)
            for item in retained:
                self.assertEqual(boundary.digest(args.out / item['path']), item['sha256'])
        self.prepare.assert_called_once_with(self.root, json.loads(self.plan_bytes), self.candidate, '0.4.0')
        self.assertEqual(self.clean.call_count, 2)

    def test_actual_consumer_authorize_and_publish_reject_bootstrap(self):
        _, receipt = self.bootstrap()
        for mode in ('authorize', 'publish'):
            args = SimpleNamespace(workspace=self.root, output=self.base / ('consumer-' + mode),
                                   candidate=self.candidate, policy=self.policy_path, contract=self.contract,
                                   artifact_id=42, tag=self.tag, mode=mode)
            with patch.object(consumer, 'git', side_effect=[self.candidate, '']), \
                    patch.object(consumer, 'GitHubAPI'), \
                    patch.object(consumer, 'fetch_boundary', return_value=(receipt, json.loads(self.plan_bytes), {})), \
                    patch.object(consumer, 'verify_candidate_tag', side_effect=AssertionError('must reject before signing')), \
                    self.assertRaisesRegex(boundary.ReleaseError, 'rehearsal receipt cannot authorize publication'):
                consumer.run(args)
            self.assertFalse((args.output / 'release-gate.json').exists())

    def test_missing_or_wrong_hosted_context_cannot_create_compact(self):
        for key, value in (('GITHUB_ACTIONS', ''), ('BOOTSTRAP_RUNNER_ENVIRONMENT', 'self-hosted'),
                           ('GITHUB_SHA', '9' * 40), ('GITHUB_REF', 'refs/heads/topic'),
                           ('GITHUB_REF_PROTECTED', 'false'), ('GITHUB_RUN_ATTEMPT', '2'),
                           ('GITHUB_EVENT_NAME', 'push'), ('GITHUB_REPOSITORY', 'fork/repo'),
                           ('GITHUB_WORKSPACE', str(self.base)), ('GITHUB_WORKFLOW_REF', 'forged'),
                           ('GITHUB_RUN_ID', 'invalid')):
            with self.subTest(key=key), patch.dict(os.environ, {key: value}):
                self.reject()

    def test_independent_policy_pin_and_source_contract_are_required(self):
        with patch.dict(os.environ, {'RELEASE_TRUST_POLICY_JSON': '{}'}):
            self.reject()
        self.model['contract_digest'] = '9' * 64
        self.reject()

    def test_actual_run_validator_rejects_fork_attempt_and_wrong_workflow(self):
        for key, value in (('id', 99), ('head_repository', {'id': 99}), ('run_attempt', 2),
                           ('path', 'other.yml'), ('head_sha', '9' * 40), ('status', 'completed')):
            with self.subTest(key=key):
                previous = self.current[key]
                self.current[key] = value
                self.reject()
                self.current[key] = previous

    def test_api_workflow_digest_and_approved_environment_are_required(self):
        self.api.workflow_digest.return_value = '0' * 64
        self.reject()
        self.api.workflow_digest.return_value = self.policy['boundary_workflow_sha256']
        for approvals in ([], [{'environments': [{'id': 7, 'name': 'release-certification'}],
                               'state': 'approved', 'user': {'id': 99}}]):
            self.approvals = approvals
            self.reject()

    def test_caller_plan_path_and_material_pin_cannot_replace_original(self):
        replacement = self.temp / 'caller-plan.json'
        replacement.write_bytes(self.plan_bytes)
        args = self.args()
        args.preparation_plan = replacement
        with self.assertRaisesRegex(boundary.ReleaseError, 'path/material scope'):
            boundary.bootstrap_rehearsal(args)
        altered = copy.deepcopy(self.material_manifest)
        altered['files'][self.plan.name]['sha256'] = '0' * 64
        with patch.dict(os.environ, {'OA18_MATERIALS_JSON': json.dumps(altered)}):
            self.reject()
        self.prepare.assert_not_called()

    def test_protected_material_reference_staging_receipt_and_scope_are_required(self):
        with patch.dict(os.environ, {'OA18_INPUTS_JSON': json.dumps({'collector': {'preparation_plan': str(self.plan)}})}):
            self.reject()
        altered = copy.deepcopy(self.material_manifest)
        altered['files'][self.plan.name]['profiles'] = ['baseline']
        with patch.dict(os.environ, {'OA18_MATERIALS_JSON': json.dumps(altered)}):
            self.reject()
        boundary.write(self.materials / 'staging-receipt.json', {'schema_version': 1, 'profile': 'baseline', 'materials': []})
        self.reject()
        self.prepare.assert_not_called()

    def test_candidate_module_import_follows_signature_and_g03_identity(self):
        events = []
        def verify(*_):
            events.append('authenticated signature')
            return self.signed, '4' * 64
        def prepare(*_):
            events.append('candidate preparation module')
        self.verify_tag.side_effect = verify
        self.prepare.side_effect = prepare
        self.bootstrap()
        self.assertEqual(events, ['authenticated signature', 'candidate preparation module'])

    def test_actual_preparation_and_signature_failures_block_compact(self):
        self.prepare.side_effect = boundary.ReleaseError('actual preparation failed')
        self.reject()
        self.prepare.side_effect = None
        self.verify_tag.side_effect = boundary.ReleaseError('actual signature failed')
        self.reject()

    def test_real_authority_digest_and_raw_proof_are_required(self):
        with patch.dict(os.environ, {'OA18_AUTHORITY_SHA256': '0' * 64}):
            self.reject()
        self.raw.write_bytes(b'changed raw proof')
        self.reject()

    def test_g03_exact_tag_and_public_trust_are_required(self):
        self.signed['tag'] = 'wrong-tag'
        self.reject()
        self.signed['tag'] = self.tag
        self.verify_tag.return_value = (self.signed, '9' * 64)
        self.reject()
        self.prepare.assert_not_called()

    def test_output_must_be_fresh_and_external(self):
        for output in (self.root / 'output', self.temp):
            args = self.args()
            args.out = output
            with self.assertRaisesRegex(boundary.ReleaseError, 'fresh external'):
                boundary.bootstrap_rehearsal(args)

    def test_source_change_after_inspection_blocks_compact(self):
        with patch.object(boundary, 'sources', side_effect=[{'file': '1'}, {'file': '2'}]):
            self.reject()

    def test_full_certification_with_missing_profiles_still_cannot_emit_compact(self):
        args = self.args()
        args.receipts = self.base / 'missing-profiles'
        args.receipts.mkdir()
        args.trusted = False
        args.purpose = 'certification'
        self.assertEqual(boundary.assemble(args), 2)
        self.assertFalse((args.out / 'compact').exists())
        packet = json.loads((args.out / 'packet.json').read_bytes())
        self.assertEqual(packet['status'], 'BLOCKED')
        self.assertTrue(all(item['status'] == 'BLOCKED' for item in packet['profiles'].values()))

    def test_cli_has_no_purpose_or_pass_override(self):
        for flag in ('--purpose', '--status', '--oracle'):
            with patch.object(sys, 'argv', ['operational-assurance.py', 'bootstrap-rehearsal',
                    '--candidate', self.candidate, '--tag', self.tag, '--contract', str(self.contract),
                    '--policy', str(self.policy_path), '--inputs', str(self.inputs),
                    '--preparation-plan', str(self.plan), '--out', str(self.base / 'cli-out'), flag, 'PASS']), \
                    self.assertRaises(SystemExit) as rejected:
                boundary.main()
            self.assertEqual(rejected.exception.code, 2)


if __name__ == '__main__':
    unittest.main(verbosity=2)
