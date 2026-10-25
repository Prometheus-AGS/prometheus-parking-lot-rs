#!/usr/bin/env python3
"""Exercise the actual public immutable-material staging trust boundary offline."""
from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
spec = importlib.util.spec_from_file_location('hosted_materials', SCRIPTS / 'prepare-hosted-assurance.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
trust_spec = importlib.util.spec_from_file_location('release_trust_staging', SCRIPTS / 'stage-release-trust.py')
trust = importlib.util.module_from_spec(trust_spec)
trust_spec.loader.exec_module(trust)
platform_spec = importlib.util.spec_from_file_location('hosted_platform', SCRIPTS / 'prepare-hosted-assurance-platform.py')
platform_setup = importlib.util.module_from_spec(platform_spec)
platform_spec.loader.exec_module(platform_setup)


class Materials(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.destination = Path(self.temp.name) / 'materials'
        self.payload = b'{"authority":"public original bytes"}\n'
        self.archive = self.zip_bytes([('public/authority.json', self.payload)])
        self.record = {'artifact_id': 42, 'artifact_digest': 'sha256:' + hashlib.sha256(self.archive).hexdigest(),
                       'member': 'public/authority.json', 'sha256': hashlib.sha256(self.payload).hexdigest(),
                       'profiles': ['collector']}

    @staticmethod
    def zip_bytes(entries):
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, 'w') as bundle:
            for name, value in entries:
                bundle.writestr(name, value)
        return stream.getvalue()

    def run_stage(self, record=None, archive=None, metadata=None, name='authority.json', profile='collector'):
        record = self.record if record is None else record
        archive = self.archive if archive is None else archive
        metadata = ({'id': 42, 'expired': False, 'digest': record['artifact_digest'], 'size_in_bytes': len(archive)}
                    if metadata is None else metadata)
        with patch.dict(os.environ, {'GITHUB_REPOSITORY': 'example/project',
                                     'OA18_MATERIALS_JSON': json.dumps({'schema_version': 1, 'files': {name: record}})}):
            with patch.object(module, 'GitHubAPI') as api:
                api.return_value.get.return_value = metadata
                api.return_value.opener.open.return_value = io.BytesIO(archive)
                return module.stage(profile, self.destination)

    def test_exact_public_bytes_preserved_outside_checkout(self):
        result = self.run_stage()
        self.assertEqual(result['authority.json'].read_bytes(), self.payload)
        receipt = json.loads((self.destination / 'staging-receipt.json').read_bytes())
        self.assertEqual(receipt['materials'][0]['sha256'], self.record['sha256'])

    def test_missing_independent_manifest_blocks(self):
        with patch.dict(os.environ, {'OA18_MATERIALS_JSON': '{}'}):
            with self.assertRaises(module.ReleaseError):
                module.stage('collector', self.destination)

    def test_artifact_metadata_digest_cannot_replace_independent_pin(self):
        with self.assertRaises(module.ReleaseError):
            self.run_stage(metadata={'id': 42, 'expired': False, 'digest': 'sha256:' + 'a' * 64, 'size_in_bytes': 100})

    def test_expired_artifact_blocks(self):
        with self.assertRaises(module.ReleaseError):
            self.run_stage(metadata={'id': 42, 'expired': True, 'digest': self.record['artifact_digest'], 'size_in_bytes': 100})

    def test_download_bytes_must_match_pinned_archive(self):
        with self.assertRaises(module.ReleaseError):
            self.run_stage(archive=self.archive + b'changed')

    def test_member_bytes_must_match_separate_pin(self):
        with self.assertRaises(module.ReleaseError):
            self.run_stage(record={**self.record, 'sha256': 'b' * 64})

    def test_unknown_schema_field_blocks(self):
        with self.assertRaises(module.ReleaseError):
            self.run_stage(record={**self.record, 'download_url': 'https://example.invalid/caller-url'})

    def test_unselected_profile_does_not_download_manual_archive(self):
        result = self.run_stage(profile='baseline')
        self.assertEqual(result, {})
        self.assertFalse((self.destination / 'artifact-42.zip').exists())

    def test_destination_traversal_and_internal_names_block(self):
        for name in ('../authority.json', 'folder/authority.json', 'staging-receipt.json', 'artifact-42.zip'):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as folder:
                self.destination = Path(folder) / 'materials'
                with self.assertRaises(module.ReleaseError):
                    self.run_stage(name=name)

    def test_even_unselected_zip_traversal_member_blocks(self):
        archive = self.zip_bytes([('public/authority.json', self.payload), ('../escape', b'unsafe')])
        with self.assertRaises(module.ReleaseError):
            self.run_stage(archive=archive, record={**self.record, 'artifact_digest': 'sha256:' + hashlib.sha256(archive).hexdigest()})

    def test_zip_symlink_blocks(self):
        link = zipfile.ZipInfo('public/link')
        link.create_system = 3
        link.external_attr = (stat.S_IFLNK | 0o777) << 16
        archive = self.zip_bytes([('public/authority.json', self.payload), (link, b'../escape')])
        with self.assertRaises(module.ReleaseError):
            self.run_stage(archive=archive, record={**self.record, 'artifact_digest': 'sha256:' + hashlib.sha256(archive).hexdigest()})

    def test_existing_destination_is_never_overwritten(self):
        self.destination.mkdir()
        with self.assertRaises(FileExistsError):
            self.run_stage()

    def public_policy(self, trust_file, raw):
        public = {'format': 'ssh', 'identity': 'release@example.invalid', 'fingerprint': 'SHA256:fixture',
                  'allowed_signers_sha256': trust.sha256(raw.encode()), 'revocations_sha256': None}
        return {'schema_version': 1, 'repository': 'Prometheus-AGS/prometheus-parking-lot-rs',
                'branch': 'main', 'version': '0.4.0', 'max_age_seconds': 3600,
                'boundary_workflow_sha256': 'a' * 64, 'release_workflow_sha256': 'b' * 64,
                'contract_digest': 'c' * 64,
                'boundary_environment': {'name': 'release-certification', 'id': 1, 'reviewer_ids': [1]},
                'publish_environment': {'name': 'release-crates-io', 'id': 2, 'reviewer_ids': [1]},
                'signature': {**{k: public[k] for k in ('format', 'identity', 'fingerprint')},
                              'allowed_signers_file': str(trust_file),
                              'public_trust_sha256': trust.sha256(trust.canonical(public))}}

    def test_public_trust_stages_original_approved_bytes(self):
        trust_file = Path(self.temp.name) / 'trust' / 'allowed_signers'
        raw = 'release@example.invalid ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFixture\n'
        policy = self.public_policy(trust_file, raw)
        with patch.object(trust, 'TRUST_FILE', trust_file), patch.dict(os.environ, {'RELEASE_SSH_ALLOWED_SIGNERS': raw}):
            trust.stage_public_trust(policy)
        self.assertEqual(trust_file.read_bytes(), raw.encode())

    def test_changed_public_trust_bytes_block_before_write(self):
        trust_file = Path(self.temp.name) / 'trust' / 'allowed_signers'
        policy = self.public_policy(trust_file, 'approved\n')
        with patch.object(trust, 'TRUST_FILE', trust_file), patch.dict(os.environ, {'RELEASE_SSH_ALLOWED_SIGNERS': 'different\n'}):
            with self.assertRaises(module.ReleaseError):
                trust.stage_public_trust(policy)
        self.assertFalse(trust_file.exists())

    def test_public_trust_cannot_rewrite_policy_path_or_drop_revocations(self):
        trust_file = Path(self.temp.name) / 'trust' / 'allowed_signers'
        for extra in ({'allowed_signers_file': '/some/other/path'}, {'revocations_file': '/approved/revocations'}):
            policy = self.public_policy(trust_file, 'approved\n')
            policy['signature'].update(extra)
            with self.subTest(extra=extra), patch.object(trust, 'TRUST_FILE', trust_file), \
                    patch.dict(os.environ, {'RELEASE_SSH_ALLOWED_SIGNERS': 'approved\n'}):
                with self.assertRaises(module.ReleaseError):
                    trust.stage_public_trust(policy)
        self.assertFalse(trust_file.exists())

    def test_public_trust_identical_bytes_are_idempotent(self):
        trust_file = Path(self.temp.name) / 'trust' / 'allowed_signers'
        raw = 'approved\n'
        policy = self.public_policy(trust_file, raw)
        with patch.object(trust, 'TRUST_FILE', trust_file), patch.dict(os.environ, {'RELEASE_SSH_ALLOWED_SIGNERS': raw}):
            trust.stage_public_trust(policy)
            trust.stage_public_trust(policy)
        self.assertEqual(trust_file.read_bytes(), raw.encode())

    def test_preexisting_different_public_trust_and_symlinks_block(self):
        trust_file = Path(self.temp.name) / 'trust' / 'allowed_signers'
        trust_file.parent.mkdir()
        trust_file.write_bytes(b'other bytes')
        policy = self.public_policy(trust_file, 'approved\n')
        with patch.object(trust, 'TRUST_FILE', trust_file), patch.dict(os.environ, {'RELEASE_SSH_ALLOWED_SIGNERS': 'approved\n'}):
            with self.assertRaises(module.ReleaseError):
                trust.stage_public_trust(policy)
            self.assertEqual(trust_file.read_bytes(), b'other bytes')
            trust_file.unlink()
            source = Path(self.temp.name) / 'source'
            source.write_bytes(b'approved\n')
            trust_file.symlink_to(source)
            with self.assertRaises(module.ReleaseError):
                trust.stage_public_trust(policy)

    def test_nonmatching_main_candidate_is_rejected_before_fetch(self):
        with patch.dict(os.environ, {'GITHUB_ACTIONS': 'true', 'BOOTSTRAP_RUNNER_ENVIRONMENT': 'github-hosted',
                                     'CANDIDATE': 'a' * 40, 'GITHUB_SHA': 'b' * 40,
                                     'GITHUB_REF': 'refs/heads/main', 'GITHUB_RUN_ATTEMPT': '1'}), \
                patch.object(sys, 'argv', ['prepare-hosted-assurance.py', '--profile', 'collector']), \
                patch.object(module, 'stage') as stage:
            with self.assertRaises(module.ReleaseError):
                module.main()
            stage.assert_not_called()


class AndroidJavaPins(unittest.TestCase):
    """Synthetic setup controls; actual hosted setup remains a separate gate."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        home = self.root / 'sdk/ndk/27.3.13750724'
        home.mkdir(parents=True)
        (home / 'source.properties').write_text('Pkg.Revision = 27.3.13750724\n')

    def prepare(self, expected, actual):
        self.executed = []

        def run(argv, **kwargs):
            self.executed.append(argv)
            if argv == ['java', '-version']:
                return subprocess.CompletedProcess(argv, 0, '', 'openjdk version "' + actual + '"\n')
            return subprocess.CompletedProcess(argv, 0)

        with patch.dict(os.environ, {
                'GITHUB_ACTIONS': 'true', 'BOOTSTRAP_RUNNER_ENVIRONMENT': 'github-hosted',
                'RUNNER_OS': 'Linux', 'RUNNER_TEMP': str(self.root),
                'ANDROID_HOME': str(self.root / 'sdk'), 'GITHUB_ENV': str(self.root / 'environment'),
                'OA18_PLATFORM_JSON': json.dumps({'android_ndk_version': '27.3.13750724',
                                                  'java_version': expected})}), \
                patch.object(sys, 'argv', ['prepare-hosted-assurance-platform.py', '--profile', 'android']), \
                patch.object(platform_setup.subprocess, 'run', side_effect=run) as execute:
            platform_setup.main()
            return execute.call_args_list

    def test_exact_three_and_four_component_java_versions_prepare(self):
        for version in ('17.0.20', '17.0.20.1'):
            with self.subTest(version=version):
                calls = self.prepare(version, version)
                self.assertEqual(len(calls), 2)
                receipt = json.loads((self.root / 'oa18-platform-preparation.json').read_bytes())
                self.assertIn('"' + version + '"', receipt['java'])
                self.assertEqual(receipt['pins']['java_version'], version)

    def test_sdkmanager_uses_the_selected_sdk_root_without_a_path_lookup(self):
        calls = self.prepare('17.0.20.1', '17.0.20.1')
        self.assertEqual(calls[1].args[0], [
            str(self.root / 'sdk/cmdline-tools/latest/bin/sdkmanager'),
            '--install', 'ndk;27.3.13750724'])

    def test_four_component_runtime_cannot_match_a_three_component_pin(self):
        with self.assertRaisesRegex(SystemExit, 'preinstalled Java version differs'):
            self.prepare('17.0.20', '17.0.20.1')
        self.assertFalse((self.root / 'oa18-platform-preparation.json').exists())
        self.assertFalse((self.root / 'environment').exists())
        self.assertEqual(self.executed, [['java', '-version']])

    def test_malformed_or_unbounded_java_pin_is_rejected_before_execution(self):
        for version in ('17.0', '17.0.20.1.1', '17.0.20-ea', '17.0.20.1\n'):
            with self.subTest(version=version):
                with self.assertRaisesRegex(SystemExit, 'exact public java_version required'):
                    self.prepare(version, version)
                self.assertEqual(self.executed, [])


if __name__ == '__main__':
    unittest.main()
