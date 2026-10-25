#!/usr/bin/env python3
"""Exercise the real seed ZIP/download boundary without network or installation."""
from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import urllib.request
import warnings
import zipfile

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
spec = importlib.util.spec_from_file_location("assurance_seed", SCRIPTS / "stage-assurance-seed.py")
seed = importlib.util.module_from_spec(spec)
spec.loader.exec_module(seed)
from release_evidence import ReleaseError, SafeRedirect


def archive(entries):
    stream = io.BytesIO()
    with warnings.catch_warnings(), zipfile.ZipFile(stream, "w") as bundle:
        warnings.simplefilter("ignore", UserWarning)
        for name, value in entries:
            bundle.writestr(name, value)
    return stream.getvalue()


class SeedBoundary(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.out = Path(self.temp.name) / "public"
        self.plan = b'{"candidate":"original-pre-bump-source"}\n'
        self.body = archive([("release-plan.json", self.plan),
                             ("gui-profile.zip", archive([("job-receipt.json", b"observed")]))])
        self.pin = hashlib.sha256(self.body).hexdigest()
        self.url = "https://api.github.com/repos/owner/repository/releases/assets/42"
        self.metadata = {"id": 42, "url": self.url, "state": "uploaded",
                         "size": len(self.body), "digest": "sha256:" + self.pin}
        self.requests = []

    def stage(self, *, body=None, metadata=None, pin=None, repository="owner/repository"):
        body = self.body if body is None else body
        metadata = self.metadata if metadata is None else metadata
        requests = self.requests

        class Client:
            def __init__(self, current):
                requests.append(("repository", current))
                self.opener = self

            def get(self, suffix):
                requests.append(("metadata", suffix))
                return metadata

            def open(self, request, timeout):
                requests.append(("binary", request.full_url, request.headers, timeout))
                return io.BytesIO(body)

        with patch.object(seed, "GitHubAPI", Client), patch.dict(os.environ, {"GH_TOKEN": "unit-read-token"}):
            return seed.stage(repository, 42, self.pin if pin is None else pin, "a" * 40, self.out)

    def assert_bad_zip(self, body):
        with self.assertRaises((ReleaseError, zipfile.BadZipFile)):
            seed.unpack(body, self.out)
        self.assertFalse(self.out.exists())

    def test_real_files_and_nested_bundle_preserve_exact_bytes(self):
        receipt = self.stage()
        self.assertEqual((self.out / "release-plan.json").read_bytes(), self.plan)
        self.assertEqual(receipt["files_sha256"]["release-plan.json"], hashlib.sha256(self.plan).hexdigest())
        with zipfile.ZipFile(self.out / "gui-profile.zip") as bundle:
            self.assertEqual(bundle.read("job-receipt.json"), b"observed")
        saved = json.loads((self.out / seed.RECEIPT).read_bytes())
        self.assertEqual(saved, receipt)
        self.assertEqual(saved["status"], "TRANSPORT_ONLY")
        self.assertEqual(saved["authority"], "NOT_ESTABLISHED")
        self.assertEqual(saved["publication"], "NOT_EXERCISED")
        self.assertEqual(self.requests[0], ("repository", "owner/repository"))
        self.assertEqual(self.requests[1], ("metadata", "/releases/assets/42"))
        self.assertEqual(self.requests[2][1], self.url)
        self.assertEqual(self.requests[2][2]["Accept"], "application/octet-stream")

    def test_byte_mismatch_cannot_stage_a_receipt(self):
        with self.assertRaises(ReleaseError):
            self.stage(body=b"x" * len(self.body))
        self.assertFalse(self.out.exists())

    def test_server_metadata_cannot_replace_independent_pin(self):
        with self.assertRaises(ReleaseError):
            self.stage(pin="0" * 64)
        self.assertFalse(self.out.exists())
        self.assertFalse(any(r[0] == "binary" for r in self.requests))

    def test_foreign_asset_metadata_is_rejected_before_download(self):
        for changed in ({"id": 43}, {"url": self.url.replace("owner/repository", "other/repository")},
                        {"state": "new"}, {"size": seed.ARCHIVE_LIMIT + 1}, {"size": True}):
            with self.subTest(changed=changed), self.assertRaises(ReleaseError):
                self.stage(metadata={**self.metadata, **changed})
        self.assertFalse(any(r[0] == "binary" for r in self.requests))

    def test_invalid_repository_cannot_construct_an_api_client(self):
        for repository in ("https://evil.test/a/b", "owner/repo/../other", "owner/repo?ref=main"):
            with self.subTest(repository=repository), self.assertRaises(ReleaseError):
                self.stage(repository=repository)
        self.assertEqual(self.requests, [])

    def test_download_truncation_is_rejected(self):
        with self.assertRaises(ReleaseError):
            self.stage(body=self.body[:-1])
        self.assertFalse(self.out.exists())

    def test_download_growth_is_rejected(self):
        with self.assertRaises(ReleaseError):
            self.stage(body=self.body + b"extra")
        self.assertFalse(self.out.exists())

    def test_unsafe_member_paths_are_rejected(self):
        for name in ("../authority.json", "/authority.json", "dir/authority.json", "a\\b", "C:authority", ".hidden"):
            with self.subTest(name=name):
                self.assert_bad_zip(archive([(name, b"data")]))

    def test_duplicate_and_case_colliding_members_are_rejected(self):
        for names in (("authority.json", "authority.json"), ("authority.json", "AUTHORITY.json")):
            with self.subTest(names=names):
                self.assert_bad_zip(archive([(name, b"data") for name in names]))

    def test_transport_receipt_cannot_be_supplied_by_archive(self):
        for name in (seed.RECEIPT, seed.RECEIPT.upper()):
            with self.subTest(name=name):
                self.assert_bad_zip(archive([(name, b"untrusted")]))

    def test_link_and_directory_members_are_rejected(self):
        for kind in (stat.S_IFLNK, stat.S_IFDIR, stat.S_IFIFO):
            info = zipfile.ZipInfo("authority.json")
            info.create_system = 3
            info.external_attr = (kind | 0o600) << 16
            with self.subTest(kind=kind):
                self.assert_bad_zip(archive([(info, b"target")]))

    def test_encrypted_flag_is_rejected(self):
        body = bytearray(archive([("authority.json", b"data")]))
        central = body.index(b"PK\x01\x02")
        struct.pack_into("<H", body, 6, 1)
        struct.pack_into("<H", body, central + 8, 1)
        self.assert_bad_zip(bytes(body))

    def test_expanded_limit_is_enforced_before_output_creation(self):
        with patch.object(seed, "EXPANDED_LIMIT", 3):
            self.assert_bad_zip(archive([("authority.json", b"1234")]))

    def test_member_count_is_bounded_before_output_creation(self):
        with patch.object(seed, "MEMBER_LIMIT", 1):
            self.assert_bad_zip(archive([("a.json", b"1"), ("b.json", b"2")]))

    def test_empty_zip_is_rejected(self):
        self.assert_bad_zip(archive([]))

    def test_existing_output_is_never_overwritten(self):
        self.out.mkdir()
        sentinel = self.out / "keep.txt"
        sentinel.write_bytes(b"original")
        with self.assertRaises(ReleaseError):
            self.stage()
        self.assertEqual(sentinel.read_bytes(), b"original")
        self.assertEqual(list(self.out.iterdir()), [sentinel])

    def test_symlink_output_is_rejected(self):
        target = Path(self.temp.name) / "target"
        target.mkdir()
        self.out.symlink_to(target, target_is_directory=True)
        with self.assertRaises(ReleaseError):
            self.stage()
        self.assertEqual(list(target.iterdir()), [])

    def test_nonhttps_redirect_is_rejected_and_read_token_not_forwarded(self):
        request = urllib.request.Request(self.url, headers={"Authorization": "Bearer unit-read-token"})
        redirect = SafeRedirect()
        forwarded = redirect.redirect_request(request, None, 302, "Found", {}, "https://release-assets.githubusercontent.com/data")
        self.assertFalse(forwarded.has_header("Authorization"))
        with self.assertRaises(ReleaseError):
            redirect.redirect_request(request, None, 302, "Found", {}, "http://evil.test/data")

    def test_cli_unavailable_source_diagnostic_does_not_dump_environment(self):
        env = {**os.environ, "GITHUB_REPOSITORY": "invalid/repo/with/path", "GH_TOKEN": "never-print-this-token"}
        result = subprocess.run([sys.executable, str(SCRIPTS / "stage-assurance-seed.py"),
                                 "--asset-id", "42", "--sha256", self.pin, "--candidate", "a" * 40,
                                 "--out", str(self.out)], env=env, capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertIn("assurance seed transport failed", result.stderr)
        self.assertNotIn(env["GH_TOKEN"], result.stdout + result.stderr)
        self.assertNotIn("https://", result.stdout + result.stderr)
        self.assertFalse(self.out.exists())


if __name__ == "__main__":
    unittest.main()
